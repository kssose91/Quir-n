//! Run tracking for task execution grouping.
//!
//! A "Run" groups related events into a single task execution.
//! This allows tracking success/failure at the task level, not just individual events.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ulid::Ulid;

use crate::types::{Event, EventId, EventKind};

/// Detectar si una descripción indica un fallo
///
/// Busca múltiples patrones para ser más robusto que solo "failed/error"
fn is_failure_description(desc: &str) -> bool {
    let lower = desc.to_lowercase();
    // Patrones de fallo comunes
    lower.contains("failed")
        || lower.contains("error")
        || lower.contains("failó")
        || lower.contains("falló")
        || lower.starts_with("❌")
        || lower.contains("exception")
        || (lower.contains("test") && lower.contains("failed"))
}

/// Unique identifier for a run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub Ulid);

impl RunId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    /// Run is in progress
    InProgress,
    /// Run completed successfully (all verifications passed)
    Success,
    /// Run failed (at least one verification failed)
    Failed,
    /// Run was abandoned (no verification recorded)
    Abandoned,
}

/// A run represents a grouped task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub project_id: Option<String>,
    pub description: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    /// Events that belong to this run
    pub event_ids: Vec<EventId>,
    /// Patches applied during this run
    pub patch_count: u32,
    /// Verifications recorded
    pub verification_count: u32,
    /// Whether any verification failed
    pub has_failures: bool,
}

impl Run {
    pub fn new(description: impl Into<String>, project_id: Option<String>) -> Self {
        Self {
            id: RunId::new(),
            project_id,
            description: description.into(),
            started_at: Utc::now(),
            ended_at: None,
            status: RunStatus::InProgress,
            event_ids: vec![],
            patch_count: 0,
            verification_count: 0,
            has_failures: false,
        }
    }

    /// Add an event to this run
    pub fn add_event(&mut self, event: &Event) {
        self.event_ids.push(event.id.clone());

        match event.kind {
            EventKind::PatchApplied => self.patch_count += 1,
            EventKind::VerificationRecorded => {
                self.verification_count += 1;
                // Usar helper para detección más robusta
                if is_failure_description(&event.description) {
                    self.has_failures = true;
                }
            }
            _ => {}
        }
    }

    /// Complete the run
    pub fn complete(&mut self) {
        self.ended_at = Some(Utc::now());
        self.status = if self.has_failures {
            RunStatus::Failed
        } else if self.verification_count > 0 {
            RunStatus::Success
        } else {
            RunStatus::Abandoned
        };
    }

    /// Duration of the run
    pub fn duration(&self) -> Duration {
        let end = self.ended_at.unwrap_or_else(Utc::now);
        end - self.started_at
    }
}

/// Tracks runs and groups events
pub struct RunTracker {
    /// Runs indexed by ID
    runs: HashMap<RunId, Run>,
    /// Current active run (if any)
    current_run: Option<RunId>,
}

impl RunTracker {
    pub fn new() -> Self {
        Self {
            runs: HashMap::new(),
            current_run: None,
        }
    }

    /// Start a new run
    pub fn start_run(
        &mut self,
        description: impl Into<String>,
        project_id: Option<String>,
    ) -> RunId {
        let run = Run::new(description, project_id);
        let id = run.id;
        self.runs.insert(id, run);
        self.current_run = Some(id);
        tracing::info!(?id, "Started new run");
        id
    }

    /// Add event to current run (or create new run)
    pub fn track_event(&mut self, event: &Event) {
        // Auto-start run if none active
        if self.current_run.is_none() {
            self.start_run(&event.description, event.project_id.clone());
        }

        if let Some(ref run_id) = self.current_run {
            if let Some(run) = self.runs.get_mut(run_id) {
                run.add_event(event);
            }
        }
    }

    /// Complete current run
    pub fn complete_current_run(&mut self) -> Option<RunStatus> {
        if let Some(run_id) = self.current_run.take() {
            if let Some(run) = self.runs.get_mut(&run_id) {
                run.complete();
                tracing::info!(?run_id, status = ?run.status, "Run completed");
                return Some(run.status);
            }
        }
        None
    }

    /// Get a run by ID
    pub fn get(&self, id: &RunId) -> Option<&Run> {
        self.runs.get(id)
    }

    /// Get current run
    pub fn current(&self) -> Option<&Run> {
        self.current_run.and_then(|id| self.runs.get(&id))
    }

    /// Get all runs
    pub fn all(&self) -> Vec<&Run> {
        self.runs.values().collect()
    }

    /// Get runs by status
    pub fn by_status(&self, status: RunStatus) -> Vec<&Run> {
        self.runs.values().filter(|r| r.status == status).collect()
    }

    /// Get failed runs (for blame analysis)
    pub fn failed_runs(&self) -> Vec<&Run> {
        self.by_status(RunStatus::Failed)
    }
}

impl Default for RunTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_creation() {
        let run = Run::new("Test run", Some("proj-1".to_string()));
        assert_eq!(run.status, RunStatus::InProgress);
        assert_eq!(run.patch_count, 0);
    }

    #[test]
    fn test_run_tracker() {
        let mut tracker = RunTracker::new();
        let _id = tracker.start_run("Test", None);
        assert!(tracker.current().is_some());

        let status = tracker.complete_current_run();
        assert_eq!(status, Some(RunStatus::Abandoned)); // No verifications
    }
}
