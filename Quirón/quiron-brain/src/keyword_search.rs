//! # Keyword Search
//!
//! Búsqueda por keywords en eventos (fallback sin Qdrant).
//! Ranking basado en TF-IDF simplificado + recency.

use crate::ledger::LedgerReader;
use crate::types::Event;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resultado de búsqueda por keywords
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordSearchResult {
    /// Evento encontrado
    pub event: Event,
    /// Score de relevancia (0.0-1.0)
    pub score: f32,
    /// Keywords encontrados
    pub matched_keywords: Vec<String>,
}

/// Respuesta de búsqueda
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<KeywordSearchResult>,
    pub total_matches: usize,
}

/// Configuración del buscador
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Máximo de resultados
    pub max_results: usize,
    /// Peso de recency (0.0-1.0)
    pub recency_weight: f32,
    /// Días para decaimiento de recency
    pub recency_decay_days: i64,
    /// Mínimo score para incluir
    pub min_score: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: 20,
            recency_weight: 0.3,
            recency_decay_days: 30,
            min_score: 0.1,
        }
    }
}

/// Buscador por keywords
pub struct KeywordSearcher<'a> {
    reader: &'a LedgerReader,
    config: SearchConfig,
}

impl<'a> KeywordSearcher<'a> {
    /// Crear buscador con config por defecto
    pub fn new(reader: &'a LedgerReader) -> Self {
        Self {
            reader,
            config: SearchConfig::default(),
        }
    }

    /// Crear con configuración personalizada
    pub fn with_config(reader: &'a LedgerReader, config: SearchConfig) -> Self {
        Self { reader, config }
    }

    /// Buscar eventos por query de texto
    pub fn search(&self, query: &str) -> crate::error::Result<SearchResponse> {
        // Tokenizar query
        let query_tokens = self.tokenize(query);
        if query_tokens.is_empty() {
            return Ok(SearchResponse {
                query: query.to_string(),
                results: vec![],
                total_matches: 0,
            });
        }

        // Obtener eventos recientes
        let events = self.reader.recent(500)?;
        let now = Utc::now();

        // Calcular IDF para cada token
        let idf = self.compute_idf(&query_tokens, &events);

        // Scoring
        let mut results: Vec<KeywordSearchResult> = events
            .into_iter()
            .filter_map(|event| {
                let (score, matched) = self.score_event(&event, &query_tokens, &idf, now);
                if score >= self.config.min_score && !matched.is_empty() {
                    Some(KeywordSearchResult {
                        event,
                        score,
                        matched_keywords: matched,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Ordenar por score desc
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limitar resultados
        let total_matches = results.len();
        results.truncate(self.config.max_results);

        Ok(SearchResponse {
            query: query.to_string(),
            results,
            total_matches,
        })
    }

    /// Tokenizar texto
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|s| s.len() > 2)
            .map(String::from)
            .collect()
    }

    /// Calcular IDF para cada token
    fn compute_idf(&self, tokens: &[String], events: &[Event]) -> HashMap<String, f32> {
        let n = events.len() as f32;
        let mut doc_freq: HashMap<String, u32> = HashMap::new();

        for event in events {
            let event_tokens: std::collections::HashSet<_> =
                self.tokenize(&event.description).into_iter().collect();

            for token in tokens {
                if event_tokens.contains(token) {
                    *doc_freq.entry(token.clone()).or_default() += 1;
                }
            }
        }

        doc_freq
            .into_iter()
            .map(|(token, df)| {
                let idf = (n / (df as f32 + 1.0)).ln() + 1.0;
                (token, idf)
            })
            .collect()
    }

    /// Calcular score para un evento
    fn score_event(
        &self,
        event: &Event,
        query_tokens: &[String],
        idf: &HashMap<String, f32>,
        now: chrono::DateTime<Utc>,
    ) -> (f32, Vec<String>) {
        let event_tokens = self.tokenize(&event.description);
        let mut matched = Vec::new();
        let mut tf_idf_sum = 0.0f32;

        // Calcular TF-IDF
        for token in query_tokens {
            let tf = event_tokens.iter().filter(|t| *t == token).count() as f32;
            if tf > 0.0 {
                matched.push(token.clone());
                let default_idf = 1.0;
                let token_idf = idf.get(token).unwrap_or(&default_idf);
                tf_idf_sum += tf * token_idf;
            }
        }

        if matched.is_empty() {
            return (0.0, vec![]);
        }

        // Normalizar TF-IDF
        let max_tf_idf = query_tokens.len() as f32 * 2.0; // Asumiendo máximo
        let tf_idf_score = (tf_idf_sum / max_tf_idf).min(1.0);

        // Calcular recency score
        let age_days = (now - event.ts).num_days() as f32;
        let recency_score = (-age_days / self.config.recency_decay_days as f32).exp();

        // Combinar scores
        let keyword_weight = 1.0 - self.config.recency_weight;
        let final_score =
            keyword_weight * tf_idf_score + self.config.recency_weight * recency_score;

        // Boost por importancia del evento
        let importance_boost = event.importance * 0.2;
        let final_score = (final_score + importance_boost).min(1.0);

        (final_score, matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::TempDir;

    fn setup() -> (TempDir, LedgerReader) {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let reader = LedgerReader::new(storage);
        (dir, reader)
    }

    #[test]
    fn test_search_empty_query() {
        let (_dir, reader) = setup();
        let searcher = KeywordSearcher::new(&reader);

        let result = searcher.search("").unwrap();
        assert!(result.results.is_empty());
    }

    #[test]
    fn test_search_no_results() {
        let (_dir, reader) = setup();
        let searcher = KeywordSearcher::new(&reader);

        let result = searcher.search("nonexistent query terms").unwrap();
        assert!(result.results.is_empty());
    }

    #[test]
    fn test_tokenize() {
        let (_dir, reader) = setup();
        let searcher = KeywordSearcher::new(&reader);

        let tokens = searcher.tokenize("Hello, World! This is a test-query_here");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test-query_here".to_string()));
        // "is" y "a" son muy cortos, excluidos
        assert!(!tokens.contains(&"is".to_string()));
    }

    #[test]
    fn test_config_defaults() {
        let config = SearchConfig::default();
        assert_eq!(config.max_results, 20);
        assert!(config.recency_weight > 0.0);
    }
}
