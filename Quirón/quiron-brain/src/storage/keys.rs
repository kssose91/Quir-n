//! Key encoding/decoding utilities for RocksDB.

use crate::types::ids::{EdgeId, EventId, NodeId};

// === Key size constants ===
// These prevent magic numbers and make changes to ID sizes explicit.

/// Size of a NodeId in bytes (ULID = 16 bytes)
pub const NODE_ID_LEN: usize = 16;

/// Size of an EdgeId in bytes (ULID = 16 bytes)
pub const EDGE_ID_LEN: usize = 16;

/// Size of the kind byte
pub const KIND_LEN: usize = 1;

/// Offset where kind byte starts in edge keys
pub const KIND_OFFSET: usize = NODE_ID_LEN;

/// Offset where second NodeId starts in edge keys
pub const SECOND_NODE_OFFSET: usize = NODE_ID_LEN + KIND_LEN;

/// Offset where EdgeId starts in edge keys (after src|kind|dst)
pub const EDGE_ID_OFFSET: usize = NODE_ID_LEN + KIND_LEN + NODE_ID_LEN;

/// Minimum length of an edge key (without edge_id for backward compat)
pub const MIN_EDGE_KEY_LEN: usize = NODE_ID_LEN + KIND_LEN + NODE_ID_LEN;

/// Full edge key length including edge_id
pub const FULL_EDGE_KEY_LEN: usize = MIN_EDGE_KEY_LEN + EDGE_ID_LEN;

/// Encode an event ID as key bytes.
pub fn encode_event_key(id: &EventId) -> Vec<u8> {
    id.to_bytes().to_vec()
}

/// Encode a time-based key: ts_ms (big-endian) | event_id.
/// Big-endian ensures lexicographic = chronological order.
pub fn encode_time_key(ts_ms: i64, id: &EventId) -> Vec<u8> {
    let mut key = Vec::with_capacity(24);
    key.extend_from_slice(&ts_ms.to_be_bytes());
    key.extend_from_slice(&id.to_bytes());
    key
}

/// Encode a project-time key: project | 0xFF | ts_ms | event_id.
pub fn encode_project_time_key(project: &str, ts_ms: i64, id: &EventId) -> Vec<u8> {
    let mut key = Vec::with_capacity(project.len() + 25);
    key.extend_from_slice(project.as_bytes());
    key.push(0xFF); // Separator
    key.extend_from_slice(&ts_ms.to_be_bytes());
    key.extend_from_slice(&id.to_bytes());
    key
}

/// Encode a node ID as key bytes.
pub fn encode_node_key(id: &NodeId) -> Vec<u8> {
    id.to_bytes().to_vec()
}

/// Encode an edge outgoing key: src | kind (u8) | dst | edge_id.
/// Including edge_id allows multiple edges between same nodes (historical tracking).
pub fn encode_edge_out_key(src: &NodeId, kind: u8, dst: &NodeId, edge_id: &EdgeId) -> Vec<u8> {
    let mut key = Vec::with_capacity(FULL_EDGE_KEY_LEN);
    key.extend_from_slice(&src.to_bytes());
    key.push(kind);
    key.extend_from_slice(&dst.to_bytes());
    key.extend_from_slice(&edge_id.to_bytes());
    key
}

/// Encode an edge incoming key: dst | kind (u8) | src | edge_id.
/// Mirror of outgoing key for reverse lookups.
pub fn encode_edge_in_key(dst: &NodeId, kind: u8, src: &NodeId, edge_id: &EdgeId) -> Vec<u8> {
    let mut key = Vec::with_capacity(FULL_EDGE_KEY_LEN);
    key.extend_from_slice(&dst.to_bytes());
    key.push(kind);
    key.extend_from_slice(&src.to_bytes());
    key.extend_from_slice(&edge_id.to_bytes());
    key
}

/// Create a prefix for scanning edges from a source node.
pub fn edge_out_prefix(src: &NodeId) -> Vec<u8> {
    src.to_bytes().to_vec()
}

/// Create a prefix for scanning edges to a target node.
pub fn edge_in_prefix(dst: &NodeId) -> Vec<u8> {
    dst.to_bytes().to_vec()
}

/// Decode an event ID from bytes.
pub fn decode_event_id(bytes: &[u8]) -> Option<EventId> {
    EventId::from_bytes(bytes)
}

/// Decode a node ID from bytes.
pub fn decode_node_id(bytes: &[u8]) -> Option<NodeId> {
    NodeId::from_bytes(bytes)
}
