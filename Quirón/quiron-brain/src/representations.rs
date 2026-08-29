//! # Representaciones Semánticas
//!
//! Generador determinista de representaciones.
//! No usa LLM - usa heurísticas y análisis léxico.
//!
//! ## Representaciones
//! - EmbeddingVector: vector de embedding para búsqueda semántica
//! - SemanticPatchSummary: resumen de cambios de código
//! - AstPatternDetector: detecta patrones como guard_clause, error_handling
//! - LearnedCorrelations: correlaciones test ↔ símbolo

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Check if a word exists in text as a whole word (not substring).
/// Avoids false positives like "prefix" matching "fix".
fn contains_word(hay: &str, needle: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == needle)
}

// ============================================================================
// EMBEDDING VECTORS
// ============================================================================

/// Vector de embedding para representaciones semánticas.
///
/// Los embeddings permiten búsqueda por similitud semántica:
/// - Conceptos similares → vectores cercanos
/// - Cosine similarity para medir cercanía
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingVector {
    /// Vector de dimensiones (típicamente 768-1536)
    pub values: Vec<f32>,
    /// Modelo que generó el embedding
    pub model: String,
    /// Versión del modelo
    pub model_version: Option<String>,
}

impl EmbeddingVector {
    /// Crear nuevo embedding
    pub fn new(values: Vec<f32>, model: impl Into<String>) -> Self {
        Self {
            values,
            model: model.into(),
            model_version: None,
        }
    }

    /// Crear nuevo embedding con versión de modelo
    pub fn new_versioned(
        values: Vec<f32>,
        model: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            values,
            model: model.into(),
            model_version: Some(version.into()),
        }
    }

    /// Embedding determinista por hashing léxico (sin LLM).
    /// Útil como fallback cuando no hay modelo disponible.
    ///
    /// Algoritmo:
    /// - Tokeniza por whitespace
    /// - Hashea cada token con blake3 a un índice [0..dims)
    /// - Acumula +1/-1 según bit del hash
    /// - Normaliza L2
    pub fn lexical_hash(text: &str, dims: usize) -> Self {
        assert!(dims > 0, "dimensions must be > 0");

        let mut v = vec![0.0f32; dims];

        for tok in text.split_whitespace().map(|t| t.to_lowercase()) {
            let h = blake3::hash(tok.as_bytes());
            let bytes = h.as_bytes();

            // Índice: 4 bytes -> u32
            let idx = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize % dims;

            // Signo: usa un bit para repartir +1/-1
            let sign = if (bytes[4] & 1) == 0 { 1.0 } else { -1.0 };
            v[idx] += sign;
        }

        // Normalización L2
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }

        Self {
            values: v,
            model: "lexhash".to_string(),
            model_version: Some("v1".to_string()),
        }
    }

    /// Dimensionalidad del vector
    pub fn dimensions(&self) -> usize {
        self.values.len()
    }

    /// Calcular cosine similarity con otro vector.
    ///
    /// Returns None if:
    /// - Dimensions don't match
    /// - Models don't match (comparing pears to apples is meaningless)
    /// - Either vector has zero norm
    pub fn cosine_similarity(&self, other: &EmbeddingVector) -> Option<f32> {
        // FIX B: Validate model compatibility
        if self.values.len() != other.values.len() {
            return None;
        }
        if self.model != other.model {
            return None;
        }
        if self.model_version != other.model_version {
            return None;
        }

        let dot_product: f32 = self
            .values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_a: f32 = self.values.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.values.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return None;
        }

        Some(dot_product / (norm_a * norm_b))
    }

    /// Calcular cosine similarity sin validar modelo (para casos especiales)
    pub fn cosine_similarity_unchecked(&self, other: &EmbeddingVector) -> Option<f32> {
        if self.values.len() != other.values.len() {
            return None;
        }

        let dot_product: f32 = self
            .values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_a: f32 = self.values.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.values.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return None;
        }

        Some(dot_product / (norm_a * norm_b))
    }

    /// Crear embedding placeholder (para testing, sin modelo real)
    #[cfg(test)]
    pub fn placeholder(dimensions: usize) -> Self {
        Self {
            values: vec![0.0; dimensions],
            model: "placeholder".to_string(),
            model_version: None,
        }
    }
}

// ============================================================================
// SEMANTIC PATCH SUMMARY
// ============================================================================

/// Resumen semántico de un cambio (patch)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticPatchSummary {
    /// Archivo modificado
    pub file_path: String,
    /// Tipo de cambio detectado
    pub change_type: ChangeType,
    /// Símbolos afectados (funciones, clases, etc.)
    pub affected_symbols: Vec<String>,
    /// Patrones detectados
    pub patterns: Vec<Pattern>,
    /// Resumen corto generado (heurístico)
    pub summary: String,
    /// Impacto estimado (0.0-1.0)
    pub impact: f32,
}

