//! # Cursor
//!
//! Representa la posición del cursor en el editor.

/// Forma visual del cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// Línea vertical (modo inserción).
    #[default]
    Bar,
    /// Bloque completo (modo normal vim).
    Block,
    /// Subrayado.
    Underline,
}

/// Posición del cursor en el documento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    /// Línea (0-indexed).
    pub line: usize,
    /// Columna (0-indexed, en caracteres).
    pub column: usize,
    /// Offset en bytes desde el inicio del documento.
    pub offset: usize,
    /// Columna "preferida" para movimiento vertical.
    /// Al moverse arriba/abajo, el cursor intenta mantener esta columna.
    preferred_column: Option<usize>,
}

impl Cursor {
    /// Crea un cursor en la posición inicial (0, 0).
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea un cursor en una posición específica.
    pub fn at(line: usize, column: usize, offset: usize) -> Self {
        Self {
            line,
            column,
            offset,
            preferred_column: None,
        }
    }

    /// Mueve el cursor a la derecha.
    pub fn move_right(&mut self, text: &str) {
        if self.offset < text.len() {
            // Encontrar el siguiente límite de carácter
            let remaining = &text[self.offset..];
            if let Some(c) = remaining.chars().next() {
                self.offset += c.len_utf8();
                if c == '\n' {
                    self.line += 1;
                    self.column = 0;
                } else {
                    self.column += 1;
                }
                self.preferred_column = None;
            }
        }
    }

    /// Mueve el cursor a la izquierda.
    pub fn move_left(&mut self, text: &str) {
        if self.offset > 0 {
            // Encontrar el límite de carácter anterior
            let before = &text[..self.offset];
            if let Some(c) = before.chars().last() {
                self.offset -= c.len_utf8();
                if c == '\n' {
                    // Calcular nueva columna al final de la línea anterior
                    self.line = self.line.saturating_sub(1);
                    self.column = self.line_length(&text[..self.offset]);
                } else {
                    self.column = self.column.saturating_sub(1);
                }
                self.preferred_column = None;
            }
        }
    }

    /// Mueve el cursor una línea arriba.
    pub fn move_up(&mut self, text: &str) {
        if self.line > 0 {
            let target_col = self.preferred_column.unwrap_or(self.column);
            self.line -= 1;
            self.go_to_line_column(text, self.line, target_col);
            self.preferred_column = Some(target_col);
        }
    }

    /// Mueve el cursor una línea abajo.
    pub fn move_down(&mut self, text: &str) {
        let line_count = text.lines().count();
        if self.line + 1 < line_count {
            let target_col = self.preferred_column.unwrap_or(self.column);
            self.line += 1;
            self.go_to_line_column(text, self.line, target_col);
            self.preferred_column = Some(target_col);
        }
    }

    /// Mueve al inicio de la línea.
    pub fn move_to_line_start(&mut self, text: &str) {
        self.go_to_line_column(text, self.line, 0);
        self.preferred_column = None;
    }

    /// Mueve al final de la línea.
    pub fn move_to_line_end(&mut self, text: &str) {
        let line_len = self.current_line_length(text);
        self.column = line_len;
        self.offset = self.calculate_offset(text);
        self.preferred_column = None;
    }

    /// Mueve al inicio del documento.
    pub fn move_to_start(&mut self) {
        self.line = 0;
        self.column = 0;
        self.offset = 0;
        self.preferred_column = None;
    }

    /// Mueve al final del documento.
    pub fn move_to_end(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        self.line = lines.len().saturating_sub(1);
        self.column = lines.last().map(|l| l.chars().count()).unwrap_or(0);
        self.offset = text.len();
        self.preferred_column = None;
    }

    /// Calcula la longitud de la línea actual en caracteres.
    fn current_line_length(&self, text: &str) -> usize {
        text.lines()
            .nth(self.line)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    /// Calcula la longitud de una porción de texto hasta el final.
    fn line_length(&self, text: &str) -> usize {
        text.lines().last().map(|l| l.chars().count()).unwrap_or(0)
    }

    /// Va a una línea y columna específica.
    fn go_to_line_column(&mut self, text: &str, line: usize, column: usize) {
        self.line = line;
        let actual_line_length = text
            .lines()
            .nth(line)
            .map(|l| l.chars().count())
            .unwrap_or(0);
        self.column = column.min(actual_line_length);
        self.offset = self.calculate_offset(text);
    }

    /// Calcula el offset en bytes para la posición actual.
    fn calculate_offset(&self, text: &str) -> usize {
        let mut offset = 0;
        for (i, line) in text.lines().enumerate() {
            if i == self.line {
                for (col, c) in line.chars().enumerate() {
                    if col == self.column {
                        break;
                    }
                    offset += c.len_utf8();
                }
                return offset;
            }
            offset += line.len() + 1; // +1 for newline
        }
        offset.min(text.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_new() {
        let cursor = Cursor::new();
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 0);
        assert_eq!(cursor.offset, 0);
    }

    #[test]
    fn test_move_right() {
        let mut cursor = Cursor::new();
        let text = "Hello";

        cursor.move_right(text);
        assert_eq!(cursor.column, 1);
        assert_eq!(cursor.offset, 1);

        cursor.move_right(text);
        cursor.move_right(text);
        assert_eq!(cursor.column, 3);
    }

    #[test]
    fn test_move_right_newline() {
        let mut cursor = Cursor::at(0, 5, 5);
        let text = "Hello\nWorld";

        cursor.move_right(text); // Move past 'o' to newline
        assert_eq!(cursor.line, 1);
        assert_eq!(cursor.column, 0);
    }

    #[test]
    fn test_move_left() {
        let mut cursor = Cursor::at(0, 3, 3);
        let text = "Hello";

        cursor.move_left(text);
        assert_eq!(cursor.column, 2);
        assert_eq!(cursor.offset, 2);
    }

    #[test]
    fn test_move_up_down() {
        let mut cursor = Cursor::at(1, 3, 9); // "World" line, column 3
        let text = "Hello\nWorld\nTest";

        cursor.move_up(text);
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 3); // Maintains column

        cursor.move_down(text);
        assert_eq!(cursor.line, 1);
    }

    #[test]
    fn test_move_to_line_start_end() {
        let mut cursor = Cursor::at(0, 3, 3);
        let text = "Hello World";

        cursor.move_to_line_start(text);
        assert_eq!(cursor.column, 0);

        cursor.move_to_line_end(text);
        assert_eq!(cursor.column, 11);
    }
}
