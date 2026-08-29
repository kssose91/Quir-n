//! Senior Supervisor module.
//!
//! This module implements the anti-smoke protocol for code engineering.

mod project_map;
pub mod response;

pub use response::{
    ContextRead, PatchInfo, ResponseBuilder, ResponseStatus, ScopeDeclaration, SupervisorResponse,
    ValidationError, VerificationKind, VerificationResult,
};
