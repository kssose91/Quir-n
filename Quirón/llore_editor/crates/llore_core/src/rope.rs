//! # Rope - Buffer de Texto Eficiente
//!
//! Implementación de una estructura de datos Rope para edición
//! eficiente de texto. Un Rope permite inserciones y eliminaciones
//! en O(log n) en lugar de O(n) de un String normal.
//!
//! ## Uso Básico
//!
//! ```rust
//! use llore_core::Rope;
//!
//! let mut rope = Rope::new();
//! rope.push_str("Hola, ");
//! rope.push_str("mundo!");
//! assert_eq!(rope.to_string(), "Hola, mundo!");
//! ```

use std::ops::Range;

/// Un buffer de texto eficiente para edición.
///
/// Esta es una implementación simplificada inicial.
/// La implementación completa usará un árbol balanceado para O(log n).
#[derive(Clone, Default)]
pub struct Rope {
    // Implementación inicial simple - un String
    // TODO: Implementar árbol de chunks para O(log n)
    text: String,
}

impl Rope {
    /// Crea un Rope vacío.
    pub fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    /// Crea un Rope desde un string.
    pub fn from_str(s: &str) -> Self {
        Self {
            text: s.to_string(),
        }
    }

    /// Longitud en bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Verifica si está vacío.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Longitud en caracteres (code points).
    pub fn chars_len(&self) -> usize {
        self.text.chars().count()
    }

    /// Añade texto al final.
    pub fn push_str(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Inserta texto en una posición (offset en bytes).
    ///
    /// # Panics
    /// Panics si offset no está en un límite de carácter válido.
    pub fn insert(&mut self, offset: usize, s: &str) {
        assert!(
            self.text.is_char_boundary(offset),
            "offset {} is not a char boundary",
            offset
        );
        self.text.insert_str(offset, s);
    }

    /// Elimina un rango de texto (offsets en bytes).
    ///
    /// # Panics
    /// Panics si el rango no está en límites de carácter válidos.
    pub fn remove(&mut self, range: Range<usize>) {
        assert!(
            self.text.is_char_boundary(range.start),
            "range start {} is not a char boundary",
            range.start
        );
        assert!(
            self.text.is_char_boundary(range.end),
            "range end {} is not a char boundary",
            range.end
        );
        self.text.replace_range(range, "");
    }

    /// Reemplaza un rango con nuevo texto.
    pub fn replace(&mut self, range: Range<usize>, replacement: &str) {
        assert!(
            self.text.is_char_boundary(range.start),
            "range start {} is not a char boundary",
            range.start
        );
        assert!(
            self.text.is_char_boundary(range.end),
            "range end {} is not a char boundary",
            range.end
        );
        self.text.replace_range(range, replacement);
    }

    /// Obtiene una porción del texto.
    pub fn slice(&self, range: Range<usize>) -> &str {
        &self.text[range]
    }

    /// Itera sobre las líneas.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.text.lines()
    }

    /// Cuenta el número de líneas.
    pub fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }

    /// Convierte a String.
    pub fn to_string(&self) -> String {
        self.text.clone()
    }

    /// Referencia al texto interno (para debugging).
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Debug for Rope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let preview = if self.text.len() > 50 {
            format!("{}...", &self.text[..50])
        } else {
            self.text.clone()
        };
        write!(f, "Rope({:?})", preview)
    }
}

impl From<&str> for Rope {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl From<String> for Rope {
    fn from(s: String) -> Self {
        Self { text: s }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut rope = Rope::new();
        assert!(rope.is_empty());

        rope.push_str("Hello");
        assert_eq!(rope.len(), 5);
        assert_eq!(rope.to_string(), "Hello");

        rope.push_str(", World!");
        assert_eq!(rope.to_string(), "Hello, World!");
    }

    #[test]
    fn test_insert() {
        let mut rope = Rope::from_str("Hello World");
        rope.insert(5, ",");
        assert_eq!(rope.to_string(), "Hello, World");
    }

    #[test]
    fn test_remove() {
        let mut rope = Rope::from_str("Hello, World!");
        rope.remove(5..7); // Remove ", "
        assert_eq!(rope.to_string(), "HelloWorld!");
    }

    #[test]
    fn test_replace() {
        let mut rope = Rope::from_str("Hello, World!");
        rope.replace(7..12, "Llore");
        assert_eq!(rope.to_string(), "Hello, Llore!");
    }

    #[test]
    fn test_lines() {
        let rope = Rope::from_str("Line 1\nLine 2\nLine 3");
        let lines: Vec<_> = rope.lines().collect();
        assert_eq!(lines, vec!["Line 1", "Line 2", "Line 3"]);
        assert_eq!(rope.line_count(), 3);
    }

    #[test]
    fn test_unicode() {
        let mut rope = Rope::from_str("Hola 🌍");
        assert_eq!(rope.chars_len(), 6); // H-o-l-a-space-emoji
        rope.push_str(" mundo");
        assert_eq!(rope.to_string(), "Hola 🌍 mundo");
    }
}
