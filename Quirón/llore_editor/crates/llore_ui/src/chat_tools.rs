//! # Chat tools
//!
//! Las herramientas que el modelo puede invocar sobre el proyecto abierto.
//!
//! Toda ejecución pasa por [`crate::workspace_guard`] antes de tocar el disco.
//! El modelo tiene manos, pero son las del arnés: si pide leer un `.env`, la
//! guardia responde y la herramienta devuelve una negativa, no el secreto. El
//! confinamiento no depende de la buena voluntad del modelo; es la frontera del
//! sistema.
//!
//! Este módulo define **qué** se ofrece y **cómo** se ejecuta. El bucle de
//! conversación —pedir, ejecutar, devolver, repetir— vive en el cliente.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::workspace_guard::{self, Access};

/// Modelo que ejecuta el chat con herramientas.
///
/// Verificado contra la cuenta el 2026-07-10: `gpt-5.5` acepta y usa
/// herramientas; `gpt-5.6-sol` responde 400 («requires a newer version of
/// Codex») en cuanto la petición las incluye. Mientras eso no cambie, el chat
/// con manos va siempre por `gpt-5.5`, elija lo que elija el selector.
pub const TOOLS_CHAT_MODEL: &str = "gpt-5.5";

/// Caracteres de un archivo que devuelve `read_file`. Un archivo mayor se
/// recorta y el recorte se declara.
const READ_FILE_MAX_CHARS: usize = 24_000;
/// Máximo de rutas que devuelve `list_files`.
const LIST_FILES_MAX: usize = 400;
/// Máximo de coincidencias que devuelve `search_text`.
const SEARCH_MAX_HITS: usize = 60;
/// Cada coincidencia se recorta a esta longitud: una línea de una maqueta HTML
/// o de un JSON de evidencia puede medir cientos de kilobytes, y dos búsquedas
/// así llevaron el contexto del modelo a 349 KB (5 de septiembre).
const SEARCH_LINE_MAX_CHARS: usize = 200;
/// Tope de la salida completa de `search_text`.
const SEARCH_MAX_CHARS: usize = 12_000;

/// Una herramienta ofrecida al modelo, en formato Anthropic (`input_schema`).
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Catálogo de herramientas ofrecidas al modelo cuando hay un proyecto abierto.
pub fn tool_catalog() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_file",
            description: "Lee el contenido de un archivo del proyecto abierto. \
                La ruta es relativa a la raíz del proyecto. No puede leer \
                secretos ni archivos fuera del proyecto.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Ruta relativa a la raíz del proyecto"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolSpec {
            name: "list_files",
            description: "Enumera las rutas de los archivos del proyecto, \
                opcionalmente bajo un prefijo. No muestra secretos ni \
                artefactos generados.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prefix": {
                        "type": "string",
                        "description": "Prefijo de ruta para acotar (opcional)"
                    }
                }
            }),
        },
        ToolSpec {
            name: "search_text",
            description: "Busca una cadena de texto en los archivos del proyecto \
                y devuelve las líneas coincidentes con su ruta. Confinado al \
                proyecto.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Texto a buscar"
                    }
                },
                "required": ["query"]
            }),
        },
    ]
}

/// Resultado de ejecutar una herramienta: el texto que se devuelve al modelo y
/// si hubo error (para marcarlo como tal en el protocolo).
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutcome {
    fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// Vista del proyecto que necesita una herramienta para ejecutarse.
///
/// Se pasa explícitamente, en vez de acoplar este módulo al estado del editor,
/// para poder probar las herramientas contra un proyecto de prueba.
pub struct ToolContext<'a> {
    pub workspace_root: &'a Path,
    pub show_noise: bool,
}

/// Ejecuta la herramienta `name` con los argumentos `input`.
///
/// Nunca toca el disco sin consultar antes a la guardia. Un nombre desconocido,
/// un argumento ausente o un acceso denegado devuelven un error legible para el
/// modelo, no un pánico ni una fuga.
pub fn execute(ctx: &ToolContext, name: &str, input: &Value) -> ToolOutcome {
    if ctx.workspace_root.as_os_str().is_empty() {
        return ToolOutcome::error("no hay ningún proyecto abierto");
    }
    match name {
        "read_file" => read_file(ctx, input),
        "list_files" => list_files(ctx, input),
        "search_text" => search_text(ctx, input),
        otra => ToolOutcome::error(format!("herramienta desconocida: {otra}")),
    }
}

/// Resuelve una ruta relativa contra la raíz y la clasifica.
fn resolve_and_classify(ctx: &ToolContext, relative: &str) -> (PathBuf, Access) {
    let candidate = ctx.workspace_root.join(relative);
    let access = workspace_guard::classify(ctx.workspace_root, &candidate);
    (candidate, access)
}

