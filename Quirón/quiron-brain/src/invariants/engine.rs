//! Invariant engine for validating actions.

use crate::error::Result;
use crate::storage::cf::*;
use crate::storage::Storage;
#[allow(unused_imports)] // Reserved for future event correlation
use crate::types::EventId;
use crate::types::{Alert, Invariant, InvariantId, Predicate, Scope, Severity};
use serde::{Deserialize, Serialize};

/// Engine for managing and evaluating invariants.
pub struct InvariantEngine {
    storage: Storage,
}

/// Result of validating an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the action is allowed.
    pub allowed: bool,
    /// Reason for blocking (if blocked).
    pub reason: Option<String>,
    /// Invariants that block this action.
    pub blocking_invariants: Vec<String>,
    /// Warnings (non-blocking).
    pub warnings: Vec<String>,
}

/// Request to validate an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    /// Type of action (e.g., "write", "delete", "execute").
    pub action: String,
    /// Target of the action (e.g., file path).
    pub target: Option<String>,
    /// Project context.
    pub project_id: Option<String>,
    /// Agent making the request.
    pub agent_id: Option<String>,
    /// Additional context.
    pub context: serde_json::Value,
}

impl InvariantEngine {
    /// Create a new invariant engine.
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Register a new invariant.
    pub fn register(&self, invariant: &Invariant) -> Result<()> {
        let key = invariant.id.to_bytes();
        let value = bincode::serialize(invariant)?;
        self.storage.put(CF_INVARIANTS, &key, &value)?;
        tracing::info!(
            "Registered invariant: {} ({:?})",
            invariant.name,
            invariant.severity
        );
        Ok(())
    }

