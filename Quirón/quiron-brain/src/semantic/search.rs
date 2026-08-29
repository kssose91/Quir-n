//! Search result types and utilities

use serde::{Deserialize, Serialize};

/// Result from a semantic search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Internal Qdrant point ID
    pub id: String,
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Event ID from payload
    pub event_id: Option<String>,
    /// Event kind from payload
    pub kind: Option<String>,
    /// Event description from payload
    pub description: Option<String>,
    /// Event timestamp from payload
    pub timestamp: Option<String>,
    /// Event importance from payload (0.0 - 1.0)
    pub importance: Option<f32>,
}

/// Result with re-ranking applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedResult {
    /// Original search result
    #[serde(flatten)]
    pub result: SearchResult,
    /// Final score after re-ranking: similarity × importance × freshness
    pub final_score: f32,
    /// Reasons for the ranking boost/penalty
    pub reasons: Vec<String>,
}

/// Result from CRAG (Corrective RAG) search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CragResult {
    /// Search results (empty if blocked)
    pub results: Vec<SearchResult>,
    /// Confidence level: "HIGH", "MEDIUM", or "BLOCKED"
    pub confidence: String,
    /// Whether the search was blocked due to insufficient evidence
    pub blocked: bool,
    /// Number of search attempts made
    pub attempts: u32,
    /// Original query
    pub original_query: String,
    /// Reformulated query if any
    pub reformulated_query: Option<String>,
}

impl CragResult {
    /// Check if this result has high confidence
    pub fn is_high_confidence(&self) -> bool {
        self.confidence == "HIGH"
    }

    /// Check if this result was blocked
    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// Get the best result if any
    pub fn best(&self) -> Option<&SearchResult> {
        self.results.first()
    }
}

/// Search query parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Query text
    pub q: String,
    /// Maximum results to return
    #[serde(default = "default_limit")]
    pub limit: u64,
    /// Minimum score threshold
    pub threshold: Option<f32>,
    /// Collection to search (optional)
    pub collection: Option<String>,
}

fn default_limit() -> u64 {
    5
}

/// Response for search endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Original query
    pub query: String,
    /// Search results
    pub results: Vec<SearchResult>,
    /// Whether results were blocked due to threshold
    pub blocked: bool,
    /// Confidence level
    pub confidence: String,
}

/// Response for CRAG endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CragResponse {
    /// CRAG result
    #[serde(flatten)]
    pub result: CragResult,
}
