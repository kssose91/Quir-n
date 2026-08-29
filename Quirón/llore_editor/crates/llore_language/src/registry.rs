//! # Registro de Lenguajes
//!
//! Mantiene un catálogo de lenguajes disponibles y permite
//! detectar el lenguaje apropiado para un archivo.

use crate::language::{Language, LanguageConfig, LanguageId};
use std::collections::HashMap;
use std::path::Path;

/// Registro de lenguajes disponibles.
#[derive(Debug, Default)]
pub struct LanguageRegistry {
    /// Lenguajes registrados por ID.
    languages: HashMap<LanguageId, Language>,
    /// Mapeo de extensión a LanguageId.
    by_extension: HashMap<String, LanguageId>,
    /// Mapeo de nombre de archivo a LanguageId.
    by_filename: HashMap<String, LanguageId>,
    /// Contador para generar IDs.
    next_id: u32,
}

impl LanguageRegistry {
    /// Crea un registro vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea un registro con lenguajes comunes pre-cargados.
    pub fn with_builtin_languages() -> Self {
        let mut registry = Self::new();

        registry.register(LanguageConfig::rust());
        registry.register(LanguageConfig::python());
        registry.register(LanguageConfig::javascript());
        registry.register(LanguageConfig::typescript());
        registry.register(LanguageConfig::markdown());
        registry.register(LanguageConfig::json());
        registry.register(LanguageConfig::toml());
        registry.register(LanguageConfig::default()); // Plain Text

        registry
    }

    /// Registra un nuevo lenguaje.
    pub fn register(&mut self, config: LanguageConfig) -> LanguageId {
        let id = LanguageId(self.next_id);
        self.next_id += 1;

        // Registrar extensiones
        for ext in &config.extensions {
            self.by_extension.insert(ext.to_lowercase(), id);
        }

        // Registrar nombres de archivo
        for filename in &config.file_names {
            self.by_filename.insert(filename.clone(), id);
        }

        let language = Language::new(id, config);
        self.languages.insert(id, language);

        id
    }

    /// Obtiene un lenguaje por ID.
    pub fn get(&self, id: LanguageId) -> Option<&Language> {
        self.languages.get(&id)
    }

    /// Detecta el lenguaje de un archivo por su ruta.
    pub fn detect_language(&self, path: &Path) -> Option<&Language> {
        // Primero intentar por nombre de archivo completo
        let filename = path.file_name()?.to_str()?;
        if let Some(&id) = self.by_filename.get(filename) {
            return self.get(id);
        }

        // Luego intentar por extensión
        let extension = path.extension()?.to_str()?.to_lowercase();
        if let Some(&id) = self.by_extension.get(&extension) {
            return self.get(id);
        }

        None
    }

    /// Busca un lenguaje por nombre.
    pub fn find_by_name(&self, name: &str) -> Option<&Language> {
        self.languages
            .values()
            .find(|lang| lang.name().eq_ignore_ascii_case(name))
    }

    /// Lista todos los lenguajes registrados.
    pub fn all_languages(&self) -> impl Iterator<Item = &Language> {
        self.languages.values()
    }

    /// Número de lenguajes registrados.
    pub fn count(&self) -> usize {
        self.languages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_languages() {
        let registry = LanguageRegistry::with_builtin_languages();
        assert!(registry.count() >= 7); // At least 7 built-in languages
    }

    #[test]
    fn test_detect_by_extension() {
        let registry = LanguageRegistry::with_builtin_languages();

        let path = Path::new("/home/user/main.rs");
        let lang = registry.detect_language(path);
        assert!(lang.is_some());
        assert_eq!(lang.unwrap().name(), "Rust");

        let path = Path::new("/home/user/script.py");
        let lang = registry.detect_language(path);
        assert!(lang.is_some());
        assert_eq!(lang.unwrap().name(), "Python");
    }

    #[test]
    fn test_detect_by_filename() {
        let registry = LanguageRegistry::with_builtin_languages();

        let path = Path::new("/home/user/project/Cargo.toml");
        let lang = registry.detect_language(path);
        assert!(lang.is_some());
        assert_eq!(lang.unwrap().name(), "TOML");
    }

    #[test]
    fn test_find_by_name() {
        let registry = LanguageRegistry::with_builtin_languages();

        let lang = registry.find_by_name("rust");
        assert!(lang.is_some());
        assert_eq!(lang.unwrap().name(), "Rust");

        let lang = registry.find_by_name("PYTHON");
        assert!(lang.is_some());
        assert_eq!(lang.unwrap().name(), "Python");
    }

    #[test]
    fn test_register_custom() {
        let mut registry = LanguageRegistry::new();

        let config = LanguageConfig {
            name: "MyLang".to_string(),
            extensions: vec!["ml".to_string()],
            ..Default::default()
        };

        let id = registry.register(config);

        let lang = registry.get(id);
        assert!(lang.is_some());
        assert_eq!(lang.unwrap().name(), "MyLang");

        let path = Path::new("test.ml");
        let detected = registry.detect_language(path);
        assert!(detected.is_some());
        assert_eq!(detected.unwrap().name(), "MyLang");
    }
}
