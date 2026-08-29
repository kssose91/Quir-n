//! # Llore Language
//!
//! Soporte de lenguajes de programación para Llore Editor.
//!
//! ## Componentes
//!
//! - `Language` - Definición de un lenguaje
//! - `LanguageRegistry` - Registro de lenguajes disponibles
//! - `Syntax` - Parsing y highlighting con tree-sitter
//!
//! ## Filosofía
//!
//! Cada lenguaje se define mediante:
//! - Nombre y extensiones de archivo
//! - Gramática tree-sitter para parsing
//! - Configuración de comentarios, indentación, etc.

mod language;
mod registry;
mod syntax;

pub use language::{Language, LanguageConfig, LanguageId};
pub use registry::LanguageRegistry;
pub use syntax::{Syntax, SyntaxNode, SyntaxParser};
