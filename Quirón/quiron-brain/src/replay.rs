//! Canonical replay and backfill utilities for memory projections.

use crate::error::Result;
use crate::graph::GraphBuilder;
use crate::ledger::LedgerReader;
use crate::memory::MemoryEnvelopeStore;
use crate::storage::cf::{CF_EDGES_IN, CF_EDGES_OUT, CF_NODES};
use crate::storage::Storage;

#[cfg(feature = "neo4j")]
use crate::neo4j::{Neo4jConnector, SyncService as Neo4jSyncService};
#[cfg(feature = "semantic")]
use crate::semantic::{SemanticClient, SemanticSyncService};

#[cfg(any(feature = "neo4j", feature = "semantic"))]
use std::sync::Arc;

/// Replay and backfill report for envelopes.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct EnvelopeBackfillReport {
    pub total_events: u64,
    pub created: u64,
    pub updated: u64,
    pub unchanged: u64,
}

/// Replay report for the local graph projection.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct GraphReplayReport {
    pub total_events: u64,
    pub events_replayed: u64,
    pub cleared_nodes: u64,
    pub cleared_edges_out: u64,
    pub cleared_edges_in: u64,
    pub node_count_after: u64,
}

/// Replay report for an external projection.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ProjectionReplayReport {
    pub total_events: u64,
    pub events_replayed: u64,
}

/// Canonical replay service. The ledger remains the only source of truth;
/// every other layer is rebuilt from it.
pub struct ReplayService {
    storage: Storage,
    ledger_reader: LedgerReader,
    graph: GraphBuilder,
}

impl ReplayService {
    /// Create a new replay service over the local storage.
    pub fn new(storage: Storage) -> Self {
        Self {
            ledger_reader: LedgerReader::new(storage.clone()),
            graph: GraphBuilder::new(storage.clone()),
            storage,
        }
    }

    /// Backfill or enrich memory envelopes for all existing events.
    ///
    /// This does not create new epistemic revisions. It only upgrades
    /// derived metadata so old events can participate in the current policy.
    pub fn backfill_memory_envelopes(&self) -> Result<EnvelopeBackfillReport> {
        let events = self.ledger_reader.all_events()?;
        let total_events = events.len() as u64;
        let store = MemoryEnvelopeStore::new(self.storage.clone());

        let mut created = 0_u64;
        let mut updated = 0_u64;
        let mut unchanged = 0_u64;

        for event in events {
            match store.get(&event.id)? {
                Some(existing) => {
                    let mut enriched = existing.clone();
                    enriched.enrich_from_event(&event);

                    if enriched != existing {
                        store.save(&enriched)?;
                        updated += 1;
                    } else {
                        unchanged += 1;
                    }
                }
                None => {
                    store.save_default_for_event(&event)?;
                    created += 1;
                }
            }
        }

        Ok(EnvelopeBackfillReport {
            total_events,
            created,
            updated,
            unchanged,
        })
    }

    /// Rebuild the local graph projection from the full ledger.
    pub fn rebuild_local_graph(&self) -> Result<GraphReplayReport> {
        let events = self.ledger_reader.all_events()?;
        let total_events = events.len() as u64;

        let cleared_nodes = self.storage.clear_tree(CF_NODES)?;
        let cleared_edges_out = self.storage.clear_tree(CF_EDGES_OUT)?;
        let cleared_edges_in = self.storage.clear_tree(CF_EDGES_IN)?;

        let mut events_replayed = 0_u64;
        for event in events {
            self.graph.project_event(&event)?;
            events_replayed += 1;

            if events_replayed % 100 == 0 {
                tracing::info!(
                    replayed = events_replayed,
                    total = total_events,
                    "Rebuilding local graph from ledger"
                );
            }
        }

        Ok(GraphReplayReport {
            total_events,
            events_replayed,
            cleared_nodes,
            cleared_edges_out,
            cleared_edges_in,
            node_count_after: self.graph.node_count()?,
        })
    }

    /// Rebuild the semantic projection from the full ledger.
    #[cfg(feature = "semantic")]
    pub async fn rebuild_semantic_index(
        &self,
        semantic_client: Arc<SemanticClient>,
    ) -> Result<ProjectionReplayReport> {
        let total_events = self.ledger_reader.count()?;
        let sync = SemanticSyncService::new(self.storage.clone(), semantic_client);
        let events_replayed = sync.rebuild_from_scratch().await? as u64;

        Ok(ProjectionReplayReport {
            total_events,
            events_replayed,
        })
    }

