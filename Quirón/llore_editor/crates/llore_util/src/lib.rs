//! # Llore Util
//!
//! Utilidades esenciales para Llore Editor.
//!
//! ## Módulos
//!
//! - `paths` - Rutas del sistema (config, data, logs)
//! - `result_ext` - Extensiones para Result y Option
//! - `string_ext` - Extensiones para String

pub mod paths;
pub mod result_ext;
pub mod string_ext;

// Re-exports
pub use paths::LlorePaths;
pub use result_ext::ResultExt;
