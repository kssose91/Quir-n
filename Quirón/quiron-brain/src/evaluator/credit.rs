//! Credit assignment for successful verifications.
//!
//! When a verification passes, we assign credit to patches and decisions
//! that contributed to success. This allows learning which approaches work.

use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;
use crate::ledger::LedgerReader;
use crate::types::{EventId, EventKind};

/// Result of credit assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditResult {
    /// The successful verification that triggered analysis
    pub verification: EventId,
    /// Events that receive credit
    pub credited: Vec<CreditNode>,
    /// Total credit distributed (should sum to 1.0)
    pub total_credit: f32,
}

/// An event that receives credit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditNode {
    pub event_id: EventId,
    pub kind: EventKind,
    pub description: String,
    /// Credit amount (0.0 to 1.0)
    pub credit: f32,
}

/// Engine for assigning credit to contributing events
pub struct CreditAssigner {
    reader: LedgerReader,
}

impl CreditAssigner {
    pub fn new(reader: LedgerReader) -> Self {
        Self { reader }
    }

    /// Assign credit for a successful verification
    pub fn assign(&self, verification_id: &EventId) -> Result<CreditResult> {
        let verification = self
            .reader
            .get(verification_id)?
            .ok_or_else(|| anyhow::anyhow!("Verification not found"))?;

        // Verify this is a passed verification
        let desc_lower = verification.description.to_lowercase();
        if desc_lower.contains("failed") || desc_lower.contains("error") {
            return Err(anyhow::anyhow!("Not a successful verification").into());
        }

        // Window for credit: patches applied in last 30 minutes
        let window = Duration::minutes(30);
        let cutoff = verification.ts - window;

        let recent = self.reader.recent(50)?;

        // Find contributing events
        let mut credited = Vec::new();

        for event in recent.iter() {
            if event.ts < cutoff || event.ts > verification.ts {
                continue;
            }

            match event.kind {
                EventKind::PatchApplied => {
                    credited.push(CreditNode {
                        event_id: event.id.clone(),
                        kind: event.kind,
                        description: event.description.clone(),
                        credit: 0.6, // Patches get most credit
                    });
                }
                EventKind::Decision => {
                    credited.push(CreditNode {
                        event_id: event.id.clone(),
                        kind: event.kind,
                        description: event.description.clone(),
                        credit: 0.3, // Decisions get some credit
                    });
                }
                EventKind::FileRead => {
                    credited.push(CreditNode {
                        event_id: event.id.clone(),
                        kind: event.kind,
                        description: event.description.clone(),
                        credit: 0.1, // Reads get minimal credit
                    });
                }
                _ => {}
            }
        }

        // Normalize credits to sum to 1.0
        let total: f32 = credited.iter().map(|n| n.credit).sum();
        if total > 0.0 {
            for node in &mut credited {
                node.credit /= total;
            }
        }

        let total_credit = credited.iter().map(|n| n.credit).sum();

        Ok(CreditResult {
            verification: verification_id.clone(),
            credited,
            total_credit,
        })
    }

    /// Get lifetime credit scores for all events
    pub fn cumulative_scores(&self) -> Result<HashMap<EventId, f32>> {
        let mut scores: HashMap<EventId, f32> = HashMap::new();

        let all_events = self.reader.recent(1000)?;

        // Find all successful verifications
        let successes: Vec<_> = all_events
            .iter()
            .filter(|e| {
                e.kind == EventKind::VerificationRecorded
                    && !e.description.to_lowercase().contains("failed")
                    && !e.description.to_lowercase().contains("error")
            })
            .collect();

        // Assign credit from each success
        for success in successes {
            if let Ok(result) = self.assign(&success.id) {
                for node in &result.credited {
                    *scores.entry(node.event_id.clone()).or_insert(0.0) += node.credit;
                }
            }
        }

        Ok(scores)
    }

    /// Get top credited events
    pub fn top_contributors(&self, limit: usize) -> Result<Vec<(EventId, f32)>> {
        let scores = self.cumulative_scores()?;

        let mut sorted: Vec<_> = scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(limit);

        Ok(sorted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credit_node_creation() {
        let node = CreditNode {
            event_id: EventId(ulid::Ulid::new()),
            kind: EventKind::PatchApplied,
            description: "Test patch".to_string(),
            credit: 0.5,
        };
        assert!(node.credit >= 0.0);
    }
}
