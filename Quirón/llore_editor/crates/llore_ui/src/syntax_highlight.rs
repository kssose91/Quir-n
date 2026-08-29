//! Syntax highlighting ligero por línea.
//!
//! Implementa una capa de resaltado incremental sin bloquear render.

use std::cell::RefCell;

use tree_sitter::{Node, Parser};

thread_local! {
    static RUST_TS_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
    static PYTHON_TS_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxClass {
    Plain,
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
    Macro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxSegment {
    pub start_col: usize,
    pub end_col: usize,
    pub class: SyntaxClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

pub fn highlight_line(language_name: Option<&str>, line: &str) -> Vec<SyntaxSegment> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return vec![];
    }

    let lang = language_name.unwrap_or("").to_ascii_lowercase();
    let mut styles = vec![SyntaxClass::Plain; chars.len()];
    let comment_marker = comment_marker_for_language(&lang);
    let comment_start = comment_marker.and_then(|marker| find_comment_start(&chars, marker));
    let code_end = comment_start.unwrap_or(chars.len());

    let mut i = 0usize;
    while i < code_end {
        let ch = chars[i];
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let mut j = i + 1;
            let mut escaped = false;
            while j < code_end {
                let current = chars[j];
                if escaped {
                    escaped = false;
                    j += 1;
                    continue;
                }
                if current == '\\' {
                    escaped = true;
                    j += 1;
                    continue;
                }
                if current == quote {
                    j += 1;
                    break;
                }
                j += 1;
            }
            paint_range(&mut styles, i, j.min(code_end), SyntaxClass::String);
            i = j;
            continue;
        }

        if ch.is_ascii_digit() {
            let mut j = i + 1;
            while j < code_end {
                let c = chars[j];
                if c.is_ascii_alphanumeric()
                    || c == '_'
                    || c == '.'
                    || c == 'x'
                    || c == 'X'
                    || c == 'o'
                    || c == 'O'
                    || c == 'b'
                    || c == 'B'
                {
                    j += 1;
                } else {
                    break;
                }
            }
            paint_range(&mut styles, i, j, SyntaxClass::Number);
            i = j;
            continue;
        }

        if is_ident_start(ch) {
            let mut j = i + 1;
            while j < code_end && is_ident_continue(chars[j]) {
                j += 1;
            }
            let token: String = chars[i..j].iter().collect();
            let class = classify_identifier(&lang, &chars, i, j, &token);
            if class != SyntaxClass::Plain {
                paint_range(&mut styles, i, j, class);
            }
            i = j;
            continue;
        }

        i += 1;
    }

    if let Some(start) = comment_start {
        paint_range(&mut styles, start, chars.len(), SyntaxClass::Comment);
    }

    if let Some(ts_overlays) = tree_sitter_overlays(&lang, line) {
        for overlay in ts_overlays {
            paint_range(
                &mut styles,
                overlay.start_col,
                overlay.end_col,
                overlay.class,
            );
        }
    }

    compress_segments(&styles)
}

pub fn syntax_diagnostic(language_name: Option<&str>, source: &str) -> Option<SyntaxDiagnostic> {
    if source.is_empty() || source.len() > 1_000_000 {
        return None;
    }
    let lang = language_name.unwrap_or("").to_ascii_lowercase();

    match lang.as_str() {
        "rust" | "python" => with_tree_sitter_parser(&lang, |parser| {
            let tree = parser.parse(source, None)?;
            let root = tree.root_node();
            if !root.has_error() {
                return None;
            }
            let first_error = find_first_error_node(root).unwrap_or(root);
            let pos = first_error.start_position();
            let message = if first_error.is_missing() {
                format!("missing {}", first_error.kind())
            } else if first_error.is_error() {
                format!("unexpected {}", first_error.kind())
            } else {
                "syntax error".to_string()
            };
            Some(SyntaxDiagnostic {
                line: pos.row,
                column: pos.column,
                message,
            })
        })
        .flatten(),
        _ => None,
    }
}

fn find_first_error_node(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_first_error_node(child) {
            return Some(found);
        }
    }
    None
}

fn tree_sitter_overlays(language: &str, line: &str) -> Option<Vec<SyntaxSegment>> {
    if line.is_empty() || line.len() > 800 {
        return None;
    }

    match language {
        "rust" => with_tree_sitter_parser(language, |parser| {
            collect_tree_sitter_segments(parser, language, line)
        }),
        "python" => with_tree_sitter_parser(language, |parser| {
            collect_tree_sitter_segments(parser, language, line)
        }),
        _ => None,
    }
}

