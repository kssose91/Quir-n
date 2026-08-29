//! # Virtual Context Tools
//!
//! Herramientas para manipular el contexto virtual del agente.
//! Inspirado en: "If I don't know, I search" - Virtual Context Tools
//!
//! ## Herramientas
//! - `recall(query)` - Buscar en la memoria
//! - `archive(event_ids)` - Mover eventos a cold storage
//! - `pin(event_id)` - Marcar evento como importante (alta importance)
//! - `timeline(entity)` - Historia de un símbolo/archivo

use crate::distillation::DistilledMemoryStore;
use crate::error::Result;
use crate::graph::GraphBuilder;
use crate::ledger::LedgerReader;
use crate::memory::{
    MemoryEnvelopeStore, MemoryKind, MemoryScope, MemorySource, MemoryTarget, PromotionStatus,
    TruthStatus,
};
#[cfg(feature = "neo4j")]
use crate::neo4j::Neo4jConnector;
#[cfg(feature = "semantic")]
use crate::semantic::SemanticClient;
use crate::storage::Storage;
use crate::types::Event;
use chrono::Utc;
#[cfg(feature = "neo4j")]
use neo4rs::Query as CypherQuery;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[cfg(feature = "semantic")]
use std::sync::Arc;
use tokio::sync::RwLock;

/// Resultado de recall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    /// Query original
    pub query: String,
    /// Eventos encontrados (ordenados por relevancia)
    pub events: Vec<RecallMatch>,
    /// Tiempo de búsqueda en ms
    pub search_time_ms: u64,
    /// Estrategia usada (recency, semantic, keyword)
    pub strategy: String,
}

/// Un match de recall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallMatch {
    /// Tipo de registro recuperado.
    #[serde(default)]
    pub record_kind: RecallRecordKind,
    /// ID del evento
    pub event_id: String,
    /// ID de memoria destilada si aplica.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    /// Eventos fuente cuando el match procede de memoria consolidada.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_event_ids: Option<Vec<String>>,
    /// Descripción del evento
    pub description: String,
    /// Kind del evento
    pub kind: String,
    /// Timestamp
    pub timestamp: String,
    /// Score de relevancia (0-1)
    pub score: f32,
    /// Por qué se seleccionó este evento
    pub why_selected: String,
    /// Estado epistemológico derivado del memory envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truth_status: Option<TruthStatus>,
    /// Confianza derivada del memory envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Origen del recuerdo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_source: Option<MemorySource>,
    /// Ámbito del recuerdo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope: Option<MemoryScope>,
    /// Módulo lógico principal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_id: Option<String>,
    /// Archivos asociados al recuerdo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_refs: Option<Vec<String>>,
    /// Símbolos asociados al recuerdo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_refs: Option<Vec<String>>,
    /// Etiquetas de lógica o dominio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logic_tags: Option<Vec<String>>,
    /// Clase canónica de memoria.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_kind: Option<MemoryKind>,
    /// Event ID que retractó esta memoria si existe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retracted_by: Option<String>,
    /// Estado de promoción derivado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_status: Option<PromotionStatus>,
    /// Destinos de promoción previstos.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_targets: Option<Vec<MemoryTarget>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecallRecordKind {
    #[default]
    Event,
    DistilledMemory,
}

impl RecallMatch {
    fn merge_key(&self) -> String {
        match self.record_kind {
            RecallRecordKind::Event => format!("event:{}", self.event_id),
            RecallRecordKind::DistilledMemory => match self.memory_id.as_deref() {
                Some(memory_id) => format!("distilled:{}", memory_id),
                None => format!("distilled-anchor:{}", self.event_id),
            },
        }
    }
}

/// Filtros estructurados para recall híbrido.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallFilters {
    /// Limitar a un proyecto concreto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Estado epistemológico concreto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truth_status: Option<TruthStatus>,
    /// Origen del recuerdo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_source: Option<MemorySource>,
    /// Ámbito del recuerdo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope: Option<MemoryScope>,
    /// Filtrar por módulo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_id: Option<String>,
    /// Filtrar por archivo asociado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_ref: Option<String>,
    /// Filtrar por símbolo asociado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_ref: Option<String>,
    /// Filtrar por etiqueta lógica.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logic_tag: Option<String>,
    /// Filtrar por tipo canónico de memoria.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_kind: Option<MemoryKind>,
    /// Estado de promoción actual.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_status: Option<PromotionStatus>,
    /// Destino de promoción esperado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_target: Option<MemoryTarget>,
}

impl RecallFilters {
    pub fn is_active(&self) -> bool {
        self.project_id.is_some()
            || self.truth_status.is_some()
            || self.memory_source.is_some()
            || self.memory_scope.is_some()
            || self.module_id.is_some()
            || self.file_ref.is_some()
            || self.symbol_ref.is_some()
            || self.logic_tag.is_some()
            || self.memory_kind.is_some()
            || self.promotion_status.is_some()
            || self.memory_target.is_some()
    }
}

/// Scope para recall
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallScope {
    /// Solo eventos recientes (últimas N horas)
    Recent,
    /// Solo de un proyecto específico
    Project,
    /// Solo archivos tocados
    Files,
    /// Todo (búsqueda semántica si disponible)
    All,
}

/// Resultado de archive
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResult {
    /// Eventos archivados
    pub archived: Vec<String>,
    /// Eventos que no se pudieron archivar
    pub failed: Vec<String>,
}

/// Resultado de pin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinResult {
    /// Si fue exitoso
    pub success: bool,
    /// ID del evento pinnado
    pub event_id: String,
    /// Nueva importance del evento
    pub new_importance: f32,
}

/// Timeline de una entidad
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineResult {
    /// Entidad consultada
    pub entity: String,
    /// Tipo de entidad (file, symbol, project)
    pub entity_type: String,
    /// Eventos ordenados cronológicamente
    pub events: Vec<TimelineEntry>,
}

/// Entrada en timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub event_id: String,
    pub kind: String,
    pub description: String,
    pub timestamp: String,
    /// Cambios específicos (si es un archivo)
    pub changes: Option<String>,
}

/// Tokeniza texto en palabras completas (evita substring matching)
fn tokens(s: &str) -> std::collections::HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3) // filtra ruido: "is", "a", "de"
        .map(|t| t.to_string())
        .collect()
}