    /// Get an invariant by ID.
    pub fn get(&self, id: &InvariantId) -> Result<Option<Invariant>> {
        let key = id.to_bytes();
        match self.storage.get(CF_INVARIANTS, &key)? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Get all invariants.
    pub fn all(&self) -> Result<Vec<Invariant>> {
        let mut invariants = Vec::new();

        for (_, value) in self.storage.iter_tree(CF_INVARIANTS)? {
            let inv: Invariant = bincode::deserialize(&value)?;
            invariants.push(inv);
        }

        Ok(invariants)
    }

    /// Get enabled invariants matching a scope.
    pub fn matching_scope(&self, project_id: Option<&str>) -> Result<Vec<Invariant>> {
        let all = self.all()?;

        Ok(all
            .into_iter()
            .filter(|inv| {
                if !inv.enabled {
                    return false;
                }
                match &inv.scope {
                    Scope::Global => true,
                    Scope::Project { id } => project_id.map_or(false, |p| p == id),
                    _ => true, // File/Agent scopes need more context
                }
            })
            .collect())
    }

    /// Validate an action request.
    pub fn validate(&self, request: &ActionRequest) -> Result<ValidationResult> {
        let invariants = self.matching_scope(request.project_id.as_deref())?;

        let mut result = ValidationResult {
            allowed: true,
            reason: None,
            blocking_invariants: vec![],
            warnings: vec![],
        };

        for inv in invariants {
            let (passes, message) = self.check_predicate(&inv.predicate, request);

            if !passes {
                match inv.severity {
                    Severity::Info => {
                        tracing::info!("Invariant info: {}", inv.name);
                    }
                    Severity::Warn => {
                        result.warnings.push(format!("{}: {}", inv.name, message));
                    }
                    Severity::Block | Severity::Critical => {
                        result.allowed = false;
                        result.blocking_invariants.push(inv.name.clone());
                        if result.reason.is_none() {
                            result.reason = Some(message);
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Check a single predicate against a request.
    fn check_predicate(&self, pred: &Predicate, request: &ActionRequest) -> (bool, String) {
        match pred {
            Predicate::ForbiddenAction { pattern } => {
                if request.action.contains(pattern) {
                    (
                        false,
                        format!(
                            "Action '{}' matches forbidden pattern '{}'",
                            request.action, pattern
                        ),
                    )
                } else {
                    (true, String::new())
                }
            }

            Predicate::ProtectedPath { path, allowed_ops } => {
                if let Some(ref target) = request.target {
                    if target.starts_with(path) || target == path {
                        if !allowed_ops.contains(&request.action) {
                            return (
                                false,
                                format!(
                                    "Path '{}' is protected. Allowed ops: {:?}, got: '{}'",
                                    path, allowed_ops, request.action
                                ),
                            );
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::RequiresTest { file_pattern } => {
                // Check if the action modifies files matching pattern
                if let Some(ref target) = request.target {
                    if target.contains(file_pattern) && request.action == "write" {
                        // Check context for test coverage indicator
                        if let Some(tested) = request.context.get("tested") {
                            if tested.as_bool() != Some(true) {
                                return (
                                    false,
                                    format!("File '{}' requires tests before modification", target),
                                );
                            }
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::NumericRange { metric, min, max } => {
                // Check if a metric in context is within range
                if let Some(value) = request.context.get(metric) {
                    if let Some(num) = value.as_f64() {
                        if let Some(min_val) = min {
                            if num < *min_val {
                                return (
                                    false,
                                    format!(
                                        "Metric '{}' value {} is below minimum {}",
                                        metric, num, min_val
                                    ),
                                );
                            }
                        }
                        if let Some(max_val) = max {
                            if num > *max_val {
                                return (
                                    false,
                                    format!(
                                        "Metric '{}' value {} exceeds maximum {}",
                                        metric, num, max_val
                                    ),
                                );
                            }
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::ContentPattern {
                file_pattern,
                forbidden_pattern,
            } => {
                // Check if the content in context contains forbidden pattern
                if let Some(ref target) = request.target {
                    if target.contains(file_pattern) {
                        if let Some(content) = request.context.get("content") {
                            if let Some(text) = content.as_str() {
                                if text.contains(forbidden_pattern) {
                                    return (
                                        false,
                                        format!(
                                            "Content contains forbidden pattern '{}' in file matching '{}'",
                                            forbidden_pattern, file_pattern
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::RequiresDependency {
                action_pattern,
                required_action,
            } => {
                // Check if a required action was performed before this one
                if request.action.contains(action_pattern) {
                    if let Some(deps) = request.context.get("completed_actions") {
                        if let Some(arr) = deps.as_array() {
                            let has_dep = arr
                                .iter()
                                .any(|v| v.as_str().map_or(false, |s| s.contains(required_action)));
                            if !has_dep {
                                return (
                                    false,
                                    format!(
                                        "Action '{}' requires '{}' to be completed first",
                                        request.action, required_action
                                    ),
                                );
                            }
                        } else {
                            return (
                                false,
                                format!(
                                    "Action '{}' requires '{}' to be completed first",
                                    request.action, required_action
                                ),
                            );
                        }
                    } else {
                        return (
                            false,
                            format!(
                                "Action '{}' requires '{}' to be completed first",
                                request.action, required_action
                            ),
                        );
                    }
                }
                (true, String::new())
            }

            Predicate::Always { value } => {
                if *value {
                    (true, String::new())
                } else {
                    (false, "Always-false invariant triggered".to_string())
                }
            }

            Predicate::Custom { script: _, args: _ } => {
                // Custom predicates require external execution
                // For now, allow by default
                tracing::warn!("Custom predicate not yet implemented, allowing action");
                (true, String::new())
            }

            // === Senior Supervisor Protocol Gates ===
            Predicate::NoClaimWithoutEvidence {
                claim_patterns,
                required_evidence,
            } => {
                // Gate A: Claims like "arreglado", "fixed" require evidence
                let desc = request.action.to_lowercase();

                // Check if action matches any claim pattern
                let is_claim = claim_patterns.iter().any(|p| desc.contains(p));

                if is_claim {
                    // Check if required evidence exists in context
                    if let Some(evidence) = request.context.get("evidence") {
                        if let Some(arr) = evidence.as_array() {
                            let has_required = arr.iter().any(|e| {
                                e.get("kind")
                                    .and_then(|k| k.as_str())
                                    .map_or(false, |k| k == required_evidence)
                            });
                            if !has_required {
                                return (
                                    false,
                                    format!(
                                        "Claim '{}' requires '{}' evidence. Provide proof or remove claim.",
                                        request.action, required_evidence
                                    ),
                                );
                            }
                        } else {
                            return (
                                false,
                                format!(
                                    "Claim '{}' requires '{}' evidence (no evidence array found)",
                                    request.action, required_evidence
                                ),
                            );
                        }
                    } else {
                        return (
                            false,
                            format!(
                                "Claim '{}' requires '{}' evidence (no evidence in context)",
                                request.action, required_evidence
                            ),
                        );
                    }
                }
                (true, String::new())
            }

            Predicate::NoGhostEditing { max_age_seconds } => {
                // Gate B: Cannot edit without prior read
                if request.action == "write" || request.action == "patch" {
                    if let Some(ref target) = request.target {
                        // Check for prior read in context
                        if let Some(reads) = request.context.get("files_read") {
                            if let Some(arr) = reads.as_array() {
                                let was_read = arr.iter().any(|r| {
                                    let path_match = r
                                        .get("path")
                                        .and_then(|p| p.as_str())
                                        .map_or(false, |p| p == target);
                                    let age_ok = r
                                        .get("age_seconds")
                                        .and_then(|a| a.as_u64())
                                        .map_or(false, |a| a <= *max_age_seconds);
                                    path_match && age_ok
                                });
                                if !was_read {
                                    return (
                                        false,
                                        format!(
                                            "Cannot {} '{}' without reading it first (max age: {}s)",
                                            request.action, target, max_age_seconds
                                        ),
                                    );
                                }
                            }
                        } else {
                            return (
                                false,
                                format!(
                                    "Cannot {} '{}' - no files_read in context (ghost editing)",
                                    request.action, target
                                ),
                            );
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::ScopeLock { allowed_paths } => {
                // Gate C: Cannot edit outside scope
                if request.action == "write"
                    || request.action == "patch"
                    || request.action == "delete"
                {
                    if let Some(ref target) = request.target {
                        let in_scope = allowed_paths.iter().any(|pattern| {
                            // Simple glob matching: * matches anything
                            if pattern.ends_with("/*") {
                                let prefix = pattern.trim_end_matches("/*");
                                target.starts_with(prefix)
                            } else if pattern.ends_with("/**") {
                                let prefix = pattern.trim_end_matches("/**");
                                target.starts_with(prefix)
                            } else {
                                target == pattern || target.starts_with(pattern)
                            }
                        });

                        if !in_scope && !allowed_paths.is_empty() {
                            return (
                                false,
                                format!(
                                    "Path '{}' is outside allowed scope: {:?}. Define scope before editing.",
                                    target, allowed_paths
                                ),
                            );
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::NoSilentBreak {
                max_seconds_after_patch,
            } => {
                // Gate D: Changes require verification
                if request.action == "patch_applied" {
                    // This is a post-action check - warn if verification not found
                    if let Some(verification) = request.context.get("verification") {
                        if let Some(age) = verification.get("age_seconds").and_then(|a| a.as_u64())
                        {
                            if age > *max_seconds_after_patch {
                                return (
                                    false,
                                    format!(
                                        "Verification too old ({}s > {}s). Run tests after applying patch.",
                                        age, max_seconds_after_patch
                                    ),
                                );
                            }
                        }
                    } else {
                        // This is a warning, not a block - allow but flag
                        tracing::warn!(
                            "Patch applied without verification within {}s",
                            max_seconds_after_patch
                        );
                    }
                }
                (true, String::new())
            }

            Predicate::AutoRevertOnFailure => {
                // Gate E: If verification failed, reject the patch claim
                if request.action == "claim_verified" || request.action == "patch_success" {
                    if let Some(verification) = request.context.get("verification") {
                        let passed = verification
                            .get("passed")
                            .and_then(|p| p.as_bool())
                            .unwrap_or(false);

                        if !passed {
                            let failed_count = verification
                                .get("tests_failed")
                                .and_then(|f| f.as_u64())
                                .unwrap_or(0);
                            return (
                                false,
                                format!(
                                    "Cannot mark as success - verification failed ({} tests failed). Fix or revert.",
                                    failed_count
                                ),
                            );
                        }
                    }
                }
                (true, String::new())
            }

            Predicate::NoAutonomousRouting => {
                // Gate F: worker route only allowed with explicit delegated task.
                if request.action == "llm_messages_proxy" {
                    let route = request
                        .context
                        .get("route")
                        .and_then(|v| v.as_str())
                        .unwrap_or("primary")
                        .trim()
                        .to_ascii_lowercase();
                    let has_worker_task = request
                        .context
                        .get("has_worker_task")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if route == "worker" && !has_worker_task {
                        return (
                            false,
                            "Worker route requires explicit worker_task delegation".to_string(),
                        );
                    }

                    if route != "primary" && route != "worker" {
                        return (
                            false,
                            format!("Invalid route '{}': expected primary|worker", route),
                        );
                    }
                }
                (true, String::new())
            }

            Predicate::NoClaimFromWorker => {
                // Gate G: worker cannot emit final factual claims.
                if request.action == "llm_worker_result" {
                    let route = request
                        .context
                        .get("route")
                        .and_then(|v| v.as_str())
                        .unwrap_or("primary")
                        .trim()
                        .to_ascii_lowercase();

                    let claims_count = request
                        .context
                        .get("claims_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);

                    if route == "worker" && claims_count > 0 {
                        return (
                            false,
                            format!(
                                "Worker output contains {} factual claim(s); escalate to primary route",
                                claims_count
                            ),
                        );
                    }
                }
                (true, String::new())
            }
        }
    }

    /// Record an alert for a violation.
    pub fn record_alert(&self, alert: &Alert) -> Result<()> {
        let key = alert.id.to_bytes();
        let value = bincode::serialize(alert)?;
        self.storage.put(CF_ALERTS, &key, &value)?;

        // Increment violation count on invariant
        if let Some(mut inv) = self.get(&alert.invariant_id)? {
            inv.violation_count += 1;
            self.register(&inv)?;
        }

        Ok(())
    }

    /// Get recent alerts.
    pub fn recent_alerts(&self, limit: usize) -> Result<Vec<Alert>> {
        let mut alerts = Vec::new();

        for (_, value) in self.storage.iter_tree(CF_ALERTS)? {
            if alerts.len() >= limit {
                break;
            }
            let alert: Alert = bincode::deserialize(&value)?;
            alerts.push(alert);
        }

        // Sort by timestamp descending
        alerts.sort_by(|a, b| b.ts.cmp(&a.ts));
        alerts.truncate(limit);

        Ok(alerts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_register_and_get() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        let inv = Invariant::blocking(
            "no-delete",
            Predicate::ForbiddenAction {
                pattern: "delete".to_string(),
            },
        );
        let inv_id = inv.id;

        engine.register(&inv).unwrap();

        let loaded = engine.get(&inv_id).unwrap().unwrap();
        assert_eq!(loaded.name, "no-delete");
    }

    #[test]
    fn test_validate_blocked() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register blocking invariant
        let inv = Invariant::blocking(
            "no-delete",
            Predicate::ForbiddenAction {
                pattern: "delete".to_string(),
            },
        );
        engine.register(&inv).unwrap();

        // Try forbidden action
        let request = ActionRequest {
            action: "delete".to_string(),
            target: Some("/some/file".to_string()),
            project_id: None,
            agent_id: None,
            context: serde_json::json!({}),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .blocking_invariants
            .contains(&"no-delete".to_string()));
    }

    #[test]
    fn test_validate_allowed() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register blocking invariant for delete
        let inv = Invariant::blocking(
            "no-delete",
            Predicate::ForbiddenAction {
                pattern: "delete".to_string(),
            },
        );
        engine.register(&inv).unwrap();

        // Try allowed action
        let request = ActionRequest {
            action: "read".to_string(),
            target: Some("/some/file".to_string()),
            project_id: None,
            agent_id: None,
            context: serde_json::json!({}),
        };

        let result = engine.validate(&request).unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_gate_no_claim_without_evidence_blocks() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register Gate A
        let inv = Invariant::blocking(
            "no-smoke",
            Predicate::NoClaimWithoutEvidence {
                claim_patterns: vec!["fixed".to_string(), "arreglado".to_string()],
                required_evidence: "verification".to_string(),
            },
        );
        engine.register(&inv).unwrap();

        // Claim "fixed" without evidence - should be blocked
        let request = ActionRequest {
            action: "fixed the bug".to_string(),
            target: Some("src/main.rs".to_string()),
            project_id: None,
            agent_id: None,
            context: serde_json::json!({}),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .reason
            .as_ref()
            .map_or(false, |r| r.contains("evidence")));
    }

    #[test]
    fn test_gate_no_claim_with_evidence_allows() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register Gate A
        let inv = Invariant::blocking(
            "no-smoke",
            Predicate::NoClaimWithoutEvidence {
                claim_patterns: vec!["fixed".to_string()],
                required_evidence: "verification".to_string(),
            },
        );
        engine.register(&inv).unwrap();

        // Claim "fixed" WITH evidence - should be allowed
        let request = ActionRequest {
            action: "fixed the bug".to_string(),
            target: Some("src/main.rs".to_string()),
            project_id: None,
            agent_id: None,
            context: serde_json::json!({
                "evidence": [
                    {"kind": "verification", "passed": true, "tests_run": 12}
                ]
            }),
        };

        let result = engine.validate(&request).unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_gate_no_ghost_editing_blocks() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register Gate B
        let inv = Invariant::blocking(
            "no-ghost",
            Predicate::NoGhostEditing {
                max_age_seconds: 3600,
            },
        );
        engine.register(&inv).unwrap();

        // Try to write without reading first - should be blocked
        let request = ActionRequest {
            action: "write".to_string(),
            target: Some("src/main.rs".to_string()),
            project_id: None,
            agent_id: None,
            context: serde_json::json!({}),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .reason
            .as_ref()
            .map_or(false, |r| r.contains("ghost")));
    }

    #[test]
    fn test_gate_scope_lock_blocks() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register Gate C - only allow editing src/
        let inv = Invariant::blocking(
            "scope",
            Predicate::ScopeLock {
                allowed_paths: vec!["src/*".to_string()],
            },
        );
        engine.register(&inv).unwrap();

        // Try to write outside scope - should be blocked
        let request = ActionRequest {
            action: "write".to_string(),
            target: Some("Cargo.toml".to_string()),
            project_id: None,
            agent_id: None,
            context: serde_json::json!({
                "files_read": [{"path": "Cargo.toml", "age_seconds": 10}]
            }),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .reason
            .as_ref()
            .map_or(false, |r| r.contains("outside")));
    }

    #[test]
    fn test_gate_auto_revert_blocks_on_failure() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        // Register Gate E
        let inv = Invariant::blocking("auto-revert", Predicate::AutoRevertOnFailure);
        engine.register(&inv).unwrap();

        // Try to claim success with failed verification - should be blocked
        let request = ActionRequest {
            action: "claim_verified".to_string(),
            target: None,
            project_id: None,
            agent_id: None,
            context: serde_json::json!({
                "verification": {"passed": false, "tests_failed": 3}
            }),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .reason
            .as_ref()
            .map_or(false, |r| r.contains("failed")));
    }

    #[test]
    fn test_gate_no_autonomous_routing_blocks_worker_without_task() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        let inv = Invariant::blocking("no-autonomous-routing", Predicate::NoAutonomousRouting);
        engine.register(&inv).unwrap();

        let request = ActionRequest {
            action: "llm_messages_proxy".to_string(),
            target: None,
            project_id: None,
            agent_id: Some("messages_proxy".to_string()),
            context: serde_json::json!({
                "route": "worker",
                "has_worker_task": false
            }),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .reason
            .as_ref()
            .map_or(false, |r| r.contains("worker_task")));
    }

    #[test]
    fn test_gate_no_claim_from_worker_blocks_claims() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let engine = InvariantEngine::new(storage);

        let inv = Invariant::blocking("no-claim-from-worker", Predicate::NoClaimFromWorker);
        engine.register(&inv).unwrap();

        let request = ActionRequest {
            action: "llm_worker_result".to_string(),
            target: None,
            project_id: None,
            agent_id: Some("messages_proxy".to_string()),
            context: serde_json::json!({
                "route": "worker",
                "claims_count": 2
            }),
        };

        let result = engine.validate(&request).unwrap();
        assert!(!result.allowed);
        assert!(result
            .reason
            .as_ref()
            .map_or(false, |r| r.contains("Worker output contains")));
    }
}