fn with_tree_sitter_parser<T>(language: &str, f: impl FnOnce(&mut Parser) -> T) -> Option<T> {
    match language {
        "rust" => RUST_TS_PARSER.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                let mut parser = Parser::new();
                let ts_lang = tree_sitter_rust::LANGUAGE.into();
                parser.set_language(&ts_lang).ok()?;
                *slot = Some(parser);
            }
            slot.as_mut().map(f)
        }),
        "python" => PYTHON_TS_PARSER.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                let mut parser = Parser::new();
                let ts_lang = tree_sitter_python::LANGUAGE.into();
                parser.set_language(&ts_lang).ok()?;
                *slot = Some(parser);
            }
            slot.as_mut().map(f)
        }),
        _ => None,
    }
}

fn collect_tree_sitter_segments(
    parser: &mut Parser,
    language: &str,
    line: &str,
) -> Vec<SyntaxSegment> {
    let Some(tree) = parser.parse(line, None) else {
        return vec![];
    };
    let mut out = Vec::new();
    let root = tree.root_node();
    collect_tree_sitter_node_segments(language, line, root, None, &mut out);
    out.sort_by(|a, b| {
        a.start_col
            .cmp(&b.start_col)
            .then_with(|| a.end_col.cmp(&b.end_col))
    });
    out
}

fn collect_tree_sitter_node_segments(
    language: &str,
    line: &str,
    node: Node<'_>,
    parent_kind: Option<&str>,
    out: &mut Vec<SyntaxSegment>,
) {
    let kind = node.kind();
    if let Some(class) = classify_tree_sitter_node(language, kind, parent_kind) {
        let start_col = byte_to_char_col(line, node.start_byte());
        let end_col = byte_to_char_col(line, node.end_byte());
        if end_col > start_col {
            out.push(SyntaxSegment {
                start_col,
                end_col,
                class,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_node_segments(language, line, child, Some(kind), out);
    }
}

fn classify_tree_sitter_node(
    language: &str,
    kind: &str,
    parent_kind: Option<&str>,
) -> Option<SyntaxClass> {
    if kind.contains("comment") {
        return Some(SyntaxClass::Comment);
    }
    if kind.contains("string") || kind == "char_literal" {
        return Some(SyntaxClass::String);
    }
    if matches!(
        kind,
        "integer"
            | "float"
            | "integer_literal"
            | "float_literal"
            | "hex_literal"
            | "octal_literal"
            | "binary_literal"
    ) {
        return Some(SyntaxClass::Number);
    }

    match language {
        "rust" => {
            if matches!(
                kind,
                "type_identifier" | "primitive_type" | "scoped_type_identifier"
            ) {
                return Some(SyntaxClass::Type);
            }
            if kind == "identifier" && parent_kind == Some("function_item") {
                return Some(SyntaxClass::Function);
            }
            if kind == "identifier" && parent_kind == Some("call_expression") {
                return Some(SyntaxClass::Function);
            }
            if kind == "identifier" && parent_kind == Some("macro_invocation") {
                return Some(SyntaxClass::Macro);
            }
        }
        "python" => {
            if kind == "identifier" && parent_kind == Some("function_definition") {
                return Some(SyntaxClass::Function);
            }
            if kind == "identifier" && parent_kind == Some("class_definition") {
                return Some(SyntaxClass::Type);
            }
            if kind == "identifier" && parent_kind == Some("call") {
                return Some(SyntaxClass::Function);
            }
        }
        _ => {}
    }

    None
}

fn byte_to_char_col(text: &str, byte: usize) -> usize {
    let clamped = byte.min(text.len());
    text[..clamped].chars().count()
}

fn comment_marker_for_language(language: &str) -> Option<&'static str> {
    if matches!(language, "rust" | "javascript" | "typescript") {
        Some("//")
    } else if matches!(language, "python" | "toml" | "yaml" | "markdown") {
        Some("#")
    } else {
        None
    }
}

fn find_comment_start(chars: &[char], marker: &str) -> Option<usize> {
    let marker_chars: Vec<char> = marker.chars().collect();
    if marker_chars.is_empty() || chars.len() < marker_chars.len() {
        return None;
    }

    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if ch == quote {
                in_string = false;
                quote = '\0';
            }
            i += 1;
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = true;
            quote = ch;
            i += 1;
            continue;
        }

        if i + marker_chars.len() <= chars.len()
            && chars[i..i + marker_chars.len()] == marker_chars[..]
        {
            return Some(i);
        }

        i += 1;
    }

    None
}

