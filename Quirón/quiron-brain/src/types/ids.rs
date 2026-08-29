//! Strongly typed IDs for the brain system.

use serde::{Deserialize, Serialize};
use std::fmt;
use ulid::Ulid;

/// Event ID - unique identifier for ledger events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Ulid);

/// Node ID - unique identifier for graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub Ulid);

/// Edge ID - unique identifier for graph edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub Ulid);

/// Invariant ID - unique identifier for invariant rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InvariantId(pub Ulid);

/// Alert ID - unique identifier for alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlertId(pub Ulid);

/// Run ID - unique identifier for test/build runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub Ulid);

impl EventId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        let arr: [u8; 16] = bytes[..16].try_into().ok()?;
        Some(Self(Ulid::from_bytes(arr)))
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl NodeId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    /// Create a deterministic NodeId from content bytes (for idempotent MERGE behavior).
    /// Uses blake3 hash to generate a consistent ID for the same content.
    pub fn from_content(content: &[u8]) -> Self {
        let hash = blake3::hash(content);
        let bytes = hash.as_bytes();
        // Take first 16 bytes to create a ULID-compatible ID
        let arr: [u8; 16] = bytes[..16].try_into().unwrap();
        Self(Ulid::from_bytes(arr))
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        let arr: [u8; 16] = bytes[..16].try_into().ok()?;
        Some(Self(Ulid::from_bytes(arr)))
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl EdgeId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    /// Create a deterministic EdgeId from content bytes.
    pub fn from_content(content: &[u8]) -> Self {
        let hash = blake3::hash(content);
        let bytes = hash.as_bytes();
        let arr: [u8; 16] = bytes[..16].try_into().unwrap();
        Self(Ulid::from_bytes(arr))
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for EdgeId {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for InvariantId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for InvariantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AlertId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for AlertId {
    fn default() -> Self {
        Self::new()
    }
}

impl RunId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}
