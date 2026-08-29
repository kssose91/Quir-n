//! Error handling - updated for sled.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BrainError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Event not found: {0}")]
    EventNotFound(String),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Invariant not found: {0}")]
    InvariantNotFound(String),

    #[error("Invariant violation: {0}")]
    InvariantViolation(String),

    #[error("Action blocked: {reason}")]
    ActionBlocked {
        reason: String,
        invariants: Vec<String>,
    },

    #[error("Invalid predicate: {0}")]
    InvalidPredicate(String),

    #[error("Invalid ID: {0}")]
    InvalidId(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, BrainError>;