fn read_file(ctx: &ToolContext, input: &Value) -> ToolOutcome {
    let Some(path) = input.get("path").and_then(|p| p.as_str()) else {
        return ToolOutcome::error("falta el argumento 'path'");
    };

    let (resolved, access) = resolve_and_classify(ctx, path);
    match access {
        Access::Allowed => {}
        Access::Secret => {
            return ToolOutcome::error(format!(
                "acceso denegado: '{path}' es una credencial o secreto"
            ))
        }
        Access::Outside => {
            return ToolOutcome::error(format!(
                "acceso denegado: '{path}' está fuera del proyecto"
            ))
        }
        Access::Noise => {
            return ToolOutcome::error(format!(
                "'{path}' es un artefacto generado; no se lee por defecto"
            ))
        }
    }

    let contenido = match std::fs::read_to_string(&resolved) {
        Ok(c) => c,
        Err(e) => return ToolOutcome::error(format!("no se pudo leer '{path}': {e}")),
    };

    if contenido.len() > READ_FILE_MAX_CHARS {
        let corte = contenido
            .char_indices()
            .take(READ_FILE_MAX_CHARS)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        ToolOutcome::ok(format!(
            "{}\n\n[recortado: {} de {} caracteres]",
            &contenido[..corte],
            READ_FILE_MAX_CHARS,
            contenido.len()
        ))
    } else {
        ToolOutcome::ok(contenido)
    }
}

fn list_files(ctx: &ToolContext, input: &Value) -> ToolOutcome {
    let prefix = input
        .get("prefix")
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .trim_start_matches('/');

    let mut rutas = Vec::new();
    recolectar(ctx, ctx.workspace_root, prefix, &mut rutas);
    rutas.sort();

    let total = rutas.len();
    rutas.truncate(LIST_FILES_MAX);

    if rutas.is_empty() {
        return ToolOutcome::ok("(sin archivos visibles)".to_string());
    }

    let mut salida = rutas.join("\n");
    if total > LIST_FILES_MAX {
        salida.push_str(&format!("\n\n[{} rutas más no mostradas]", total - LIST_FILES_MAX));
    }
    ToolOutcome::ok(salida)
}

fn recolectar(ctx: &ToolContext, dir: &Path, prefix: &str, out: &mut Vec<String>) {
    if out.len() >= LIST_FILES_MAX * 4 {
        return;
    }
    if !matches!(workspace_guard::classify(ctx.workspace_root, dir), Access::Allowed) {
        return;
    }
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        let visible = match workspace_guard::classify(ctx.workspace_root, &ruta) {
            Access::Allowed => true,
            Access::Noise => ctx.show_noise,
            Access::Secret | Access::Outside => false,
        };
        if !visible {
            continue;
        }
        let Ok(tipo) = entrada.file_type() else {
            continue;
        };
        if tipo.is_dir() {
            recolectar(ctx, &ruta, prefix, out);
        } else if tipo.is_file() {
            let rel = ruta
                .strip_prefix(ctx.workspace_root)
                .unwrap_or(&ruta)
                .to_string_lossy()
                .to_string();
            if prefix.is_empty() || rel.starts_with(prefix) {
                out.push(rel);
            }
        }
    }
}

