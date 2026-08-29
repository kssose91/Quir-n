//! # Rutas del Sistema para Llore
//!
//! Define las rutas estándar donde Llore guarda sus datos:
//! - Configuración
//! - Datos persistentes
//! - Logs
//! - Estado de la sesión

use std::path::PathBuf;
use std::sync::OnceLock;

/// Rutas del sistema para Llore.
///
/// Por defecto usa las rutas estándar de XDG en Linux:
/// - Config: ~/.config/llore/
/// - Data: ~/.local/share/llore/
/// - Logs: ~/.local/share/llore/logs/
#[derive(Debug, Clone)]
pub struct LlorePaths {
    /// Directorio de configuración (settings.json, themes, etc.)
    pub config_dir: PathBuf,
    /// Directorio de datos (memoria, estado, etc.)
    pub data_dir: PathBuf,
    /// Directorio de logs
    pub logs_dir: PathBuf,
    /// Directorio de memoria persistente (Quirón brain)
    pub memory_dir: PathBuf,
}

impl LlorePaths {
    /// Nombre de la aplicación (usado para rutas)
    const APP_NAME: &'static str = "llore";

    /// Crea rutas con los valores por defecto del sistema.
    pub fn default_paths() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join(Self::APP_NAME);

        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join(Self::APP_NAME);

        let logs_dir = data_dir.join("logs");
        let memory_dir = data_dir.join("memory");

        Self {
            config_dir,
            data_dir,
            logs_dir,
            memory_dir,
        }
    }

    /// Crea rutas personalizadas desde un directorio base.
    pub fn from_base(base: PathBuf) -> Self {
        Self {
            config_dir: base.join("config"),
            data_dir: base.join("data"),
            logs_dir: base.join("logs"),
            memory_dir: base.join("memory"),
        }
    }

    /// Ruta al archivo de configuración principal.
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    /// Ruta al archivo de keybindings.
    pub fn keymap_file(&self) -> PathBuf {
        self.config_dir.join("keymap.json")
    }

    /// Ruta al log actual.
    pub fn current_log(&self) -> PathBuf {
        self.logs_dir.join("llore.log")
    }

    /// Ruta al estado de memoria de Quirón.
    pub fn quiron_state(&self) -> PathBuf {
        self.memory_dir.join("state.json")
    }

    /// Asegura que todos los directorios existen.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        std::fs::create_dir_all(&self.memory_dir)?;
        Ok(())
    }
}

impl Default for LlorePaths {
    fn default() -> Self {
        Self::default_paths()
    }
}

/// Singleton global de rutas.
static PATHS: OnceLock<LlorePaths> = OnceLock::new();

/// Obtiene las rutas globales de la aplicación.
pub fn paths() -> &'static LlorePaths {
    PATHS.get_or_init(LlorePaths::default_paths)
}

/// Inicializa las rutas globales con rutas personalizadas.
/// Debe llamarse antes del primer uso de `paths()`.
pub fn init_paths(custom: LlorePaths) -> Result<(), LlorePaths> {
    PATHS.set(custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_paths() {
        let paths = LlorePaths::default_paths();
        assert!(paths.config_dir.ends_with("llore"));
        assert!(paths.data_dir.ends_with("llore"));
    }

    #[test]
    fn test_from_base() {
        let base = PathBuf::from("/tmp/llore_test");
        let paths = LlorePaths::from_base(base);
        assert_eq!(paths.config_dir, PathBuf::from("/tmp/llore_test/config"));
        assert_eq!(
            paths.settings_file(),
            PathBuf::from("/tmp/llore_test/config/settings.json")
        );
    }

    #[test]
    fn test_special_paths() {
        let paths = LlorePaths::default_paths();
        assert!(paths.quiron_state().ends_with("memory/state.json"));
    }
}
