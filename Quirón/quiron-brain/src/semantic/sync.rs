//! Sync service - Background synchronization from ledger to Qdrant (Semantic Search).

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

use super::client::SemanticClient;
use crate::error::Result;
use crate::ledger::LedgerReader;
use crate::memory::MemoryEnvelopeStore;
use crate::storage::Storage;
use crate::types::ids::EventId;
use std::sync::Arc;

const SEMANTIC_CHECKPOINT_KEY: &[u8] = b"semantic_last_processed";

/// Background service that syncs ledger events to Qdrant.
pub struct SemanticSyncService {
    storage: Storage,
    ledger_reader: LedgerReader,
    semantic_client: Arc<SemanticClient>,
}

impl SemanticSyncService {
    /// Create a new semantic sync service.
    pub fn new(storage: Storage, semantic_client: Arc<SemanticClient>) -> Self {
        Self {
            ledger_reader: LedgerReader::new(storage.clone()),
            storage,
            semantic_client,
        }
    }

    /// Run continuous sync (poll every interval_ms).
    pub async fn run_continuous(
        &self,
        interval_ms: u64,
        mut shutdown: mpsc::Receiver<()>,
    ) -> Result<()> {
        let mut ticker = interval(Duration::from_millis(interval_ms));

        tracing::info!(
            interval_ms = interval_ms,
            "Starting Semantic (Qdrant) sync service"
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match self.sync_batch(10).await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(projected = count, "Synced events to Qdrant");
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Qdrant sync error");
                        }
                    }
                }
                _ = shutdown.recv() => {
                    tracing::info!("Semantic sync service shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    fn get_checkpoint(&self) -> Result<Option<EventId>> {
        let bytes = self
            .storage
            .get(crate::storage::cf::CF_KV, SEMANTIC_CHECKPOINT_KEY)?;
        match bytes {
            Some(b) => {
                let id_str = String::from_utf8_lossy(&b).to_string();
                if let Ok(ulid) = ulid::Ulid::from_string(&id_str) {
                    Ok(Some(EventId(ulid)))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    fn set_checkpoint(&self, id: &EventId) -> Result<()> {
        self.storage.put(
            crate::storage::cf::CF_KV,
            SEMANTIC_CHECKPOINT_KEY,
            id.0.to_string().as_bytes(),
        )?;
        Ok(())
    }

    /// Sync a specific number of events (for batching).
    pub async fn sync_batch(&self, max_events: usize) -> Result<u32> {
        let checkpoint = self.get_checkpoint()?;

        let events = if let Some(ref last_id) = checkpoint {
            self.ledger_reader.events_after(last_id)?
        } else {
            // Cold start: set checkpoint to the latest event and sync only new events forward.
            // This avoids blocking the entire server during the initial bulk indexing of thousands
            // of historical events. Use `rebuild_from_scratch()` for full re-index when needed.
            let all = self.ledger_reader.all_events()?;
            if let Some(last) = all.last() {
                tracing::info!(
                    checkpoint = %last.id,
                    total_events = all.len(),
                    "Semantic sync cold start: setting checkpoint to latest event (use rebuild_neo4j for full re-index)"
                );
                self.set_checkpoint(&last.id)?;
            }
            // Return empty — we'll pick up new events on the next tick
            Vec::new()
        };

        if events.is_empty() {
            return Ok(0);
        }

        let mut count = 0;
        let mut last_event_id = None;

        for event in events.into_iter().take(max_events) {
            let envelope =
                match MemoryEnvelopeStore::new(self.storage.clone()).get_or_default(&event) {
                    Ok(envelope) => envelope,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load memory envelope for {} during semantic sync: {}",
                            event.id.0,
                            e
                        );
                        last_event_id = Some(event.id);
                        continue;
                    }
                };

            if !envelope.indexes_semantic() {
                tracing::debug!(
                    event_id = %event.id,
                    promotion_status = ?envelope.promotion_status,
                    "Skipping Qdrant sync due to memory promotion policy"
                );
                last_event_id = Some(event.id);
                continue;
            }

            if let Err(e) = self
                .semantic_client
                .upsert_event_with_envelope(&event, Some(&envelope))
                .await
            {
                tracing::warn!("Failed to upsert event {} to Qdrant: {}", event.id.0, e);
                // We continue on failure to prevent one bad event from blocking the queue
                // However, we should probably stop the checkpoint advancement if it's a structural DB error
                // For now, we continue and skip.
            } else {
                count += 1;
            }
            last_event_id = Some(event.id);

            // Sleep briefly to let HTTP embedding requests acquire the model Mutex
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if let Some(id) = last_event_id {
            self.set_checkpoint(&id)?;
        }

        Ok(count)
    }

    /// Rebuild the semantic index from the full ledger.
    pub async fn rebuild_from_scratch(&self) -> Result<u32> {
        tracing::warn!("Rebuilding Qdrant semantic index from scratch...");

        self.semantic_client.recreate_collection().await?;
        self.storage
            .delete(crate::storage::cf::CF_KV, SEMANTIC_CHECKPOINT_KEY)?;

        let events = self.ledger_reader.all_events()?;
        let total = events.len();
        let envelopes = MemoryEnvelopeStore::new(self.storage.clone());

        let mut indexed = 0_u32;
        let mut last_event_id = None;

        for (idx, event) in events.into_iter().enumerate() {
            let envelope = envelopes.get_or_default(&event)?;

            if envelope.indexes_semantic() {
                self.semantic_client
                    .upsert_event_with_envelope(&event, Some(&envelope))
                    .await?;
                indexed += 1;
            }

            last_event_id = Some(event.id);

            let processed = (idx + 1) as u32;
            if processed % 100 == 0 {
                tracing::info!(
                    processed,
                    total,
                    indexed,
                    "Rebuilding Qdrant semantic index"
                );
            }
        }

        if let Some(id) = last_event_id {
            self.set_checkpoint(&id)?;
        }

        tracing::info!(total, indexed, "Qdrant semantic rebuild complete");
        Ok(indexed)
    }
}
