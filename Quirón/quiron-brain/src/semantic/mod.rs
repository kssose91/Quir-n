//! Semantic search module for Quirón Brain
//!
//! This module provides vector-based semantic search using Qdrant.
//! It allows finding "similar" events even when exact keywords don't match.
//!
//! # Architecture
//! - `client.rs`: semantic client facade with in-process or remote backend
//! - `remote.rs`: HTTP client for the external semantic_ia service
//! - `embed.rs`: Local embedding runtime in Rust
//! - `search.rs`: Search with threshold enforcement + CRAG
//! - `consolidation.rs`: Experimental sleep cycle logic, not part of the canonical memory path

use serde::{Deserialize, Serialize};

#[cfg(feature = "semantic")]
mod client;
#[cfg(feature = "semantic")]
mod consolidation;
#[cfg(feature = "semantic")]
mod embed;
#[cfg(feature = "semantic")]
mod remote;
#[cfg(feature = "semantic")]
mod search;
#[cfg(feature = "semantic")]
mod sync;

#[cfg(feature = "semantic")]
pub use client::create_semantic_client;
#[cfg(feature = "semantic")]
pub use client::SemanticClient;
#[cfg(feature = "semantic")]
pub use embed::EmbedService;
#[cfg(feature = "semantic")]
pub use search::{
    CragResponse, CragResult, RankedResult, SearchQuery, SearchResponse, SearchResult,
};
#[cfg(feature = "semantic")]
pub use sync::SemanticSyncService;

/// Semantic backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticBackendKind {
    InProcess,
    Remote,
}

impl SemanticBackendKind {
    pub fn from_env() -> Self {
        let raw = std::env::var("SEMANTIC_BACKEND")
            .ok()
            .or_else(|| std::env::var("QUIRON_SEMANTIC_BACKEND").ok());
        parse_backend_kind(raw.as_deref())
    }
}

fn parse_backend_kind(raw: Option<&str>) -> SemanticBackendKind {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("remote") => SemanticBackendKind::Remote,
        Some("inprocess") | Some("in_process") | Some("local") | Some("in-process") => {
            SemanticBackendKind::InProcess
        }
        _ => SemanticBackendKind::InProcess,
    }
}

fn normalize_qdrant_url(raw: String) -> String {
    let mut normalized = raw.trim().trim_end_matches('/').to_string();

    // qdrant-client talks to Qdrant over gRPC by default.
    // Qdrant's default gRPC port is 6334.
    if normalized.contains(":6333") {
        let fixed = normalized.replacen(":6333", ":6334", 1);
        tracing::warn!(
            qdrant_url = %normalized,
            normalized_qdrant_url = %fixed,
            "QDRANT_URL points to the REST port; normalizing to the gRPC port"
        );
        normalized = fixed;
    }

    // In this environment `localhost` resolves to ::1 only. The gRPC client is
    // more reliable with an explicit IPv4 loopback address.
    if normalized.contains("://localhost") {
        let fixed = normalized.replacen("://localhost", "://127.0.0.1", 1);
        tracing::warn!(
            qdrant_url = %normalized,
            normalized_qdrant_url = %fixed,
            "Normalizing QDRANT_URL host from localhost to 127.0.0.1"
        );
        normalized = fixed;
    }

    normalized
}

fn normalize_service_url(raw: String) -> String {
    let mut normalized = raw.trim().trim_end_matches('/').to_string();

    if normalized.contains("://localhost") {
        let fixed = normalized.replacen("://localhost", "://127.0.0.1", 1);
        tracing::warn!(
            service_url = %normalized,
            normalized_service_url = %fixed,
            "Normalizing service URL host from localhost to 127.0.0.1"
        );
        normalized = fixed;
    }

    normalized
}

/// Configuration for semantic search
#[derive(Debug, Clone)]
pub struct SemanticConfig {
    /// Semantic backend mode (`inprocess` or `remote`)
    pub backend: SemanticBackendKind,
    /// Qdrant server URL (default: http://127.0.0.1:6334)
    pub qdrant_url: String,
    /// Embedding model identifier (default: BAAI/bge-large-en-v1.5)
    pub embed_model: String,
    /// Default collection name
    pub collection: String,
    /// Minimum score threshold for results (default: 0.2)
    pub min_threshold: f32,
    /// High confidence threshold (default: 0.5)
    pub high_confidence: f32,
    /// Remote semantic service URL (used only when backend=remote)
    pub remote_url: String,
    /// Optional bearer token for the remote semantic service
    pub remote_token: Option<String>,
    /// Remote semantic service timeout in seconds
    pub remote_timeout_secs: u64,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            backend: SemanticBackendKind::InProcess,
            qdrant_url: "http://127.0.0.1:6334".to_string(),
            embed_model: "BAAI/bge-large-en-v1.5".to_string(),
            collection: "quiron_events".to_string(),
            min_threshold: 0.2,
            high_confidence: 0.5,
            remote_url: "http://127.0.0.1:8091".to_string(),
            remote_token: None,
            remote_timeout_secs: 120,
        }
    }
}

impl SemanticConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self {
            backend: SemanticBackendKind::from_env(),
            qdrant_url: normalize_qdrant_url(
                std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".to_string()),
            ),
            embed_model: std::env::var("EMBED_MODEL")
                .unwrap_or_else(|_| "BAAI/bge-large-en-v1.5".to_string()),
            collection: std::env::var("QDRANT_COLLECTION")
                .unwrap_or_else(|_| "quiron_events".to_string()),
            min_threshold: std::env::var("SEMANTIC_MIN_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.2),
            high_confidence: std::env::var("SEMANTIC_HIGH_CONFIDENCE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.5),
            remote_url: normalize_service_url(
                std::env::var("SEMANTIC_REMOTE_URL")
                    .or_else(|_| std::env::var("QUIRON_SEMANTIC_REMOTE_URL"))
                    .unwrap_or_else(|_| "http://127.0.0.1:8091".to_string()),
            ),
            remote_token: std::env::var("SEMANTIC_REMOTE_TOKEN")
                .ok()
                .or_else(|| std::env::var("QUIRON_SEMANTIC_REMOTE_TOKEN").ok())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            remote_timeout_secs: std::env::var("SEMANTIC_REMOTE_TIMEOUT_SECS")
                .ok()
                .or_else(|| std::env::var("QUIRON_SEMANTIC_REMOTE_TIMEOUT_SECS").ok())
                .and_then(|s| s.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(120),
        }
    }

    pub fn backend_label(&self) -> &'static str {
        match self.backend {
            SemanticBackendKind::InProcess => "inprocess",
            SemanticBackendKind::Remote => "remote",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_backend_defaults_to_inprocess() {
        assert_eq!(parse_backend_kind(None), SemanticBackendKind::InProcess);
    }

    #[test]
    fn parse_backend_accepts_remote() {
        assert_eq!(
            parse_backend_kind(Some("remote")),
            SemanticBackendKind::Remote
        );
    }

    #[test]
    fn parse_backend_accepts_inprocess_aliases() {
        assert_eq!(
            parse_backend_kind(Some("in_process")),
            SemanticBackendKind::InProcess
        );
        assert_eq!(
            parse_backend_kind(Some("in-process")),
            SemanticBackendKind::InProcess
        );
    }
}
