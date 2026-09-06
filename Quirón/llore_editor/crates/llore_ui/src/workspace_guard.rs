//! # Workspace Guard
//!
//! Confina el editor —y por tanto el modelo— a la carpeta del proyecto abierto.
//!
//! Toda lectura de disco debe pasar por [`classify`] antes de tocar el sistema
//! de archivos. La guardia distingue tres razones para negar el acceso, porque
//! no todas admiten la misma respuesta:
//!
//! - [`Access::Secret`]: credenciales. Se niega siempre, sin excepción posible.
//! - [`Access::Outside`]: fuera de la raíz. Se abre ese proyecto o se autoriza.
//! - [`Access::Noise`]: artefactos generados. Ocultos, pero legibles si se pide.
//!
//! Las rutas se canonicalizan antes de comparar, de modo que ni `..` ni un
//! enlace simbólico permitan salir de la raíz.

use std::path::{Component, Path, PathBuf};

/// Veredicto de la guardia para una ruta concreta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Dentro del proyecto y sin restricción.
    Allowed,
    /// Fuera de la raíz del proyecto.
    Outside,
    /// Credencial o material sensible. Nunca se lee.
    Secret,
    /// Artefacto generado, binario o log. Oculto salvo petición explícita.
    Noise,
}

impl Access {
    /// Cierto si la ruta puede leerse sin autorización adicional.
    pub fn is_allowed(self) -> bool {
        matches!(self, Access::Allowed)
    }

    /// Cierto si ninguna autorización del usuario puede levantar la negativa.
    pub fn is_absolute_denial(self) -> bool {
        matches!(self, Access::Secret)
    }

    /// Motivo legible para la interfaz.
    pub fn reason(self) -> &'static str {
        match self {
            Access::Allowed => "permitido",
            Access::Outside => "fuera de la carpeta del proyecto",
            Access::Secret => "credencial o secreto",
            Access::Noise => "artefacto generado",
        }
    }
}

/// Directorios cuyo contenido es sensible en cualquier nivel del árbol.
const SECRET_DIRS: &[&str] = &[
    ".ssh", ".gnupg", ".aws", ".codex", ".claude", ".config", ".docker", ".kube",
];

/// Nombres de fichero sensibles, comparados sin distinguir mayúsculas.
const SECRET_FILES: &[&str] = &[
    "auth.json",
    "credentials",
    "id_rsa",
    "id_ecdsa",
    "id_ed25519",
    ".netrc",
    ".pgpass",
    ".htpasswd",
];

/// Extensiones de material criptográfico.
const SECRET_EXTS: &[&str] = &["pem", "key", "p12", "pfx", "keystore", "jks", "asc", "gpg"];

/// Sufijos que degradan un `.env` a plantilla inofensiva.
const ENV_TEMPLATE_SUFFIXES: &[&str] = &[".example", ".sample", ".template", ".dist"];

/// Directorios generados. Ruido, no secreto.
const NOISE_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    ".fastembed_cache",
    ".venv",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
    // Estado del propio editor: identidad del proyecto, sesión y disposición.
    ".quiron",
    ".llore",
];

/// Extensiones sin valor semántico para el índice ni para el modelo.
const NOISE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "svg", "pdf", "zip", "gz", "xz", "tar", "so",
    "dylib", "dll", "rlib", "rmeta", "bin", "wasm", "onnx", "safetensors", "sqlite", "db", "log",
    "lock", "woff", "woff2", "ttf", "mp4", "wav",
];

/// Cierto si el nombre corresponde a un fichero de entorno con secretos.
///
/// `.env.example` es una plantilla y no se bloquea; `.env.local` sí, aunque en
/// este proyecto además sea un enlace hacia fuera de la raíz.
fn is_secret_env_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower != ".env" && !lower.starts_with(".env.") {
        return false;
    }
    !ENV_TEMPLATE_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

/// Cierto si el nombre, por sí solo, delata material sensible.
fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();

    if is_secret_env_file(&lower) {
        return true;
    }
    if SECRET_FILES.iter().any(|candidate| lower == *candidate) {
        return true;
    }
    if lower.starts_with("id_rsa") || lower.starts_with("id_ed25519") {
        return true;
    }
    if let Some((_, ext)) = lower.rsplit_once('.') {
        if SECRET_EXTS.contains(&ext) {
            return true;
        }
    }
    false
}

/// Cierto si el nombre corresponde a un artefacto generado.
fn is_noise_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();

    if NOISE_DIRS.iter().any(|candidate| lower == *candidate) {
        return true;
    }
    if let Some((_, ext)) = lower.rsplit_once('.') {
        if NOISE_EXTS.contains(&ext) {
            return true;
        }
    }
    false
}

/// Resuelve una ruta a su forma canónica.
///
/// `canonicalize` exige que la ruta exista. Para ficheros aún no creados se
/// canonicaliza el ancestro existente más cercano y se le añade el resto, de
/// modo que un enlace simbólico intermedio tampoco pueda escapar.
fn resolve(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Some(canonical);
    }

    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path;

    loop {
        let parent = cursor.parent()?;
        let name = cursor.file_name()?;
        suffix.push(name.to_os_string());

        if let Ok(canonical) = parent.canonicalize() {
            let mut resolved = canonical;
            for component in suffix.iter().rev() {
                resolved.push(component);
            }
            return Some(resolved);
        }
        cursor = parent;
    }
}

