//! # Syntax Parsing
//!
//! Integración con tree-sitter para parsing de código.
//!
//! Este módulo provee una abstracción sobre tree-sitter para
//! obtener el árbol de sintaxis de código fuente.

use std::fmt;
use tree_sitter::{Parser, Tree};

/// Resultado de parsing con tree-sitter.
#[derive(Clone)]
pub struct Syntax {
    /// Árbol de sintaxis parseado.
    tree: Tree,
}

impl Syntax {
    /// Crea un Syntax desde un árbol tree-sitter.
    pub fn new(tree: Tree) -> Self {
        Self { tree }
    }

    /// Obtiene el nodo raíz del árbol.
    pub fn root_node(&self) -> SyntaxNode {
        SyntaxNode::from_tree_sitter(self.tree.root_node())
    }

    /// Verifica si el árbol tiene errores de sintaxis.
    pub fn has_errors(&self) -> bool {
        self.tree.root_node().has_error()
    }
}

impl fmt::Debug for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Syntax {{ root: {:?}, has_errors: {} }}",
            self.tree.root_node().kind(),
            self.has_errors()
        )
    }
}

/// Un nodo en el árbol de sintaxis.
#[derive(Debug, Clone)]
pub struct SyntaxNode {
    /// Tipo del nodo (e.g., "function_definition", "identifier").
    pub kind: String,
    /// Offset de inicio en bytes.
    pub start_byte: usize,
    /// Offset de fin en bytes.
    pub end_byte: usize,
    /// Línea de inicio (0-indexed).
    pub start_row: usize,
    /// Columna de inicio (0-indexed).
    pub start_col: usize,
    /// Línea de fin (0-indexed).
    pub end_row: usize,
    /// Columna de fin (0-indexed).
    pub end_col: usize,
    /// Si es un nodo con nombre (vs anónimo).
    pub is_named: bool,
}

impl SyntaxNode {
    /// Crea un SyntaxNode desde un nodo tree-sitter.
    fn from_tree_sitter(node: tree_sitter::Node) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        Self {
            kind: node.kind().to_string(),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_row: start.row,
            start_col: start.column,
            end_row: end.row,
            end_col: end.column,
            is_named: node.is_named(),
        }
    }

    /// Rango en bytes.
    pub fn byte_range(&self) -> std::ops::Range<usize> {
        self.start_byte..self.end_byte
    }

    /// Longitud en bytes.
    pub fn len(&self) -> usize {
        self.end_byte - self.start_byte
    }

    /// Si el nodo está vacío.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Parser de sintaxis que puede parsear múltiples lenguajes.
///
/// NOTA: Esta es una estructura base. Para usar gramáticas específicas,
/// necesitamos cargar las gramáticas tree-sitter correspondientes.
pub struct SyntaxParser {
    parser: Parser,
}

impl std::fmt::Debug for SyntaxParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyntaxParser {{ ... }}")
    }
}

impl SyntaxParser {
    /// Crea un parser nuevo sin lenguaje asignado.
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    /// Parsea código fuente y devuelve el árbol de sintaxis.
    ///
    /// Requiere que se haya configurado un lenguaje previamente.
    /// Devuelve None si no hay lenguaje configurado o hay error.
    pub fn parse(&mut self, source: &str) -> Option<Syntax> {
        let tree = self.parser.parse(source, None)?;
        Some(Syntax::new(tree))
    }

    /// Parsea código con un árbol previo para edición incremental.
    pub fn parse_incremental(&mut self, source: &str, old_tree: &Syntax) -> Option<Syntax> {
        let tree = self.parser.parse(source, Some(&old_tree.tree))?;
        Some(Syntax::new(tree))
    }
}

impl Default for SyntaxParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_node_creation() {
        let node = SyntaxNode {
            kind: "function".to_string(),
            start_byte: 0,
            end_byte: 50,
            start_row: 0,
            start_col: 0,
            end_row: 2,
            end_col: 1,
            is_named: true,
        };

        assert_eq!(node.kind, "function");
        assert_eq!(node.len(), 50);
        assert_eq!(node.byte_range(), 0..50);
    }

    #[test]
    fn test_parser_creation() {
        let parser = SyntaxParser::new();
        // Parser created successfully (no language set yet)
        assert!(true);
    }

    // Note: Actual parsing tests require loading a tree-sitter grammar,
    // which we'll add when we include specific language support crates.
}
