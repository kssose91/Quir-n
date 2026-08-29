//! Sync service - Background synchronization from ledger to Neo4j.

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

use super::schema::Schema;
use super::{Neo4jConnector, Projector};
use crate::error::Result;
use crate::ledger::LedgerReader;
use crate::storage::Storage;

/// Background service that syncs ledger events to Neo4j.
pub struct SyncService {
    projector: Projector,
    ledger_reader: LedgerReader,
}

impl SyncService {
    /// Create a new sync service.
    pub fn new(storage: Storage, neo4j: Neo4jConnector) -> Self {
        Self {
            projector: Projector::new(storage.clone(), neo4j),
            ledger_reader: LedgerReader::new(storage),
        }
    }

    /// Run continuous sync (poll every interval_ms).
    pub async fn run_continuous(
        &self,
        interval_ms: u64,
        mut shutdown: mpsc::Receiver<()>,
    ) -> Result<()> {
        let mut ticker = interval(Duration::from_millis(interval_ms));

        tracing::info!(interval_ms = interval_ms, "Starting Neo4j sync service");

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match self.sync_pending().await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::debug!(projected = count, "Synced events to Neo4j");
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Neo4j sync error");
                        }
                    }
                }
                _ = shutdown.recv() => {
                    tracing::info!("Neo4j sync service shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Sync all pending events since last checkpoint.
    pub async fn sync_pending(&self) -> Result<u32> {
        let checkpoint = self.projector.get_checkpoint()?;

        // Get events after checkpoint
        let events = if let Some(ref last_id) = checkpoint {
            self.ledger_reader.events_after(last_id)?
        } else {
            self.ledger_reader.all_events()?
        };

        let mut count = 0;
        for event in events {
            self.projector.project_event(&event).await?;
            count += 1;
        }

        Ok(count)
    }

    /// Sync a specific number of events (for batching).
    pub async fn sync_batch(&self, max_events: usize) -> Result<u32> {
        let checkpoint = self.projector.get_checkpoint()?;

        let events = if let Some(ref last_id) = checkpoint {
            self.ledger_reader.events_after(last_id)?
        } else {
            self.ledger_reader.all_events()?
        };

        let mut count = 0;
        for event in events.into_iter().take(max_events) {
            self.projector.project_event(&event).await?;
            count += 1;
        }

        Ok(count)
    }

    /// Rebuild the entire Neo4j graph from scratch.
    pub async fn rebuild_from_scratch(&self) -> Result<u32> {
        tracing::warn!("Rebuilding Neo4j graph from scratch...");

        // 1. Clear all Neo4j data
        Schema::clear_all(self.projector.neo4j()).await?;

        // 2. Re-apply schema
        Schema::ensure(self.projector.neo4j()).await?;

        // 3. Clear checkpoint
        self.projector.clear_checkpoint()?;

        // 4. Replay all events
        let events = self.ledger_reader.all_events()?;
        let total = events.len();

        let mut count = 0;
        for event in events {
            self.projector.project_event(&event).await?;
            count += 1;

            if count % 100 == 0 {
                tracing::info!(progress = count, total = total, "Rebuilding...");
            }
        }

        tracing::info!(total = count, "Neo4j rebuild complete");
        Ok(count)
    }

    /// Get sync status.
    pub async fn status(&self) -> Result<SyncStatus> {
        let checkpoint = self.projector.get_checkpoint()?;
        let pending = if let Some(ref last_id) = checkpoint {
            self.ledger_reader.events_after(last_id)?.len() as u32
        } else {
            self.ledger_reader.all_events()?.len() as u32
        };

        let healthy = self.projector.neo4j().health_check().await?;
        let node_counts = Schema::node_counts(self.projector.neo4j()).await?;

        Ok(SyncStatus {
            healthy,
            last_checkpoint: checkpoint.map(|id| id.0.to_string()),
            pending_events: pending,
            node_counts,
        })
    }
}

/// Status of the sync service.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncStatus {
    /// Whether Neo4j is reachable.
    pub healthy: bool,
    /// Last projected event ID.
    pub last_checkpoint: Option<String>,
    /// Number of events not yet projected.
    pub pending_events: u32,
    /// Node counts in Neo4j.
    pub node_counts: String,
}
