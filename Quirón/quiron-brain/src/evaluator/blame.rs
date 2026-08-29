//! Blame graph for tracing failures to their root cause.
//!
//! When a verification fails, we trace backwards through:
//! Verification → Patch → FileRead → Decision
//! to find the causal chain that led to failure.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::ledger::LedgerReader;
use crate::types::{EventId, EventKind};

/// Ventana de tiempo para análisis de blame (en horas)
/// TODO: Mover a configuración cuando se refactorice
const BLAME_ANALYSIS_WINDOW_HOURS: i64 = 1;

/// Detectar si una descripción indica un fallo
fn is_failure_description(desc: &str) -> bool {
    let lower = desc.to_lowercase();
    lower.contains("failed")
        || lower.contains("error")
        || lower.contains("falló")
        || lower.starts_with("❌")
        || lower.contains("exception")
}

/// Result of blame analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameResult {
    /// The failed verification that triggered analysis
    pub failed_verification: EventId,
    /// Chain of events leading to failure (oldest first)
    pub causal_chain: Vec<BlameNode>,
    /// The root cause event (first in chain)
    pub root_cause: Option<EventId>,
    /// Summary of the failure
    pub summary: String,
}

/// A node in the blame chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameNode {
    pub event_id: EventId,
    pub kind: EventKind,
    pub description: String,
    pub ts: DateTime<Utc>,
    /// Blame weight (1.0 = fully responsible)
    pub weight: f32,
}

/// Engine for computing blame graphs
pub struct BlameGraph {
    reader: LedgerReader,
}

impl BlameGraph {
    pub fn new(reader: LedgerReader) -> Self {
        Self { reader }
    }

    /// Analyze a failed verification to find root cause
    pub fn analyze(&self, verification_id: &EventId) -> Result<BlameResult> {
        let verification = self
            .reader
            .get(verification_id)?
            .ok_or_else(|| anyhow::anyhow!("Verification not found"))?;

        // Verify this is actually a failed verification
        if !is_failure_description(&verification.description) {
            return Err(anyhow::anyhow!("Not a failed verification").into());
        }

        // Ventana de análisis configurable
        let window = Duration::hours(BLAME_ANALYSIS_WINDOW_HOURS);
        let cutoff = verification.ts - window;

        let mut causal_chain = Vec::new();

        // Get recent events in time order
        let recent = self.reader.recent(100)?;

        // Filter to events before the verification
        let relevant: Vec<_> = recent
            .into_iter()
            .filter(|e| e.ts >= cutoff && e.ts <= verification.ts)
            .collect();

        // Build causal chain
        for event in relevant.iter().rev() {
            // oldest first
            match event.kind {
                EventKind::PatchApplied | EventKind::PatchProposed => {
                    causal_chain.push(BlameNode {
                        event_id: event.id.clone(),
                        kind: event.kind,
                        description: event.description.clone(),
                        ts: event.ts,
                        weight: 0.7, // Patches are high blame
                    });
                }
                EventKind::FileRead => {
                    causal_chain.push(BlameNode {
                        event_id: event.id.clone(),
                        kind: event.kind,
                        description: event.description.clone(),
                        ts: event.ts,
                        weight: 0.2, // File reads are low blame
                    });
                }
                EventKind::Decision => {
                    causal_chain.push(BlameNode {
                        event_id: event.id.clone(),
                        kind: event.kind,
                        description: event.description.clone(),
                        ts: event.ts,
                        weight: 0.5, // Decisions are medium blame
                    });
                }
                _ => {}
            }
        }

        // Normalize weights
        let total_weight: f32 = causal_chain.iter().map(|n| n.weight).sum();
        if total_weight > 0.0 {
            for node in &mut causal_chain {
                node.weight /= total_weight;
            }
        }

        // Root cause is the first decision or patch in chain
        let root_cause = causal_chain
            .iter()
            .find(|n| n.kind == EventKind::Decision || n.kind == EventKind::PatchApplied)
            .map(|n| n.event_id.clone());

        let summary = if let Some(ref root) = root_cause {
            format!(
                "Failure traced to {} events, root cause: {}",
                causal_chain.len(),
                causal_chain
                    .iter()
                    .find(|n| &n.event_id == root)
                    .map(|n| n.description.chars().take(50).collect::<String>())
                    .unwrap_or_default()
            )
        } else {
            format!(
                "Failure with {} related events in causal window",
                causal_chain.len()
            )
        };

        Ok(BlameResult {
            failed_verification: verification_id.clone(),
            causal_chain,
            root_cause,
            summary,
        })
    }

    /// Find the most blamed patches (by frequency in failures)
    pub fn most_blamed_patches(&self, limit: usize) -> Result<Vec<(EventId, u32)>> {
        let recent = self.reader.recent(500)?;

        // Find all failed verifications
        let failures: Vec<_> = recent
            .iter()
            .filter(|e| {
                e.kind == EventKind::VerificationRecorded && is_failure_description(&e.description)
            })
            .collect();

        // Count patches that appear in blame chains
        use std::collections::HashMap;
        let mut patch_blame_count: HashMap<EventId, u32> = HashMap::new();

        for failure in failures {
            if let Ok(result) = self.analyze(&failure.id) {
                for node in &result.causal_chain {
                    if node.kind == EventKind::PatchApplied {
                        *patch_blame_count.entry(node.event_id.clone()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Sort by count
        let mut sorted: Vec<_> = patch_blame_count.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(limit);

        Ok(sorted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blame_node_creation() {
        let node = BlameNode {
            event_id: EventId(ulid::Ulid::new()),
            kind: EventKind::PatchApplied,
            description: "Test patch".to_string(),
            ts: Utc::now(),
            weight: 0.5,
        };
        assert!(node.weight >= 0.0 && node.weight <= 1.0);
    }
}
