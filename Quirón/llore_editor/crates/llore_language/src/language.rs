//! # Definición de Lenguaje
//!
//! Estructura que define las propiedades de un lenguaje de programación.

use serde::{Deserialize, Serialize};

/// Configuración de un lenguaje de programación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageConfig {
    /// Nombre del lenguaje (e.g., "Rust", "Python").
    pub name: String,

    /// Extensiones de archivo (e.g., ["rs"], ["py", "pyw"]).
    pub extensions: Vec<String>,

    /// Nombres de archivo que identifican este lenguaje (e.g., ["Cargo.toml"]).
    pub file_names: Vec<String>,

    /// Prefijo de comentario de línea (e.g., "//", "#").
    pub line_comment: Option<String>,

    /// Delimitadores de comentario de bloque (e.g., ("/*", "*/")).
    pub block_comment: Option<(String, String)>,

    /// Caracteres que incrementan indentación.
    pub indent_on_open: Vec<char>,

    /// Caracteres que decrementan indentación.
    pub outdent_on_close: Vec<char>,

    /// Ancho de indentación por defecto.
    pub indent_size: usize,

    /// Usar tabs en lugar de espacios.
    pub use_tabs: bool,
}

impl Default for LanguageConfig {
    fn default() -> Self {
        Self {
            name: "Plain Text".to_string(),
            extensions: vec![],
            file_names: vec![],
            line_comment: None,
            block_comment: None,
            indent_on_open: vec!['{', '[', '('],
            outdent_on_close: vec!['}', ']', ')'],
            indent_size: 4,
            use_tabs: false,
        }
    }
}

impl LanguageConfig {
    /// Crea configuración para Rust.
    pub fn rust() -> Self {
        Self {
            name: "Rust".to_string(),
            extensions: vec!["rs".to_string()],
            file_names: vec![],
            line_comment: Some("//".to_string()),
            block_comment: Some(("/*".to_string(), "*/".to_string())),
            ..Default::default()
        }
    }

    /// Crea configuración para Python.
    pub fn python() -> Self {
        Self {
            name: "Python".to_string(),
            extensions: vec!["py".to_string(), "pyw".to_string()],
            file_names: vec![],
            line_comment: Some("#".to_string()),
            block_comment: None,
            indent_on_open: vec![':', '[', '(', '{'],
            ..Default::default()
        }
    }

    /// Crea configuración para JavaScript.
    pub fn javascript() -> Self {
        Self {
            name: "JavaScript".to_string(),
            extensions: vec!["js".to_string(), "mjs".to_string()],
            file_names: vec![],
            line_comment: Some("//".to_string()),
            block_comment: Some(("/*".to_string(), "*/".to_string())),
            ..Default::default()
        }
    }

    /// Crea configuración para TypeScript.
    pub fn typescript() -> Self {
        Self {
            name: "TypeScript".to_string(),
            extensions: vec!["ts".to_string(), "tsx".to_string()],
            file_names: vec![],
            line_comment: Some("//".to_string()),
            block_comment: Some(("/*".to_string(), "*/".to_string())),
            ..Default::default()
        }
    }

    /// Crea configuración para Markdown.
    pub fn markdown() -> Self {
        Self {
            name: "Markdown".to_string(),
            extensions: vec!["md".to_string(), "markdown".to_string()],
            file_names: vec!["README".to_string()],
            line_comment: None,
            block_comment: Some(("<!--".to_string(), "-->".to_string())),
            indent_size: 2,
            ..Default::default()
        }
    }

    /// Crea configuración para JSON.
    pub fn json() -> Self {
        Self {
            name: "JSON".to_string(),
            extensions: vec!["json".to_string()],
            file_names: vec![],
            line_comment: None,
            block_comment: None,
            indent_size: 2,
            ..Default::default()
        }
    }

    /// Crea configuración para TOML.
    pub fn toml() -> Self {
        Self {
            name: "TOML".to_string(),
            extensions: vec!["toml".to_string()],
            file_names: vec!["Cargo.toml".to_string()],
            line_comment: Some("#".to_string()),
            block_comment: None,
            indent_size: 2,
            ..Default::default()
        }
    }
}

/// Un lenguaje completo con configuración y potencial gramática.
#[derive(Debug, Clone)]
pub struct Language {
    /// Configuración del lenguaje.
    pub config: LanguageConfig,
    /// ID único del lenguaje.
    pub id: LanguageId,
}

/// Identificador único de lenguaje.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LanguageId(pub u32);

impl Language {
    /// Crea un nuevo lenguaje con la configuración dada.
    pub fn new(id: LanguageId, config: LanguageConfig) -> Self {
        Self { config, id }
    }

    /// Nombre del lenguaje.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Verifica si una extensión pertenece a este lenguaje.
    pub fn matches_extension(&self, ext: &str) -> bool {
        self.config
            .extensions
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    }

    /// Verifica si un nombre de archivo pertenece a este lenguaje.
    pub fn matches_filename(&self, filename: &str) -> bool {
        self.config.file_names.iter().any(|f| f == filename)
    }

    /// String de indentación según configuración.
    pub fn indent_string(&self) -> String {
        if self.config.use_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.config.indent_size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_config_defaults() {
        let config = LanguageConfig::default();
        assert_eq!(config.name, "Plain Text");
        assert_eq!(config.indent_size, 4);
    }

    #[test]
    fn test_rust_config() {
        let config = LanguageConfig::rust();
        assert_eq!(config.name, "Rust");
        assert!(config.extensions.contains(&"rs".to_string()));
        assert_eq!(config.line_comment, Some("//".to_string()));
    }

    #[test]
    fn test_language_matching() {
        let lang = Language::new(LanguageId(1), LanguageConfig::rust());
        assert!(lang.matches_extension("rs"));
        assert!(lang.matches_extension("RS")); // Case insensitive
        assert!(!lang.matches_extension("py"));
    }

    #[test]
    fn test_filename_matching() {
        let lang = Language::new(LanguageId(1), LanguageConfig::toml());
        assert!(lang.matches_filename("Cargo.toml"));
        assert!(!lang.matches_filename("package.json"));
    }

    #[test]
    fn test_indent_string() {
        let mut config = LanguageConfig::default();
        config.indent_size = 2;
        let lang = Language::new(LanguageId(1), config);
        assert_eq!(lang.indent_string(), "  ");

        let mut config_tabs = LanguageConfig::default();
        config_tabs.use_tabs = true;
        let lang_tabs = Language::new(LanguageId(2), config_tabs);
        assert_eq!(lang_tabs.indent_string(), "\t");
    }
}