/// Tipo de cambio detectado
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    /// Nueva función/método
    AddFunction,
    /// Eliminar función/método
    RemoveFunction,
    /// Modificar función existente
    ModifyFunction,
    /// Añadir/modificar imports
    ModifyImports,
    /// Cambiar documentación/comentarios
    Documentation,
    /// Refactor (rename, move)
    Refactor,
    /// Fix de bug
    BugFix,
    /// Cambio de configuración
    Config,
    /// Test nuevo/modificado
    Test,
    /// Cambio mixto o no clasificable
    Mixed,
}

/// Patrón de código detectado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub name: String,
    pub confidence: f32,
    pub location: Option<String>,
}

impl SemanticPatchSummary {
    /// Generar resumen a partir de un diff unificado
    pub fn from_diff(file_path: &str, diff: &str) -> Self {
        let analyzer = DiffAnalyzer::new(diff);

        let change_type = analyzer.detect_change_type();
        let affected_symbols = analyzer.extract_affected_symbols();
        let patterns = analyzer.detect_patterns();
        let summary = analyzer.generate_summary();
        let impact = analyzer.estimate_impact();

        Self {
            file_path: file_path.to_string(),
            change_type,
            affected_symbols,
            patterns,
            summary,
            impact,
        }
    }
}

/// Analizador de diff
struct DiffAnalyzer<'a> {
    diff: &'a str,
    added_lines: Vec<&'a str>,
    removed_lines: Vec<&'a str>,
}

