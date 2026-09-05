//! Extracción determinista de unidades desde el árbol sintáctico.
//!
//! Dado el texto de un archivo, produce su unidad Archivo y las unidades Lógica
//! que contiene. No interviene ningún modelo: las relaciones y los símbolos se
//! derivan del árbol, y si el análisis no resuelve algo, no se inventa.
//!
//! Hoy soporta Rust. Añadir un lenguaje es añadir un extractor; el resto del
//! sistema no cambia.

use super::unit::{content_hash, stable_id, FileUnit, LogicKind, LogicUnit};
use tree_sitter::{Node as TsNode, Parser};

/// Detecta el lenguaje por extensión. `None` si no se sabe analizar.
pub fn language_of(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next() {
        Some("rs") => Some("rust"),
        _ => None,
    }
}

/// Extrae la unidad Archivo y sus unidades Lógica.
/// Devuelve `None` si el lenguaje no se soporta o el análisis falla.
pub fn extract(project_id: &str, path: &str, source: &str) -> Option<(FileUnit, Vec<LogicUnit>)> {
    match language_of(path)? {
        "rust" => extract_rust(project_id, path, source),
        _ => None,
    }
}

fn extract_rust(project_id: &str, path: &str, source: &str) -> Option<(FileUnit, Vec<LogicUnit>)> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();

    let mut file = FileUnit::new(project_id, path, "rust", bytes);
    let mut logic = Vec::new();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect_item(project_id, path, source, child, None, &mut file.symbols, &mut logic);
    }

    Some((file, logic))
}

/// Recorre un item de nivel superior (o un método dentro de un impl).
/// `impl_type`: prefijo `Tipo` cuando el item es un método de un `impl`.
fn collect_item(
    project_id: &str,
    path: &str,
    source: &str,
    node: TsNode,
    impl_type: Option<&str>,
    file_symbols: &mut Vec<String>,
    out: &mut Vec<LogicUnit>,
) {
    let (kind, base_name) = match node.kind() {
        "function_item" => (
            if impl_type.is_some() { LogicKind::Method } else { LogicKind::Function },
            field_name(node, source),
        ),
        "struct_item" => (LogicKind::Struct, field_name(node, source)),
        "enum_item" => (LogicKind::Enum, field_name(node, source)),
        "trait_item" => (LogicKind::Trait, field_name(node, source)),
        "impl_item" => {
            // El impl no es una unidad por sí mismo, pero sus métodos sí lo son.
            // El símbolo de cada método se prefija con el tipo implementado.
            let ty = impl_type_name(node, source);
            if let Some(ty) = &ty {
                if impl_type.is_none() && !file_symbols.iter().any(|s| s == ty) {
                    file_symbols.push(ty.clone());
                }
            }
            if let Some(body) = node.child_by_field_name("body") {
                let mut c = body.walk();
                for m in body.children(&mut c) {
                    if m.kind() == "function_item" {
                        collect_item(project_id, path, source, m, ty.as_deref(), file_symbols, out);
                    }
                }
            }
            return;
        }
        _ => return,
    };

    let Some(base_name) = base_name else { return };

    let symbol = match impl_type {
        Some(ty) => format!("{ty}::{base_name}"),
        None => base_name.clone(),
    };

    if impl_type.is_none() {
        file_symbols.push(base_name);
    }

    let signature = signature_text(node, source);
    let body_text = node
        .child_by_field_name("body")
        .map(|b| slice(source, b.start_byte(), b.end_byte()))
        .unwrap_or_else(|| slice(source, node.start_byte(), node.end_byte()));

    // Las llamadas solo tienen sentido en cuerpos ejecutables. `Self::x` se
    // escribe con el tipo del impl, que es como se llama la unidad destino.
    let calls = match kind {
        LogicKind::Function | LogicKind::Method => node
            .child_by_field_name("body")
            .map(|body| collect_calls(body, source))
            .unwrap_or_default()
            .into_iter()
            .map(|call| match (impl_type, call.strip_prefix("Self::")) {
                (Some(ty), Some(method)) => format!("{ty}::{method}"),
                _ => call,
            })
            .collect(),
        _ => Vec::new(),
    };

    out.push(LogicUnit {
        id: stable_id(project_id, path, &symbol),
        project_id: project_id.to_string(),
        path: path.to_string(),
        symbol,
        kind,
        signature,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        normalized_hash: content_hash(normalize(&body_text).as_bytes()),
        semantic_text: None,
        calls,
    });
}

