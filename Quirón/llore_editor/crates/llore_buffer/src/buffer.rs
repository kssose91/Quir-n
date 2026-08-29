//! # Buffer de Edición
//!
//! Combina Rope + History para un buffer de texto completo.

use crate::{Edit, EditKind, History};
use llore_core::{Clock, Rope};
use std::ops::Range;
use std::path::PathBuf;

/// Buffer de texto con soporte para undo/redo.
#[derive(Debug)]
pub struct Buffer {
    /// Contenido del buffer.
    content: Rope,
    /// Historial de ediciones.
    history: History,
    /// Reloj lógico para timestamps.
    clock: Clock,
    /// Ruta del archivo (si está asociado a uno).
    file_path: Option<PathBuf>,
    /// Nombre del buffer (para buffers sin archivo).
    name: String,
}

impl Buffer {
    /// Crea un buffer vacío.
    pub fn new() -> Self {
        Self {
            content: Rope::new(),
            history: History::new(),
            clock: Clock::default_local(),
            file_path: None,
            name: String::from("untitled"),
        }
    }

    /// Crea un buffer con contenido inicial.
    pub fn from_str(text: &str) -> Self {
        let mut buffer = Self::new();
        buffer.content = Rope::from_str(text);
        buffer
    }

    /// Crea un buffer desde un archivo.
    pub fn from_file(path: PathBuf, content: &str) -> Self {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_string());

        Self {
            content: Rope::from_str(content),
            history: History::new(),
            clock: Clock::default_local(),
            file_path: Some(path),
            name,
        }
    }

    /// Devuelve el contenido como String.
    pub fn text(&self) -> String {
        self.content.to_string()
    }

    /// Devuelve una referencia al contenido.
    pub fn as_str(&self) -> &str {
        self.content.as_str()
    }

    /// Longitud en bytes.
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Verifica si está vacío.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Número de líneas.
    pub fn line_count(&self) -> usize {
        self.content.line_count()
    }

    /// Nombre del buffer.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Ruta del archivo, si existe.
    pub fn file_path(&self) -> Option<&PathBuf> {
        self.file_path.as_ref()
    }

    // === Operaciones de Edición ===

    /// Inserta texto en una posición.
    pub fn insert(&mut self, offset: usize, text: &str) {
        if text.is_empty() {
            return;
        }

        let timestamp = self.clock.tick();
        let edit = Edit::insert(offset, text, timestamp);

        self.content.insert(offset, text);
        self.history.push(edit);
    }

    /// Elimina un rango de texto.
    pub fn delete(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }

        let old_text = self.content.slice(range.clone()).to_string();
        let timestamp = self.clock.tick();
        let edit = Edit::delete(range.clone(), &old_text, timestamp);

        self.content.remove(range);
        self.history.push(edit);
    }

    /// Reemplaza un rango con nuevo texto.
    pub fn replace(&mut self, range: Range<usize>, new_text: &str) {
        let old_text = if range.is_empty() {
            String::new()
        } else {
            self.content.slice(range.clone()).to_string()
        };

        let timestamp = self.clock.tick();
        let edit = Edit::replace(range.clone(), &old_text, new_text, timestamp);

        self.content.replace(range, new_text);
        self.history.push(edit);
    }

    // === Undo/Redo ===

    /// Deshace la última edición.
    pub fn undo(&mut self) -> bool {
        if let Some(edit) = self.history.pop_for_undo() {
            self.apply_inverse(&edit);
            true
        } else {
            false
        }
    }

    /// Rehace la última edición deshecha.
    pub fn redo(&mut self) -> bool {
        if let Some(edit) = self.history.pop_for_redo() {
            self.apply_forward(&edit);
            true
        } else {
            false
        }
    }

    /// Aplica una edición en reversa (para undo).
    fn apply_inverse(&mut self, edit: &Edit) {
        match edit.kind {
            EditKind::Insert => {
                // Undo insert = delete
                let end = edit.range.start + edit.new_text.len();
                self.content.remove(edit.range.start..end);
            }
            EditKind::Delete => {
                // Undo delete = insert
                self.content.insert(edit.range.start, &edit.old_text);
            }
            EditKind::Replace => {
                // Undo replace = replace back
                let end = edit.range.start + edit.new_text.len();
                self.content.replace(edit.range.start..end, &edit.old_text);
            }
        }
    }

    /// Aplica una edición hacia adelante (para redo).
    fn apply_forward(&mut self, edit: &Edit) {
        match edit.kind {
            EditKind::Insert => {
                self.content.insert(edit.range.start, &edit.new_text);
            }
            EditKind::Delete => {
                self.content.remove(edit.range.clone());
            }
            EditKind::Replace => {
                self.content.replace(edit.range.clone(), &edit.new_text);
            }
        }
    }

    /// Verifica si puede deshacer.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Verifica si puede rehacer.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    // === Estado ===

    /// Verifica si hay cambios sin guardar.
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Marca el buffer como guardado.
    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_text() {
        let mut buffer = Buffer::new();
        buffer.insert(0, "Hello");
        assert_eq!(buffer.text(), "Hello");

        buffer.insert(5, " World");
        assert_eq!(buffer.text(), "Hello World");
    }

    #[test]
    fn test_delete() {
        let mut buffer = Buffer::from_str("Hello World");
        buffer.delete(5..6); // Delete space
        assert_eq!(buffer.text(), "HelloWorld");
    }

    #[test]
    fn test_replace() {
        let mut buffer = Buffer::from_str("Hello World");
        buffer.replace(6..11, "Llore");
        assert_eq!(buffer.text(), "Hello Llore");
    }

    #[test]
    fn test_undo_redo() {
        let mut buffer = Buffer::new();

        buffer.insert(0, "Hello");
        buffer.insert(5, " World");
        assert_eq!(buffer.text(), "Hello World");

        assert!(buffer.undo());
        assert_eq!(buffer.text(), "Hello");

        assert!(buffer.undo());
        assert_eq!(buffer.text(), "");

        assert!(buffer.redo());
        assert_eq!(buffer.text(), "Hello");

        assert!(buffer.redo());
        assert_eq!(buffer.text(), "Hello World");
    }

    #[test]
    fn test_undo_delete() {
        let mut buffer = Buffer::from_str("Hello World");
        buffer.delete(5..11); // Delete " World"
        assert_eq!(buffer.text(), "Hello");

        buffer.undo();
        assert_eq!(buffer.text(), "Hello World");
    }

    #[test]
    fn test_modified_state() {
        let mut buffer = Buffer::new();
        // New buffer is technically "modified" (never saved)
        // Mark as saved first to establish baseline
        buffer.mark_saved();
        assert!(!buffer.is_modified());

        buffer.insert(0, "Hello");
        assert!(buffer.is_modified());

        buffer.mark_saved();
        assert!(!buffer.is_modified());

        buffer.insert(5, "!");
        assert!(buffer.is_modified());
    }

    #[test]
    fn test_from_file() {
        let buffer = Buffer::from_file(PathBuf::from("/home/user/test.rs"), "fn main() {}");
        assert_eq!(buffer.name(), "test.rs");
        assert_eq!(buffer.text(), "fn main() {}");
    }
}
