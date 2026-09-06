//! # Recents
//!
//! Lista de proyectos abiertos recientemente.
//!
//! A diferencia del resto del estado del editor, esta lista no pertenece a
//! ningún proyecto: se guarda en el directorio de datos del usuario, según la
//! especificación de directorios base de XDG.

use std::fs;
use std::path::{Path, PathBuf};

/// Número máximo de proyectos recordados.
const MAX_RECENTS: usize = 8;

/// Ruta del fichero de recientes, bajo `$XDG_DATA_HOME` o `~/.local/share`.
pub fn recents_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))?;

    // Carpeta propia de Quirón: el editor del que nació (Llore) usa la suya,
    // y sus proyectos recientes no tienen que aparecer aquí.
    Some(base.join("quiron").join("recents.txt"))
}

/// Lee la lista de proyectos recientes, descartando los que ya no existen.
pub fn load() -> Vec<PathBuf> {
    recents_path().map(|path| load_from(&path)).unwrap_or_default()
}

/// Coloca `project` a la cabeza de la lista y la persiste.
pub fn remember(project: &Path) {
    if let Some(path) = recents_path() {
        remember_in(&path, project);
    }
}

/// Variante de [`load`] sobre un fichero explícito.
///
/// La ruta se inyecta para poder probar sin alterar variables de entorno
/// globales, que las pruebas en paralelo se pisarían entre sí.
pub fn load_from(path: &Path) -> Vec<PathBuf> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };

    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|candidate| candidate.is_dir())
        .take(MAX_RECENTS)
        .collect()
}

/// Variante de [`remember`] sobre un fichero explícito.
///
/// Un proyecto ya presente sube al principio en vez de duplicarse.
pub fn remember_in(path: &Path, project: &Path) {
    let canonical = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());

    let mut entries = load_from(path);
    entries.retain(|existing| existing != &canonical);
    entries.insert(0, canonical);
    entries.truncate(MAX_RECENTS);

    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let payload: String = entries
        .iter()
        .map(|entry| format!("{}\n", entry.display()))
        .collect();
    let _ = fs::write(path, payload);
}

/// Nombre visible de un proyecto: su última componente.
pub fn display_name(project: &Path) -> String {
    project
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("?")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CONTADOR: AtomicU32 = AtomicU32::new(0);

    fn ruta_unica(tag: &str) -> PathBuf {
        let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("llore_recents_{}_{n}_{tag}", std::process::id()))
    }

    fn fichero(tag: &str) -> PathBuf {
        ruta_unica(tag).join("recents.txt")
    }

    fn carpeta(tag: &str) -> PathBuf {
        let ruta = ruta_unica(tag);
        fs::create_dir_all(&ruta).expect("crear proyecto de prueba");
        ruta.canonicalize().expect("canonicalizar")
    }

    #[test]
    fn recuerda_y_ordena_por_uso_reciente() {
        let lista = fichero("orden");
        let primero = carpeta("alfa");
        let segundo = carpeta("beta");

        remember_in(&lista, &primero);
        remember_in(&lista, &segundo);

        let recientes = load_from(&lista);
        assert_eq!(
            recientes.first(),
            Some(&segundo),
            "el último abierto va primero"
        );
        assert_eq!(recientes.len(), 2);

        // Reabrir el primero lo sube, no lo duplica.
        remember_in(&lista, &primero);
        let recientes = load_from(&lista);
        assert_eq!(recientes.first(), Some(&primero));
        assert_eq!(recientes.len(), 2, "no debe duplicarse");
    }

    #[test]
    fn descarta_proyectos_que_ya_no_existen() {
        let lista = fichero("borrado");
        let vivo = carpeta("vivo");
        let muerto = carpeta("muerto");

        remember_in(&lista, &vivo);
        remember_in(&lista, &muerto);
        fs::remove_dir_all(&muerto).expect("borrar proyecto");

        assert_eq!(load_from(&lista), vec![vivo]);
    }

    #[test]
    fn una_lista_inexistente_no_es_un_error() {
        assert!(load_from(&fichero("vacia")).is_empty());
    }
}
