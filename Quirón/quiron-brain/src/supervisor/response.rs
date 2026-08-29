//! Supervisor Response - Protocol for validated responses.
//!
//! Every supervisor response must follow this format with mandatory evidence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{EventId, EvidenceRef};

/// Status of a supervisor response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ResponseStatus {
    /// Changes were integrated and verified.
    Integrated = 0,
    /// Changes were rejected (failed verification or out of scope).
    Rejected = 1,
    /// Changes are proposed but not yet applied.
    Proposed = 2,
    /// Waiting for more information.
    Blocked = 3,
}

/// A validated supervisor response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorResponse {
    /// Unique response ID (links to event).
    pub id: EventId,
    /// When the response was created.
    pub created_at: DateTime<Utc>,
    /// Final status.
    pub status: ResponseStatus,
    /// Scope that was declared for this change.
    pub scope: ScopeDeclaration,
    /// Files that were read (evidence of context).
    pub context_read: Vec<ContextRead>,
    /// Patch that was proposed/applied.
    pub patch: Option<PatchInfo>,
    /// Verification results.
    pub verification: Option<VerificationResult>,
    /// Human-readable summary.
    pub summary: String,
    /// Reason for rejection (if rejected).
    pub rejection_reason: Option<String>,
}

/// Declaration of scope before making changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeDeclaration {
    /// Files that will be modified.
    pub files: Vec<String>,
    /// Description of what will change.
    pub description: String,
    /// Whether scope was explicitly approved.
    pub approved: bool,
}

/// Evidence that context was read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRead {
    /// File path that was read.
    pub path: String,
    /// Line range (if partial).
    pub lines: Option<(u32, u32)>,
    /// Blake3 hash of content read.
    pub content_hash: [u8; 32],
    /// Evidence reference.
    pub evidence: EvidenceRef,
}

/// Information about a patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    /// Files affected.
    pub files: Vec<String>,
    /// Lines added.
    pub lines_added: u32,
    /// Lines removed.
    pub lines_removed: u32,
    /// Blake3 hash of the unified diff.
    pub diff_hash: [u8; 32],
    /// Whether patch was applied.
    pub applied: bool,
    /// Evidence reference.
    pub evidence: EvidenceRef,
}

/// Verification result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Type of verification (test, build, lint).
    pub kind: VerificationKind,
    /// Whether verification passed.
    pub passed: bool,
    /// Number of tests run (if applicable).
    pub tests_run: Option<u32>,
    /// Number of tests failed (if applicable).
    pub tests_failed: Option<u32>,
    /// Output summary.
    pub output_summary: String,
    /// Evidence reference.
    pub evidence: EvidenceRef,
}

/// Type of verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum VerificationKind {
    /// Unit/integration tests.
    Test = 0,
    /// Build/compile.
    Build = 1,
    /// Linting.
    Lint = 2,
    /// Type checking.
    TypeCheck = 3,
    /// Manual verification.
    Manual = 4,
}

/// Builder for SupervisorResponse.
#[derive(Debug)]
pub struct ResponseBuilder {
    id: EventId,
    scope: Option<ScopeDeclaration>,
    context_read: Vec<ContextRead>,
    patch: Option<PatchInfo>,
    verification: Option<VerificationResult>,
    summary: String,
}

impl ResponseBuilder {
    /// Create a new response builder.
    pub fn new(id: EventId) -> Self {
        Self {
            id,
            scope: None,
            context_read: Vec::new(),
            patch: None,
            verification: None,
            summary: String::new(),
        }
    }

    /// Declare scope.
    pub fn scope(mut self, files: Vec<String>, description: impl Into<String>) -> Self {
        self.scope = Some(ScopeDeclaration {
            files,
            description: description.into(),
            approved: false,
        });
        self
    }

    /// Add context read evidence.
    pub fn read_context(mut self, read: ContextRead) -> Self {
        self.context_read.push(read);
        self
    }

    /// Set patch info.
    pub fn patch(mut self, patch: PatchInfo) -> Self {
        self.patch = Some(patch);
        self
    }

    /// Set verification result.
    pub fn verification(mut self, verification: VerificationResult) -> Self {
        self.verification = Some(verification);
        self
    }

    /// Set summary.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    /// Build as integrated (success).
    pub fn integrate(self) -> Result<SupervisorResponse, ValidationError> {
        self.validate()?;
        self.finalize(ResponseStatus::Integrated, None)
    }

    /// Build as rejected.
    pub fn reject(self, reason: impl Into<String>) -> Result<SupervisorResponse, ValidationError> {
        self.finalize(ResponseStatus::Rejected, Some(reason.into()))
    }

