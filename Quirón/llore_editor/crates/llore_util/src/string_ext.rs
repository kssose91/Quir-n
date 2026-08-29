//! # Extensiones de String
//!
//! Métodos útiles para trabajar con strings en un editor.

/// Extensiones para &str y String.
pub trait StringExt {
    /// Trunca a N caracteres con "..." si es necesario.
    fn truncate_with_ellipsis(&self, max_chars: usize) -> String;

    /// Cuenta las líneas en el string.
    fn line_count(&self) -> usize;

    /// Obtiene la línea N (0-indexed).
    fn get_line(&self, n: usize) -> Option<&str>;

    /// Remueve espacios en blanco al final de cada línea.
    fn trim_trailing_whitespace(&self) -> String;
}

impl StringExt for str {
    fn truncate_with_ellipsis(&self, max_chars: usize) -> String {
        let chars: Vec<char> = self.chars().collect();
        if chars.len() <= max_chars {
            self.to_string()
        } else if max_chars <= 3 {
            "...".to_string()
        } else {
            let truncated: String = chars[..max_chars - 3].iter().collect();
            format!("{}...", truncated)
        }
    }

    fn line_count(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.lines().count()
        }
    }

    fn get_line(&self, n: usize) -> Option<&str> {
        self.lines().nth(n)
    }

    fn trim_trailing_whitespace(&self) -> String {
        self.lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl StringExt for String {
    fn truncate_with_ellipsis(&self, max_chars: usize) -> String {
        self.as_str().truncate_with_ellipsis(max_chars)
    }

    fn line_count(&self) -> usize {
        self.as_str().line_count()
    }

    fn get_line(&self, n: usize) -> Option<&str> {
        self.as_str().get_line(n)
    }

    fn trim_trailing_whitespace(&self) -> String {
        self.as_str().trim_trailing_whitespace()
    }
}

/// Formatea bytes en una representación legible (KB, MB, GB).
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!("Hello".truncate_with_ellipsis(10), "Hello");
        assert_eq!("Hello World".truncate_with_ellipsis(8), "Hello...");
        // For very short max, just return the string if smaller than max
        assert_eq!("Hi".truncate_with_ellipsis(5), "Hi");
        assert_eq!("Hello".truncate_with_ellipsis(3), "...");
    }

    #[test]
    fn test_line_operations() {
        let text = "Line 1\nLine 2\nLine 3";
        assert_eq!(text.line_count(), 3);
        assert_eq!(text.get_line(0), Some("Line 1"));
        assert_eq!(text.get_line(1), Some("Line 2"));
        assert_eq!(text.get_line(5), None);
    }

    #[test]
    fn test_trim_trailing() {
        let text = "Hello   \nWorld  ";
        assert_eq!(text.trim_trailing_whitespace(), "Hello\nWorld");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 bytes");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1_500_000), "1.43 MB");
        assert_eq!(format_bytes(2_000_000_000), "1.86 GB");
    }
}
