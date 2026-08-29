#![allow(dead_code)]
//! Experimental semantic consolidation kept outside the canonical memory path.
//! Consolidation engine for memory sleep cycles (Phase 49).
//!
//! Creates Concepts from clusters of similar events, reducing noise
//! without losing the original data. All operations are deterministic
//! and reconstructible from the ledger.

use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;

use super::SemanticClient;
use crate::types::{Concept, ConceptId, Event, EventId};

/// Minimum number of similar events to form a concept
const MIN_CLUSTER_SIZE: usize = 3;

/// Similarity threshold for clustering (high to avoid mixing unrelated events)
const CLUSTER_THRESHOLD: f32 = 0.80;

/// Engine for consolidating events into concepts
pub struct ConsolidationEngine {
    semantic: Arc<SemanticClient>,
    /// Cached concepts (derived from ledger, can be rebuilt)
    concepts: HashMap<ConceptId, Concept>,
}

impl ConsolidationEngine {
    /// Create a new consolidation engine
    pub fn new(semantic: Arc<SemanticClient>) -> Self {
        Self {
            semantic,
            concepts: HashMap::new(),
        }
    }

    /// Run consolidation on a set of events
    /// Returns newly created concepts
    pub async fn consolidate(&mut self, events: &[Event]) -> Result<Vec<Concept>> {
        if events.len() < MIN_CLUSTER_SIZE {
            return Ok(vec![]);
        }

        // 1. Group events by similarity using embeddings
        let clusters = self.cluster_events(events).await?;

        // 2. Create concepts from valid clusters
        let mut new_concepts = vec![];
        for cluster in clusters {
            if cluster.len() >= MIN_CLUSTER_SIZE {
                let concept = self.create_concept_from_cluster(&cluster)?;
                self.concepts.insert(concept.id.clone(), concept.clone());
                new_concepts.push(concept);
            }
        }

        tracing::info!(
            "Consolidation complete: {} events → {} concepts",
            events.len(),
            new_concepts.len()
        );

        Ok(new_concepts)
    }

    /// Cluster events by semantic similarity
    async fn cluster_events(&self, events: &[Event]) -> Result<Vec<Vec<Event>>> {
        // Simple greedy clustering: for each event, find similar events
        let mut used: Vec<bool> = vec![false; events.len()];
        let mut clusters: Vec<Vec<Event>> = vec![];

        for (i, event) in events.iter().enumerate() {
            if used[i] {
                continue;
            }

            // Search for events similar to this one
            let similar = self
                .semantic
                .search(&event.description, 20, Some(CLUSTER_THRESHOLD))
                .await
                .unwrap_or_default();

            let mut cluster = vec![event.clone()];
            used[i] = true;

            // Find matching events from our input set
            for result in similar {
                if let Some(event_id) = &result.event_id {
                    for (j, other_event) in events.iter().enumerate() {
                        if !used[j] && other_event.id.to_string() == *event_id {
                            cluster.push(other_event.clone());
                            used[j] = true;
                            break;
                        }
                    }
                }
            }

            if cluster.len() >= MIN_CLUSTER_SIZE {
                clusters.push(cluster);
            }
        }

        Ok(clusters)
    }

    /// Create a concept from a cluster of similar events
    fn create_concept_from_cluster(&self, cluster: &[Event]) -> Result<Concept> {
        // Generate summary from event descriptions
        let summary = self.generate_summary(cluster);

        // Collect event IDs
        let event_ids: Vec<EventId> = cluster.iter().map(|e| e.id.clone()).collect();

        // Max importance from cluster
        let importance = cluster.iter().map(|e| e.importance).fold(0.0_f32, f32::max);

        // Collect unique tags
        let mut tags: Vec<String> = cluster
            .iter()
            .flat_map(|e| e.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();

        // Project from first event (assuming cluster is same project)
        let project_id = cluster.first().and_then(|e| e.project_id.clone());

        let concept = Concept {
            id: ConceptId::new(),
            summary,
            event_ids,
            importance,
            tags,
            project_id,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            embedding: None,
        };

        Ok(concept)
    }

    /// Generate a summary from cluster events
    fn generate_summary(&self, cluster: &[Event]) -> String {
        // Simple heuristic: use the description of the highest-importance event
        // In the future, could use LLM for better summaries
        if let Some(best) = cluster.iter().max_by(|a, b| {
            a.importance
                .partial_cmp(&b.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            format!(
                "[CONCEPT: {} events] {}",
                cluster.len(),
                best.description.chars().take(200).collect::<String>()
            )
        } else {
            format!("[CONCEPT: {} related events]", cluster.len())
        }
    }

    /// Get all concepts
    pub fn all_concepts(&self) -> Vec<&Concept> {
        self.concepts.values().collect()
    }

    /// Get a concept by ID
    pub fn get_concept(&self, id: &ConceptId) -> Option<&Concept> {
        self.concepts.get(id)
    }

    /// Clear all concepts (for rebuild)
    pub fn clear(&mut self) {
        self.concepts.clear();
    }

    /// Rebuild concepts from a list of events (deterministic)
    pub async fn rebuild(&mut self, events: &[Event]) -> Result<()> {
        self.clear();
        self.consolidate(events).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_min_cluster_size() {
        assert!(
            MIN_CLUSTER_SIZE >= 3,
            "Cluster should have at least 3 events"
        );
    }

    #[test]
    fn test_cluster_threshold() {
        assert!(
            CLUSTER_THRESHOLD >= 0.7,
            "Threshold should be high to avoid mixing"
        );
    }
}