/// Clasifica el acceso a `path` para un proyecto con raíz `root`.
///
/// `root` debe existir. Si no puede canonicalizarse, se niega todo acceso.
pub fn classify(root: &Path, path: &Path) -> Access {
    // Un nombre sensible se rechaza antes de tocar el disco, incluso si el
    // enlace apunta a un destino inocuo o inexistente.
    for component in path.components() {
        if let Component::Normal(raw) = component {
            let name = raw.to_string_lossy();
            if is_secret_name(&name) || SECRET_DIRS.contains(&name.as_ref()) {
                return Access::Secret;
            }
        }
    }

    let Some(canonical_root) = resolve(root) else {
        return Access::Outside;
    };
    let Some(canonical_path) = resolve(path) else {
        return Access::Outside;
    };

    if !canonical_path.starts_with(&canonical_root) {
        return Access::Outside;
    }

    // Tras resolver enlaces, el destino real puede seguir siendo sensible.
    let Ok(relative) = canonical_path.strip_prefix(&canonical_root) else {
        return Access::Outside;
    };
    for component in relative.components() {
        if let Component::Normal(raw) = component {
            let name = raw.to_string_lossy();
            if is_secret_name(&name) || SECRET_DIRS.contains(&name.as_ref()) {
                return Access::Secret;
            }
        }
    }
    for component in relative.components() {
        if let Component::Normal(raw) = component {
            if is_noise_name(&raw.to_string_lossy()) {
                return Access::Noise;
            }
        }
    }

    Access::Allowed
}

/// Devuelve la ruta canónica si puede leerse sin autorización adicional.
pub fn ensure_within_workspace(root: &Path, path: &Path) -> Result<PathBuf, Access> {
    match classify(root, path) {
        Access::Allowed => resolve(path).ok_or(Access::Outside),
        denial => Err(denial),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("llore_guard_{}_{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crear directorio de prueba");
        dir.canonicalize().expect("canonicalizar raíz de prueba")
    }

    #[test]
    fn permite_un_archivo_dentro_del_proyecto() {
        let root = scratch("dentro");
        let file = root.join("src/main.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fn main() {}").unwrap();

        assert_eq!(classify(&root, &file), Access::Allowed);
        assert!(ensure_within_workspace(&root, &file).is_ok());
    }

    #[test]
    fn permite_un_archivo_todavia_inexistente() {
        let root = scratch("nuevo");
        let file = root.join("src/aun_no_existe.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();

        assert_eq!(classify(&root, &file), Access::Allowed);
    }

    #[test]
    fn rechaza_el_ascenso_con_puntos_dobles() {
        let root = scratch("ascenso");
        fs::create_dir_all(root.join("src")).unwrap();
        let fuera = root.join("src/../../secretos.txt");

        assert_eq!(classify(&root, &fuera), Access::Outside);
    }

    #[test]
    fn rechaza_una_ruta_absoluta_ajena() {
        let root = scratch("ajena");
        assert_eq!(classify(&root, Path::new("/etc/hostname")), Access::Outside);
    }

    #[cfg(unix)]
    #[test]
    fn rechaza_un_enlace_que_escapa_de_la_raiz() {
        let root = scratch("enlace");
        let fuera = std::env::temp_dir().join(format!("llore_fuera_{}", std::process::id()));
        fs::write(&fuera, "contenido ajeno").unwrap();

        let enlace = root.join("atajo.txt");
        std::os::unix::fs::symlink(&fuera, &enlace).unwrap();

        assert_eq!(classify(&root, &enlace), Access::Outside);
        let _ = fs::remove_file(&fuera);
    }

    #[test]
    fn bloquea_ficheros_de_entorno_pero_no_sus_plantillas() {
        let root = scratch("entorno");

        assert_eq!(classify(&root, &root.join(".env")), Access::Secret);
        assert_eq!(classify(&root, &root.join(".env.local")), Access::Secret);
        assert_eq!(classify(&root, &root.join(".env.production")), Access::Secret);

        // Las plantillas no llevan secretos y deben poder leerse.
        fs::write(root.join(".env.example"), "CLAVE=").unwrap();
        assert_eq!(classify(&root, &root.join(".env.example")), Access::Allowed);
    }

    #[test]
    fn bloquea_credenciales_a_cualquier_profundidad() {
        let root = scratch("credenciales");

        for ruta in [
            ".ssh/id_rsa",
            "sub/.ssh/id_ed25519",
            "infra/servidor.pem",
            "app/private.key",
            ".codex/auth.json",
            ".claude/settings.json",
        ] {
            assert_eq!(
                classify(&root, &root.join(ruta)),
                Access::Secret,
                "debería bloquearse: {ruta}"
            );
        }
    }

    #[test]
    fn marca_artefactos_como_ruido_y_no_como_secreto() {
        let root = scratch("ruido");

        for ruta in ["target/debug/app", "node_modules/x/index.js", ".git/config"] {
            let veredicto = classify(&root, &root.join(ruta));
            assert_eq!(veredicto, Access::Noise, "debería ser ruido: {ruta}");
            assert!(!veredicto.is_absolute_denial());
        }
        assert_eq!(classify(&root, &root.join("datos/logs.sqlite")), Access::Noise);
    }

    #[test]
    fn un_secreto_dentro_de_ruido_sigue_siendo_secreto() {
        let root = scratch("prioridad");
        assert_eq!(classify(&root, &root.join("target/release/.env")), Access::Secret);
    }

    #[test]
    fn la_raiz_misma_es_accesible() {
        let root = scratch("raiz");
        assert_eq!(classify(&root, &root), Access::Allowed);
    }
}