/// Nombres llamados dentro de un cuerpo, deduplicados y ordenados. Solo cuenta
/// las `call_expression` del árbol; las macros no se consideran llamadas.
fn collect_calls(body: TsNode, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression" {
            if let Some(name) = node
                .child_by_field_name("function")
                .and_then(|function| callee_name(function, source))
            {
                out.push(name);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Nombre del destino de una llamada según la forma de la expresión.
fn callee_name(node: TsNode, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(slice(source, node.start_byte(), node.end_byte())),
        // `x.metodo(...)`: el receptor no se resuelve sin tipos; se anota el método.
        "field_expression" => node
            .child_by_field_name("field")
            .map(|field| format!(".{}", slice(source, field.start_byte(), field.end_byte()))),
        "scoped_identifier" => {
            let text = slice(source, node.start_byte(), node.end_byte());
            let segments: Vec<&str> = text.split("::").collect();
            let name = (*segments.last()?).to_string();
            match segments.iter().rev().nth(1) {
                // `Tipo::metodo`: el tipo forma parte del símbolo de la unidad.
                Some(owner) if owner.chars().next().is_some_and(char::is_uppercase) => {
                    Some(format!("{owner}::{name}"))
                }
                // `modulo::funcion`: la unidad se llama solo `funcion`.
                _ => Some(name),
            }
        }
        "generic_function" => node
            .child_by_field_name("function")
            .and_then(|function| callee_name(function, source)),
        _ => None,
    }
}

/// Texto del campo `name` de un item.
fn field_name(node: TsNode, source: &str) -> Option<String> {
    let n = node.child_by_field_name("name")?;
    Some(slice(source, n.start_byte(), n.end_byte()))
}

/// Tipo implementado por un `impl_item` (campo `type`).
fn impl_type_name(node: TsNode, source: &str) -> Option<String> {
    let t = node.child_by_field_name("type")?;
    Some(slice(source, t.start_byte(), t.end_byte()))
}

/// La firma: el texto desde el inicio del item hasta el inicio de su cuerpo.
/// Si no hay cuerpo (p. ej. un struct de tupla), la primera línea.
fn signature_text(node: TsNode, source: &str) -> String {
    if let Some(body) = node.child_by_field_name("body") {
        return slice(source, node.start_byte(), body.start_byte())
            .trim()
            .trim_end_matches('{')
            .trim()
            .to_string();
    }
    slice(source, node.start_byte(), node.end_byte())
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn slice(source: &str, start: usize, end: usize) -> String {
    source.get(start..end).unwrap_or("").to_string()
}

/// Normaliza el cuerpo colapsando espacios en blanco, para que dos
/// implementaciones idénticas salvo formato compartan `normalized_hash`.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
use std::fmt;

pub struct Motor {
    rpm: u32,
}

impl Motor {
    pub fn new() -> Self {
        Motor { rpm: 0 }
    }

    fn acelera(&mut self, delta: u32) {
        self.rpm += delta;
    }
}

pub enum Estado {
    Parado,
    Girando,
}

pub fn arranca(m: &mut Motor) {
    m.acelera(100);
}

trait Ruido {
    fn suena(&self);
}
"#;

    #[test]
    fn extrae_las_unidades_logica_de_rust() {
        let (file, logic) = extract("proj", "src/motor.rs", SRC).expect("debe analizar rust");
        assert_eq!(file.language, "rust");
        assert_eq!(file.project_id, "proj");

        let symbols: Vec<&str> = logic.iter().map(|l| l.symbol.as_str()).collect();
        assert!(symbols.contains(&"Motor"), "struct Motor");
        assert!(symbols.contains(&"Motor::new"), "método new prefijado por el tipo");
        assert!(symbols.contains(&"Motor::acelera"), "método acelera prefijado");
        assert!(symbols.contains(&"Estado"), "enum Estado");
        assert!(symbols.contains(&"arranca"), "función libre arranca");
        assert!(symbols.contains(&"Ruido"), "trait Ruido");
    }

    #[test]
    fn los_metodos_llevan_clase_method_y_los_libres_function() {
        let (_f, logic) = extract("proj", "src/motor.rs", SRC).unwrap();
        let new = logic.iter().find(|l| l.symbol == "Motor::new").unwrap();
        assert_eq!(new.kind, LogicKind::Method);
        let arranca = logic.iter().find(|l| l.symbol == "arranca").unwrap();
        assert_eq!(arranca.kind, LogicKind::Function);
    }

    #[test]
    fn el_rango_de_lineas_se_captura() {
        let (_f, logic) = extract("proj", "src/motor.rs", SRC).unwrap();
        let acelera = logic.iter().find(|l| l.symbol == "Motor::acelera").unwrap();
        assert!(acelera.start_line < acelera.end_line);
        assert!(acelera.signature.contains("acelera"));
    }

    #[test]
    fn las_llamadas_del_cuerpo_se_registran_por_su_forma() {
        let (_f, logic) = extract("proj", "src/motor.rs", SRC).unwrap();
        let arranca = logic.iter().find(|l| l.symbol == "arranca").unwrap();
        assert_eq!(arranca.calls, vec![".acelera"]);
        let new = logic.iter().find(|l| l.symbol == "Motor::new").unwrap();
        assert!(new.calls.is_empty());

        let src = "impl A { fn f(&self) { Self::g(); helper(); modulo::otra(); x.m::<u8>(); helper(); } fn g() {} }\nfn helper() {}";
        let (_f, logic) = extract("p", "a.rs", src).unwrap();
        let f = logic.iter().find(|l| l.symbol == "A::f").unwrap();
        assert_eq!(f.calls, vec![".m", "A::g", "helper", "otra"]);
        let g = logic.iter().find(|l| l.symbol == "A::g").unwrap();
        assert!(g.calls.is_empty());
    }

    #[test]
    fn cuerpos_equivalentes_comparten_hash_normalizado() {
        let a = extract("p", "a.rs", "fn f() {\n    let x = 1;\n}").unwrap().1;
        let b = extract("p", "b.rs", "fn f() {   let x = 1; }").unwrap().1;
        assert_eq!(a[0].normalized_hash, b[0].normalized_hash);
    }

    #[test]
    fn lenguaje_no_soportado_devuelve_none() {
        assert!(extract("p", "notas.txt", "hola").is_none());
    }
}
