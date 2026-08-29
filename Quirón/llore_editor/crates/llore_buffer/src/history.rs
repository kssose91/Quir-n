//! # Historial de Ediciones
//!
//! Implementa undo/redo mediante dos stacks de ediciones.

use crate::Edit;

/// Historial de undo/redo.
#[derive(Debug, Default)]
pub struct History {
    /// Stack de ediciones que se pueden deshacer.
    undo_stack: Vec<Edit>,
    /// Stack de ediciones que se pueden rehacer.
    redo_stack: Vec<Edit>,
    /// Índice del último estado guardado (para detectar cambios).
    saved_index: Option<usize>,
}

impl History {
    /// Crea un historial vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade una edición al historial.
    /// Limpia el stack de redo ya que la línea temporal cambió.
    pub fn push(&mut self, edit: Edit) {
        self.undo_stack.push(edit);
        self.redo_stack.clear();
    }

    /// Saca la última edición para undo.
    /// La mueve al stack de redo.
    pub fn pop_for_undo(&mut self) -> Option<Edit> {
        if let Some(edit) = self.undo_stack.pop() {
            self.redo_stack.push(edit.clone());
            Some(edit)
        } else {
            None
        }
    }

    /// Saca la última edición para redo.
    /// La mueve de vuelta al stack de undo.
    pub fn pop_for_redo(&mut self) -> Option<Edit> {
        if let Some(edit) = self.redo_stack.pop() {
            self.undo_stack.push(edit.clone());
            Some(edit)
        } else {
            None
        }
    }

    /// Verifica si hay ediciones para deshacer.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Verifica si hay ediciones para rehacer.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Número de ediciones en el historial de undo.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Número de ediciones en el historial de redo.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Marca el estado actual como guardado.
    pub fn mark_saved(&mut self) {
        self.saved_index = Some(self.undo_stack.len());
    }

    /// Verifica si hay cambios desde el último guardado.
    pub fn is_modified(&self) -> bool {
        self.saved_index != Some(self.undo_stack.len())
    }

    /// Limpia todo el historial.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.saved_index = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llore_core::{Lamport, ReplicaId};

    fn make_edit(n: u32) -> Edit {
        Edit::insert(0, "test", Lamport::new(n, ReplicaId::DEFAULT))
    }

    #[test]
    fn test_undo_redo() {
        let mut history = History::new();

        history.push(make_edit(1));
        history.push(make_edit(2));
        assert_eq!(history.undo_count(), 2);

        // Undo
        let edit = history.pop_for_undo();
        assert!(edit.is_some());
        assert_eq!(history.undo_count(), 1);
        assert_eq!(history.redo_count(), 1);

        // Redo
        let edit = history.pop_for_redo();
        assert!(edit.is_some());
        assert_eq!(history.undo_count(), 2);
        assert_eq!(history.redo_count(), 0);
    }

    #[test]
    fn test_new_edit_clears_redo() {
        let mut history = History::new();

        history.push(make_edit(1));
        history.push(make_edit(2));
        history.pop_for_undo(); // redo_count = 1

        history.push(make_edit(3)); // Should clear redo
        assert_eq!(history.redo_count(), 0);
    }

    #[test]
    fn test_save_state() {
        let mut history = History::new();

        history.push(make_edit(1));
        assert!(history.is_modified());

        history.mark_saved();
        assert!(!history.is_modified());

        history.push(make_edit(2));
        assert!(history.is_modified());

        history.pop_for_undo();
        assert!(!history.is_modified());
    }
}
