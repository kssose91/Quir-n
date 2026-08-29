//! # Retrieval y Context Building
//!
//! Sistema para generar contexto relevante para el agente.
//! Fase 4 del roadmap de Quirón.
//!
//! ## Componentes
//! - ContextBuilder: genera paquetes de contexto
//! - CandidateGenerator: filtros deterministas para archivos y símbolos
//! - ContextPacket: estructura del contexto entregado al agente

use crate::graph::GraphBuilder;
use crate::keyword_search::{KeywordSearcher, SearchConfig};
use crate::ledger::LedgerReader;
#[cfg(feature = "neo4j")]
use crate::neo4j::Neo4jConnector;
#[cfg(feature = "semantic")]
use crate::semantic::SemanticClient;
use crate::storage::Storage;
use crate::types::Event;
use crate::vct::{RecallFilters, RecallResult, RecallScope, VirtualContextTools};
use serde::{Deserialize, Serialize};
#[cfg(feature = "semantic")]
use std::sync::Arc;
use tokio::sync::RwLock;

/// Paquete de contexto para el agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPacket {
    /// Identidad del cerebro
    pub identity: Identity,
    /// Estado actual del sistema
    pub brain_status: BrainStatus,
    /// Gates activos
    pub active_gates: Vec<Gate>,
    /// Eventos recientes relevantes
    pub recent_events: Vec<EventSummary>,
    /// Resultados de recall si se pidió
    pub recall_results: Option<RecallResult>,
    /// Instrucciones adicionales
    pub instructions: Vec<String>,
    /// Tokens usados en este contexto
    pub token_count: u32,
}

/// Identidad del cerebro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    pub version: String,
    pub user: String,
    pub project: Option<String>,
}

/// Estado del cerebro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainStatus {
    pub ledger_events: u64,
    pub chain_valid: bool,
    pub last_event_ts: Option<String>,
    pub uptime_seconds: u64,
}

/// Gate activo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub name: String,
    pub description: String,
    pub active: bool,
}

/// Resumen de evento para contexto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSummary {
    pub id: String,
    pub kind: String,
    pub description: String,
    pub timestamp: String,
    pub importance: f32,
}

impl From<&Event> for EventSummary {
    fn from(e: &Event) -> Self {
        Self {
            id: e.id.to_string(),
            kind: format!("{:?}", e.kind),
            description: e.description.clone(),
            timestamp: e.ts.to_rfc3339(),
            importance: e.importance,
        }
    }
}

/// Configuración del builder
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Máximo de eventos recientes a incluir
    pub max_recent_events: usize,
    /// Presupuesto de tokens para contexto
    pub token_budget: u32,
    /// Incluir recall automático
    pub auto_recall: bool,
    /// Query para recall automático
    pub recall_query: Option<String>,
    /// Proyecto al que debe quedar limitada la recuperación.
    pub project_id: Option<String>,

    // === INYECTABLES (evita hardcoding) ===
    /// Override para chain_valid (None = no verificado = false)
    pub chain_valid_override: Option<bool>,
    /// Override para uptime_seconds (None = 0)
    pub uptime_seconds_override: Option<u64>,
    /// Override para gates (None = usa defaults)
    pub gates_override: Option<Vec<Gate>>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_recent_events: 10,
            token_budget: 2000,
            auto_recall: false,
            recall_query: None,
            project_id: None,
            chain_valid_override: None,
            uptime_seconds_override: None,
            gates_override: None,
        }
    }
}

/// Builder de contexto
pub struct ContextBuilder<'a> {
    storage: &'a Storage,
    reader: &'a LedgerReader,
    graph: Option<&'a RwLock<GraphBuilder>>,
    #[cfg(feature = "semantic")]
    semantic: Option<Arc<SemanticClient>>,
    #[cfg(feature = "neo4j")]
    neo4j: Option<&'a Neo4jConnector>,
    config: ContextConfig,
}

impl<'a> ContextBuilder<'a> {
    /// Crear nuevo builder
    pub fn new(storage: &'a Storage, reader: &'a LedgerReader) -> Self {
        Self {
            storage,
            reader,
            graph: None,
            #[cfg(feature = "semantic")]
            semantic: None,
            #[cfg(feature = "neo4j")]
            neo4j: None,
            config: ContextConfig::default(),
        }
    }

    /// Añadir grafo local para que el auto-recall use la ruta canónica híbrida.
    pub fn with_graph(mut self, graph: &'a RwLock<GraphBuilder>) -> Self {
        self.graph = Some(graph);
        self
    }

