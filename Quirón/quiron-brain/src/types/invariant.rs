//! Invariant types for the gate system.

use crate::types::ids::{AlertId, EventId, InvariantId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An invariant rule that must be satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invariant {
    /// Unique invariant ID.
    pub id: InvariantId,

    /// Name of the invariant.
    pub name: String,

    /// Description of what it checks.
    pub description: String,

    /// Scope of application.
    pub scope: Scope,

    /// The predicate to evaluate.
    pub predicate: Predicate,

    /// Severity when violated.
    pub severity: Severity,

    /// Optional autofix command.
    pub autofix: Option<String>,

    /// Whether the invariant is active.
    pub enabled: bool,

    /// When the invariant was created.
    pub created_at: DateTime<Utc>,

    /// Event that created this invariant.
    pub created_by_event: Option<EventId>,

    /// How many times this has been violated.
    pub violation_count: u32,
}

/// Scope of an invariant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Scope {
    /// Applies to everything.
    Global,
    /// Applies to a specific project.
    Project { id: String },
    /// Applies to a specific file pattern.
    File { pattern: String },
    /// Applies to a specific agent.
    Agent { id: String },
}

/// Severity of an invariant violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Just log.
    Info,
    /// Log and create warning.
    Warn,
    /// Block the action.
    Block,
    /// Block and notify human.
    Critical,
}

/// A predicate that can be evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Predicate {
    /// Action pattern is forbidden.
    ForbiddenAction { pattern: String },

    /// Path is protected, only certain ops allowed.
    ProtectedPath {
        path: String,
        allowed_ops: Vec<String>,
    },

    /// File changes require tests.
    RequiresTest { file_pattern: String },

    /// Metric must be in range.
    NumericRange {
        metric: String,
        min: Option<f64>,
        max: Option<f64>,
    },

    /// One action requires another first.
    RequiresDependency {
        action_pattern: String,
        required_action: String,
    },

    /// Custom script evaluation.
    Custom { script: String, args: Vec<String> },

    /// Content pattern is forbidden.
    ContentPattern {
        file_pattern: String,
        forbidden_pattern: String,
    },

    /// Always returns the given value (for testing).
    Always { value: bool },

    // === Senior Supervisor Protocol Gates ===
    /// Gate A: Claims require evidence (no "está arreglado" without proof).
    NoClaimWithoutEvidence {
        /// Patterns that trigger this check (e.g., "arreglado", "fixed").
        claim_patterns: Vec<String>,
        /// Required evidence kind (e.g., "verification").
        required_evidence: String,
    },

    /// Gate B: Cannot edit files without reading them first.
    NoGhostEditing {
        /// Time window for read-before-write (seconds).
        max_age_seconds: u64,
    },

    /// Gate C: Cannot edit files outside defined scope.
    ScopeLock {
        /// Allowed paths (glob patterns).
        allowed_paths: Vec<String>,
    },

    /// Gate D: Changes require verification within time window.
    NoSilentBreak {
        /// Max seconds after patch to require verification.
        max_seconds_after_patch: u64,
    },

    /// Gate E: If verification fails, mark patch as failed.
    AutoRevertOnFailure,

    /// Gate F: Route worker requires explicit delegated worker task.
    NoAutonomousRouting,

    /// Gate G: Worker output cannot contain final factual claims.
    NoClaimFromWorker,
}

/// An alert generated when an invariant is violated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique alert ID.
    pub id: AlertId,

    /// Invariant that was violated.
    pub invariant_id: InvariantId,

    /// Event that triggered the alert.
    pub event_id: EventId,

    /// When the alert was created.
    pub ts: DateTime<Utc>,

    /// Severity of the alert.
    pub severity: Severity,

    /// Human-readable message.
    pub message: String,

    /// Context about the violation (JSON string).
    pub context: String,

    /// Whether the alert has been acknowledged.
    pub acknowledged: bool,
}

impl Invariant {
    /// Create a new invariant.
    pub fn new(name: impl Into<String>, predicate: Predicate) -> Self {
        Self {
            id: InvariantId::new(),
            name: name.into(),
            description: String::new(),
            scope: Scope::Global,
            predicate,
            severity: Severity::Warn,
            autofix: None,
            enabled: true,
            created_at: Utc::now(),
            created_by_event: None,
            violation_count: 0,
        }
    }

    /// Create a blocking invariant.
    pub fn blocking(name: impl Into<String>, predicate: Predicate) -> Self {
        let mut inv = Self::new(name, predicate);
        inv.severity = Severity::Block;
        inv
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set scope.
    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Set severity.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Set autofix command.
    pub fn with_autofix(mut self, autofix: impl Into<String>) -> Self {
        self.autofix = Some(autofix.into());
        self
    }
}

impl Alert {
    /// Create a new alert.
    pub fn new(
        invariant_id: InvariantId,
        event_id: EventId,
        severity: Severity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: AlertId::new(),
            invariant_id,
            event_id,
            ts: Utc::now(),
            severity,
            message: message.into(),
            context: "{}".to_string(),
            acknowledged: false,
        }
    }

    /// Set context.
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = context.to_string();
        self
    }
}
