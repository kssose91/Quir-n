//! Recorrido del proyecto respetando las exclusiones obligatorias.
//!
//! Espeja la frontera del `workspace_guard` del editor: lo que no puede abrirse
//! en una pestaña tampoco puede convertirse en una unidad. Los secretos se
//! niegan siempre; los artefactos y las dependencias vendorizadas se ignoran.
//! El indexador hereda el alcance del arnés y no lo amplía (memoria §4.2.3).

use std::path::{Path, PathBuf};

/// Un archivo indexable, con su ruta relativa a la raíz y su contenido.
pub struct WalkedFile {
    pub rel_path: String,
    pub content: String,
}

/// Directorios que nunca se recorren.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".llore",
    "dist",
    "build",
    "__pycache__",
    ".fastembed_cache",
    "vendor",
    ".venv",
    "venv",
];

/// Nombres o sufijos que son secretos: negación absoluta, incluso dentro de la
/// raíz. Un `.env.example` es una plantilla y no cae aquí.
fn is_secret(name: &str) -> bool {
    if name == ".env" || name.starts_with(".env.") && !name.ends_with(".example") {
        return true;
    }
    name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
        || name == ".ssh"
        || name == "auth.json"
}

/// Extensiones de artefacto binario o generado que no tienen valor semántico.
fn is_binary_like(name: &str) -> bool {
    const EXT: &[&str] = &[
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".ico", ".pdf", ".zip", ".gz",
        ".tar", ".bin", ".so", ".rlib", ".o", ".a", ".lock", ".wasm", ".ttf",
        ".otf", ".woff", ".woff2", ".mp4", ".mp3", ".onnx", ".safetensors",
    ];
    let lower = name.to_ascii_lowercase();
    EXT.iter().any(|e| lower.ends_with(e))
}

/// Recorre `root` y devuelve los archivos indexables. `max_bytes` descarta
/// ficheros anómalamente grandes (generados, minificados, datos embebidos).
pub fn walk(root: &Path, max_bytes: u64) -> std::io::Result<Vec<WalkedFile>> {
    let mut out = Vec::new();
    walk_dir(root, root, max_bytes, &mut out)?;
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path)); // orden determinista
    Ok(out)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    max_bytes: u64,
    out: &mut Vec<WalkedFile>,
) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();

    for path in entries {
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if is_secret(name) {
            continue;
        }

        if path.is_dir() {
            if EXCLUDED_DIRS.contains(&name) {
                continue;
            }
            walk_dir(root, &path, max_bytes, out)?;
            continue;
        }

        if is_binary_like(name) {
            continue;
        }

        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > max_bytes {
            continue;
        }

        // Solo texto UTF-8 válido. Un binario disfrazado se descarta al leerlo.
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        out.push(WalkedFile { rel_path: rel, content });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_env_es_secreto_pero_el_example_no() {
        assert!(is_secret(".env"));
        assert!(is_secret(".env.local"));
        assert!(!is_secret(".env.example"));
        assert!(is_secret("server.pem"));
        assert!(is_secret("id_rsa"));
    }

    #[test]
    fn los_binarios_se_reconocen_por_extension() {
        assert!(is_binary_like("logo.PNG"));
        assert!(is_binary_like("Cargo.lock"));
        assert!(is_binary_like("modelo.onnx"));
        assert!(!is_binary_like("main.rs"));
    }
}