    /// Añadir búsqueda semántica al builder de contexto.
    #[cfg(feature = "semantic")]
    pub fn with_semantic(mut self, semantic: Arc<SemanticClient>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Añadir Neo4j al builder de contexto.
    #[cfg(feature = "neo4j")]
    pub fn with_neo4j(mut self, neo4j: &'a Neo4jConnector) -> Self {
        self.neo4j = Some(neo4j);
        self
    }

    /// Configurar el builder
    pub fn with_config(mut self, config: ContextConfig) -> Self {
        self.config = config;
        self
    }

    /// Construir paquete de contexto usando el recall canónico.
    pub async fn build(&self) -> crate::error::Result<ContextPacket> {
        // 1. Identity
        let identity = self.build_identity();

        // 2. Brain status
        let brain_status = self.build_status()?;

        // 3. Active gates
        let active_gates = self.build_gates();

        // 4. Recent events
        let recent_events = self.build_recent_events()?;

        // 5. Auto-recall si está configurado
        let recall_results = if self.config.auto_recall {
            if let Some(ref query) = self.config.recall_query {
                Some(
                    self.build_vct()
                        .recall_async_filtered(
                            query,
                            RecallScope::All,
                            5,
                            &RecallFilters {
                                project_id: self.config.project_id.clone(),
                                ..Default::default()
                            },
                        )
                        .await?,
                )
            } else {
                None
            }
        } else {
            None
        };

        // 6. Instructions
        let instructions = self.build_instructions();

        // 7. Token count (estimate)
        let token_count = self.estimate_tokens(&recent_events, &recall_results);

        Ok(ContextPacket {
            identity,
            brain_status,
            active_gates,
            recent_events,
            recall_results,
            instructions,
            token_count,
        })
    }

    fn build_vct(&self) -> VirtualContextTools<'a> {
        let mut vct = VirtualContextTools::new(self.storage, self.reader);

        if let Some(graph) = self.graph {
            vct = vct.with_graph(graph);
        }

        #[cfg(feature = "semantic")]
        if let Some(ref semantic) = self.semantic {
            vct = vct.with_semantic(Arc::clone(semantic));
        }

        #[cfg(feature = "neo4j")]
        if let Some(neo4j) = self.neo4j {
            vct = vct.with_neo4j(neo4j);
        }

        vct
    }

    fn build_identity(&self) -> Identity {
        // Try to detect project from current working directory
        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

        Identity {
            name: "Quirón".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            user: std::env::var("USER").unwrap_or_else(|_| "unknown".to_string()),
            project,
        }
    }

    fn build_status(&self) -> crate::error::Result<BrainStatus> {
        let event_count = self.reader.count()?;
        let recent = self.reader.recent(1)?;
        let last_ts = recent.first().map(|e| e.ts.to_rfc3339());

        Ok(BrainStatus {
            ledger_events: event_count,
            // FIX: Use override or default to false (don't lie about chain validity)
            chain_valid: self.config.chain_valid_override.unwrap_or(false),
            last_event_ts: last_ts,
            // FIX: Use override or default to 0
            uptime_seconds: self.config.uptime_seconds_override.unwrap_or(0),
        })
    }

    fn build_gates(&self) -> Vec<Gate> {
        // FIX: Use override if provided, otherwise use defaults
        if let Some(ref gates) = self.config.gates_override {
            return gates.clone();
        }

        // Default gates - these should come from invariant engine in production
        vec![
            Gate {
                name: "no-claim-without-evidence".to_string(),
                description: "Toda afirmación necesita evidencia".to_string(),
                active: true,
            },
            Gate {
                name: "no-ghost-editing".to_string(),
                description: "No editar archivos sin leerlos primero".to_string(),
                active: true,
            },
            Gate {
                name: "auto-revert-on-failure".to_string(),
                description: "Revertir automáticamente si falla".to_string(),
                active: true,
            },
        ]
    }

    fn build_recent_events(&self) -> crate::error::Result<Vec<EventSummary>> {
        let events = self.reader.recent(self.config.max_recent_events)?;
        Ok(events.iter().map(EventSummary::from).collect())
    }

    fn build_instructions(&self) -> Vec<String> {
        vec![
            "Usa recall() si necesitas buscar información pasada.".to_string(),
            "Registra todas las lecturas de archivos antes de editarlos.".to_string(),
            "Si no sabes algo, usa think_more en lugar de inventar.".to_string(),
        ]
    }

    fn estimate_tokens(&self, events: &[EventSummary], recall: &Option<RecallResult>) -> u32 {
        // Rough estimation: ~4 chars per token
        let events_chars: usize = events
            .iter()
            .map(|e| e.description.len() + e.kind.len() + 50)
            .sum();

        let recall_chars: usize = recall
            .as_ref()
            .map(|r| r.events.iter().map(|e| e.description.len() + 100).sum())
            .unwrap_or(0);

        let base = 200; // identity, status, gates
        ((events_chars + recall_chars + base) / 4) as u32
    }
}

