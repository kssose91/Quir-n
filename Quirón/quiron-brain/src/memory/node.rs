//! Consolidated Memory Nodes
//!
//! MemoryNode represents a "distilled" memory - a summary of multiple events
//! that reduces noise while preserving key facts and evidence pointers.
//!
//! This is the "Level B" of memory architecture:
//! - Level A: Raw events (ledger) - immutable, complete
//! - Level B: Distilled memories (MemoryNode) - navigable, summarized
//! - Level C: Associations (edges) - connections between memories

use crate::types::{EventId, NodeId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A consolidated memory node representing distilled knowledge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    /// Unique identifier
    pub id: NodeId,

    /// Short title for this memory
    pub title: String,

    /// Summary (5-15 lines typically)
    pub summary: String,

    /// Key facts extracted from source events
    pub key_facts: Vec<String>,

    /// Pointers to evidence: source events that back this memory
    pub evidence: Vec<EventId>,

    /// Tags for categorization
    pub tags: Vec<String>,

    /// Confidence score (0.0-1.0)
    pub confidence: f32,

    /// When this memory was created
    pub created_at: DateTime<Utc>,

    /// When this memory was last accessed
    pub last_accessed: DateTime<Utc>,

    /// How many times this memory has been accessed
    pub access_count: u32,

    /// Project this memory belongs to (optional)
    pub project_id: Option<String>,
}

impl MemoryNode {
    /// Create a new memory node
    pub fn new(title: impl Into<String>, summary: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: NodeId::new(),
            title: title.into(),
            summary: summary.into(),
            key_facts: Vec::new(),
            evidence: Vec::new(),
            tags: Vec::new(),
            confidence: 1.0,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            project_id: None,
        }
    }

    /// Add key facts
    pub fn with_facts(mut self, facts: Vec<String>) -> Self {
        self.key_facts = facts;
        self
    }

    /// Add evidence pointers
    pub fn with_evidence(mut self, evidence: Vec<EventId>) -> Self {
        self.evidence = evidence;
        self
    }

    /// Add tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Set project
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project_id = Some(project.into());
        self
    }

    /// Set confidence
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Record an access to this memory
    pub fn record_access(&mut self) {
        self.last_accessed = Utc::now();
        self.access_count += 1;
    }
}

/// Link types between memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLinkKind {
    /// This memory explains another
    Explains,
    /// This memory depends on another
    DependsOn,
    /// This memory contradicts another
    Contradicts,
    /// This memory refines/updates another
    Refines,
    /// These memories are about the same thing
    SameAs,
    /// This memory is an example of another (more general) one
    ExampleOf,
    /// This memory is the root cause of another
    RootCauseOf,
    /// Parent/child hierarchy
    ParentOf,
    /// Temporal: this memory comes after another
    FollowsFrom,
    /// Generic association
    RelatedTo,
}

/// A link between two memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLink {
    /// Source memory
    pub from: NodeId,
    /// Target memory
    pub to: NodeId,
    /// Type of link
    pub kind: MemoryLinkKind,
    /// Strength of association (0.0-1.0)
    pub weight: f32,
    /// When this link was created
    pub created_at: DateTime<Utc>,
}

impl MemoryLink {
    /// Create a new memory link
    pub fn new(from: NodeId, to: NodeId, kind: MemoryLinkKind) -> Self {
        Self {
            from,
            to,
            kind,
            weight: 1.0,
            created_at: Utc::now(),
        }
    }

    /// Set weight
    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight.clamp(0.0, 1.0);
        self
    }
}

/// Request to distill events into a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillRequest {
    /// Title for the distilled memory
    pub title: String,
    /// Events to distill (if empty, uses query)
    pub event_ids: Vec<EventId>,
    /// Query to find events to distill (alternative to event_ids)
    pub query: Option<String>,
    /// Tags to apply
    pub tags: Vec<String>,
    /// Project filter
    pub project_id: Option<String>,
}

/// Response from memory expansion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandResponse {
    /// The requested memory
    pub memory: MemoryNode,
    /// Linked memories (neighbors)
    pub neighbors: Vec<MemoryNode>,
    /// Links to neighbors
    pub links: Vec<MemoryLink>,
    /// Evidence events (sampled, not all)
    pub evidence_sample: Vec<EventId>,
    /// Stats about the expansion
    pub stats: ExpandStats,
}

/// Stats about memory expansion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandStats {
    /// How many neighbors were found
    pub neighbor_count: usize,
    /// How many were returned (might be limited by k)
    pub returned_count: usize,
    /// Total evidence events
    pub evidence_count: usize,
    /// Depth of expansion
    pub depth: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_node_creation() {
        let memory = MemoryNode::new("Test Memory", "This is a test summary")
            .with_facts(vec!["Fact 1".into(), "Fact 2".into()])
            .with_tags(vec!["test".into()]);

        assert_eq!(memory.title, "Test Memory");
        assert_eq!(memory.key_facts.len(), 2);
        assert_eq!(memory.access_count, 0);
    }

    #[test]
    fn test_memory_link() {
        let link = MemoryLink::new(NodeId::new(), NodeId::new(), MemoryLinkKind::DependsOn)
            .with_weight(0.8);

        assert_eq!(link.weight, 0.8);
        assert_eq!(link.kind, MemoryLinkKind::DependsOn);
    }

    #[test]
    fn test_access_tracking() {
        let mut memory = MemoryNode::new("Test", "Summary");
        assert_eq!(memory.access_count, 0);

        memory.record_access();
        assert_eq!(memory.access_count, 1);

        memory.record_access();
        assert_eq!(memory.access_count, 2);
    }
}
