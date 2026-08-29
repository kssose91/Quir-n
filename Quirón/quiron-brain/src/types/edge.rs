//! Edge types for the cognitive graph.

use crate::types::ids::{EdgeId, EventId, NodeId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An edge in the cognitive graph connecting two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Unique edge ID.
    pub id: EdgeId,

    /// Type of relationship.
    pub kind: EdgeKind,

    /// Source node.
    pub source: NodeId,

    /// Target node.
    pub target: NodeId,

    /// When the edge was created.
    pub created_at: DateTime<Utc>,

    /// Event that created this edge.
    pub created_by_event: EventId,

    /// Strength of the relationship (0.0 - 1.0).
    pub strength: f32,

    /// Confidence in this edge (0.0 - 1.0).
    pub confidence: f32,

    /// Additional properties (JSON string).
    pub properties: String,
}

/// Types of edges in the cognitive graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(u8)]
pub enum EdgeKind {
    // === Structure ===
    /// Artifact belongs to project.
    BelongsTo = 0,
    /// Node is part of another node.
    PartOf = 1,
    /// Task depends on artifact/decision.
    DependsOn = 2,

    // === File operations ===
    /// Action reads artifact.
    Reads = 10,
    /// Action writes artifact.
    Writes = 11,
    /// Action modifies artifact.
    Modifies = 12,
    /// Action produces artifact.
    Produces = 13,
    /// Action deletes artifact.
    Deletes = 14,

    // === Causality ===
    /// Change causes failure.
    Causes = 20,
    /// Change fixes failure.
    Fixes = 21,
    /// Event precedes another (temporal).
    Precedes = 22,

    // === Verification ===
    /// Run validates claim.
    Validates = 30,
    /// Run refutes claim.
    Refutes = 31,
    /// Evidence supports hypothesis.
    Supports = 32,
    /// Claim contradicts another.
    Contradicts = 33,

    // === Semantic ===
    /// Chunk is about concept.
    About = 40,
    /// Artifact mentions concept.
    Mentions = 41,
    /// Concept related to another.
    RelatedTo = 42,

    // === System ===
    /// Invariant triggers alert.
    Triggers = 50,
    /// Action violates invariant.
    Violates = 51,
    /// Action implements decision.
    Implements = 52,

    // === Credit assignment ===
    /// Run blames change for failure.
    Blamed = 60,
    /// Run credits change for success.
    Credited = 61,
}

impl Edge {
    /// Create a new edge.
    pub fn new(kind: EdgeKind, source: NodeId, target: NodeId, event_id: EventId) -> Self {
        Self {
            id: EdgeId::new(),
            kind,
            source,
            target,
            created_at: Utc::now(),
            created_by_event: event_id,
            strength: 1.0,
            confidence: 1.0,
            properties: "{}".to_string(),
        }
    }

    /// Create a replay-safe deterministic edge keyed by source, target, kind and event.
    pub fn deterministic(
        kind: EdgeKind,
        source: NodeId,
        target: NodeId,
        event_id: EventId,
    ) -> Self {
        let mut key = Vec::with_capacity(16 + 1 + 16 + 16);
        key.extend_from_slice(&source.to_bytes());
        key.push(kind.as_u8());
        key.extend_from_slice(&target.to_bytes());
        key.extend_from_slice(&event_id.to_bytes());

        Self {
            id: EdgeId::from_content(&key),
            kind,
            source,
            target,
            created_at: Utc::now(),
            created_by_event: event_id,
            strength: 1.0,
            confidence: 1.0,
            properties: "{}".to_string(),
        }
    }

    /// Set strength.
    pub fn with_strength(mut self, strength: f32) -> Self {
        self.strength = strength.clamp(0.0, 1.0);
        self
    }

    /// Set confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_properties(mut self, props: serde_json::Value) -> Self {
        self.properties = props.to_string();
        self
    }
}

impl EdgeKind {
    /// Get the u8 representation for storage.
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    /// Create from u8.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::BelongsTo),
            1 => Some(Self::PartOf),
            2 => Some(Self::DependsOn),
            10 => Some(Self::Reads),
            11 => Some(Self::Writes),
            12 => Some(Self::Modifies),
            13 => Some(Self::Produces),
            14 => Some(Self::Deletes),
            20 => Some(Self::Causes),
            21 => Some(Self::Fixes),
            22 => Some(Self::Precedes),
            30 => Some(Self::Validates),
            31 => Some(Self::Refutes),
            32 => Some(Self::Supports),
            33 => Some(Self::Contradicts),
            40 => Some(Self::About),
            41 => Some(Self::Mentions),
            42 => Some(Self::RelatedTo),
            50 => Some(Self::Triggers),
            51 => Some(Self::Violates),
            52 => Some(Self::Implements),
            60 => Some(Self::Blamed),
            61 => Some(Self::Credited),
            _ => None,
        }
    }
}
