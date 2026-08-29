//! # Selection
//!
//! Representa una selección de texto en el editor.

use crate::Cursor;

/// Una selección de texto.
///
/// Una selección tiene un ancla (donde empezó) y un head (donde está el cursor).
/// Si anchor == head, no hay selección (solo cursor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Ancla de la selección (punto fijo).
    pub anchor: Cursor,
    /// Cabeza de la selección (punto móvil / cursor actual).
    pub head: Cursor,
}

impl Selection {
    /// Crea una selección vacía (solo cursor) en la posición inicial.
    pub fn new() -> Self {
        Self {
            anchor: Cursor::new(),
            head: Cursor::new(),
        }
    }

    /// Crea una selección desde un cursor.
    pub fn from_cursor(cursor: Cursor) -> Self {
        Self {
            anchor: cursor.clone(),
            head: cursor,
        }
    }

    /// Crea una selección entre dos cursores.
    pub fn between(anchor: Cursor, head: Cursor) -> Self {
        Self { anchor, head }
    }

    /// Verifica si hay una selección activa (anchor != head).
    pub fn is_empty(&self) -> bool {
        self.anchor.offset == self.head.offset
    }

    /// Devuelve el offset de inicio de la selección.
    pub fn start_offset(&self) -> usize {
        self.anchor.offset.min(self.head.offset)
    }

    /// Devuelve el offset de fin de la selección.
    pub fn end_offset(&self) -> usize {
        self.anchor.offset.max(self.head.offset)
    }

    /// Devuelve el rango de bytes seleccionado.
    pub fn byte_range(&self) -> std::ops::Range<usize> {
        self.start_offset()..self.end_offset()
    }

    /// Longitud de la selección en bytes.
    pub fn len(&self) -> usize {
        self.end_offset() - self.start_offset()
    }

    /// Extrae el texto seleccionado de un string.
    pub fn selected_text<'a>(&self, text: &'a str) -> &'a str {
        let range = self.byte_range();
        if range.end <= text.len() {
            &text[range]
        } else {
            ""
        }
    }

    /// Colapsa la selección al cursor (head).
    pub fn collapse(&mut self) {
        self.anchor = self.head.clone();
    }

    /// Colapsa la selección al inicio.
    pub fn collapse_to_start(&mut self) {
        if self.anchor.offset < self.head.offset {
            self.head = self.anchor.clone();
        } else {
            self.anchor = self.head.clone();
        }
    }

    /// Colapsa la selección al final.
    pub fn collapse_to_end(&mut self) {
        if self.anchor.offset > self.head.offset {
            self.head = self.anchor.clone();
        } else {
            self.anchor = self.head.clone();
        }
    }

    /// Selecciona todo el texto.
    pub fn select_all(&mut self, text: &str) {
        self.anchor = Cursor::new();
        self.head.move_to_end(text);
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_empty() {
        let sel = Selection::new();
        assert!(sel.is_empty());
    }

    #[test]
    fn test_selection_range() {
        let anchor = Cursor::at(0, 0, 0);
        let head = Cursor::at(0, 5, 5);
        let sel = Selection::between(anchor, head);

        assert!(!sel.is_empty());
        assert_eq!(sel.start_offset(), 0);
        assert_eq!(sel.end_offset(), 5);
        assert_eq!(sel.len(), 5);
    }

    #[test]
    fn test_selected_text() {
        let anchor = Cursor::at(0, 0, 0);
        let head = Cursor::at(0, 5, 5);
        let sel = Selection::between(anchor, head);

        assert_eq!(sel.selected_text("Hello World"), "Hello");
    }

    #[test]
    fn test_reverse_selection() {
        // Head before anchor
        let anchor = Cursor::at(0, 5, 5);
        let head = Cursor::at(0, 0, 0);
        let sel = Selection::between(anchor, head);

        assert_eq!(sel.start_offset(), 0);
        assert_eq!(sel.end_offset(), 5);
    }

    #[test]
    fn test_collapse() {
        let anchor = Cursor::at(0, 0, 0);
        let head = Cursor::at(0, 5, 5);
        let mut sel = Selection::between(anchor, head);

        sel.collapse();
        assert!(sel.is_empty());
        assert_eq!(sel.head.offset, 5);
    }
}
