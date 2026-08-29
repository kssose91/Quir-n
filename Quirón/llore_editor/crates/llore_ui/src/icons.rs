//! # Icons
//!
//! Iconos de la interfaz, tomados de la fuente Lucide empotrada en el binario.
//!
//! Los glifos viven en el área de uso privado de Unicode, de modo que su punto
//! de código no significa nada por sí solo. Se nombran aquí para que el resto
//! del código no arrastre literales opacos, y se dibujan con
//! [`crate::text::TextSystem::create_icon_buffer`].
//!
//! Los valores proceden del mapa oficial de Lucide 0.469.0. Si se actualiza la
//! fuente hay que regenerarlos: un punto de código obsoleto no falla, pinta otro
//! icono.

/// Panel de exploración de archivos.
pub const EXPLORER: char = '\u{e0d3}';
/// Búsqueda de texto en el proyecto.
pub const SEARCH: char = '\u{e154}';
/// Control de versiones.
pub const GIT: char = '\u{e0e5}';
/// Diagnósticos y errores.
pub const PROBLEMS: char = '\u{e192}';
/// Esquema de símbolos del archivo.
pub const OUTLINE: char = '\u{e40c}';
/// Apariencia y tema.
pub const APPEARANCE: char = '\u{e1dc}';
/// Estado de seguridad y confinamiento.
pub const SECURITY: char = '\u{e15b}';
/// Paleta de comandos.
pub const COMMANDS: char = '\u{e184}';

/// Crear archivo.
pub const NEW_FILE: char = '\u{e0cd}';
/// Crear carpeta.
pub const NEW_FOLDER: char = '\u{e0de}';
/// Recargar el árbol.
pub const REFRESH: char = '\u{e148}';
/// Abrir carpeta.
pub const FOLDER_OPEN: char = '\u{e246}';

/// Mensaje del usuario en el chat.
pub const USER: char = '\u{e19e}';
/// Mensaje del modelo en el chat.
pub const ASSISTANT: char = '\u{e416}';

/// Almacén vectorial.
pub const DATABASE: char = '\u{e0b1}';
/// Grafo de dependencias.
pub const NETWORK: char = '\u{e128}';
/// Conexión con el modelo de lenguaje.
pub const PLUG: char = '\u{e382}';

/// Devuelve el icono como cadena, listo para pasar al sistema de texto.
pub fn glyph(icon: char) -> String {
    icon.to_string()
}