/// Motor de Virtual Context Tools
pub struct VirtualContextTools<'a> {
    #[allow(dead_code)] // Reserved for future archive/pin operations
    storage: &'a Storage,
    reader: &'a LedgerReader,
    graph: Option<&'a RwLock<GraphBuilder>>,
    #[cfg(feature = "semantic")]
    semantic: Option<Arc<SemanticClient>>,
    #[cfg(feature = "neo4j")]
    neo4j: Option<&'a Neo4jConnector>,
}

impl<'a> VirtualContextTools<'a> {
    /// Crear instancia (sin búsqueda semántica)
    pub fn new(storage: &'a Storage, reader: &'a LedgerReader) -> Self {
        Self {
            storage,
            reader,
            graph: None,
            #[cfg(feature = "semantic")]
            semantic: None,
            #[cfg(feature = "neo4j")]
            neo4j: None,
        }
    }

    /// Añadir acceso al grafo local para recall relacional.
    pub fn with_graph(mut self, graph: &'a RwLock<GraphBuilder>) -> Self {
        self.graph = Some(graph);
        self
    }

    /// Añadir búsqueda semántica al recall canónico.
    #[cfg(feature = "semantic")]
    pub fn with_semantic(mut self, semantic: Arc<SemanticClient>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Añadir Neo4j al recall canónico.
    #[cfg(feature = "neo4j")]
    pub fn with_neo4j(mut self, neo4j: &'a Neo4jConnector) -> Self {
        self.neo4j = Some(neo4j);
        self
    }

    /// Recall: buscar en memoria
    ///
    /// Estrategia:
    /// 1. Si hay búsqueda semántica → usarla
    /// 2. Si no → keyword matching en description + tags
    /// 3. Siempre: boost por recency e importance
    pub fn recall(&self, query: &str, scope: RecallScope, limit: usize) -> Result<RecallResult> {
        let start = std::time::Instant::now();

        // Obtener eventos candidatos según scope
        let candidates = match scope {
            RecallScope::Recent => self.reader.recent(limit * 3)?,
            RecallScope::All => {
                // Usar keyword search para recall semántico
                // Cuando semantic feature esté disponible, se usará embedding search
                use crate::keyword_search::KeywordSearcher;
                let searcher = KeywordSearcher::new(self.reader);
                match searcher.search(query) {
                    Ok(results) => results.results.into_iter().map(|r| r.event).collect(),
                    Err(_) => self.reader.recent(limit * 3)?, // fallback
                }
            }
            RecallScope::Project => {
                // Intentar extraer proyecto del query
                // Por ahora, fallback a recent
                self.reader.recent(limit * 3)?
            }
            RecallScope::Files => {
                // Buscar eventos que mencionen archivos
                self.reader
                    .recent(limit * 3)?
                    .into_iter()
                    .filter(|e| !e.inputs.is_empty() || !e.outputs.is_empty())
                    .collect()
            }
        };

        // Scoring con tokenización (evita substring matching)
        let query_tokens = tokens(query);
        let now = Utc::now();

        let mut scored: Vec<(Event, f32)> = candidates
            .into_iter()
            .map(|e| {
                // Tokenizar descripción + tags + inputs
                let desc_tokens = tokens(&e.description);
                let tag_tokens: std::collections::HashSet<String> =
                    e.tags.iter().flat_map(|t| tokens(t)).collect();
                let input_tokens: std::collections::HashSet<String> =
                    e.inputs.iter().flat_map(|i| tokens(i)).collect();

                // Keyword matching (solo tokens completos)
                let desc_matches = query_tokens.intersection(&desc_tokens).count();
                let tag_matches = query_tokens.intersection(&tag_tokens).count();
                let input_matches = query_tokens.intersection(&input_tokens).count();
                let total_matches = desc_matches + tag_matches + input_matches;

                // Score por matches (0.0-0.5)
                let keyword_score = ((desc_matches as f32 * 0.15)
                    + (tag_matches as f32 * 0.1)
                    + (input_matches as f32 * 0.05))
                    .min(0.5);

                // Recency boost escalado (0.0-0.3), NO clamped
                let age_hours = (now - e.ts).num_hours().max(0) as f32;
                let base = 24.0 / (24.0 + age_hours);
                let recency_score = 0.3 * base;

                // Importance boost (0.0-0.2)
                let importance_score = e.importance * 0.2;

                let score = keyword_score + recency_score + importance_score;

                (e, score, total_matches)
            })
            // Bug fix #4: Exigir al menos 1 match si la query no está vacía
            .filter(|(_, _, matches)| query_tokens.is_empty() || *matches > 0)
            .map(|(e, score, _)| (e, score))
            .filter(|(_, s)| *s > 0.0)
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        let events: Vec<RecallMatch> = scored
            .into_iter()
            .map(|(e, score)| RecallMatch {
                record_kind: RecallRecordKind::Event,
                event_id: e.id.to_string(),
                memory_id: None,
                source_event_ids: None,
                description: e.description.clone(),
                kind: format!("{:?}", e.kind),
                timestamp: e.ts.to_rfc3339(),
                score,
                why_selected: self.explain_selection(&e, &query_tokens, score),
                truth_status: None,
                confidence: None,
                memory_source: None,
                memory_scope: None,
                module_id: None,
                file_refs: None,
                symbol_refs: None,
                logic_tags: None,
                memory_kind: None,
                retracted_by: None,
                promotion_status: None,
                promotion_targets: None,
            })
            .collect();

        Ok(self.annotate_recall_result(RecallResult {
            query: query.to_string(),
            events,
            search_time_ms: start.elapsed().as_millis() as u64,
            strategy: "keyword+recency".to_string(),
        }))
    }

    /// Recall asíncrono canónico: combina ledger, semántica, grafo y Neo4j cuando existen.
    pub async fn recall_async(
        &self,
        query: &str,
        scope: RecallScope,
        limit: usize,
    ) -> Result<RecallResult> {
        let start = std::time::Instant::now();
        let keyword = self.recall(query, scope, limit)?;

        #[cfg(feature = "semantic")]
        let semantic = if scope == RecallScope::All {
            self.semantic_recall_matches(query, limit).await
        } else {
            Vec::new()
        };
        #[cfg(not(feature = "semantic"))]
        let semantic: Vec<RecallMatch> = Vec::new();

        let graph = self.graph_recall_matches(query, limit).await;

        #[cfg(feature = "neo4j")]
        let neo4j = if scope == RecallScope::All {
            self.neo4j_recall_matches(query, limit).await
        } else {
            Vec::new()
        };
        #[cfg(not(feature = "neo4j"))]
        let neo4j: Vec<RecallMatch> = Vec::new();

        let distilled = if matches!(scope, RecallScope::All | RecallScope::Project) {
            self.distilled_recall_matches(query, limit).await
        } else {
            Vec::new()
        };

        Ok(self.annotate_recall_result(merge_recall_sources(
            query,
            limit,
            keyword,
            semantic,
            graph,
            neo4j,
            distilled,
            start.elapsed().as_millis() as u64,
        )))
    }

    /// Recall canónico con filtros estructurados sobre la memoria ya anotada.
    pub async fn recall_async_filtered(
        &self,
        query: &str,
        scope: RecallScope,
        limit: usize,
        filters: &RecallFilters,
    ) -> Result<RecallResult> {
        let fetch_limit = if filters.is_active() {
            limit.saturating_mul(5).min(500)
        } else {
            limit
        };

        let mut result = self.recall_async(query, scope, fetch_limit).await?;
        if !filters.is_active() {
            return Ok(result);
        }

        result
            .events
            .retain(|item| self.recall_match_satisfies_filters(item, filters));
        result.events.truncate(limit);
        result.strategy = format!("{}+filters", result.strategy);
        Ok(result)
    }

    /// Explicar por qué se seleccionó un evento (usa tokens para match exacto)
    fn explain_selection(
        &self,
        event: &Event,
        query_tokens: &HashSet<String>,
        score: f32,
    ) -> String {
        let mut reasons = Vec::new();

        // Token matches (palabras completas)
        let desc_tokens = tokens(&event.description);
        let matched: Vec<&String> = query_tokens.intersection(&desc_tokens).collect();
        if !matched.is_empty() {
            reasons.push(format!(
                "matched: {}",
                matched
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Recent
        let now = Utc::now();
        let age_hours = (now - event.ts).num_hours();
        if age_hours < 24 {
            reasons.push("recent (last 24h)".to_string());
        }

        // High importance
        if event.importance > 0.7 {
            reasons.push(format!("important ({:.0}%)", event.importance * 100.0));
        }

        if reasons.is_empty() {
            format!("score: {:.2}", score)
        } else {
            reasons.join("; ")
        }
    }

    fn annotate_recall_result(&self, mut result: RecallResult) -> RecallResult {
        for item in &mut result.events {
            if let Some(envelope) = self.lookup_memory_envelope(&item.event_id) {
                if item.truth_status.is_none() {
                    item.truth_status = Some(envelope.truth_status);
                }
                if item.confidence.is_none() {
                    item.confidence = Some(envelope.confidence);
                }
                if item.memory_source.is_none() {
                    item.memory_source = Some(envelope.source);
                }
                if item.memory_scope.is_none() {
                    item.memory_scope = Some(envelope.scope);
                }
                if item.module_id.is_none() {
                    item.module_id = envelope.module_id.clone();
                }
                if item.file_refs.is_none() {
                    item.file_refs =
                        (!envelope.file_refs.is_empty()).then(|| envelope.file_refs.clone());
                }
                if item.symbol_refs.is_none() {
                    item.symbol_refs =
                        (!envelope.symbol_refs.is_empty()).then(|| envelope.symbol_refs.clone());
                }
                if item.logic_tags.is_none() {
                    item.logic_tags =
                        (!envelope.logic_tags.is_empty()).then(|| envelope.logic_tags.clone());
                }
                if item.memory_kind.is_none() {
                    item.memory_kind = Some(envelope.memory_kind);
                }
                if item.retracted_by.is_none() {
                    item.retracted_by = envelope.retracted_by.map(|event_id| event_id.to_string());
                }
                if item.promotion_status.is_none() {
                    item.promotion_status = Some(envelope.promotion_status);
                }
                if item.promotion_targets.is_none() {
                    item.promotion_targets = Some(envelope.promotion_targets.clone());
                }
            }
        }

        result
    }

    fn recall_match_satisfies_filters(&self, item: &RecallMatch, filters: &RecallFilters) -> bool {
        if let Some(project_id) = filters.project_id.as_deref() {
            let Some(event) = self.lookup_event(&item.event_id) else {
                return false;
            };
            if event.project_id.as_deref() != Some(project_id) {
                return false;
            }
        }

        if let Some(truth_status) = filters.truth_status {
            if item.truth_status != Some(truth_status) {
                return false;
            }
        }

        if let Some(memory_source) = filters.memory_source {
            if item.memory_source != Some(memory_source) {
                return false;
            }
        }

        if let Some(memory_scope) = filters.memory_scope {
            if item.memory_scope != Some(memory_scope) {
                return false;
            }
        }

        if let Some(module_id) = filters.module_id.as_deref() {
            if item.module_id.as_deref() != Some(module_id) {
                return false;
            }
        }

        if let Some(file_ref) = filters.file_ref.as_deref() {
            let Some(file_refs) = item.file_refs.as_ref() else {
                return false;
            };
            if !file_refs.iter().any(|candidate| candidate == file_ref) {
                return false;
            }
        }

        if let Some(symbol_ref) = filters.symbol_ref.as_deref() {
            let Some(symbol_refs) = item.symbol_refs.as_ref() else {
                return false;
            };
            if !symbol_refs.iter().any(|candidate| candidate == symbol_ref) {
                return false;
            }
        }

        if let Some(logic_tag) = filters.logic_tag.as_deref() {
            let Some(logic_tags) = item.logic_tags.as_ref() else {
                return false;
            };
            if !logic_tags.iter().any(|candidate| candidate == logic_tag) {
                return false;
            }
        }

        if let Some(memory_kind) = filters.memory_kind {
            if item.memory_kind != Some(memory_kind) {
                return false;
            }
        }

        if let Some(promotion_status) = filters.promotion_status {
            if item.promotion_status != Some(promotion_status) {
                return false;
            }
        }

        if let Some(memory_target) = filters.memory_target {
            let Some(targets) = item.promotion_targets.as_ref() else {
                return false;
            };
            if !targets.contains(&memory_target) {
                return false;
            }
        }

        true
    }

    fn lookup_event(&self, event_id: &str) -> Option<Event> {
        let parsed = ulid::Ulid::from_string(event_id).ok()?;
        let event_id = crate::types::EventId::from_bytes(&parsed.to_bytes())?;
        match self.reader.get(&event_id) {
            Ok(Some(event)) => Some(event),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("Failed to load event {} for recall filter: {}", event_id, e);
                None
            }
        }
    }

    fn lookup_memory_envelope(&self, event_id: &str) -> Option<crate::memory::MemoryEnvelope> {
        let event = self.lookup_event(event_id)?;
        let store = MemoryEnvelopeStore::new(self.storage.clone());

        match store.get_or_default(&event) {
            Ok(envelope) => Some(envelope),
            Err(e) => {
                tracing::warn!(
                    "Failed to load memory envelope for recall event {}: {}",
                    event_id,
                    e
                );
                None
            }
        }
    }

    /// Archive: mover eventos a cold storage
    ///
    /// NOTA: Sin un LedgerWriter, no podemos persistir cambios.
    /// Por honestidad, todos los eventos se reportan en `failed` hasta implementar escritura.
    pub fn archive(&self, event_ids: &[String]) -> Result<ArchiveResult> {
        // HONESTO: sin escritura real, todo falla
        let failed: Vec<String> = event_ids.to_vec();
        Ok(ArchiveResult {
            archived: vec![],
            failed,
        })
    }

    /// Pin: marcar evento como importante
    ///
    /// NOTA: Sin un LedgerWriter, no podemos persistir el cambio de importance.
    /// Por honestidad, siempre retorna success=false hasta implementar escritura.
    #[allow(unused_variables)]
    pub fn pin(&self, event_id: &str) -> Result<PinResult> {
        // HONESTO: sin escritura real, siempre falla
        Ok(PinResult {
            success: false,
            event_id: event_id.to_string(),
            new_importance: 0.0,
        })
    }

    /// Timeline: historia de una entidad
    ///
    /// Tipos de entidad:
    /// - file: /path/to/file
    /// - symbol: class::method
    /// - project: project-name
    pub fn timeline(&self, entity: &str, limit: usize) -> Result<TimelineResult> {
        // Determine entity type
        let entity_type = if entity.starts_with('/') {
            "file"
        } else if entity.contains("::") {
            "symbol"
        } else {
            "project"
        };

        // Get events mentioning this entity
        let all_events = self.reader.recent(500)?;

        // Bug fix #1: filtra primero, ordena cronológicamente, LUEGO trunca
        let mut matching: Vec<Event> = all_events
            .into_iter()
            .filter(|e| {
                // Check inputs/outputs for files
                e.inputs.iter().any(|f| f.contains(entity))
                    || e.outputs.iter().any(|f| f.contains(entity))
                    || e.description.contains(entity)
                    || e.project_id.as_deref() == Some(entity)
            })
            .collect();

        // Ordenar cronológicamente (ascendente: más antiguo primero)
        matching.sort_by(|a, b| a.ts.cmp(&b.ts));
        matching.truncate(limit);

        let events: Vec<TimelineEntry> = matching
            .into_iter()
            .map(|e| TimelineEntry {
                event_id: e.id.to_string(),
                kind: format!("{:?}", e.kind),
                description: e.description.clone(),
                timestamp: e.ts.to_rfc3339(),
                changes: if !e.outputs.is_empty() {
                    Some(e.outputs.join(", "))
                } else {
                    None
                },
            })
            .collect();

        Ok(TimelineResult {
            entity: entity.to_string(),
            entity_type: entity_type.to_string(),
            events,
        })
    }

    #[cfg(feature = "semantic")]
    async fn semantic_recall_matches(&self, query: &str, limit: usize) -> Vec<RecallMatch> {
        let Some(ref semantic) = self.semantic else {
            return Vec::new();
        };

        match semantic.search_ranked(query, limit as u64).await {
            Ok(results) => results
                .into_iter()
                .map(|ranked| RecallMatch {
                    record_kind: RecallRecordKind::Event,
                    event_id: ranked
                        .result
                        .event_id
                        .unwrap_or_else(|| ranked.result.id.clone()),
                    memory_id: None,
                    source_event_ids: None,
                    description: ranked.result.description.unwrap_or_default(),
                    kind: ranked.result.kind.unwrap_or_else(|| "unknown".to_string()),
                    timestamp: ranked.result.timestamp.unwrap_or_default(),
                    score: ranked.final_score,
                    why_selected: if ranked.reasons.is_empty() {
                        format!("semantic similarity: {:.2}", ranked.result.score)
                    } else {
                        format!(
                            "semantic similarity: {:.2}; {}",
                            ranked.result.score,
                            ranked.reasons.join(", ")
                        )
                    },
                    truth_status: None,
                    confidence: None,
                    memory_source: None,
                    memory_scope: None,
                    module_id: None,
                    file_refs: None,
                    symbol_refs: None,
                    logic_tags: None,
                    memory_kind: None,
                    retracted_by: None,
                    promotion_status: None,
                    promotion_targets: None,
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Semantic recall failed: {}", e);
                Vec::new()
            }
        }
    }

    async fn graph_recall_matches(&self, query: &str, limit: usize) -> Vec<RecallMatch> {
        let Some(graph_lock) = self.graph else {
            return Vec::new();
        };

        let query_tokens = tokens(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let related_events: Vec<(crate::types::EventId, f32, String)> = {
            let graph = graph_lock.read().await;
            let nodes = match graph.all_nodes() {
                Ok(nodes) => nodes,
                Err(e) => {
                    tracing::warn!("Graph recall failed to list nodes: {}", e);
                    return Vec::new();
                }
            };

            let mut related: HashMap<crate::types::EventId, (f32, String)> = HashMap::new();

            for node in nodes {
                let mut score = recall_text_score(query, &query_tokens, &node.name);
                if let Some(desc) = &node.description {
                    score = score.max(recall_text_score(query, &query_tokens, desc));
                }

                if score <= 0.0 {
                    continue;
                }

                let node_reason = format!("matched graph node '{}' ({:?})", node.name, node.kind);
                related
                    .entry(node.created_by_event)
                    .and_modify(|existing| {
                        if score > existing.0 {
                            *existing = (score, node_reason.clone());
                        }
                    })
                    .or_insert((score, node_reason.clone()));

                let mut adjacent_edges = Vec::new();
                if let Ok(mut edges) = graph.edges_to(&node.id) {
                    adjacent_edges.append(&mut edges);
                }
                if let Ok(mut edges) = graph.edges_from(&node.id) {
                    adjacent_edges.append(&mut edges);
                }

                for edge in adjacent_edges {
                    let neighbor_id = if edge.source == node.id {
                        edge.target
                    } else {
                        edge.source
                    };

                    if let Ok(Some(neighbor)) = graph.get_node(&neighbor_id) {
                        let neighbor_score = (score * 0.85).min(0.85);
                        let neighbor_reason =
                            format!("graph relation {:?} via '{}'", edge.kind, node.name);
                        related
                            .entry(neighbor.created_by_event)
                            .and_modify(|existing| {
                                if neighbor_score > existing.0 {
                                    *existing = (neighbor_score, neighbor_reason.clone());
                                }
                            })
                            .or_insert((neighbor_score, neighbor_reason));
                    }
                }
            }

            let mut collected: Vec<_> = related
                .into_iter()
                .map(|(event_id, (score, reason))| (event_id, score, reason))
                .collect();

            collected.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            collected.truncate(limit * 3);
            collected
        };

        let mut matches = Vec::new();
        for (event_id, score, reason) in related_events {
            match self.reader.get(&event_id) {
                Ok(Some(event)) => matches.push(RecallMatch {
                    record_kind: RecallRecordKind::Event,
                    event_id: event.id.to_string(),
                    memory_id: None,
                    source_event_ids: None,
                    description: event.description,
                    kind: format!("{:?}", event.kind),
                    timestamp: event.ts.to_rfc3339(),
                    score,
                    why_selected: reason,
                    truth_status: None,
                    confidence: None,
                    memory_source: None,
                    memory_scope: None,
                    module_id: None,
                    file_refs: None,
                    symbol_refs: None,
                    logic_tags: None,
                    memory_kind: None,
                    retracted_by: None,
                    promotion_status: None,
                    promotion_targets: None,
                }),
                Ok(None) => {}
                Err(e) => tracing::warn!("Graph recall failed to load event {}: {}", event_id, e),
            }
        }

        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(limit);
        matches
    }

    #[cfg(feature = "neo4j")]
    async fn neo4j_recall_matches(&self, query: &str, limit: usize) -> Vec<RecallMatch> {
        let Some(neo4j) = self.neo4j else {
            return Vec::new();
        };

        let needles = recall_needles(query);
        if needles.is_empty() {
            return Vec::new();
        }

        let query_lower = query.trim().to_lowercase();
        let mut merged: HashMap<String, RecallAggregate> = HashMap::new();

        for needle in needles {
            let is_full_query = needle == query_lower;
            let direct_query = CypherQuery::new(
                r#"
                MATCH (e:Event)
                WHERE toLower(coalesce(e.description, '')) CONTAINS $needle
                   OR toLower(coalesce(e.kind, '')) CONTAINS $needle
                   OR toLower(coalesce(e.project_id, '')) CONTAINS $needle
                   OR toLower(coalesce(e.agent_id, '')) CONTAINS $needle
                RETURN e.id AS event_id,
                       coalesce(e.description, '') AS description,
                       coalesce(e.kind, '') AS kind,
                       toString(e.ts) AS timestamp,
                       $reason AS reason,
                       $score AS score
                ORDER BY e.ts DESC
                LIMIT $limit
            "#
                .to_string(),
            )
            .param("needle", needle.clone())
            .param(
                "reason",
                format!("matched Neo4j Event fields for '{}'", needle),
            )
            .param("score", if is_full_query { 0.48_f64 } else { 0.38_f64 })
            .param("limit", (limit * 2) as i64);

            match neo4j.fetch_all_query(direct_query).await {
                Ok(rows) => {
                    for row in rows {
                        let (
                            Ok(event_id),
                            Ok(description),
                            Ok(kind),
                            Ok(timestamp),
                            Ok(reason),
                            Ok(score),
                        ) = (
                            row.get::<String>("event_id"),
                            row.get::<String>("description"),
                            row.get::<String>("kind"),
                            row.get::<String>("timestamp"),
                            row.get::<String>("reason"),
                            row.get::<f64>("score"),
                        )
                        else {
                            continue;
                        };

                        let entry =
                            merged
                                .entry(event_id.clone())
                                .or_insert_with(|| RecallAggregate {
                                    template: RecallMatch {
                                        record_kind: RecallRecordKind::Event,
                                        event_id: event_id.clone(),
                                        memory_id: None,
                                        source_event_ids: None,
                                        description: description.clone(),
                                        kind: kind.clone(),
                                        timestamp: timestamp.clone(),
                                        score: score as f32,
                                        why_selected: reason.clone(),
                                        truth_status: None,
                                        confidence: None,
                                        memory_source: None,
                                        memory_scope: None,
                                        module_id: None,
                                        file_refs: None,
                                        symbol_refs: None,
                                        logic_tags: None,
                                        memory_kind: None,
                                        retracted_by: None,
                                        promotion_status: None,
                                        promotion_targets: None,
                                    },
                                    description: description.clone(),
                                    kind: kind.clone(),
                                    timestamp: timestamp.clone(),
                                    best_score: score as f32,
                                    sources: HashSet::new(),
                                    reasons: Vec::new(),
                                });

                        if entry.description.is_empty() && !description.is_empty() {
                            entry.description = description;
                        }
                        if entry.kind.is_empty() && !kind.is_empty() {
                            entry.kind = kind;
                        }
                        if entry.timestamp.is_empty() && !timestamp.is_empty() {
                            entry.timestamp = timestamp;
                        }

                        entry.best_score = entry.best_score.max(score as f32);
                        entry.sources.insert("neo4j");
                        if !entry.reasons.iter().any(|existing| existing == &reason) {
                            entry.reasons.push(reason);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Neo4j direct recall failed for '{}': {}", needle, e);
                }
            }

            let related_query = CypherQuery::new(
                r#"
                MATCH path = (e:Event)-[*1..2]-(n)
                WHERE NOT n:Event
                  AND (
                        toLower(coalesce(n.path, '')) CONTAINS $needle
                     OR toLower(coalesce(n.content, '')) CONTAINS $needle
                     OR toLower(coalesce(n.description, '')) CONTAINS $needle
                     OR toLower(coalesce(n.fqn, '')) CONTAINS $needle
                     OR toLower(coalesce(n.name, '')) CONTAINS $needle
                     OR toLower(coalesce(n.id, '')) CONTAINS $needle
                  )
                RETURN DISTINCT e.id AS event_id,
                       coalesce(e.description, '') AS description,
                       coalesce(e.kind, '') AS kind,
                       toString(e.ts) AS timestamp,
                       head(labels(n)) AS node_label,
                       coalesce(n.path, n.content, n.description, n.fqn, n.name, n.id, '') AS matched_text,
                       length(path) AS hops
                ORDER BY hops ASC, timestamp DESC
                LIMIT $limit
            "#
                .to_string(),
            )
            .param("needle", needle.clone())
            .param("limit", (limit * 2) as i64);

            match neo4j.fetch_all_query(related_query).await {
                Ok(rows) => {
                    for row in rows {
                        let (
                            Ok(event_id),
                            Ok(description),
                            Ok(kind),
                            Ok(timestamp),
                            Ok(node_label),
                            Ok(matched_text),
                            Ok(hops),
                        ) = (
                            row.get::<String>("event_id"),
                            row.get::<String>("description"),
                            row.get::<String>("kind"),
                            row.get::<String>("timestamp"),
                            row.get::<String>("node_label"),
                            row.get::<String>("matched_text"),
                            row.get::<i64>("hops"),
                        )
                        else {
                            continue;
                        };

                        let hop_penalty = ((hops.saturating_sub(1)) as f32) * 0.06;
                        let base_score = if is_full_query { 0.56 } else { 0.44 };
                        let score = (base_score - hop_penalty).max(0.22);
                        let reason = format!(
                            "matched Neo4j {} at {} hop(s): {}",
                            node_label,
                            hops,
                            summarize_recall_text(&matched_text, 96)
                        );

                        let entry =
                            merged
                                .entry(event_id.clone())
                                .or_insert_with(|| RecallAggregate {
                                    template: RecallMatch {
                                        record_kind: RecallRecordKind::Event,
                                        event_id: event_id.clone(),
                                        memory_id: None,
                                        source_event_ids: None,
                                        description: description.clone(),
                                        kind: kind.clone(),
                                        timestamp: timestamp.clone(),
                                        score,
                                        why_selected: reason.clone(),
                                        truth_status: None,
                                        confidence: None,
                                        memory_source: None,
                                        memory_scope: None,
                                        module_id: None,
                                        file_refs: None,
                                        symbol_refs: None,
                                        logic_tags: None,
                                        memory_kind: None,
                                        retracted_by: None,
                                        promotion_status: None,
                                        promotion_targets: None,
                                    },
                                    description: description.clone(),
                                    kind: kind.clone(),
                                    timestamp: timestamp.clone(),
                                    best_score: score,
                                    sources: HashSet::new(),
                                    reasons: Vec::new(),
                                });

                        if entry.description.is_empty() && !description.is_empty() {
                            entry.description = description;
                        }
                        if entry.kind.is_empty() && !kind.is_empty() {
                            entry.kind = kind;
                        }
                        if entry.timestamp.is_empty() && !timestamp.is_empty() {
                            entry.timestamp = timestamp;
                        }

                        entry.best_score = entry.best_score.max(score);
                        entry.sources.insert("neo4j");
                        if !entry.reasons.iter().any(|existing| existing == &reason) {
                            entry.reasons.push(reason);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Neo4j relational recall failed for '{}': {}", needle, e);
                }
            }
        }

        let mut matches: Vec<_> = merged
            .into_iter()
            .map(|(event_id, aggregate)| RecallMatch {
                record_kind: RecallRecordKind::Event,
                event_id,
                memory_id: None,
                source_event_ids: None,
                description: aggregate.description,
                kind: aggregate.kind,
                timestamp: aggregate.timestamp,
                score: aggregate.best_score.min(0.9),
                why_selected: aggregate.reasons.join(" | "),
                truth_status: None,
                confidence: None,
                memory_source: None,
                memory_scope: None,
                module_id: None,
                file_refs: None,
                symbol_refs: None,
                logic_tags: None,
                memory_kind: None,
                retracted_by: None,
                promotion_status: None,
                promotion_targets: None,
            })
            .collect();

        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(limit);
        matches
    }

    async fn distilled_recall_matches(&self, query: &str, limit: usize) -> Vec<RecallMatch> {
        let query_tokens = tokens(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let store = DistilledMemoryStore::new(self.storage.clone());
        let memories = match store.list() {
            Ok(memories) => memories,
            Err(e) => {
                tracing::warn!("Distilled recall failed to list memories: {}", e);
                return Vec::new();
            }
        };

        let mut matches = Vec::new();
        for memory in memories {
            let topic_score = recall_text_score(query, &query_tokens, &memory.topic);
            let essence_score = recall_text_score(query, &query_tokens, &memory.essence);
            let tags_score = recall_text_score(query, &query_tokens, &memory.tags.join(" "));
            let pattern_names = memory
                .patterns
                .iter()
                .map(|pattern| pattern.name.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let pattern_score = recall_text_score(query, &query_tokens, &pattern_names);
            let lexical_score = topic_score
                .max(essence_score)
                .max(tags_score * 0.8)
                .max(pattern_score * 0.85);

            if lexical_score <= 0.0 {
                continue;
            }

            let reinforcement_boost = (memory.reinforcement_count as f32 / 12.0).min(0.18);
            let age_hours = (Utc::now() - memory.last_seen).num_hours().max(0) as f32;
            let recency_boost = 0.12 * (24.0 / (24.0 + age_hours));
            let score =
                (lexical_score + (memory.importance * 0.18) + reinforcement_boost + recency_boost)
                    .min(0.96);

            let anchor_event = memory
                .source_events
                .first()
                .and_then(|event_id| self.reader.get(event_id).ok().flatten());
            let anchor_event_id = anchor_event
                .as_ref()
                .map(|event| event.id.to_string())
                .or_else(|| {
                    memory
                        .source_events
                        .first()
                        .map(|event_id| event_id.to_string())
                })
                .unwrap_or_else(|| memory.id.to_string());
            let memory_scope = if anchor_event
                .as_ref()
                .and_then(|event| event.project_id.as_ref())
                .is_some()
            {
                MemoryScope::Project
            } else {
                MemoryScope::Global
            };
            let pattern_confidence = if memory.patterns.is_empty() {
                0.72
            } else {
                memory
                    .patterns
                    .iter()
                    .map(|pattern| pattern.confidence)
                    .sum::<f32>()
                    / memory.patterns.len() as f32
            };
            let confidence =
                ((memory.importance * 0.55) + (pattern_confidence * 0.45)).clamp(0.6, 0.95);

            let mut reasons = Vec::new();
            if topic_score > 0.0 {
                reasons.push(format!("distilled topic: {}", memory.topic));
            }
            if essence_score > 0.0 {
                reasons.push(format!(
                    "distilled essence: {}",
                    summarize_recall_text(&memory.essence, 96)
                ));
            }
            if !memory.patterns.is_empty() {
                let pattern_labels = memory
                    .patterns
                    .iter()
                    .take(2)
                    .map(|pattern| pattern.name.clone())
                    .collect::<Vec<_>>();
                reasons.push(format!("patterns: {}", pattern_labels.join(", ")));
            }

            matches.push(RecallMatch {
                record_kind: RecallRecordKind::DistilledMemory,
                event_id: anchor_event_id,
                memory_id: Some(memory.id.to_string()),
                source_event_ids: Some(
                    memory
                        .source_events
                        .iter()
                        .map(|event_id| event_id.to_string())
                        .collect(),
                ),
                description: memory.essence.clone(),
                kind: "DistilledMemory".to_string(),
                timestamp: memory.last_seen.to_rfc3339(),
                score,
                why_selected: reasons.join("; "),
                truth_status: Some(TruthStatus::Summarized),
                confidence: Some(confidence),
                memory_source: Some(MemorySource::Derived),
                memory_scope: Some(memory_scope),
                module_id: None,
                file_refs: None,
                symbol_refs: None,
                logic_tags: None,
                memory_kind: Some(MemoryKind::Insight),
                retracted_by: None,
                promotion_status: Some(PromotionStatus::Promoted),
                promotion_targets: Some(vec![MemoryTarget::Distilled]),
            });
        }

        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(limit);
        matches
    }
}

fn recall_text_score(query: &str, query_tokens: &HashSet<String>, text: &str) -> f32 {
    let text_lower = text.to_lowercase();
    let text_tokens = tokens(&text_lower);
    let overlap = query_tokens.intersection(&text_tokens).count() as f32;

    let mut score = 0.0;
    if !query.trim().is_empty() && text_lower.contains(&query.to_lowercase()) {
        score += 0.45;
    }
    score += (overlap * 0.12).min(0.36);
    score.min(0.9)
}

#[cfg(feature = "neo4j")]
fn recall_needles(query: &str) -> Vec<String> {
    let query_lower = query.trim().to_lowercase();
    let mut needles = Vec::new();

    if query_lower.len() >= 2 {
        needles.push(query_lower.clone());
    }

    let mut parts: Vec<_> = tokens(query).into_iter().collect();
    parts.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    for token in parts.into_iter().take(5) {
        if !needles.iter().any(|needle| needle == &token) {
            needles.push(token);
        }
    }

    needles
}

fn summarize_recall_text(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    let mut chars = compact.chars();

    for _ in 0..max_chars {
        if let Some(ch) = chars.next() {
            out.push(ch);
        } else {
            return compact;
        }
    }

    if chars.next().is_some() {
        out.push_str("...");
    }

    out
}

#[derive(Debug)]
struct RecallAggregate {
    template: RecallMatch,
    description: String,
    kind: String,
    timestamp: String,
    best_score: f32,
    sources: HashSet<&'static str>,
    reasons: Vec<String>,
}

fn merge_recall_sources(
    query: &str,
    limit: usize,
    keyword: RecallResult,
    semantic: Vec<RecallMatch>,
    graph: Vec<RecallMatch>,
    neo4j: Vec<RecallMatch>,
    distilled: Vec<RecallMatch>,
    elapsed_ms: u64,
) -> RecallResult {
    let mut merged: HashMap<String, RecallAggregate> = HashMap::new();

    let mut ingest = |source: &'static str, matches: Vec<RecallMatch>| {
        for item in matches {
            let merge_key = item.merge_key();
            let entry = merged.entry(merge_key).or_insert_with(|| RecallAggregate {
                template: item.clone(),
                description: item.description.clone(),
                kind: item.kind.clone(),
                timestamp: item.timestamp.clone(),
                best_score: item.score,
                sources: HashSet::new(),
                reasons: Vec::new(),
            });

            if entry.description.is_empty() && !item.description.is_empty() {
                entry.description = item.description.clone();
            }
            if entry.kind.is_empty() && !item.kind.is_empty() {
                entry.kind = item.kind.clone();
            }
            if entry.timestamp.is_empty() && !item.timestamp.is_empty() {
                entry.timestamp = item.timestamp.clone();
            }

            entry.best_score = entry.best_score.max(item.score);
            entry.sources.insert(source);

            let reason = format!("{}: {}", source, item.why_selected);
            if !entry.reasons.iter().any(|r| r == &reason) {
                entry.reasons.push(reason);
            }
        }
    };

    let mut strategy_parts = vec!["ledger"];
    ingest("ledger", keyword.events);

    if !semantic.is_empty() {
        strategy_parts.push("semantic");
        ingest("semantic", semantic);
    }

    if !graph.is_empty() {
        strategy_parts.push("graph");
        ingest("graph", graph);
    }

    if !neo4j.is_empty() {
        strategy_parts.push("neo4j");
        ingest("neo4j", neo4j);
    }

    if !distilled.is_empty() {
        strategy_parts.push("distilled");
        ingest("distilled", distilled);
    }

    let mut events: Vec<RecallMatch> = merged
        .into_iter()
        .map(|(_, aggregate)| {
            let source_bonus = (aggregate.sources.len().saturating_sub(1) as f32) * 0.08;
            let mut item = aggregate.template;
            item.description = aggregate.description;
            item.kind = aggregate.kind;
            item.timestamp = aggregate.timestamp;
            item.score = (aggregate.best_score + source_bonus).min(1.0);
            item.why_selected = aggregate.reasons.join(" | ");
            item
        })
        .collect();

    events.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    events.truncate(limit);

    RecallResult {
        query: query.to_string(),
        events,
        search_time_ms: elapsed_ms,
        strategy: format!("hybrid:{}", strategy_parts.join("+")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distillation::{DistilledMemory, DistilledMemoryStore, Pattern};
    use crate::ledger::LedgerWriter;
    use crate::types::EventKind;
    use chrono::Utc;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Storage, LedgerReader) {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let reader = LedgerReader::new(storage.clone());
        (dir, storage, reader)
    }

    #[test]
    fn test_recall_empty() {
        let (_dir, storage, reader) = setup();
        let vct = VirtualContextTools::new(&storage, &reader);
        let result = vct.recall("test", RecallScope::Recent, 10).unwrap();
        assert!(result.events.is_empty());
    }

    #[tokio::test]
    async fn test_recall_async_uses_canonical_hybrid_path() {
        let (_dir, storage, reader) = setup();
        let vct = VirtualContextTools::new(&storage, &reader);
        let result = vct
            .recall_async("test", RecallScope::All, 10)
            .await
            .unwrap();
        assert!(result.events.is_empty());
        assert_eq!(result.strategy, "hybrid:ledger");
    }

    #[tokio::test]
    async fn test_recall_async_filtered_applies_structured_memory_filters() {
        let (_dir, storage, reader) = setup();
        let writer = LedgerWriter::new(storage.clone());
        let derived = writer
            .append(
                Event::new(EventKind::PatternLearned, "filterable derived memory")
                    .with_project("proj-a")
                    .with_tags(vec![
                        "module:memory/core".to_string(),
                        "symbol:VirtualContextTools::recall_async".to_string(),
                        "logic:hybrid_recall".to_string(),
                        "memory_kind:insight".to_string(),
                    ])
                    .with_inputs(vec!["src/vct.rs".to_string()]),
            )
            .unwrap();
        writer
            .append(
                Event::new(EventKind::Decision, "filterable architectural decision")
                    .with_project("proj-a"),
            )
            .unwrap();

        let vct = VirtualContextTools::new(&storage, &reader);
        let filters = RecallFilters {
            project_id: Some("proj-a".to_string()),
            truth_status: Some(TruthStatus::Summarized),
            memory_source: Some(MemorySource::Derived),
            memory_scope: Some(MemoryScope::Project),
            module_id: Some("memory/core".to_string()),
            file_ref: Some("src/vct.rs".to_string()),
            symbol_ref: Some("VirtualContextTools::recall_async".to_string()),
            logic_tag: Some("hybrid_recall".to_string()),
            memory_kind: Some(MemoryKind::Insight),
            promotion_status: Some(PromotionStatus::Promoted),
            memory_target: Some(MemoryTarget::Distilled),
        };

        let result = vct
            .recall_async_filtered("filterable", RecallScope::All, 10, &filters)
            .await
            .unwrap();

        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].event_id, derived.id.to_string());
        assert_eq!(result.events[0].truth_status, Some(TruthStatus::Summarized));
        assert_eq!(result.events[0].memory_source, Some(MemorySource::Derived));
        assert_eq!(result.events[0].module_id.as_deref(), Some("memory/core"));
        assert_eq!(result.events[0].memory_kind, Some(MemoryKind::Insight));
        assert!(result.strategy.ends_with("+filters"));
    }

    #[tokio::test]
    async fn test_recall_async_includes_distilled_memories() {
        let (_dir, storage, reader) = setup();
        let writer = LedgerWriter::new(storage.clone());
        let anchor = writer
            .append(Event::new(
                EventKind::Observation,
                "raw traces from pipeline execution",
            ))
            .unwrap();

        DistilledMemoryStore::new(storage.clone())
            .save(&DistilledMemory {
                id: ulid::Ulid::new(),
                topic: "pipeline/backend".to_string(),
                essence: "backend retries become stable after adaptive throttling".to_string(),
                patterns: vec![Pattern {
                    name: "keyword_retries".to_string(),
                    frequency: 3,
                    confidence: 0.84,
                    example: None,
                }],
                source_events: vec![anchor.id],
                importance: 0.82,
                first_seen: Utc::now(),
                last_seen: Utc::now(),
                reinforcement_count: 3,
                tags: vec!["backend".to_string(), "retry".to_string()],
            })
            .unwrap();

        let vct = VirtualContextTools::new(&storage, &reader);
        let result = vct
            .recall_async("adaptive retries", RecallScope::All, 10)
            .await
            .unwrap();

        assert!(
            result.strategy.contains("distilled"),
            "distilled source should appear in strategy"
        );
        let distilled = result
            .events
            .iter()
            .find(|item| item.record_kind == RecallRecordKind::DistilledMemory)
            .expect("distilled memory should be included");
        assert_eq!(distilled.kind, "DistilledMemory");
        assert!(distilled.memory_id.is_some());
        assert_eq!(distilled.truth_status, Some(TruthStatus::Summarized));
        assert_eq!(distilled.memory_source, Some(MemorySource::Derived));
        assert_eq!(
            distilled.promotion_targets,
            Some(vec![MemoryTarget::Distilled])
        );
    }

    #[test]
    fn test_recall_includes_memory_envelope_metadata() {
        let (_dir, storage, reader) = setup();
        let writer = LedgerWriter::new(storage.clone());
        writer
            .append(Event::new(EventKind::ClaimMade, "claim metadata"))
            .unwrap();

        let vct = VirtualContextTools::new(&storage, &reader);
        let result = vct.recall("claim", RecallScope::All, 10).unwrap();

        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].truth_status, Some(TruthStatus::Inferred));
        assert_eq!(result.events[0].confidence, Some(0.6));
        assert_eq!(
            result.events[0].promotion_status,
            Some(PromotionStatus::WorkingSet)
        );
    }

    #[test]
    fn test_timeline_empty() {
        let (_dir, storage, reader) = setup();
        let vct = VirtualContextTools::new(&storage, &reader);
        let result = vct.timeline("/path/to/file.rs", 10).unwrap();
        assert_eq!(result.entity_type, "file");
        assert!(result.events.is_empty());
    }
}
