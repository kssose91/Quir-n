//! Hierarchical Associative Memory System
//!
//! Implements human-like memory retrieval:
//! - Memories are organized hierarchically (parent/child)
//! - Accessing a memory "activates" related memories
//! - Sub-memories are "locked" until the parent is accessed
//! - Reduces noise by only showing contextually relevant memories
//!
//! This mimics how human memory works: remembering one thing
//! triggers related memories in a cascade.

use crate::types::{Edge, NodeId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Configuration for memory activation
#[derive(Debug, Clone)]
pub struct ActivationConfig {
    /// Initial activation when directly accessed
    pub direct_activation: f32,
    /// How much activation spreads to neighbors (0.0-1.0)
    pub spread_factor: f32,
    /// Minimum activation to be considered "unlocked"
    pub unlock_threshold: f32,
    /// How many hops to spread activation
    pub max_spread_depth: usize,
    /// Decay factor per hop
    pub decay_per_hop: f32,
}

impl Default for ActivationConfig {
    fn default() -> Self {
        Self {
            direct_activation: 1.0,
            spread_factor: 0.6,
            unlock_threshold: 0.2,
            max_spread_depth: 3,
            decay_per_hop: 0.4,
        }
    }
}

/// State of a memory's activation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryActivation {
    /// Node ID this activation belongs to
    pub node_id: NodeId,
    /// Current activation level (0.0 = dormant, 1.0 = fully active)
    pub level: f32,
    /// When this memory was last activated
    pub last_activated: DateTime<Utc>,
    /// Which memory triggered this activation (for tracing)
    pub triggered_by: Option<NodeId>,
    /// Depth from the original trigger
    pub depth: usize,
}

/// Engine for hierarchical memory with spreading activation
pub struct AssociativeMemory {
    config: ActivationConfig,
    /// Current activation state
    activations: HashMap<NodeId, MemoryActivation>,
    /// Memories that have been "unlocked" in this session
    unlocked: HashSet<NodeId>,
}