    /// Build as proposed (pending).
    pub fn propose(self) -> Result<SupervisorResponse, ValidationError> {
        self.validate_proposal()?;
        self.finalize(ResponseStatus::Proposed, None)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        // Gate A: Must have scope
        if self.scope.is_none() {
            return Err(ValidationError::NoScope);
        }

        // Gate B: Must have read context
        if self.context_read.is_empty() {
            return Err(ValidationError::NoContextRead);
        }

        // Gate D: If patch exists, must have verification
        if self.patch.is_some() && self.verification.is_none() {
            return Err(ValidationError::NoVerification);
        }

        // Gate E: If verification exists and failed, cannot integrate
        if let Some(ref v) = self.verification {
            if !v.passed {
                return Err(ValidationError::VerificationFailed);
            }
        }

        Ok(())
    }

    fn validate_proposal(&self) -> Result<(), ValidationError> {
        // Proposals need scope but not verification yet
        if self.scope.is_none() {
            return Err(ValidationError::NoScope);
        }
        Ok(())
    }

    fn finalize(
        self,
        status: ResponseStatus,
        rejection_reason: Option<String>,
    ) -> Result<SupervisorResponse, ValidationError> {
        Ok(SupervisorResponse {
            id: self.id,
            created_at: Utc::now(),
            status,
            scope: self.scope.unwrap_or(ScopeDeclaration {
                files: Vec::new(),
                description: String::new(),
                approved: false,
            }),
            context_read: self.context_read,
            patch: self.patch,
            verification: self.verification,
            summary: self.summary,
            rejection_reason,
        })
    }
}

/// Validation error for response building.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// No scope was declared.
    NoScope,
    /// No context was read (ghost editing).
    NoContextRead,
    /// No verification after patch.
    NoVerification,
    /// Verification failed, cannot integrate.
    VerificationFailed,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoScope => write!(f, "No scope declared - define scope before changes"),
            Self::NoContextRead => write!(f, "No context read - read files before modifying"),
            Self::NoVerification => write!(f, "No verification - run tests after patch"),
            Self::VerificationFailed => write!(f, "Verification failed - fix or revert"),
        }
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventId;

    fn dummy_evidence() -> EvidenceRef {
        EvidenceRef::file_span("test.rs", 1, 10, [0u8; 32])
    }

    #[test]
    fn test_valid_integration() {
        let id = EventId::new();

        let context = ContextRead {
            path: "src/main.rs".into(),
            lines: Some((1, 50)),
            content_hash: [0u8; 32],
            evidence: dummy_evidence(),
        };

        let patch = PatchInfo {
            files: vec!["src/main.rs".into()],
            lines_added: 5,
            lines_removed: 2,
            diff_hash: [1u8; 32],
            applied: true,
            evidence: dummy_evidence(),
        };

        let verification = VerificationResult {
            kind: VerificationKind::Test,
            passed: true,
            tests_run: Some(12),
            tests_failed: Some(0),
            output_summary: "All tests passed".into(),
            evidence: dummy_evidence(),
        };

        let response = ResponseBuilder::new(id)
            .scope(vec!["src/main.rs".into()], "Fix bug in main")
            .read_context(context)
            .patch(patch)
            .verification(verification)
            .summary("Fixed the bug")
            .integrate();

        assert!(response.is_ok());
        let resp = response.unwrap();
        assert_eq!(resp.status, ResponseStatus::Integrated);
    }

    #[test]
    fn test_reject_no_scope() {
        let id = EventId::new();

        let result = ResponseBuilder::new(id).summary("No scope").integrate();

        assert_eq!(result.unwrap_err(), ValidationError::NoScope);
    }

    #[test]
    fn test_reject_no_verification() {
        let id = EventId::new();

        let context = ContextRead {
            path: "test.rs".into(),
            lines: None,
            content_hash: [0u8; 32],
            evidence: dummy_evidence(),
        };

        let patch = PatchInfo {
            files: vec!["test.rs".into()],
            lines_added: 1,
            lines_removed: 0,
            diff_hash: [1u8; 32],
            applied: true,
            evidence: dummy_evidence(),
        };

        let result = ResponseBuilder::new(id)
            .scope(vec!["test.rs".into()], "Add feature")
            .read_context(context)
            .patch(patch)
            .integrate();

        assert_eq!(result.unwrap_err(), ValidationError::NoVerification);
    }

    #[test]
    fn test_reject_failed_verification() {
        let id = EventId::new();

        let context = ContextRead {
            path: "test.rs".into(),
            lines: None,
            content_hash: [0u8; 32],
            evidence: dummy_evidence(),
        };

        let patch = PatchInfo {
            files: vec!["test.rs".into()],
            lines_added: 1,
            lines_removed: 0,
            diff_hash: [1u8; 32],
            applied: true,
            evidence: dummy_evidence(),
        };

        let verification = VerificationResult {
            kind: VerificationKind::Test,
            passed: false,
            tests_run: Some(12),
            tests_failed: Some(3),
            output_summary: "3 tests failed".into(),
            evidence: dummy_evidence(),
        };

        let result = ResponseBuilder::new(id)
            .scope(vec!["test.rs".into()], "Add feature")
            .read_context(context)
            .patch(patch)
            .verification(verification)
            .integrate();

        assert_eq!(result.unwrap_err(), ValidationError::VerificationFailed);
    }
}
