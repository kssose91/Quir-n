use crate::error::Result;
use crate::ledger::{LedgerReader, LedgerWriter};
use crate::llm_distiller::LlmDistiller;
use crate::storage::Storage;
use crate::types::{Event, EventKind};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

const DISTILLATION_CHECKPOINT_KEY: &[u8] = b"distillation_last_processed";

pub struct DistillationWorker {
    storage: Storage,
    reader: LedgerReader,
    writer: Arc<Mutex<LedgerWriter>>,
    distiller: LlmDistiller,
}

impl DistillationWorker {
    pub fn new(
        storage: Storage,
        reader: LedgerReader,
        writer: Arc<Mutex<LedgerWriter>>,
        distiller: LlmDistiller,
    ) -> Self {
        Self {
            storage,
            reader,
            writer,
            distiller,
        }
    }

    pub async fn run_continuous(self, interval_secs: u64) {
        tracing::info!(
            "🧠 Asynchronous Distillation Worker started (interval: {}s)",
            interval_secs
        );

        loop {
            if let Err(e) = self.process_batch().await {
                tracing::error!("Distillation Worker error: {}", e);
            }
            sleep(Duration::from_secs(interval_secs)).await;
        }
    }

    async fn process_batch(&self) -> Result<()> {
        // 1. Get the last processed event ID from checkpoint
        let last_id_bytes = self
            .storage
            .get(crate::storage::cf::CF_KV, DISTILLATION_CHECKPOINT_KEY)?;

        let last_id = match last_id_bytes {
            Some(bytes) => {
                let id_str = String::from_utf8_lossy(&bytes).to_string();
                ulid::Ulid::from_string(&id_str).ok()
            }
            None => None, // Start from beginning if no checkpoint
        };

        // 2. Fetch events after the checkpoint
        let mut events = Vec::new();

        let fetched_events = if let Some(lid) = last_id {
            self.reader.events_after(&crate::types::ids::EventId(lid))?
        } else {
            let recent = self.reader.recent(100)?;
            recent.into_iter().rev().collect()
        };

        for event in fetched_events {
            // Ignore previously distilled events to avoid infinite loops!
            if event.tags.contains(&"distilled_insight".to_string()) {
                continue;
            }
            events.push(event);
        }

        if events.is_empty() {
            return Ok(());
        }

        tracing::info!("🔍 Distilling {} new events...", events.len());

        // 3. Send to LLM Distiller
        let insights = self.distiller.distill_events(&events).await?;

        if insights.is_empty() {
            tracing::debug!("No actionable insights found in batch.");
        } else {
            // 4. Save insights back into the Ledger as new structured Events
            let writer = self.writer.lock().await;
            for insight in insights {
                tracing::info!(
                    "✨ New Cognitive Insight: [{}] {}",
                    insight.kind,
                    insight.title
                );

                let kind = match insight.kind.to_uppercase().as_str() {
                    "CLAIM" => EventKind::ClaimMade,
                    "DECISION" => EventKind::Decision,
                    "BUG" => EventKind::Observation,
                    _ => EventKind::Observation,
                };

                let event = Event::new(kind, format!("{}: {}", insight.title, insight.description))
                    .with_tags(vec!["distilled_insight".to_string()]);

                writer.append(event)?;
            }
        }

        // 5. Update Checkpoint
        if let Some(last_event) = events.last() {
            self.storage.put(
                crate::storage::cf::CF_KV,
                DISTILLATION_CHECKPOINT_KEY,
                last_event.id.to_string().as_bytes(),
            )?;
        }

        Ok(())
    }
}
