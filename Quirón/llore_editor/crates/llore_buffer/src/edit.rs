//! # Ediciones
//!
//! Representa una operación de edición en el buffer.

use llore_core::Lamport;
use std::ops::Range;

/// Tipo de edición.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditKind {
    /// Inserción de texto.
    Insert,
    /// Eliminación de texto.
    Delete,
    /// Reemplazo (delete + insert combinados).
    Replace,
}

/// Una edición atómica en el buffer.
#[derive(Debug, Clone)]
pub struct Edit {
    /// Tipo de edición.
    pub kind: EditKind,
    /// Rango afectado ANTES de la edición (en bytes).
    pub range: Range<usize>,
    /// Texto que había en el rango (para undo).
    pub old_text: String,
    /// Texto nuevo insertado (para redo).
    pub new_text: String,
    /// Timestamp lógico de cuando se hizo la edición.
    pub timestamp: Lamport,
}

impl Edit {
    /// Crea una edición de inserción.
    pub fn insert(offset: usize, text: &str, timestamp: Lamport) -> Self {
        Self {
            kind: EditKind::Insert,
            range: offset..offset,
            old_text: String::new(),
            new_text: text.to_string(),
            timestamp,
        }
    }

    /// Crea una edición de eliminación.
    pub fn delete(range: Range<usize>, old_text: &str, timestamp: Lamport) -> Self {
        Self {
            kind: EditKind::Delete,
            range,
            old_text: old_text.to_string(),
            new_text: String::new(),
            timestamp,
        }
    }

    /// Crea una edición de reemplazo.
    pub fn replace(
        range: Range<usize>,
        old_text: &str,
        new_text: &str,
        timestamp: Lamport,
    ) -> Self {
        Self {
            kind: EditKind::Replace,
            range,
            old_text: old_text.to_string(),
            new_text: new_text.to_string(),
            timestamp,
        }
    }

    /// Calcula el cambio en longitud del buffer por esta edición.
    pub fn length_delta(&self) -> isize {
        self.new_text.len() as isize - self.old_text.len() as isize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llore_core::ReplicaId;

    #[test]
    fn test_edit_insert() {
        let ts = Lamport::new(1, ReplicaId::DEFAULT);
        let edit = Edit::insert(5, "Hello", ts);
        assert_eq!(edit.kind, EditKind::Insert);
        assert_eq!(edit.range, 5..5);
        assert_eq!(edit.new_text, "Hello");
        assert_eq!(edit.length_delta(), 5);
    }

    #[test]
    fn test_edit_delete() {
        let ts = Lamport::new(2, ReplicaId::DEFAULT);
        let edit = Edit::delete(0..5, "Hello", ts);
        assert_eq!(edit.kind, EditKind::Delete);
        assert_eq!(edit.old_text, "Hello");
        assert_eq!(edit.length_delta(), -5);
    }

    #[test]
    fn test_edit_replace() {
        let ts = Lamport::new(3, ReplicaId::DEFAULT);
        let edit = Edit::replace(0..5, "Hello", "Hi", ts);
        assert_eq!(edit.kind, EditKind::Replace);
        assert_eq!(edit.length_delta(), -3); // "Hi" is 3 chars shorter than "Hello"
    }
}
