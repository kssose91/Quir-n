//! Memory subsystems for Quirón brain.
//!
//! ## Modules
//!
//! - **associative**: Hierarchical memory with spreading activation
//!   - Memories organized in parent/child relationships
//!   - Accessing a memory "unlocks" related memories
//!   - Reduces noise by only showing contextually relevant memories
//!
//! - **node**: Consolidated memory nodes (distilled knowledge)
//!   - MemoryNode: summarized, navigable memories
//!   - MemoryLink: associations between memories
//!
//! - **envelope**: Versioned semantic metadata per ledger event
//!   - truth_status, confidence, source and scope without mutating Event
//!   - current snapshot + revision history

pub mod associative;
pub mod envelope;
pub mod node;

pub use associative::{ActivationConfig, ActivationStats, AssociativeMemory, MemoryActivation};
pub use envelope::{
    MemoryEnvelope, MemoryEnvelopeStore, MemoryKind, MemoryScope, MemorySource, MemoryTarget,
    PromotionStatus, TruthStatus,
};
pub use node::{
    DistillRequest, ExpandResponse, ExpandStats, MemoryLink, MemoryLinkKind, MemoryNode,
};
