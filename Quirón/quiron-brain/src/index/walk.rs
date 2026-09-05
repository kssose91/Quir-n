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
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
];

/// Nombres o sufijos que son secretos: negación absoluta, incluso dentro de la
/// raíz. Un `.env.example` es una plantilla y no cae aquí.
pub(super) fn is_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let name = lower.as_str();
    if name == ".env" || name.starts_with(".env.") {
        return ![".example", ".sample", ".template", ".dist"]
            .iter().any(|suffix| name.ends_with(suffix));
    }
    [".ssh", ".gnupg", ".aws", ".codex", ".claude", ".config", ".docker", ".kube",
     "auth.json", "credentials", "id_rsa", "id_ecdsa", "id_ed25519",
     ".netrc", ".pgpass", ".htpasswd"].contains(&name)
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.rsplit_once('.').map(|(_, ext)|
            ["pem", "key", "p12", "pfx", "keystore", "jks", "asc", "gpg"].contains(&ext)
        ).unwrap_or(false)
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
    let root = std::fs::canonicalize(root)?;
    let mut out = Vec::new();
    // Git aplica sus exclusiones reales: caches, datos y artefactos propios del
    // proyecto no deben convertirse en fichas aunque tengan extensión de texto.
    let tracked = if root.join(".git").exists() {
        std::process::Command::new("git").arg("-C").arg(&root)
            .args(["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "."])
            .env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").output().ok()
            .filter(|o| o.status.success()).map(|o| o.stdout.split(|b| *b == 0)
                .filter_map(|b| std::str::from_utf8(b).ok()).map(str::to_owned)
                .collect::<std::collections::HashSet<_>>())
    } else { None };
    walk_dir(&root, &root, max_bytes, &mut out, tracked.as_ref())?;
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path)); // orden determinista
    Ok(out)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    max_bytes: u64,
    out: &mut Vec<WalkedFile>,
    tracked: Option<&std::collections::HashSet<String>>,
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

        // No seguir enlaces: evita fugas fuera del proyecto, alias de secretos
        // y ciclos de directorios. Tampoco abrir pipes/dispositivos como texto.
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if EXCLUDED_DIRS.contains(&name.to_ascii_lowercase().as_str()) {
                continue;
            }
            if let Some(tracked) = tracked {
                let prefix = format!("{}/", path.strip_prefix(root).unwrap().to_string_lossy());
                if !tracked.iter().any(|p| p.starts_with(&prefix)) { continue; }
            }
            walk_dir(root, &path, max_bytes, out, tracked)?;
            continue;
        }

        if let Some(tracked) = tracked {
            if !tracked.contains(path.strip_prefix(root).unwrap().to_string_lossy().as_ref()) { continue; }
        }
        if is_binary_like(name) {
            continue;
        }

        if !meta.is_file() || meta.len() > max_bytes {
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

        if out.len() >= 10000 || out.iter().map(|f| f.content.len()).sum::<usize>() + content.len() > 64 * 1024 * 1024 {
            return Err(std::io::Error::other("Proyecto demasiado grande (10000 archivos / 64 MiB de texto); abre una subcarpeta"));
        }
        out.push(WalkedFile { rel_path: rel, content });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_ignored_generated_text_is_not_indexed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git").args(["init", "-q"])
            .arg(dir.path()).status().unwrap().success());
        std::fs::write(dir.path().join(".gitignore"), "cache/\n").unwrap();
        std::fs::create_dir(dir.path().join("cache")).unwrap();
        std::fs::write(dir.path().join("cache/generated.rs"), "fn stale() {}").unwrap();
        std::fs::write(dir.path().join("source.rs"), "fn current() {}").unwrap();
        let files = walk(dir.path(), 4096).unwrap();
        assert!(files.iter().any(|f| f.rel_path == "source.rs"));
        assert!(!files.iter().any(|f| f.rel_path.starts_with("cache/")));
    }

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

    #[test]
    fn el_barrido_excluye_directorios_secretos_y_admite_plantillas() {
        let root = tempfile::tempdir().unwrap();
        for dir in [".codex", ".claude", ".aws", ".config", ".SSH"] {
            let p = root.path().join(dir);
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("secret.rs"), "const SECRET: &str = \"no\";").unwrap();
        }
        for name in [".ENV.local", "private.P12", "id_ed25519", "credentials"] {
            std::fs::write(root.path().join(name), "secret").unwrap();
        }
        std::fs::write(root.path().join(".env.template"), "PORT=1234").unwrap();
        std::fs::write(root.path().join("main.rs"), "fn main() {}").unwrap();
        let files = walk(root.path(), 1024).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(paths, [".env.template", "main.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn el_barrido_no_sigue_enlaces_a_exteriores_secretos_o_ciclos() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("external.rs"), "fn external() {}").unwrap();
        std::fs::write(root.path().join(".env"), "SECRET=yes").unwrap();
        symlink(outside.path(), root.path().join("outside")).unwrap();
        symlink(outside.path().join("external.rs"), root.path().join("alias.rs")).unwrap();
        symlink(root.path().join(".env"), root.path().join("config.rs")).unwrap();
        symlink(root.path(), root.path().join("cycle")).unwrap();
        assert!(walk(root.path(), 1024).unwrap().is_empty());
    }
}