    /// Rebuild the Neo4j projection from the full ledger.
    #[cfg(feature = "neo4j")]
    pub async fn rebuild_neo4j_projection(
        &self,
        neo4j: Neo4jConnector,
    ) -> Result<ProjectionReplayReport> {
        let total_events = self.ledger_reader.count()?;
        let sync = Neo4jSyncService::new(self.storage.clone(), neo4j);
        let events_replayed = sync.rebuild_from_scratch().await? as u64;

        Ok(ProjectionReplayReport {
            total_events,
            events_replayed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::LedgerWriter;
    use crate::memory::envelope::{MEMORY_CLASSIFICATION_VERSION, MEMORY_ENVELOPE_SCHEMA_VERSION};
    use crate::memory::{
        MemoryEnvelope, MemoryKind, MemoryScope, MemorySource, MemoryTarget, PromotionStatus,
        TruthStatus,
    };
    use crate::types::{Event, EventId, EventKind, Node, NodeId, NodeKind};
    use tempfile::TempDir;

    #[test]
    fn backfill_memory_envelopes_enriches_legacy_like_entries() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let store = MemoryEnvelopeStore::new(storage.clone());
        let replay = ReplayService::new(storage.clone());

        let event = Event::new(EventKind::Observation, "Observed replay metadata").with_tags(vec![
            "module:api/server".to_string(),
            "file:src/api/server.rs".to_string(),
            "symbol:VirtualContextTools::recall_async".to_string(),
            "logic:hybrid_recall".to_string(),
            "memory_kind:evidence".to_string(),
        ]);
        let saved = writer.append(event).unwrap();

        let legacy_like = MemoryEnvelope {
            schema_version: 1,
            classification_version: 1,
            revision: 1,
            event_id: saved.id,
            truth_status: TruthStatus::Observed,
            confidence: 0.85,
            source: MemorySource::Agent,
            scope: MemoryScope::Project,
            actor: saved.agent_id.clone(),
            asserted_at: saved.ts,
            supersedes: None,
            retracted_by: None,
            promotion_status: PromotionStatus::Candidate,
            promotion_targets: vec![MemoryTarget::Semantic],
            promotion_basis: vec!["legacy".to_string()],
            module_id: None,
            file_refs: Vec::new(),
            symbol_refs: Vec::new(),
            logic_tags: Vec::new(),
            memory_kind: MemoryKind::Observation,
        };
        store.save(&legacy_like).unwrap();

        let report = replay.backfill_memory_envelopes().unwrap();
        let enriched = store.get(&saved.id).unwrap().unwrap();

        assert_eq!(report.total_events, 1);
        assert_eq!(report.created, 0);
        assert_eq!(report.updated, 1);
        assert_eq!(enriched.schema_version, MEMORY_ENVELOPE_SCHEMA_VERSION);
        assert_eq!(
            enriched.classification_version,
            MEMORY_CLASSIFICATION_VERSION
        );
        assert_eq!(enriched.module_id.as_deref(), Some("api/server"));
        assert_eq!(enriched.file_refs, vec!["src/api/server.rs".to_string()]);
        assert_eq!(
            enriched.symbol_refs,
            vec!["VirtualContextTools::recall_async".to_string()]
        );
        assert_eq!(enriched.logic_tags, vec!["hybrid_recall".to_string()]);
        assert_eq!(enriched.memory_kind, MemoryKind::Evidence);
    }

    #[test]
    fn rebuild_local_graph_clears_stale_nodes_and_reprojects() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let replay = ReplayService::new(storage.clone());
        let graph = GraphBuilder::new(storage.clone());

        let event = Event::new(EventKind::Decision, "Replay decision")
            .with_project("replay")
            .with_inputs(vec!["src/api/server.rs".to_string()]);
        let saved = writer.append(event).unwrap();

        let stale = Node::new(NodeKind::Artifact, "stale", EventId::new());
        let stale_id = stale.id;
        graph.add_node(&stale).unwrap();

        let report = replay.rebuild_local_graph().unwrap();
        let event_node_id = NodeId::from_content(&saved.id.to_bytes());

        assert_eq!(report.total_events, 1);
        assert_eq!(report.events_replayed, 1);
        assert!(report.cleared_nodes >= 1);
        assert!(graph.get_node(&stale_id).unwrap().is_none());
        assert!(graph.get_node(&event_node_id).unwrap().is_some());
        assert!(report.node_count_after >= 1);
    }
}
