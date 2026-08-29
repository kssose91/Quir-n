//! # Llore Editor
//!
//! Componente de edición de texto para Llore.
//!
//! Este crate proporciona:
//! - `Editor` - El editor de texto principal
//! - `Cursor` - Posición y movimiento del cursor
//! - `Selection` - Selección de texto
//! - `DisplayMap` - Mapeo de texto a pantalla
//!
//! ## Filosofía
//!
//! Diseñado desde cero con énfasis en:
//! - Operaciones atómicas y seguras
//! - Sin estado global ni side effects ocultos
//! - Extensible pero simple

mod cursor;
mod display;
mod editor;
mod selection;

pub use cursor::{Cursor, CursorShape};
pub use display::DisplayMap;
pub use editor::{Editor, EditorMode};
pub use selection::Selection;
