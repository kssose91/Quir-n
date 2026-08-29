//! # Editor Principal
//!
//! El componente central de edición de texto.

use crate::{Cursor, CursorShape, DisplayMap, Selection};
use llore_buffer::Buffer;
use llore_language::{LanguageId, LanguageRegistry};
use std::path::PathBuf;

/// Estado del editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    /// Modo normal (navegación).
    Normal,
    /// Modo inserción.
    Insert,
    /// Modo visual (selección).
    Visual,
    /// Modo línea de comandos.
    Command,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::Insert
    }
}

/// El editor de texto principal.
pub struct Editor {
    /// Buffer de texto subyacente.
    buffer: Buffer,
    /// Selección actual.
    selection: Selection,
    /// Display map para rendering.
    display_map: DisplayMap,
    /// Modo de edición actual.
    mode: EditorMode,
    /// Forma del cursor.
    cursor_shape: CursorShape,
    /// ID del lenguaje detectado.
    language_id: Option<LanguageId>,
    /// Scroll vertical (primera línea visible).
    scroll_top: usize,
    /// Scroll horizontal.
    scroll_left: usize,
}

impl Editor {
    /// Crea un editor vacío.
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            selection: Selection::new(),
            display_map: DisplayMap::new(),
            mode: EditorMode::Insert,
            cursor_shape: CursorShape::Bar,
            language_id: None,
            scroll_top: 0,
            scroll_left: 0,
        }
    }

    /// Crea un editor con contenido inicial.
    pub fn with_text(text: &str) -> Self {
        let mut editor = Self::new();
        editor.buffer = Buffer::from_str(text);
        editor.display_map.update(text);
        editor
    }

    /// Abre un archivo en el editor.
    pub fn open_file(path: PathBuf, content: &str, registry: &LanguageRegistry) -> Self {
        let mut editor = Self::new();
        editor.buffer = Buffer::from_file(path.clone(), content);
        editor.display_map.update(content);

        // Detectar lenguaje
        if let Some(lang) = registry.detect_language(&path) {
            editor.language_id = Some(lang.id);
        }

        editor
    }

    // === Acceso a estado ===

    /// Texto completo del buffer.
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Cursor actual.
    pub fn cursor(&self) -> &Cursor {
        &self.selection.head
    }

    /// Selección actual.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Modo de edición.
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    /// Forma del cursor.
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// Nombre del buffer.
    pub fn buffer_name(&self) -> &str {
        self.buffer.name()
    }

    /// Ruta del archivo, si existe.
    pub fn file_path(&self) -> Option<&PathBuf> {
        self.buffer.file_path()
    }

    /// Si hay cambios sin guardar.
    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }

    /// Marca el estado actual como guardado.
    pub fn mark_saved(&mut self) {
        self.buffer.mark_saved();
    }

    /// Número de líneas en el buffer.
    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Display map para rendering.
    pub fn display_map(&self) -> &DisplayMap {
        &self.display_map
    }

    /// Scroll vertical actual (línea inicial visible).
    pub fn scroll_top(&self) -> usize {
        self.scroll_top
    }

    /// Scroll horizontal actual (columna inicial visible).
    pub fn scroll_left(&self) -> usize {
        self.scroll_left
    }

    /// Define explícitamente la primera línea visible.
    pub fn set_scroll_top(&mut self, line: usize) {
        let max_scroll = self.line_count().saturating_sub(1);
        self.scroll_top = line.min(max_scroll);
    }

    /// Posiciona el cursor en una línea/columna concretas (clamp seguro).
    pub fn set_cursor_line_column(&mut self, line: usize, column: usize) {
        let text = self.buffer.text();
        self.selection.head = Self::cursor_from_line_column(&text, line, column);
        self.selection.collapse();
        self.ensure_cursor_visible();
    }

    /// Define una selección explícita entre dos posiciones línea/columna.
    pub fn set_selection_line_columns(
        &mut self,
        anchor_line: usize,
        anchor_column: usize,
        head_line: usize,
        head_column: usize,
    ) {
        let text = self.buffer.text();
        let anchor = Self::cursor_from_line_column(&text, anchor_line, anchor_column);
        let head = Self::cursor_from_line_column(&text, head_line, head_column);
        self.selection = Selection::between(anchor, head);
        self.ensure_cursor_visible();
    }

    /// Selecciona la palabra en la posición dada (doble click).
    pub fn select_word_at(&mut self, line: usize, column: usize) {
        let text = self.buffer.text();
        let lines: Vec<&str> = text.split('\n').collect();
        let max_line = lines.len().saturating_sub(1);
        let line = line.min(max_line);
        let line_text = lines.get(line).copied().unwrap_or("");
        let line_chars: Vec<char> = line_text.chars().collect();

        if line_chars.is_empty() {
            self.set_cursor_line_column(line, 0);
            return;
        }

        let line_len = line_chars.len();
        let clamped_col = column.min(line_len);
        let probe_col = if clamped_col == line_len {
            clamped_col.saturating_sub(1)
        } else {
            clamped_col
        };

        let ch = line_chars.get(probe_col).copied().unwrap_or(' ');
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        if ch.is_whitespace() {
            self.set_cursor_line_column(line, clamped_col);
            return;
        }

        let mut start = probe_col;
        let mut end = probe_col + 1;

        if is_word_char(ch) {
            while start > 0 && is_word_char(line_chars[start - 1]) {
                start -= 1;
            }
            while end < line_len && is_word_char(line_chars[end]) {
                end += 1;
            }
        }

        self.set_selection_line_columns(line, start, line, end);
    }

    /// Selecciona la línea completa en la posición dada (triple click).
    pub fn select_line_at(&mut self, line: usize) {
        let text = self.buffer.text();
        let lines: Vec<&str> = text.split('\n').collect();
        let max_line = lines.len().saturating_sub(1);
        let line = line.min(max_line);
        let line_len = lines.get(line).map(|l| l.chars().count()).unwrap_or(0);
        self.set_selection_line_columns(line, 0, line, line_len);
    }

    /// Ajusta scroll vertical por líneas (positivo baja, negativo sube).
    pub fn scroll_lines(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }

        let max_scroll = self.line_count().saturating_sub(1);
        self.scroll_top = if delta_lines > 0 {
            self.scroll_top
                .saturating_add(delta_lines as usize)
                .min(max_scroll)
        } else {
            self.scroll_top
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
    }

    // === Movimiento de cursor ===

    /// Mueve el cursor derecha.
    pub fn move_right(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_right(&text);
        self.selection.collapse();
    }

    /// Mueve el cursor izquierda.
    pub fn move_left(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_left(&text);
        self.selection.collapse();
    }

    /// Mueve el cursor arriba.
    pub fn move_up(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_up(&text);
        self.selection.collapse();
        self.ensure_cursor_visible();
    }

    /// Mueve el cursor abajo.
    pub fn move_down(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_down(&text);
        self.selection.collapse();
        self.ensure_cursor_visible();
    }

    /// Mueve al inicio de línea.
    pub fn move_to_line_start(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_to_line_start(&text);
        self.selection.collapse();
    }

    /// Mueve al final de línea.
    pub fn move_to_line_end(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_to_line_end(&text);
        self.selection.collapse();
    }

    /// Mueve cursor al inicio de palabra previa.
    pub fn move_word_left(&mut self) {
        let text = self.buffer.text();
        if !self.selection.is_empty() {
            let start = self.selection.start_offset();
            self.selection.head = Self::cursor_from_offset(&text, start);
            self.selection.collapse();
            return;
        }

        let target = Self::previous_word_start(&text, self.selection.head.offset);
        self.selection.head = Self::cursor_from_offset(&text, target);
        self.selection.collapse();
    }

    /// Mueve cursor al inicio de la siguiente palabra.
    pub fn move_word_right(&mut self) {
        let text = self.buffer.text();
        if !self.selection.is_empty() {
            let end = self.selection.end_offset();
            self.selection.head = Self::cursor_from_offset(&text, end);
            self.selection.collapse();
            return;
        }

        let target = Self::next_word_start(&text, self.selection.head.offset);
        self.selection.head = Self::cursor_from_offset(&text, target);
        self.selection.collapse();
    }

    // === Edición ===

    /// Inserta texto en la posición del cursor.
    pub fn insert(&mut self, text: &str) {
        // Si hay selección, eliminarla primero
        if !self.selection.is_empty() {
            self.delete_selection();
        }

        let offset = self.selection.head.offset;
        self.buffer.insert(offset, text);

        // Mover cursor después del texto insertado
        for _ in 0..text.chars().count() {
            let buffer_text = self.buffer.text();
            self.selection.head.move_right(&buffer_text);
        }
        self.selection.collapse();

        self.update_display();
    }

    /// Inserta un newline.
    pub fn insert_newline(&mut self) {
        self.insert("\n");
    }

    /// Elimina el carácter antes del cursor (backspace).
    pub fn backspace(&mut self) {
        if !self.selection.is_empty() {
            self.delete_selection();
        } else if self.selection.head.offset > 0 {
            self.move_left();
            self.delete_forward();
        }
    }

    /// Elimina el carácter después del cursor (delete).
    pub fn delete_forward(&mut self) {
        if !self.selection.is_empty() {
            self.delete_selection();
        } else {
            let text = self.buffer.text();
            let offset = self.selection.head.offset;
            if offset < text.len() {
                // Encontrar el final del carácter actual
                let char_len = text[offset..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
                if char_len > 0 {
                    self.buffer.delete(offset..offset + char_len);
                    self.update_display();
                }
            }
        }
    }

    /// Elimina la selección actual.
    fn delete_selection(&mut self) {
        let range = self.selection.byte_range();
        self.buffer.delete(range);
        self.selection.collapse_to_start();
        self.update_display();
    }

    /// Elimina la palabra previa al cursor.
    pub fn delete_word_backward(&mut self) {
        if !self.selection.is_empty() {
            self.delete_selection();
            return;
        }

        let text = self.buffer.text();
        let offset = self.selection.head.offset;
        let start = Self::previous_word_start(&text, offset);
        let mut end = offset;
        if let Some(ch) = Self::char_at(&text, end) {
            if ch == ' ' || ch == '\t' {
                while let Some(ws) = Self::char_at(&text, end) {
                    if ws == ' ' || ws == '\t' {
                        end = end.saturating_add(ws.len_utf8());
                    } else {
                        break;
                    }
                }
            }
        }

        if start >= end {
            return;
        }

        self.buffer.delete(start..end);
        let updated_text = self.buffer.text();
        self.selection.head = Self::cursor_from_offset(&updated_text, start);
        self.selection.collapse();
        self.update_display();
    }

    /// Elimina hasta el inicio de la siguiente palabra.
    pub fn delete_word_forward(&mut self) {
        if !self.selection.is_empty() {
            self.delete_selection();
            return;
        }

        let text = self.buffer.text();
        let offset = self.selection.head.offset;
        let end = Self::next_word_start(&text, offset);
        if end <= offset {
            return;
        }

        self.buffer.delete(offset..end);
        let updated_text = self.buffer.text();
        self.selection.head = Self::cursor_from_offset(&updated_text, offset);
        self.selection.collapse();
        self.update_display();
    }

    // === Undo/Redo ===

    /// Deshace la última edición.
    pub fn undo(&mut self) -> bool {
        let result = self.buffer.undo();
        if result {
            self.update_display();
        }
        result
    }

    /// Rehace la última edición deshecha.
    pub fn redo(&mut self) -> bool {
        let result = self.buffer.redo();
        if result {
            self.update_display();
        }
        result
    }

    // === Selección ===

    /// Selecciona todo el texto.
    pub fn select_all(&mut self) {
        let text = self.buffer.text();
        self.selection.select_all(&text);
    }

    /// Expande la selección a la derecha.
    pub fn extend_selection_right(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_right(&text);
    }

    /// Expande la selección a la izquierda.
    pub fn extend_selection_left(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_left(&text);
    }

    /// Expande la selección al inicio de palabra previa.
    pub fn extend_selection_word_left(&mut self) {
        let text = self.buffer.text();
        let target = Self::previous_word_start(&text, self.selection.head.offset);
        self.selection.head = Self::cursor_from_offset(&text, target);
    }

    /// Expande la selección al inicio de palabra siguiente.
    pub fn extend_selection_word_right(&mut self) {
        let text = self.buffer.text();
        let target = Self::next_word_start(&text, self.selection.head.offset);
        self.selection.head = Self::cursor_from_offset(&text, target);
    }

    /// Expande la selección una línea arriba.
    pub fn extend_selection_up(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_up(&text);
        self.ensure_cursor_visible();
    }

    /// Expande la selección una línea abajo.
    pub fn extend_selection_down(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_down(&text);
        self.ensure_cursor_visible();
    }

    /// Expande selección hasta inicio de línea.
    pub fn extend_selection_to_line_start(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_to_line_start(&text);
    }

    /// Expande selección hasta fin de línea.
    pub fn extend_selection_to_line_end(&mut self) {
        let text = self.buffer.text();
        self.selection.head.move_to_line_end(&text);
    }

    // === Modo ===

    /// Cambia al modo inserción.
    pub fn enter_insert_mode(&mut self) {
        self.mode = EditorMode::Insert;
        self.cursor_shape = CursorShape::Bar;
    }

    /// Cambia al modo normal.
    pub fn enter_normal_mode(&mut self) {
        self.mode = EditorMode::Normal;
        self.cursor_shape = CursorShape::Block;
        self.selection.collapse();
    }

    /// Cambia al modo visual.
    pub fn enter_visual_mode(&mut self) {
        self.mode = EditorMode::Visual;
        self.cursor_shape = CursorShape::Block;
        // El anchor se queda donde estaba el cursor
    }

    // === Helpers privados ===

    /// Convierte línea/columna (con clamp) a cursor.
    fn cursor_from_line_column(text: &str, line: usize, column: usize) -> Cursor {
        let lines: Vec<&str> = text.split('\n').collect();
        let max_line = lines.len().saturating_sub(1);
        let line = line.min(max_line);

        let max_col = lines.get(line).map(|l| l.chars().count()).unwrap_or(0);
        let column = column.min(max_col);

        let mut offset = 0usize;
        for line_text in lines.iter().take(line) {
            offset = offset.saturating_add(line_text.len().saturating_add(1));
        }

        if let Some(line_text) = lines.get(line) {
            for (idx, ch) in line_text.chars().enumerate() {
                if idx >= column {
                    break;
                }
                offset = offset.saturating_add(ch.len_utf8());
            }
        }

        Cursor::at(line, column, offset.min(text.len()))
    }

    /// Convierte offset en bytes (con clamp) a cursor.
    fn cursor_from_offset(text: &str, offset: usize) -> Cursor {
        let mut line = 0usize;
        let mut column = 0usize;
        let mut current = 0usize;
        let target = offset.min(text.len());

        for ch in text.chars() {
            if current >= target {
                break;
            }
            let ch_len = ch.len_utf8();
            if current + ch_len > target {
                break;
            }
            current += ch_len;
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }

        Cursor::at(line, column, current)
    }

    fn char_class(ch: char) -> u8 {
        if ch.is_whitespace() {
            0
        } else if ch.is_alphanumeric() || ch == '_' {
            1
        } else {
            2
        }
    }

    fn char_before(text: &str, offset: usize) -> Option<(usize, char)> {
        if offset == 0 || offset > text.len() {
            return None;
        }
        let ch = text[..offset].chars().next_back()?;
        let start = offset.saturating_sub(ch.len_utf8());
        Some((start, ch))
    }

    fn char_at(text: &str, offset: usize) -> Option<char> {
        if offset >= text.len() {
            return None;
        }
        text[offset..].chars().next()
    }

    fn previous_word_start(text: &str, offset: usize) -> usize {
        let mut idx = offset.min(text.len());
        while let Some((start, ch)) = Self::char_before(text, idx) {
            if Self::char_class(ch) != 0 {
                break;
            }
            idx = start;
        }
        let Some((_, prev_ch)) = Self::char_before(text, idx) else {
            return 0;
        };
        let class = Self::char_class(prev_ch);
        while let Some((start, ch)) = Self::char_before(text, idx) {
            if Self::char_class(ch) != class {
                break;
            }
            idx = start;
        }
        idx
    }

    fn next_word_start(text: &str, offset: usize) -> usize {
        let mut idx = offset.min(text.len());
        let Some(ch) = Self::char_at(text, idx) else {
            return text.len();
        };
        let class = Self::char_class(ch);
        while let Some(ch) = Self::char_at(text, idx) {
            if Self::char_class(ch) != class {
                break;
            }
            idx = idx.saturating_add(ch.len_utf8());
        }
        if class != 0 {
            while let Some(ch) = Self::char_at(text, idx) {
                if Self::char_class(ch) != 0 {
                    break;
                }
                idx = idx.saturating_add(ch.len_utf8());
            }
        }
        idx.min(text.len())
    }

    /// Actualiza el display map.
    fn update_display(&mut self) {
        self.display_map.update(&self.buffer.text());
    }

    /// Asegura que el cursor es visible en el viewport.
    fn ensure_cursor_visible(&mut self) {
        // Por ahora, simple - mantener cursor en vista
        let cursor_line = self.selection.head.line;
        if cursor_line < self.scroll_top {
            self.scroll_top = cursor_line;
        }
        // TODO: Implementar viewport height
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_new() {
        let editor = Editor::new();
        assert_eq!(editor.text(), "");
        assert_eq!(editor.mode(), EditorMode::Insert);
    }

    #[test]
    fn test_editor_with_text() {
        let editor = Editor::with_text("Hello World");
        assert_eq!(editor.text(), "Hello World");
        assert_eq!(editor.line_count(), 1);
    }

    #[test]
    fn test_insert() {
        let mut editor = Editor::new();
        editor.insert("Hello");
        assert_eq!(editor.text(), "Hello");
        assert_eq!(editor.cursor().offset, 5);
    }

    #[test]
    fn test_backspace() {
        let mut editor = Editor::with_text("Hello");
        editor.selection.head = Cursor::at(0, 5, 5);
        editor.selection.collapse();

        editor.backspace();
        assert_eq!(editor.text(), "Hell");
    }

    #[test]
    fn test_undo_redo() {
        let mut editor = Editor::new();
        editor.insert("Hello");
        editor.insert(" World");
        assert_eq!(editor.text(), "Hello World");

        editor.undo();
        assert_eq!(editor.text(), "Hello");

        editor.redo();
        assert_eq!(editor.text(), "Hello World");
    }

    #[test]
    fn test_select_all() {
        let mut editor = Editor::with_text("Hello World");
        editor.select_all();
        assert_eq!(editor.selection().len(), 11);
    }

    #[test]
    fn test_mode_switching() {
        let mut editor = Editor::new();
        assert_eq!(editor.mode(), EditorMode::Insert);

        editor.enter_normal_mode();
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.cursor_shape(), CursorShape::Block);

        editor.enter_insert_mode();
        assert_eq!(editor.mode(), EditorMode::Insert);
        assert_eq!(editor.cursor_shape(), CursorShape::Bar);
    }

    #[test]
    fn test_set_cursor_line_column() {
        let mut editor = Editor::with_text("abc\ndef");
        editor.set_cursor_line_column(1, 2);
        assert_eq!(editor.cursor().line, 1);
        assert_eq!(editor.cursor().column, 2);
        assert_eq!(editor.cursor().offset, 6);
    }

    #[test]
    fn test_set_cursor_line_column_with_trailing_newline() {
        let mut editor = Editor::with_text("a\n");
        editor.set_cursor_line_column(1, 0);
        assert_eq!(editor.cursor().line, 1);
        assert_eq!(editor.cursor().column, 0);
        assert_eq!(editor.cursor().offset, 2);
    }

    #[test]
    fn test_scroll_lines_clamped() {
        let mut editor = Editor::with_text("a\nb\nc");
        editor.scroll_lines(100);
        assert_eq!(editor.scroll_top(), 2);
        editor.scroll_lines(-1);
        assert_eq!(editor.scroll_top(), 1);
        editor.scroll_lines(-100);
        assert_eq!(editor.scroll_top(), 0);
    }

    #[test]
    fn test_set_selection_line_columns() {
        let mut editor = Editor::with_text("abcd\nefgh");
        editor.set_selection_line_columns(0, 1, 0, 3);
        let sel = editor.selection();
        assert!(!sel.is_empty());
        assert_eq!(sel.selected_text(&editor.text()), "bc");
    }

    #[test]
    fn test_select_word_at_word() {
        let mut editor = Editor::with_text("hola_quiron mundo");
        editor.select_word_at(0, 3);
        let sel = editor.selection();
        assert_eq!(sel.selected_text(&editor.text()), "hola_quiron");
    }

    #[test]
    fn test_select_word_at_whitespace_collapses() {
        let mut editor = Editor::with_text("hola mundo");
        editor.select_word_at(0, 4);
        assert!(editor.selection().is_empty());
        assert_eq!(editor.cursor().column, 4);
    }

    #[test]
    fn test_select_line_at() {
        let mut editor = Editor::with_text("uno\ndos\ntres");
        editor.select_line_at(1);
        let sel = editor.selection();
        assert_eq!(sel.selected_text(&editor.text()), "dos");
    }

    #[test]
    fn test_extend_selection_vertical() {
        let mut editor = Editor::with_text("abcd\nefgh");
        editor.set_cursor_line_column(0, 2);
        editor.extend_selection_down();
        assert!(!editor.selection().is_empty());
        assert_eq!(editor.selection().head.line, 1);
        editor.extend_selection_up();
        assert_eq!(editor.selection().head.line, 0);
    }

    #[test]
    fn test_extend_selection_line_edges() {
        let mut editor = Editor::with_text("abcd");
        editor.set_cursor_line_column(0, 2);
        editor.extend_selection_to_line_start();
        assert_eq!(editor.selection().head.column, 0);
        editor.extend_selection_to_line_end();
        assert_eq!(editor.selection().head.column, 4);
    }

    #[test]
    fn test_move_word_navigation() {
        let mut editor = Editor::with_text("hola quiron_test final");
        editor.set_cursor_line_column(0, 0);
        editor.move_word_right();
        assert_eq!(editor.cursor().column, 5);
        editor.move_word_right();
        assert_eq!(editor.cursor().column, 17);
        editor.move_word_left();
        assert_eq!(editor.cursor().column, 5);
    }

    #[test]
    fn test_delete_word_backward_forward() {
        let mut editor = Editor::with_text("hola quiron mundo");
        editor.set_cursor_line_column(0, 11);
        editor.delete_word_backward();
        assert_eq!(editor.text(), "hola mundo");
        editor.set_cursor_line_column(0, 5);
        editor.delete_word_forward();
        assert_eq!(editor.text(), "hola ");
    }

    #[test]
    fn test_extend_selection_word() {
        let mut editor = Editor::with_text("hola quiron mundo");
        editor.set_cursor_line_column(0, 5);
        editor.extend_selection_word_right();
        assert_eq!(editor.selection().selected_text(&editor.text()), "quiron ");
        editor.extend_selection_word_left();
        assert_eq!(editor.selection().selected_text(&editor.text()), "");
        assert!(editor.selection().is_empty());
    }
}
