//! Tree names (equivalent to RocksDB column families).

/// Events ledger (source of truth).
pub const CF_EVENTS: &str = "events";

/// Event time index: ts|event_id -> event_id.
pub const CF_EVENT_TIME: &str = "event_time";

/// Event project index: project|ts|event_id -> event_id.
pub const CF_EVENT_PROJECT: &str = "event_proj";

/// Graph nodes: node_id -> Node bytes.
pub const CF_NODES: &str = "nodes";

/// Graph edges (outgoing): src|kind|dst -> Edge bytes.
pub const CF_EDGES_OUT: &str = "edges_out";

/// Graph edges (incoming): dst|kind|src -> edge_id.
pub const CF_EDGES_IN: &str = "edges_in";

/// Invariants: inv_id -> Invariant bytes.
pub const CF_INVARIANTS: &str = "invariants";

/// Alerts: alert_id -> Alert bytes.
pub const CF_ALERTS: &str = "alerts";

/// Test/build runs: run_id -> Run bytes.
pub const CF_RUNS: &str = "runs";

/// Artifact metadata: hash -> ArtifactMeta bytes.
pub const CF_ARTIFACTS: &str = "artifacts";

/// Key-value store for misc data.
pub const CF_KV: &str = "kv";

/// Chain metadata (head hash, etc).
pub const CF_CHAIN: &str = "chain";

/// Evidence records: event_id|evidence_idx -> EvidenceRef bytes.
pub const CF_EVIDENCE: &str = "evidence";

/// Distilled memories: memory_id -> DistilledMemory bytes.
pub const CF_MEMORIES: &str = "memories";

/// Current semantic memory envelope per event: event_id -> MemoryEnvelope bytes.
pub const CF_MEMORY_ENVELOPES: &str = "memory_envelopes";

/// Historical envelope revisions: event_id|revision -> MemoryEnvelope bytes.
pub const CF_MEMORY_ENVELOPE_HISTORY: &str = "memory_envelope_history";

/// Session telemetry: dedicated namespace for checkpoints/anomalies.
pub const CF_TELEMETRY_SESSION: &str = "telemetry_session";

/// Get all tree names.
pub fn all_tree_names() -> Vec<&'static str> {
    vec![
        CF_EVENTS,
        CF_EVENT_TIME,
        CF_EVENT_PROJECT,
        CF_NODES,
        CF_EDGES_OUT,
        CF_EDGES_IN,
        CF_INVARIANTS,
        CF_ALERTS,
        CF_RUNS,
        CF_ARTIFACTS,
        CF_KV,
        CF_CHAIN,
        CF_EVIDENCE,
        CF_MEMORIES,
        CF_MEMORY_ENVELOPES,
        CF_MEMORY_ENVELOPE_HISTORY,
        CF_TELEMETRY_SESSION,
    ]
}

// Backward compatibility alias
pub fn all_cf_names() -> Vec<&'static str> {
    all_tree_names()
}
