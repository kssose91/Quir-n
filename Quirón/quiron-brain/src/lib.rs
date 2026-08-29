//! Quiron Brain - Neural Memory System
//!
//! A mandatory local brain for AI agents where everything passes through:
//! - Ledger (immutable event log)
//! - Graph (derived knowledge network)
//! - Gates (invariants that block errors)
//! - Evidence (proof of every action)
//! - Supervisor (anti-smoke protocol)
//! - Neo4j (optional graph projection for queries)

pub mod api;
pub mod claims;
pub mod distillation;
pub mod distillation_worker;
pub mod enrichment_worker;
pub mod error;
pub mod evaluator;
pub mod graph;
pub mod invariants;
pub mod keyword_search;
pub mod ledger;
pub mod llm_client;
pub mod llm_distiller;
pub mod llm_enricher;
pub mod memory;
pub mod replay;
pub mod representations;
pub mod retrieval;
pub mod storage;
pub mod supervisor;
pub mod types;
pub mod vct;

#[cfg(feature = "neo4j")]
pub mod neo4j;

#[cfg(feature = "semantic")]
pub mod semantic;

pub use error::{BrainError, Result};