/// Generador de candidatos para retrieval
pub struct CandidateGenerator<'a> {
    reader: &'a LedgerReader,
}

impl<'a> CandidateGenerator<'a> {
    pub fn new(reader: &'a LedgerReader) -> Self {
        Self { reader }
    }

    /// Generar candidatos usando la política léxica canónica.
    pub fn generate(&self, query: &str, limit: usize) -> crate::error::Result<Vec<Event>> {
        let searcher = KeywordSearcher::with_config(
            self.reader,
            SearchConfig {
                max_results: limit,
                ..SearchConfig::default()
            },
        );

        Ok(searcher
            .search(query)?
            .results
            .into_iter()
            .map(|result| result.event)
            .collect())
    }

    /// Generar candidatos por similitud de archivos.
    /// FIX: sort ANTES de take para no perder eventos relevantes.
    pub fn by_files(&self, files: &[&str], limit: usize) -> crate::error::Result<Vec<Event>> {
        let all = self.reader.recent(200)?;

        let mut matching: Vec<Event> = all
            .into_iter()
            .filter(|e| {
                files.iter().any(|f| {
                    e.inputs.iter().any(|i| i.contains(*f))
                        || e.outputs.iter().any(|o| o.contains(*f))
                        || e.artifacts.iter().any(|a| a.path.contains(*f))
                })
            })
            .collect();

        // Sort by timestamp desc, id asc for determinism BEFORE taking limit
        matching.sort_by(|a, b| {
            b.ts.cmp(&a.ts)
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        matching.truncate(limit);
        Ok(matching)
    }

    /// Generar candidatos por símbolos en descripción.
    /// FIX: sort ANTES de take para no perder eventos relevantes.
    pub fn by_symbols(&self, symbols: &[&str], limit: usize) -> crate::error::Result<Vec<Event>> {
        let all = self.reader.recent(200)?;

        let mut matching: Vec<Event> = all
            .into_iter()
            .filter(|e| symbols.iter().any(|s| e.description.contains(*s)))
            .collect();

        // Sort by timestamp desc, id asc for determinism BEFORE taking limit
        matching.sort_by(|a, b| {
            b.ts.cmp(&a.ts)
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        matching.truncate(limit);
        Ok(matching)
    }
}

/// Resultado de retrieval con score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub event: EventSummary,
    pub score: f32,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::LedgerWriter;
    use crate::types::EventKind;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Storage, LedgerReader) {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let reader = LedgerReader::new(storage.clone());
        (dir, storage, reader)
    }

    #[tokio::test]
    async fn test_context_builder() {
        let (_dir, storage, reader) = setup();

        let packet = ContextBuilder::new(&storage, &reader)
            .build()
            .await
            .unwrap();

        assert_eq!(packet.identity.name, "Quirón");
        assert_eq!(packet.active_gates.len(), 3);
        assert!(!packet.instructions.is_empty());
    }

    #[tokio::test]
    async fn test_context_builder_auto_recall_uses_canonical_recall() {
        let (_dir, storage, reader) = setup();

        let packet = ContextBuilder::new(&storage, &reader)
            .with_config(ContextConfig {
                auto_recall: true,
                recall_query: Some("test".to_string()),
                ..ContextConfig::default()
            })
            .build()
            .await
            .unwrap();

        let recall = packet
            .recall_results
            .expect("auto recall should be present");
        assert_eq!(recall.strategy, "hybrid:ledger");
    }

    #[tokio::test]
    async fn context_builder_limits_recall_to_project() {
        let (_dir, storage, reader) = setup();
        let writer = LedgerWriter::new(storage.clone());
        writer
            .append(
                Event::new(EventKind::Observation, "isolationmarker project alpha")
                    .with_project("project-a"),
            )
            .unwrap();
        writer
            .append(
                Event::new(EventKind::Observation, "isolationmarker project beta")
                    .with_project("project-b"),
            )
            .unwrap();

        let packet = ContextBuilder::new(&storage, &reader)
            .with_config(ContextConfig {
                auto_recall: true,
                recall_query: Some("isolationmarker".to_string()),
                project_id: Some("project-a".to_string()),
                ..ContextConfig::default()
            })
            .build()
            .await
            .unwrap();

        let recall = packet.recall_results.unwrap();
        assert_eq!(recall.events.len(), 1);
        assert!(recall.events[0].description.contains("project alpha"));
    }

    #[test]
    fn test_candidate_generator() {
        let (_dir, _storage, reader) = setup();

        let generator = CandidateGenerator::new(&reader);
        let candidates = generator.generate("test", 10).unwrap();

        // Empty ledger, no candidates
        assert!(candidates.is_empty());
    }
}
