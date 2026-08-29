//! # Project ID
//!
//! Identidad de un proyecto.
//!
//! Un proyecto nace cuando el usuario concede acceso a un directorio. Su
//! identidad no es la ruta ni el nombre visible —ambos cambian— sino un ULID
//! guardado dentro de él:
//!
//! ```text
//! <proyecto>/.llore/project.id     01KX7Q2M8V3N4P5R6S7T8U9V0W
//! ```
//!
//! Ese identificador es lo que viaja al registro de eventos, a la carga útil de
//! cada punto del índice vectorial y al nodo raíz del grafo. Mover o renombrar
//! la carpeta no lo altera; dos carpetas llamadas `backend` no lo comparten.
//!
//! Se emplea la misma familia de identificadores que el registro (`ulid`), de
//! modo que no haya que traducir entre extremos.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ulid::Ulid;

/// Ruta del identificador, relativa a la raíz del proyecto.
pub const PROJECT_ID_RELATIVE_PATH: &str = ".llore/project.id";

/// Longitud de un ULID en su representación textual.
const ULID_LEN: usize = 26;

/// Error al resolver la identidad de un proyecto.
#[derive(Debug, thiserror::Error)]
pub enum ProjectIdError {
    #[error("el identificador de {0} está corrupto: {1:?}")]
    Corrupt(PathBuf, String),
    #[error("no se pudo leer o escribir el identificador: {0}")]
    Io(#[from] io::Error),
}

/// Ruta absoluta del identificador de `root`.
pub fn path_for(root: &Path) -> PathBuf {
    root.join(PROJECT_ID_RELATIVE_PATH)
}

/// Devuelve el identificador de `root` si ya existe y es válido.
///
/// No crea nada. Útil para saber si un directorio es ya un proyecto conocido.
pub fn load(root: &Path) -> Option<String> {
    let raw = fs::read_to_string(path_for(root)).ok()?;
    let candidate = raw.trim().to_string();
    is_valid(&candidate).then_some(candidate)
}

/// Devuelve el identificador de `root`, creándolo la primera vez.
///
/// Un identificador presente pero corrupto **no se sobrescribe**: se devuelve un
/// error. Sobrescribirlo desgajaría el proyecto de todo su historial, y esa es
/// una decisión que no corresponde tomar en silencio.
pub fn load_or_create(root: &Path) -> Result<String, ProjectIdError> {
    let path = path_for(root);

    match fs::read_to_string(&path) {
        Ok(raw) => {
            let candidate = raw.trim().to_string();
            if is_valid(&candidate) {
                return Ok(candidate);
            }
            Err(ProjectIdError::Corrupt(path, candidate))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let id = Ulid::new().to_string();
            write_atomically(&path, &id)?;
            Ok(id)
        }
        Err(error) => Err(ProjectIdError::Io(error)),
    }
}

/// Cierto si `candidate` tiene la forma de un ULID.
///
/// Se comprueba la forma, no la validez semántica: basta para distinguir un
/// identificador de un fichero truncado o de basura.
fn is_valid(candidate: &str) -> bool {
    candidate.len() == ULID_LEN && candidate.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Escribe el identificador sin dejar un fichero a medias.
///
/// Se escribe en un temporal del mismo directorio y se renombra: `rename` es
/// atómico dentro de un sistema de archivos, de modo que un corte de corriente
/// deja el identificador entero o inexistente, nunca partido.
fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "ruta sin directorio"))?;
    fs::create_dir_all(parent)?;

    let temporary = parent.join(format!(
        ".project.id.{}.tmp",
        std::process::id()
    ));
    fs::write(&temporary, format!("{contents}\n"))?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CONTADOR: AtomicU32 = AtomicU32::new(0);

    fn carpeta(tag: &str) -> PathBuf {
        let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
        let ruta = std::env::temp_dir().join(format!(
            "llore_projid_{}_{n}_{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&ruta);
        fs::create_dir_all(&ruta).expect("crear proyecto de prueba");
        ruta
    }

    #[test]
    fn crea_el_identificador_la_primera_vez() {
        let root = carpeta("crea");
        assert!(load(&root).is_none(), "aún no es un proyecto");

        let id = load_or_create(&root).expect("debe crearse");
        assert_eq!(id.len(), ULID_LEN);
        assert!(path_for(&root).exists());
        assert_eq!(load(&root).as_deref(), Some(id.as_str()));
    }

    #[test]
    fn reabrir_el_proyecto_devuelve_el_mismo_identificador() {
        let root = carpeta("estable");
        let primero = load_or_create(&root).expect("crear");
        let segundo = load_or_create(&root).expect("releer");
        assert_eq!(primero, segundo, "no debe regenerarse");
    }

    #[test]
    fn mover_la_carpeta_conserva_la_identidad() {
        let origen = carpeta("origen");
        let id = load_or_create(&origen).expect("crear");

        let destino = origen.with_file_name(format!(
            "llore_projid_{}_movido",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&destino);
        fs::rename(&origen, &destino).expect("mover el proyecto");

        assert_eq!(
            load_or_create(&destino).expect("releer tras mover"),
            id,
            "mover la carpeta no debe crear un mundo nuevo"
        );
        let _ = fs::remove_dir_all(&destino);
    }

    #[test]
    fn dos_carpetas_con_el_mismo_nombre_son_proyectos_distintos() {
        let a = carpeta("backend");
        let b = carpeta("backend");
        assert_ne!(
            load_or_create(&a).unwrap(),
            load_or_create(&b).unwrap(),
            "el nombre no identifica un proyecto"
        );
    }

    #[test]
    fn un_identificador_corrupto_no_se_sobrescribe() {
        let root = carpeta("corrupto");
        let path = path_for(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "basura").unwrap();

        let error = load_or_create(&root).expect_err("debe negarse");
        assert!(matches!(error, ProjectIdError::Corrupt(_, _)));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "basura",
            "el contenido original debe conservarse"
        );
        assert!(load(&root).is_none());
    }

    #[test]
    fn se_toleran_espacios_y_salto_de_linea() {
        let root = carpeta("espacios");
        let id = Ulid::new().to_string();
        let path = path_for(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("  {id}\n\n")).unwrap();

        assert_eq!(load(&root).as_deref(), Some(id.as_str()));
    }

    #[test]
    fn no_deja_temporales_tras_escribir() {
        let root = carpeta("temporales");
        load_or_create(&root).expect("crear");

        let sobrantes: Vec<_> = fs::read_dir(root.join(".llore"))
            .expect("leer .llore")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(sobrantes.is_empty(), "quedaron temporales: {sobrantes:?}");
    }
}
