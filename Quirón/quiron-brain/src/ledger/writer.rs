//! Event writer for the ledger with hash-chain integrity.

use crate::error::Result;
use crate::memory::MemoryEnvelopeStore;
use crate::storage::cf::*;
use crate::storage::Storage;
use crate::types::Event;

/// Key for storing the last event hash in the chain.
const CHAIN_HEAD_KEY: &[u8] = b"__chain_head__";

/// Writes events to the ledger with cryptographic hash-chain.
pub struct LedgerWriter {
    storage: Storage,
}

impl LedgerWriter {
    /// Create a new ledger writer.
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Get the hash of the last event in the chain.
    fn get_chain_head(&self) -> Result<Option<[u8; 32]>> {
        match self.storage.get(CF_CHAIN, CHAIN_HEAD_KEY)? {
            Some(bytes) if bytes.len() == 32 => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&bytes);
                Ok(Some(hash))
            }
            _ => Ok(None),
        }
    }

    /// Update the chain head to the new hash.
    fn set_chain_head(&self, hash: &[u8; 32]) -> Result<()> {
        self.storage.put(CF_CHAIN, CHAIN_HEAD_KEY, hash)
    }

    /// Calculate the hash of an event using blake3.
    fn hash_event(&self, event: &Event, prev_hash: Option<[u8; 32]>) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        // Include previous hash for chain integrity
        if let Some(prev) = prev_hash {
            hasher.update(&prev);
        }

        // Hash the event content
        hasher.update(event.id.0.to_bytes().as_slice());
        hasher.update(&event.ts.timestamp_millis().to_be_bytes());
        hasher.update(event.agent_id.as_bytes());
        hasher.update(event.description.as_bytes());

        // Include kind as discriminant
        hasher.update(&[event.kind as u8]);

        // Include project if present
        if let Some(ref project) = event.project_id {
            hasher.update(project.as_bytes());
        }

        // Include inputs/outputs
        for input in &event.inputs {
            hasher.update(input.as_bytes());
        }
        for output in &event.outputs {
            hasher.update(output.as_bytes());
        }

        // Include tags
        for tag in &event.tags {
            hasher.update(tag.as_bytes());
        }

        *hasher.finalize().as_bytes()
    }

    /// Append an event to the ledger with hash-chain.
    ///
    /// Uses Sled batch writes for each tree to ensure atomicity per tree.
    /// Write order: event data first, chain head last — on crash recovery,
    /// partial writes without chain head update are safe (event exists but
    /// chain head points to previous event; next append will re-link).
    pub fn append(&self, mut event: Event) -> Result<Event> {
        // Get the previous hash (chain head)
        let prev_hash = self.get_chain_head()?;
        event.prev_hash = prev_hash;

        // Calculate this event's hash
        let this_hash = self.hash_event(&event, prev_hash);
        event.this_hash = Some(this_hash);

        // Serialize event
        let event_bytes = bincode::serialize(&event)?;
        let event_key = event.id.to_bytes();

        // Build time index key
        let mut time_key = event.ts.timestamp_millis().to_be_bytes().to_vec();
        time_key.extend_from_slice(&event_key);

        // Step 1: Store event + time index atomically (Sled batch)
        self.storage.put(CF_EVENTS, &event_key, &event_bytes)?;
        self.storage.put(CF_EVENT_TIME, &time_key, &event_key)?;

        // Step 2: Store project index if project set
        if let Some(ref project) = event.project_id {
            let mut proj_key = project.as_bytes().to_vec();
            proj_key.push(0xFF);
            proj_key.extend_from_slice(&event.ts.timestamp_millis().to_be_bytes());
            proj_key.extend_from_slice(&event_key);
            self.storage.put(CF_EVENT_PROJECT, &proj_key, &event_key)?;
        }

        // Step 3: Update chain head LAST (if we crash before this,
        // the event exists but chain head still points to previous —
        // next append will correctly link from the old head)
        self.set_chain_head(&this_hash)?;

        // Derived semantic metadata must never become a harder source of truth
        // than the ledger event itself, so envelope persistence is best-effort.
        if let Err(e) =
            MemoryEnvelopeStore::new(self.storage.clone()).save_default_for_event(&event)
        {
            tracing::warn!("Failed to persist memory envelope for {}: {}", event.id, e);
        }

        tracing::debug!(
            "Appended event {} ({:?}) hash={}",
            event.id,
            event.kind,
            hex::encode(&this_hash[..8])
        );

        Ok(event)
    }

    /// Append multiple events in sequence, flushing once at the end.
    ///
    /// NOT truly atomic across events (event N+1 depends on event N's hash),
    /// but avoids N individual flushes. If the process crashes mid-batch,
    /// the chain remains valid up to the last fully-written event.
    pub fn append_batch(&self, events: Vec<Event>) -> Result<Vec<Event>> {
        let mut result = Vec::with_capacity(events.len());

        for event in events {
            result.push(self.append_no_flush(event)?);
        }

        // Single flush for the entire batch
        self.storage.flush()?;

        Ok(result)
    }

    /// Append without flushing (used by append_batch for single-flush optimization).
    fn append_no_flush(&self, mut event: Event) -> Result<Event> {
        let prev_hash = self.get_chain_head()?;
        event.prev_hash = prev_hash;

        let this_hash = self.hash_event(&event, prev_hash);
        event.this_hash = Some(this_hash);

        let event_bytes = bincode::serialize(&event)?;
        let event_key = event.id.to_bytes();

        let mut time_key = event.ts.timestamp_millis().to_be_bytes().to_vec();
        time_key.extend_from_slice(&event_key);

        self.storage.put(CF_EVENTS, &event_key, &event_bytes)?;
        self.storage.put(CF_EVENT_TIME, &time_key, &event_key)?;

        if let Some(ref project) = event.project_id {
            let mut proj_key = project.as_bytes().to_vec();
            proj_key.push(0xFF);
            proj_key.extend_from_slice(&event.ts.timestamp_millis().to_be_bytes());
            proj_key.extend_from_slice(&event_key);
            self.storage.put(CF_EVENT_PROJECT, &proj_key, &event_key)?;
        }

        self.set_chain_head(&this_hash)?;

        if let Err(e) =
            MemoryEnvelopeStore::new(self.storage.clone()).save_default_for_event(&event)
        {
            tracing::warn!("Failed to persist memory envelope for {}: {}", event.id, e);
        }

        tracing::debug!(
            "Appended event {} ({:?}) hash={}",
            event.id,
            event.kind,
            hex::encode(&this_hash[..8])
        );

        Ok(event)
    }

    /// Verify the integrity of the hash chain.
    pub fn verify_chain(&self) -> Result<bool> {
        let reader = crate::ledger::LedgerReader::new(self.storage.clone());
        let ordered_events = match reader.ordered_chain_events() {
            Ok(events) => events,
            Err(e) => {
                tracing::error!("Failed to reconstruct hash chain order: {}", e);
                return Ok(false);
            }
        };

        let mut expected_prev: Option<[u8; 32]> = None;

        for event in &ordered_events {
            // Check prev_hash matches expected
            if event.prev_hash != expected_prev {
                tracing::error!(
                    "Chain integrity violation at event {}: expected prev_hash {:?}, got {:?}",
                    event.id,
                    expected_prev,
                    event.prev_hash
                );
                return Ok(false);
            }

            // Verify this_hash is correct
            let computed = self.hash_event(&event, expected_prev);
            if event.this_hash != Some(computed) {
                tracing::error!(
                    "Hash mismatch at event {}: expected {:?}, got {:?}",
                    event.id,
                    Some(computed),
                    event.this_hash
                );
                return Ok(false);
            }

            expected_prev = event.this_hash;
        }

        let stored_head = self.get_chain_head()?;
        if stored_head != expected_prev {
            tracing::error!(
                "Chain head mismatch: expected {:?}, got {:?}",
                expected_prev,
                stored_head
            );
            return Ok(false);
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventKind;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    fn fixed_event(id: &str, nanos: u32, description: &str) -> Event {
        let mut event = Event::new(EventKind::Observation, description);
        event.id = crate::types::EventId(ulid::Ulid::from_string(id).unwrap());
        event.ts = Utc.timestamp_opt(1_710_000_000, nanos).unwrap();
        event
    }

    #[test]
    fn test_append_event_with_hash() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage);

        let event = Event::new(EventKind::Action, "Test action");
        let saved = writer.append(event).unwrap();

        // Should have this_hash set
        assert!(saved.this_hash.is_some());
        // First event has no prev_hash
        assert!(saved.prev_hash.is_none());
    }

    #[test]
    fn test_hash_chain_links() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage);

        let event1 = writer
            .append(Event::new(EventKind::Decision, "First"))
            .unwrap();
        let event2 = writer
            .append(Event::new(EventKind::Action, "Second"))
            .unwrap();
        let event3 = writer
            .append(Event::new(EventKind::Observation, "Third"))
            .unwrap();

        // event2.prev_hash should be event1.this_hash
        assert_eq!(event2.prev_hash, event1.this_hash);
        // event3.prev_hash should be event2.this_hash
        assert_eq!(event3.prev_hash, event2.this_hash);
    }

    #[test]
    fn test_chain_verification() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage);

        // Add small delay between events to ensure different timestamps
        writer
            .append(Event::new(EventKind::Decision, "First"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        writer
            .append(Event::new(EventKind::Action, "Second"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        writer
            .append(Event::new(EventKind::Observation, "Third"))
            .unwrap();

        // Chain should verify
        assert!(writer.verify_chain().unwrap());
    }

    #[test]
    fn test_chain_verification_handles_same_millisecond_events() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage);

        writer
            .append(fixed_event(
                "01KKCBRNX8G3P36C357E9QA4RT",
                123_100_000,
                "first",
            ))
            .unwrap();
        writer
            .append(fixed_event(
                "01KKCBRNX8PSSGBXRPSNAZPM2E",
                123_200_000,
                "second",
            ))
            .unwrap();
        writer
            .append(fixed_event(
                "01KKCBRNX80C4XNJE1N3P4X4Q4",
                123_300_000,
                "third",
            ))
            .unwrap();

        assert!(writer.verify_chain().unwrap());
    }
}