fn classify_identifier(
    language: &str,
    chars: &[char],
    start: usize,
    end: usize,
    token: &str,
) -> SyntaxClass {
    if is_keyword(language, token) {
        return SyntaxClass::Keyword;
    }
    if is_builtin_type(language, token) {
        return SyntaxClass::Type;
    }

    if let Some(next) = next_non_whitespace(chars, end) {
        if language == "rust" && next == '!' {
            return SyntaxClass::Macro;
        }
        if next == '(' && !matches!(token, "if" | "for" | "while" | "match" | "switch") {
            return SyntaxClass::Function;
        }
    }

    if language == "rust"
        && token
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
    {
        return SyntaxClass::Type;
    }

    if language == "python"
        && token
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
    {
        return SyntaxClass::Type;
    }

    if start == 0 && token == "use" && language == "rust" {
        return SyntaxClass::Keyword;
    }

    SyntaxClass::Plain
}

fn next_non_whitespace(chars: &[char], from: usize) -> Option<char> {
    let mut idx = from;
    while idx < chars.len() {
        let ch = chars[idx];
        if !ch.is_whitespace() {
            return Some(ch);
        }
        idx += 1;
    }
    None
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_keyword(language: &str, token: &str) -> bool {
    match language {
        "rust" => matches!(
            token,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        "python" => matches!(
            token,
            "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "False"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "None"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "True"
                | "try"
                | "while"
                | "with"
                | "yield"
        ),
        "javascript" | "typescript" => matches!(
            token,
            "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "do"
                | "else"
                | "export"
                | "extends"
                | "finally"
                | "for"
                | "function"
                | "if"
                | "import"
                | "in"
                | "instanceof"
                | "let"
                | "new"
                | "return"
                | "switch"
                | "throw"
                | "try"
                | "typeof"
                | "var"
                | "while"
                | "yield"
                | "async"
                | "await"
        ),
        "json" => matches!(token, "true" | "false" | "null"),
        _ => false,
    }
}

fn is_builtin_type(language: &str, token: &str) -> bool {
    match language {
        "rust" => matches!(
            token,
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "bool"
                | "char"
                | "str"
                | "String"
                | "Option"
                | "Result"
                | "Vec"
        ),
        "python" => matches!(
            token,
            "int" | "float" | "str" | "bool" | "dict" | "list" | "tuple" | "set"
        ),
        "javascript" | "typescript" => matches!(
            token,
            "number" | "string" | "boolean" | "object" | "Array" | "Promise" | "Date"
        ),
        _ => false,
    }
}

fn paint_range(styles: &mut [SyntaxClass], start: usize, end: usize, class: SyntaxClass) {
    let clamped_end = end.min(styles.len());
    if start >= clamped_end {
        return;
    }
    for style in styles.iter_mut().take(clamped_end).skip(start) {
        *style = class;
    }
}

fn compress_segments(styles: &[SyntaxClass]) -> Vec<SyntaxSegment> {
    if styles.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut current = styles[0];
    for (idx, class) in styles.iter().copied().enumerate().skip(1) {
        if class != current {
            out.push(SyntaxSegment {
                start_col: start,
                end_col: idx,
                class: current,
            });
            start = idx;
            current = class;
        }
    }
    out.push(SyntaxSegment {
        start_col: start,
        end_col: styles.len(),
        class: current,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_rust_keywords_and_comment() {
        let spans = highlight_line(Some("Rust"), "fn greet() { let x = 1; } // hi");
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Keyword));
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Function));
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Number));
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Comment));
    }

    #[test]
    fn comment_marker_inside_string_is_ignored() {
        let spans = highlight_line(Some("Rust"), "let s = \"http://example\";");
        assert!(spans.iter().any(|s| s.class == SyntaxClass::String));
        assert!(!spans.iter().any(|s| s.class == SyntaxClass::Comment));
    }

    #[test]
    fn python_def_and_comment() {
        let spans = highlight_line(Some("Python"), "def run(x): # ok");
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Keyword));
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Function));
        assert!(spans.iter().any(|s| s.class == SyntaxClass::Comment));
    }

    #[test]
    fn syntax_diagnostic_detects_rust_error() {
        let diag = syntax_diagnostic(Some("Rust"), "fn broken( {\n let x = 1;\n");
        assert!(diag.is_some());
    }

    #[test]
    fn syntax_diagnostic_returns_none_on_valid_python() {
        let diag = syntax_diagnostic(Some("Python"), "def run(x):\n    return x\n");
        assert!(diag.is_none());
    }
}