fn search_text(ctx: &ToolContext, input: &Value) -> ToolOutcome {
    let Some(query) = input.get("query").and_then(|q| q.as_str()) else {
        return ToolOutcome::error("falta el argumento 'query'");
    };
    let query = query.trim();
    if query.is_empty() {
        return ToolOutcome::error("la búsqueda está vacía");
    }

    let mut archivos = Vec::new();
    recolectar(ctx, ctx.workspace_root, "", &mut archivos);
    archivos.sort();

    let mut hits = Vec::new();
    for rel in &archivos {
        if hits.len() >= SEARCH_MAX_HITS {
            break;
        }
        let ruta = ctx.workspace_root.join(rel);
        // Doble verificación: la guardia manda también aquí.
        if !matches!(workspace_guard::classify(ctx.workspace_root, &ruta), Access::Allowed) {
            continue;
        }
        let Ok(contenido) = std::fs::read_to_string(&ruta) else {
            continue;
        };
        for (n, linea) in contenido.lines().enumerate() {
            if linea.contains(query) {
                let linea = linea.trim();
                let recortada = if linea.chars().count() > SEARCH_LINE_MAX_CHARS {
                    let mut corta: String = linea.chars().take(SEARCH_LINE_MAX_CHARS).collect();
                    corta.push('…');
                    corta
                } else {
                    linea.to_string()
                };
                hits.push(format!("{}:{}: {}", rel, n + 1, recortada));
                if hits.len() >= SEARCH_MAX_HITS {
                    break;
                }
            }
        }
    }

    if hits.is_empty() {
        return ToolOutcome::ok(format!("sin coincidencias de «{query}»"));
    }
    let mut salida = String::new();
    let mut omitidas = 0;
    for hit in &hits {
        if salida.len() + hit.len() + 1 > SEARCH_MAX_CHARS {
            omitidas += 1;
            continue;
        }
        if !salida.is_empty() {
            salida.push('\n');
        }
        salida.push_str(hit);
    }
    if omitidas > 0 {
        salida.push_str(&format!("\n\n[salida recortada: {omitidas} coincidencias más]"));
    }
    ToolOutcome::ok(salida)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CONTADOR: AtomicU32 = AtomicU32::new(0);

    struct Proyecto {
        raiz: PathBuf,
    }

    impl Proyecto {
        fn nuevo(tag: &str) -> Self {
            let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
            let raiz = std::env::temp_dir().join(format!("llore_tools_{}_{n}_{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&raiz);
            fs::create_dir_all(&raiz).unwrap();
            Self {
                raiz: raiz.canonicalize().unwrap(),
            }
        }
        fn archivo(&self, rel: &str, contenido: &str) {
            let p = self.raiz.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, contenido).unwrap();
        }
        fn ctx(&self) -> ToolContext {
            ToolContext {
                workspace_root: &self.raiz,
                show_noise: false,
            }
        }
    }

    #[test]
    fn read_file_lee_un_archivo_permitido() {
        let p = Proyecto::nuevo("lee");
        p.archivo("src/main.rs", "fn main() {}\n");

        let r = execute(&p.ctx(), "read_file", &json!({ "path": "src/main.rs" }));
        assert!(!r.is_error);
        assert!(r.content.contains("fn main()"));
    }

    #[test]
    fn read_file_niega_un_secreto() {
        let p = Proyecto::nuevo("secreto");
        p.archivo(".env", "TOKEN=abc\n");

        let r = execute(&p.ctx(), "read_file", &json!({ "path": ".env" }));
        assert!(r.is_error, "debe negarse");
        assert!(r.content.contains("credencial") || r.content.contains("secreto"));
        assert!(!r.content.contains("abc"), "el secreto no debe filtrarse: {}", r.content);
    }

    #[test]
    fn read_file_niega_salir_del_proyecto() {
        let p = Proyecto::nuevo("fuera");
        let r = execute(&p.ctx(), "read_file", &json!({ "path": "../../etc/hostname" }));
        assert!(r.is_error);
        assert!(r.content.contains("fuera del proyecto"));
    }

    #[test]
    fn read_file_declara_el_recorte() {
        let p = Proyecto::nuevo("grande");
        p.archivo("grande.txt", &"x".repeat(READ_FILE_MAX_CHARS + 1000));

        let r = execute(&p.ctx(), "read_file", &json!({ "path": "grande.txt" }));
        assert!(!r.is_error);
        assert!(r.content.contains("[recortado:"));
    }

    #[test]
    fn list_files_oculta_secretos_y_ruido() {
        let p = Proyecto::nuevo("listar");
        p.archivo("src/main.rs", "x");
        p.archivo(".env", "TOKEN=x");
        p.archivo("target/debug/artefacto", "bin");

        let r = execute(&p.ctx(), "list_files", &json!({}));
        assert!(!r.is_error);
        assert!(r.content.contains("src/main.rs"));
        assert!(!r.content.contains(".env"), "{}", r.content);
        assert!(!r.content.contains("target/"), "{}", r.content);
    }

    #[test]
    fn search_text_recorta_lineas_largas_y_la_salida_total() {
        let p = Proyecto::nuevo("buscar_largo");
        p.archivo("maqueta.html", &format!("<div>{}</div>\n", "gateway ".repeat(2000)));
        let r = execute(&p.ctx(), "search_text", &json!({ "query": "gateway" }));
        assert!(!r.is_error);
        assert!(r.content.chars().count() < SEARCH_LINE_MAX_CHARS + 80, "{}", r.content.len());
        assert!(r.content.ends_with('…'), "{}", r.content);

        // Muchas coincidencias largas: la salida total queda acotada y lo dice.
        let lineas = (0..SEARCH_MAX_HITS)
            .map(|i| format!("gateway {} {}", i, "y".repeat(300)))
            .collect::<Vec<_>>()
            .join("\n");
        p.archivo("src/muchas.rs", &lineas);
        let r = execute(&p.ctx(), "search_text", &json!({ "query": "gateway" }));
        assert!(r.content.len() <= SEARCH_MAX_CHARS + 80, "{}", r.content.len());
        assert!(r.content.contains("[salida recortada:"), "{}", r.content);
    }

    #[test]
    fn search_text_encuentra_y_confina() {
        let p = Proyecto::nuevo("buscar");
        p.archivo("src/a.rs", "let gateway = 1;\n");
        p.archivo(".env", "gateway_secret=xyz\n");

        let r = execute(&p.ctx(), "search_text", &json!({ "query": "gateway" }));
        assert!(!r.is_error);
        assert!(r.content.contains("src/a.rs"), "{}", r.content);
        assert!(!r.content.contains("xyz"), "no debe buscar en secretos: {}", r.content);
    }

    #[test]
    fn una_herramienta_desconocida_es_error_no_panico() {
        let p = Proyecto::nuevo("desconocida");
        let r = execute(&p.ctx(), "borrar_todo", &json!({}));
        assert!(r.is_error);
        assert!(r.content.contains("desconocida"));
    }

    #[test]
    fn sin_proyecto_ninguna_herramienta_ejecuta() {
        let vacia = PathBuf::new();
        let ctx = ToolContext {
            workspace_root: &vacia,
            show_noise: false,
        };
        let r = execute(&ctx, "read_file", &json!({ "path": "cualquiera" }));
        assert!(r.is_error);
    }
}
