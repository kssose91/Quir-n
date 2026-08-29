//! # Display Map
//!
//! Mapea el contenido del buffer a líneas visuales.
//! Maneja soft wrapping, tabs, y otros aspectos de visualización.

/// Configuración de visualización.
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    /// Ancho de la vista en columnas.
    pub viewport_width: usize,
    /// Ancho de un tab en espacios.
    pub tab_width: usize,
    /// Habilitar soft wrap.
    pub soft_wrap: bool,
    /// Mostrar números de línea.
    pub show_line_numbers: bool,
    /// Mostrar espacios en blanco.
    pub show_whitespace: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            viewport_width: 80,
            tab_width: 4,
            soft_wrap: false,
            show_line_numbers: true,
            show_whitespace: false,
        }
    }
}

/// Una línea visual (puede ser parte de una línea lógica si hay wrap).
#[derive(Debug, Clone)]
pub struct DisplayLine {
    /// Índice de línea lógica (en el buffer).
    pub buffer_line: usize,
    /// Offset dentro de la línea lógica (para soft wrap).
    pub line_offset: usize,
    /// Contenido de la línea visual.
    pub content: String,
    /// Es continuación de la línea anterior (soft wrapped).
    pub is_wrapped: bool,
}

/// Mapeo de buffer a display.
#[derive(Debug, Default)]
pub struct DisplayMap {
    /// Configuración de display.
    pub config: DisplayConfig,
    /// Líneas visuales generadas.
    lines: Vec<DisplayLine>,
}

impl DisplayMap {
    /// Crea un nuevo display map con configuración por defecto.
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea un display map con configuración específica.
    pub fn with_config(config: DisplayConfig) -> Self {
        Self {
            config,
            lines: Vec::new(),
        }
    }

    /// Recalcula las líneas visuales desde el texto del buffer.
    pub fn update(&mut self, text: &str) {
        self.lines.clear();

        for (line_idx, line) in text.lines().enumerate() {
            if self.config.soft_wrap && line.chars().count() > self.config.viewport_width {
                // Soft wrap long lines
                self.wrap_line(line_idx, line);
            } else {
                self.lines.push(DisplayLine {
                    buffer_line: line_idx,
                    line_offset: 0,
                    content: self.process_tabs(line),
                    is_wrapped: false,
                });
            }
        }

        // Handle empty text
        if self.lines.is_empty() {
            self.lines.push(DisplayLine {
                buffer_line: 0,
                line_offset: 0,
                content: String::new(),
                is_wrapped: false,
            });
        }
    }

    /// Procesa tabs en una línea.
    fn process_tabs(&self, line: &str) -> String {
        let mut result = String::new();
        for c in line.chars() {
            if c == '\t' {
                let spaces_needed =
                    self.config.tab_width - (result.chars().count() % self.config.tab_width);
                result.extend(std::iter::repeat(' ').take(spaces_needed));
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Divide una línea larga en múltiples líneas visuales.
    fn wrap_line(&mut self, buffer_line: usize, line: &str) {
        let chars: Vec<char> = line.chars().collect();
        let width = self.config.viewport_width;
        let mut offset = 0;

        while offset < chars.len() {
            let end = (offset + width).min(chars.len());
            let content: String = chars[offset..end].iter().collect();

            self.lines.push(DisplayLine {
                buffer_line,
                line_offset: offset,
                content: self.process_tabs(&content),
                is_wrapped: offset > 0,
            });

            offset = end;
        }
    }

    /// Número de líneas visuales.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Obtiene una línea visual.
    pub fn get_line(&self, idx: usize) -> Option<&DisplayLine> {
        self.lines.get(idx)
    }

    /// Itera sobre las líneas visuales.
    pub fn lines(&self) -> impl Iterator<Item = &DisplayLine> {
        self.lines.iter()
    }

    /// Convierte posición de buffer a posición de display.
    pub fn buffer_to_display(&self, buffer_line: usize, column: usize) -> (usize, usize) {
        let mut display_line = 0;
        for line in &self.lines {
            if line.buffer_line == buffer_line {
                if !line.is_wrapped || column < line.line_offset + self.config.viewport_width {
                    let display_col = if line.is_wrapped {
                        column.saturating_sub(line.line_offset)
                    } else {
                        column
                    };
                    return (display_line, display_col);
                }
            }
            display_line += 1;
        }
        (display_line.saturating_sub(1), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_display() {
        let mut dm = DisplayMap::new();
        dm.update("Hello\nWorld");
        assert_eq!(dm.line_count(), 2);
        assert_eq!(dm.get_line(0).unwrap().content, "Hello");
        assert_eq!(dm.get_line(1).unwrap().content, "World");
    }

    #[test]
    fn test_tab_expansion() {
        let mut dm = DisplayMap::with_config(DisplayConfig {
            tab_width: 4,
            ..Default::default()
        });
        dm.update("a\tb");
        assert_eq!(dm.get_line(0).unwrap().content, "a   b");
    }

    #[test]
    fn test_soft_wrap() {
        let mut dm = DisplayMap::with_config(DisplayConfig {
            viewport_width: 5,
            soft_wrap: true,
            ..Default::default()
        });
        dm.update("HelloWorld");
        assert_eq!(dm.line_count(), 2);
        assert_eq!(dm.get_line(0).unwrap().content, "Hello");
        assert_eq!(dm.get_line(1).unwrap().content, "World");
        assert!(dm.get_line(1).unwrap().is_wrapped);
    }

    #[test]
    fn test_empty_text() {
        let mut dm = DisplayMap::new();
        dm.update("");
        assert_eq!(dm.line_count(), 1);
    }
}
