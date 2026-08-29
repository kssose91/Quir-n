//! # Enrichment Worker — Background LLM-powered event tagging
//!
//! Processes new events through Codex to extract structured metadata
//! (project, files, symbols, tags) and writes the results to MemoryEnvelopes
//! via `revise()`. Never touches the immutable ledger.
//!
//! Architecture:
//! ```text
//! Ledger (immutable) → EnrichmentWorker → Codex → MemoryEnvelope.revise()
//! ```
//!
//! Note: The enrichment worker does NOT re-upsert to Qdrant. The semantic
//! sync service will include the enriched envelope metadata naturally when
//! it runs its next sync cycle. This avoids embedding Mutex contention.

use crate::error::Result;
use crate::ledger::LedgerReader;
use crate::llm_enricher::{EventEnrichment, LlmEnricher};
use crate::memory::MemoryEnvelopeStore;
use crate::storage::Storage;
use crate::types::Event;
use tokio::time::{sleep, Duration};

const ENRICHMENT_CHECKPOINT_KEY: &[u8] = b"enrichment_last_processed";
const ENRICHMENT_TAG: &str = "enriched_by_llm";
const BATCH_SIZE: usize = 5;

pub struct EnrichmentWorker {
    storage: Storage,
    reader: LedgerReader,
    enricher: LlmEnricher,
}

impl EnrichmentWorker {
    pub fn new(storage: Storage, reader: LedgerReader, enricher: LlmEnricher) -> Self {
        Self {
            storage,
            reader,
            enricher,
        }
    }

    pub async fn run_continuous(self, interval_secs: u64) {
        let batch_size = batch_size_from_env();
        tracing::info!(
            "🏷️  Enrichment Worker started (interval: {}s, batch: {})",
            interval_secs,
            batch_size,
        );

        // On cold start, skip to latest event (same as semantic sync)
        if self.get_checkpoint().unwrap_or(None).is_none() {
            if let Ok(all) = self.reader.all_events() {
                if let Some(last) = all.last() {
                    tracing::info!(
                        checkpoint = %last.id,
                        total_events = all.len(),
                        "Enrichment cold start: setting checkpoint to latest event"
                    );
                    let _ = self.set_checkpoint(&last.id);
                }
            }
        }

        loop {
            if let Err(e) = self.process_batch(batch_size).await {
                tracing::error!("Enrichment Worker error: {}", e);
            }
            sleep(Duration::from_secs(interval_secs)).await;
        }
    }

    async fn process_batch(&self, batch_size: usize) -> Result<()> {
        let checkpoint = self.get_checkpoint()?;

        // Get events after checkpoint
        let new_events = if let Some(ref last_id) = checkpoint {
            self.reader.events_after(last_id)?
        } else {
            Vec::new()
        };

        if new_events.is_empty() {
            return Ok(());
        }

        // Filter out already-enriched events and distillation insights
        let unenriched: Vec<&Event> = new_events
            .iter()
            .filter(|e| !e.tags.contains(&ENRICHMENT_TAG.to_string()))
            .filter(|e| !e.tags.contains(&"distilled_insight".to_string()))
            .take(batch_size)
            .collect();

        if !unenriched.is_empty() {
            tracing::info!("🏷️  Enriching {} events...", unenriched.len());

            let events_owned: Vec<Event> = unenriched.iter().map(|e| (*e).clone()).collect();
            let enrichments = self.enricher.enrich_batch(&events_owned).await?;

            let envelope_store = MemoryEnvelopeStore::new(self.storage.clone());

            for (event_id_str, enrichment) in &enrichments {
                if let Some(event) = events_owned
                    .iter()
                    .find(|e| e.id.to_string() == *event_id_str)
                {
                    match Self::apply_enrichment(&envelope_store, event, enrichment) {
                        Ok(()) => {
                            tracing::info!(
                                event_id = %event.id,
                                project = ?enrichment.project,
                                tags = ?enrichment.tags,
                                "🏷️  Enriched event"
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Failed to apply enrichment to {}: {}", event.id, e);
                        }
                    }
                }
            }

            if !enrichments.is_empty() {
                tracing::info!(
                    "🏷️  Enriched {} / {} events",
                    enrichments.len(),
                    unenriched.len()
                );
            }
        }

        // Always advance checkpoint
        if let Some(last) = new_events.iter().take(batch_size.max(1)).last() {
            self.set_checkpoint(&last.id)?;
        }

        Ok(())
    }

    fn apply_enrichment(
        store: &MemoryEnvelopeStore,
        event: &Event,
        enrichment: &EventEnrichment,
    ) -> Result<()> {
        store.revise(event, |envelope| {
            for f in &enrichment.files {
                if !envelope.file_refs.contains(f) {
                    envelope.file_refs.push(f.clone());
                }
            }
            for s in &enrichment.symbols {
                if !envelope.symbol_refs.contains(s) {
                    envelope.symbol_refs.push(s.clone());
                }
            }
            for t in &enrichment.tags {
                let tag = format!("llm:{}", t);
                if !envelope.logic_tags.contains(&tag) {
                    envelope.logic_tags.push(tag);
                }
            }
            if envelope.module_id.is_none() {
                envelope.module_id = enrichment.module_path.clone();
            }
        })?;
        Ok(())
    }

    fn get_checkpoint(&self) -> Result<Option<crate::types::ids::EventId>> {
        let bytes = self
            .storage
            .get(crate::storage::cf::CF_KV, ENRICHMENT_CHECKPOINT_KEY)?;
        match bytes {
            Some(b) => {
                let id_str = String::from_utf8_lossy(&b).to_string();
                if let Ok(ulid) = ulid::Ulid::from_string(&id_str) {
                    Ok(Some(crate::types::ids::EventId(ulid)))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    fn set_checkpoint(&self, id: &crate::types::ids::EventId) -> Result<()> {
        self.storage.put(
            crate::storage::cf::CF_KV,
            ENRICHMENT_CHECKPOINT_KEY,
            id.0.to_string().as_bytes(),
        )?;
        Ok(())
    }
}

fn batch_size_from_env() -> usize {
    std::env::var("QUIRON_ENRICHMENT_BATCH_SIZE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|size| *size > 0)
        .unwrap_or(BATCH_SIZE)
}