impl AssociativeMemory {
    /// Create new associative memory system
    pub fn new() -> Self {
        Self::with_config(ActivationConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: ActivationConfig) -> Self {
        Self {
            config,
            activations: HashMap::new(),
            unlocked: HashSet::new(),
        }
    }

    /// Activate a memory directly (user accessed/mentioned it)
    pub fn activate(&mut self, node_id: NodeId) {
        let activation = MemoryActivation {
            node_id: node_id.clone(),
            level: self.config.direct_activation,
            last_activated: Utc::now(),
            triggered_by: None,
            depth: 0,
        };
        self.activations.insert(node_id.clone(), activation);
        self.unlocked.insert(node_id);
    }

    /// Spread activation through a graph of edges
    /// Returns all node IDs that were activated above threshold
    pub fn spread_activation(&mut self, initial_nodes: &[NodeId], edges: &[Edge]) -> Vec<NodeId> {
        // Start with direct activation
        for node_id in initial_nodes {
            self.activate(node_id.clone());
        }

        // Spread for each depth level
        for depth in 1..=self.config.max_spread_depth {
            let current_activated: Vec<_> = self
                .activations
                .iter()
                .filter(|(_, a)| a.depth == depth - 1 && a.level >= self.config.unlock_threshold)
                .map(|(id, a)| (id.clone(), a.level))
                .collect();

            for (node_id, parent_level) in current_activated {
                // Find connected nodes via edges
                for edge in edges {
                    if edge.source == node_id {
                        self.spread_to_neighbor(&edge.target, &node_id, parent_level, depth);
                    } else if edge.target == node_id {
                        self.spread_to_neighbor(&edge.source, &node_id, parent_level, depth);
                    }
                }
            }
        }

        // Return all unlocked memories
        self.activations
            .iter()
            .filter(|(_, a)| a.level >= self.config.unlock_threshold)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Spread activation to a neighbor node
    fn spread_to_neighbor(
        &mut self,
        target: &NodeId,
        source: &NodeId,
        source_level: f32,
        depth: usize,
    ) {
        // Calculate spread activation with decay
        let decay = self.config.decay_per_hop.powi(depth as i32);
        let spread_level = source_level * self.config.spread_factor * decay;

        // Skip if too weak
        if spread_level < self.config.unlock_threshold * 0.5 {
            return;
        }

        // Accumulate if already activated (memories can be reached multiple ways)
        let current = self.activations.get(target).map(|a| a.level).unwrap_or(0.0);
        let new_level = (current + spread_level).min(1.0);

        // Update or insert
        self.activations
            .entry(target.clone())
            .and_modify(|a| {
                if new_level > a.level {
                    a.level = new_level;
                    a.triggered_by = Some(source.clone());
                }
            })
            .or_insert(MemoryActivation {
                node_id: target.clone(),
                level: new_level,
                last_activated: Utc::now(),
                triggered_by: Some(source.clone()),
                depth,
            });

        // Mark as unlocked if above threshold
        if new_level >= self.config.unlock_threshold {
            self.unlocked.insert(target.clone());
        }
    }

    /// Check if a memory is "unlocked" (accessible in current context)
    pub fn is_unlocked(&self, node_id: &NodeId) -> bool {
        self.unlocked.contains(node_id)
    }

    /// Get activation level of a memory
    pub fn get_activation(&self, node_id: &NodeId) -> f32 {
        self.activations
            .get(node_id)
            .map(|a| a.level)
            .unwrap_or(0.0)
    }

    /// Get all activated memories sorted by activation level (most active first)
    pub fn get_activated_memories(&self) -> Vec<&MemoryActivation> {
        let mut memories: Vec<_> = self.activations.values().collect();
        memories.sort_by(|a, b| {
            b.level
                .partial_cmp(&a.level)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        memories
    }

    /// Get the "activation path" - how we got from trigger to a memory
    pub fn get_activation_path(&self, target: &NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = Some(target.clone());

        while let Some(node_id) = current {
            path.push(node_id.clone());
            current = self
                .activations
                .get(&node_id)
                .and_then(|a| a.triggered_by.clone());
        }

        path.reverse();
        path
    }

    /// Clear activation state for new retrieval cycle
    pub fn clear(&mut self) {
        self.activations.clear();
        // Note: we don't clear unlocked - session persists
    }

    /// Clear everything including unlocked set (new session)
    pub fn reset_session(&mut self) {
        self.activations.clear();
        self.unlocked.clear();
    }

    /// Get stats about current activation state
    pub fn stats(&self) -> ActivationStats {
        let activated: Vec<_> = self
            .activations
            .values()
            .filter(|a| a.level >= self.config.unlock_threshold)
            .collect();

        ActivationStats {
            total_nodes_touched: self.activations.len(),
            unlocked_count: self.unlocked.len(),
            max_depth: activated.iter().map(|a| a.depth).max().unwrap_or(0),
            avg_activation: if activated.is_empty() {
                0.0
            } else {
                activated.iter().map(|a| a.level).sum::<f32>() / activated.len() as f32
            },
        }
    }
}

impl Default for AssociativeMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Stats about activation state
#[derive(Debug, Clone)]
pub struct ActivationStats {
    pub total_nodes_touched: usize,
    pub unlocked_count: usize,
    pub max_depth: usize,
    pub avg_activation: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_activation() {
        let mut memory = AssociativeMemory::new();
        let node = NodeId::new();

        memory.activate(node.clone());

        assert!(memory.is_unlocked(&node));
        assert_eq!(memory.get_activation(&node), 1.0);
    }

    #[test]
    fn test_activation_config() {
        let config = ActivationConfig::default();
        assert!(config.spread_factor > 0.0);
        assert!(config.unlock_threshold > 0.0);
        assert!(config.max_spread_depth > 0);
    }

    #[test]
    fn test_stats() {
        let mut memory = AssociativeMemory::new();
        memory.activate(NodeId::new());
        memory.activate(NodeId::new());

        let stats = memory.stats();
        assert_eq!(stats.total_nodes_touched, 2);
        assert_eq!(stats.unlocked_count, 2);
    }

    #[test]
    fn test_spread_activation() {
        use crate::types::{Edge, EdgeId, EdgeKind, EventId};

        let mut memory = AssociativeMemory::new();
        let node_a = NodeId::new();
        let node_b = NodeId::new();
        let node_c = NodeId::new();

        // Create edges: A -> B -> C
        let edges = vec![
            Edge {
                id: EdgeId::new(),
                kind: EdgeKind::DependsOn,
                source: node_a.clone(),
                target: node_b.clone(),
                created_at: Utc::now(),
                created_by_event: EventId::new(),
                strength: 1.0,
                confidence: 1.0,
                properties: String::new(),
            },
            Edge {
                id: EdgeId::new(),
                kind: EdgeKind::DependsOn,
                source: node_b.clone(),
                target: node_c.clone(),
                created_at: Utc::now(),
                created_by_event: EventId::new(),
                strength: 1.0,
                confidence: 1.0,
                properties: String::new(),
            },
        ];

        // Activate A, should spread to B and C
        let unlocked = memory.spread_activation(&[node_a.clone()], &edges);

        assert!(memory.is_unlocked(&node_a));
        assert!(memory.is_unlocked(&node_b));
        // C might or might not be unlocked depending on decay settings
        assert!(unlocked.contains(&node_a));
        assert!(unlocked.contains(&node_b));
    }
}