impl<'a> DiffAnalyzer<'a> {
    fn new(diff: &'a str) -> Self {
        let mut added_lines = Vec::new();
        let mut removed_lines = Vec::new();

        for line in diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                added_lines.push(&line[1..]);
            } else if line.starts_with('-') && !line.starts_with("---") {
                removed_lines.push(&line[1..]);
            }
        }

        Self {
            diff,
            added_lines,
            removed_lines,
        }
    }

    fn detect_change_type(&self) -> ChangeType {
        let added = self.added_lines.join("\n").to_lowercase();
        let removed = self.removed_lines.join("\n").to_lowercase();

        // Detect signals
        let has_test = added.contains("#[test]") || added.contains("fn test_");
        if has_test {
            return ChangeType::Test;
        }

        let has_fn = added.contains("fn ") || removed.contains("fn ");
        let has_use = self
            .added_lines
            .iter()
            .any(|l| l.trim_start().starts_with("use "));
        let has_docs = self.added_lines.iter().any(|l| {
            let t = l.trim_start();
            t.starts_with("///") || t.starts_with("//!")
        });

        // FIX #4: Use word boundary check to avoid false positives ("prefix" matching "fix")
        let has_bugfix = contains_word(&added, "fix")
            || contains_word(&added, "bug")
            || self.diff.contains("fixes #")
            || self.diff.contains("bug #");

        if has_bugfix {
            return ChangeType::BugFix;
        }

        // FIX #3: Prioritize symbol changes over imports/docs
        // If we touch symbols (fn/struct/enum/impl/trait), prioritize that
        if has_fn {
            // Determine if it's add, remove, or modify
            let added_fn = added.contains("fn ");
            let removed_fn = removed.contains("fn ");

            if added_fn && !removed_fn {
                return ChangeType::AddFunction;
            } else if !added_fn && removed_fn {
                return ChangeType::RemoveFunction;
            } else {
                return ChangeType::ModifyFunction;
            }
        }

        // Only classify as imports/docs if no symbol changes
        if has_use && !has_fn {
            return ChangeType::ModifyImports;
        }

        if has_docs && !has_fn {
            return ChangeType::Documentation;
        }

        // Config
        if added.contains("[package]")
            || added.contains("[dependencies]")
            || contains_word(&added, "config")
            || contains_word(&added, "settings")
        {
            return ChangeType::Config;
        }

        ChangeType::Mixed
    }

    /// Extract affected symbols using BTreeSet for O(n log n) + deterministic order.
    fn extract_affected_symbols(&self) -> Vec<String> {
        let mut symbols = BTreeSet::new();

        for line in self.added_lines.iter().chain(self.removed_lines.iter()) {
            let l = line.trim_start();

            // Skip comments
            if l.starts_with("//") {
                continue;
            }

            // Rust function: pub fn name(
            if let Some(start) = l.find("fn ") {
                let rest = &l[start + 3..];
                if let Some(end) = rest.find('(') {
                    let name = rest[..end].trim();
                    if !name.is_empty() {
                        symbols.insert(name.to_string());
                    }
                }
            }

            // Rust struct/enum/impl/trait
            for keyword in &["struct ", "enum ", "impl ", "trait "] {
                if let Some(start) = l.find(keyword) {
                    let rest = &l[start + keyword.len()..];
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        symbols.insert(name);
                    }
                }
            }
        }

        symbols.into_iter().collect()
    }

    fn detect_patterns(&self) -> Vec<Pattern> {
        let mut patterns = Vec::new();
        let code = self.added_lines.join("\n");

        // Guard clause pattern
        if code.contains("if ") && code.contains("return ") {
            patterns.push(Pattern {
                name: "guard_clause".to_string(),
                confidence: 0.7,
                location: None,
            });
        }

        // Error handling pattern
        if code.contains("Result<")
            || code.contains("Option<")
            || code.contains(".map_err(")
            || code.contains(".ok_or(")
        {
            patterns.push(Pattern {
                name: "error_handling".to_string(),
                confidence: 0.8,
                location: None,
            });
        }

        // Builder pattern
        if code.contains("-> Self") && code.contains("self.") {
            patterns.push(Pattern {
                name: "builder_pattern".to_string(),
                confidence: 0.6,
                location: None,
            });
        }

        // Logging/tracing
        if code.contains("tracing::")
            || code.contains("log::")
            || code.contains("println!")
            || code.contains("eprintln!")
        {
            patterns.push(Pattern {
                name: "logging".to_string(),
                confidence: 0.9,
                location: None,
            });
        }

        // Async pattern
        if code.contains("async ") || code.contains(".await") {
            patterns.push(Pattern {
                name: "async".to_string(),
                confidence: 0.95,
                location: None,
            });
        }

        patterns
    }

    fn generate_summary(&self) -> String {
        let change_type = self.detect_change_type();
        let symbols = self.extract_affected_symbols();
        let added_count = self.added_lines.len();
        let removed_count = self.removed_lines.len();

        let change_desc = match change_type {
            ChangeType::AddFunction => "Added",
            ChangeType::RemoveFunction => "Removed",
            ChangeType::ModifyFunction => "Modified",
            ChangeType::BugFix => "Fixed bug in",
            ChangeType::Test => "Added/modified test for",
            ChangeType::Documentation => "Updated documentation for",
            ChangeType::ModifyImports => "Updated imports for",
            ChangeType::Refactor => "Refactored",
            ChangeType::Config => "Changed configuration in",
            ChangeType::Mixed => "Changed",
        };

        let symbols_str = if symbols.is_empty() {
            "code".to_string()
        } else if symbols.len() <= 3 {
            symbols.join(", ")
        } else {
            format!("{} and {} more", symbols[..2].join(", "), symbols.len() - 2)
        };

        format!(
            "{} {} (+{}, -{})",
            change_desc, symbols_str, added_count, removed_count
        )
    }

    fn estimate_impact(&self) -> f32 {
        let total_lines = self.added_lines.len() + self.removed_lines.len();
        let symbols = self.extract_affected_symbols();

        // Base impact from line count
        let line_impact = (total_lines as f32 / 100.0).min(0.4);

        // Symbol impact
        let symbol_impact = (symbols.len() as f32 * 0.1).min(0.3);

        // Pattern impact (some patterns indicate higher impact)
        let patterns = self.detect_patterns();
        let pattern_impact: f32 = patterns
            .iter()
            .map(|p| match p.name.as_str() {
                "error_handling" => 0.15,
                "async" => 0.1,
                _ => 0.05,
            })
            .sum::<f32>()
            .min(0.3);

        (line_impact + symbol_impact + pattern_impact).min(1.0)
    }
}

/// Detector de patrones AST (sin tree-sitter aún, usa heurísticas)
pub struct AstPatternDetector;

impl AstPatternDetector {
    /// Detectar patrones en código fuente
    pub fn detect(source: &str) -> Vec<Pattern> {
        let mut patterns = Vec::new();

        // Guard clause
        let guard_count = source.matches("if ").count();
        let early_return_count = source.matches("return ").count();
        if guard_count > 0 && early_return_count > 0 {
            let confidence = (early_return_count as f32 / guard_count as f32).min(1.0);
            patterns.push(Pattern {
                name: "guard_clause".to_string(),
                confidence: confidence * 0.8,
                location: None,
            });
        }

        // Error handling pattern
        let result_count = source.matches("Result<").count();
        let question_mark_count = source.matches("?").count();
        if result_count > 0 || question_mark_count > 2 {
            patterns.push(Pattern {
                name: "error_handling".to_string(),
                confidence: 0.9,
                location: None,
            });
        }

        // Iteration patterns
        if source.contains(".iter()") || source.contains(".into_iter()") {
            patterns.push(Pattern {
                name: "iteration".to_string(),
                confidence: 0.95,
                location: None,
            });
        }

        // Functional patterns
        if source.contains(".map(")
            || source.contains(".filter(")
            || source.contains(".fold(")
            || source.contains(".reduce(")
        {
            patterns.push(Pattern {
                name: "functional".to_string(),
                confidence: 0.9,
                location: None,
            });
        }

        // Unsafe code
        if source.contains("unsafe ") {
            patterns.push(Pattern {
                name: "unsafe_code".to_string(),
                confidence: 1.0,
                location: None,
            });
        }

        patterns
    }
}

