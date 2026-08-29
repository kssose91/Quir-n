//! Neo4j integration module for the hybrid architecture.
//!
//! This module provides:
//! - Connector: Pool-based connection management
//! - Schema: Cypher constraints and indexes
//! - Projector: Ledger to graph projection
//! - Sync: Background synchronization service
//! - Queries: Audit and inspection queries
//!
//! **Principio sagrado**: RocksDB = verdad única, Neo4j = proyección reconstruible.

#[cfg(feature = "neo4j")]
pub mod connector;

#[cfg(feature = "neo4j")]
pub mod schema;

#[cfg(feature = "neo4j")]
pub mod projector;

#[cfg(feature = "neo4j")]
pub mod sync;

#[cfg(feature = "neo4j")]
pub mod queries;

#[cfg(feature = "neo4j")]
pub use connector::Neo4jConnector;

#[cfg(feature = "neo4j")]
pub use projector::Projector;

#[cfg(feature = "neo4j")]
pub use sync::SyncService;
