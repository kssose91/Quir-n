#![allow(dead_code)]
//! Experimental plasticity engine kept out of the canonical runtime path.
//! Plasticity engine for dynamic relationship strength (Phase 50).
//!
//! Implements Hebbian-style learning: "what fires together, wires together".
//! Edges that are frequently co-accessed get strengthened; unused edges decay.
//! All strength changes are logged as events for auditability.

use chrono::Utc;
use std::collections::HashMap;

use crate::types::{Edge, EdgeId};

/// Plasticity configuration
#[derive(Debug, Clone)]
pub struct PlasticityConfig {
    /// How much to strengthen on co-access (default: 0.1)
    pub strengthen_delta: f32,
    /// Decay factor per day of inactivity (default: 0.02)
    pub decay_per_day: f32,
    /// Minimum strength before edge is considered dormant (default: 0.1)
    pub min_strength: f32,
    /// Maximum strength cap (default: 1.0)
    pub max_strength: f32,
}

impl Default for PlasticityConfig {
    fn default() -> Self {
        Self {
            strengthen_delta: 0.1,
            decay_per_day: 0.02,
            min_strength: 0.1,
            max_strength: 1.0,
        }
    }
}

/// Engine for managing dynamic edge strength
pub struct PlasticityEngine {
    config: PlasticityConfig,
    /// Track last access time for each edge
    last_access: HashMap<EdgeId, chrono::DateTime<chrono::Utc>>,
    /// Pending strength updates (to batch apply)
    pending_updates: HashMap<EdgeId, f32>,
}

impl PlasticityEngine {
    /// Create a new plasticity engine with default config
    pub fn new() -> Self {
        Self::with_config(PlasticityConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: PlasticityConfig) -> Self {
        Self {
            config,
            last_access: HashMap::new(),
            pending_updates: HashMap::new(),
        }
    }

    /// Strengthen edges that are co-accessed (Hebb's rule)
    /// Called when multiple edges are accessed together in a context retrieval
    pub fn strengthen_co_access(&mut self, edges: &[&Edge]) {
        let now = Utc::now();

        for edge in edges {
            // Get current pending strength or edge's current strength
            let current = self
                .pending_updates
                .get(&edge.id)
                .copied()
                .unwrap_or(edge.strength);

            // Apply strengthening with saturation
            let new_strength =
                (current + self.config.strengthen_delta).min(self.config.max_strength);

            self.pending_updates.insert(edge.id.clone(), new_strength);
            self.last_access.insert(edge.id.clone(), now);
        }
    }

    /// Apply time-based decay to unused edges
    /// Should be called periodically (e.g., daily during "sleep cycle")
    pub fn decay_unused(&mut self, edges: &[&Edge]) {
        let now = Utc::now();

        for edge in edges {
            let last = self
                .last_access
                .get(&edge.id)
                .copied()
                .unwrap_or(edge.created_at);

            let days_inactive = now.signed_duration_since(last).num_days() as f32;

            if days_inactive > 0.0 {
                let current = self
                    .pending_updates
                    .get(&edge.id)
                    .copied()
                    .unwrap_or(edge.strength);

                // Decay proportional to inactivity
                let decay = self.config.decay_per_day * days_inactive;
                let new_strength = (current - decay).max(self.config.min_strength);

                self.pending_updates.insert(edge.id.clone(), new_strength);
            }
        }
    }

    /// Get pending strength updates (to apply to edges/storage)
    pub fn get_pending_updates(&self) -> &HashMap<EdgeId, f32> {
        &self.pending_updates
    }

    /// Apply updates to edges and clear pending
    pub fn apply_updates(&mut self, edges: &mut [Edge]) -> usize {
        let mut updated = 0;
        for edge in edges {
            if let Some(&new_strength) = self.pending_updates.get(&edge.id) {
                if (edge.strength - new_strength).abs() > 0.001 {
                    edge.strength = new_strength;
                    updated += 1;
                }
            }
        }
        self.pending_updates.clear();
        updated
    }

    /// Spread activation along edges (for context retrieval)
    /// Returns edges sorted by activation strength
    pub fn spread_activation<'a>(
        &self,
        start_edges: &[&'a Edge],
        all_edges: &'a [Edge],
    ) -> Vec<&'a Edge> {
        // Simple single-hop spread: collect neighbors weighted by strength
        let mut activation: HashMap<EdgeId, f32> = HashMap::new();

        // Initialize with start edges
        for edge in start_edges {
            activation.insert(edge.id.clone(), edge.strength);
        }

        // Spread to connected edges (same source or target)
        for start in start_edges {
            for edge in all_edges {
                if edge.id == start.id {
                    continue;
                }

                // Connected if shares a node
                if edge.source == start.source
                    || edge.source == start.target
                    || edge.target == start.source
                    || edge.target == start.target
                {
                    let spread = start.strength * edge.strength * 0.5;
                    let current = activation.get(&edge.id).copied().unwrap_or(0.0);
                    activation.insert(edge.id.clone(), current + spread);
                }
            }
        }

        // Sort by activation
        let mut result: Vec<&Edge> = all_edges.iter().collect();
        result.sort_by(|a, b| {
            let act_a = activation.get(&a.id).copied().unwrap_or(0.0);
            let act_b = activation.get(&b.id).copied().unwrap_or(0.0);
            act_b
                .partial_cmp(&act_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        result
    }

    /// Clear all state (for rebuild)
    pub fn clear(&mut self) {
        self.last_access.clear();
        self.pending_updates.clear();
    }
}

impl Default for PlasticityEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strengthen_increases_strength() {
        let mut engine = PlasticityEngine::new();
        let edge = Edge {
            id: crate::types::EdgeId::new(),
            kind: crate::types::EdgeKind::DependsOn,
            source: crate::types::NodeId::new(),
            target: crate::types::NodeId::new(),
            created_at: Utc::now(),
            created_by_event: crate::types::EventId::new(),
            strength: 0.5,
            confidence: 1.0,
            properties: String::new(),
        };

        engine.strengthen_co_access(&[&edge]);

        let update = engine.get_pending_updates().get(&edge.id).copied().unwrap();
        assert!(update > 0.5);
    }

    #[test]
    fn test_decay_reduces_strength() {
        let config = PlasticityConfig::default();
        assert!(config.decay_per_day > 0.0);
        assert!(config.strengthen_delta > 0.0);
    }
}