/// Correlaciones aprendidas (test ↔ símbolo)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearnedCorrelations {
    /// Mapa: símbolo → tests que lo cubren
    pub symbol_to_tests: HashMap<String, Vec<String>>,
    /// Mapa: test → símbolos que testa
    pub test_to_symbols: HashMap<String, Vec<String>>,
}

impl LearnedCorrelations {
    /// Crear nueva instancia
    pub fn new() -> Self {
        Self::default()
    }

    /// Registrar que un test cubre un símbolo.
    /// Evita duplicados: si el par (symbol, test) ya existe, no se añade de nuevo.
    pub fn register(&mut self, symbol: &str, test: &str) {
        let tests_vec = self.symbol_to_tests.entry(symbol.to_string()).or_default();
        if !tests_vec.iter().any(|t| t == test) {
            tests_vec.push(test.to_string());
        }

        let symbols_vec = self.test_to_symbols.entry(test.to_string()).or_default();
        if !symbols_vec.iter().any(|s| s == symbol) {
            symbols_vec.push(symbol.to_string());
        }
    }

    /// Obtener tests que cubren un símbolo
    pub fn tests_for_symbol(&self, symbol: &str) -> Vec<&str> {
        self.symbol_to_tests
            .get(symbol)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Obtener símbolos cubiertos por un test
    pub fn symbols_for_test(&self, test: &str) -> Vec<&str> {
        self.test_to_symbols
            .get(test)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_analysis() {
        let diff = r#"
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
+use std::io;
+
 pub fn hello() {
     println!("Hello");
 }
+
+pub fn new_function() -> Result<(), io::Error> {
+    Ok(())
+}
"#;

        let summary = SemanticPatchSummary::from_diff("src/lib.rs", diff);
        assert_eq!(summary.file_path, "src/lib.rs");
        assert!(!summary.affected_symbols.is_empty());
        assert!(summary.impact > 0.0);
    }

    #[test]
    fn test_pattern_detection() {
        let code = r#"
fn validate(input: &str) -> Result<(), Error> {
    if input.is_empty() {
        return Err(Error::Empty);
    }
    
    let items: Vec<_> = input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    
    Ok(())
}
"#;

        let patterns = AstPatternDetector::detect(code);
        let pattern_names: Vec<_> = patterns.iter().map(|p| p.name.as_str()).collect();

        assert!(pattern_names.contains(&"guard_clause"));
        assert!(pattern_names.contains(&"error_handling"));
        assert!(pattern_names.contains(&"functional"));
    }

    #[test]
    fn test_correlations() {
        let mut corr = LearnedCorrelations::new();

        corr.register("parse_config", "test_parse_valid_config");
        corr.register("parse_config", "test_parse_invalid_config");
        corr.register("save_config", "test_save_config");

        let tests = corr.tests_for_symbol("parse_config");
        assert_eq!(tests.len(), 2);

        let symbols = corr.symbols_for_test("test_save_config");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0], "save_config");
    }

    #[test]
    fn test_embedding_cosine_similarity() {
        // Identical vectors -> similarity = 1.0
        let v1 = EmbeddingVector::new(vec![1.0, 0.0, 0.0], "test");
        let v2 = EmbeddingVector::new(vec![1.0, 0.0, 0.0], "test");
        let sim = v1.cosine_similarity(&v2).unwrap();
        assert!((sim - 1.0).abs() < 0.0001);

        // Orthogonal vectors -> similarity = 0.0
        let v3 = EmbeddingVector::new(vec![1.0, 0.0], "test");
        let v4 = EmbeddingVector::new(vec![0.0, 1.0], "test");
        let sim2 = v3.cosine_similarity(&v4).unwrap();
        assert!((sim2 - 0.0).abs() < 0.0001);

        // Different dimensions -> None
        let v5 = EmbeddingVector::new(vec![1.0, 0.0, 0.0], "test");
        let v6 = EmbeddingVector::new(vec![1.0, 0.0], "test");
        assert!(v5.cosine_similarity(&v6).is_none());
    }
}
