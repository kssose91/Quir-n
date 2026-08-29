//! Event reader for the ledger (Sled version).

use crate::error::{BrainError, Result};
use crate::storage::cf::*;
use crate::storage::keys::*;
use crate::storage::Storage;
use crate::types::{Event, EventId};
use std::collections::HashMap;

/// Reads events from the ledger.
pub struct LedgerReader {
    storage: Storage,
}

impl LedgerReader {
    /// Create a new ledger reader.
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Get an event by ID.
    pub fn get(&self, id: &EventId) -> Result<Option<Event>> {
        let key = encode_event_key(id);
        match self.storage.get(CF_EVENTS, &key)? {
            Some(bytes) => {
                // Use tolerant deserialization for backward compatibility
                match crate::types::event::deserialize_event(&bytes) {
                    Ok(event) => Ok(Some(event)),
                    Err(e) => Err(anyhow::anyhow!("Failed to deserialize event: {}", e).into()),
                }
            }
            None => Ok(None),
        }
    }

    /// Get an event by ID, returning an error if not found.
    pub fn get_or_error(&self, id: &EventId) -> Result<Event> {
        self.get(id)?
            .ok_or_else(|| BrainError::EventNotFound(id.to_string()))
    }

    /// Get recent events (newest first).
    ///
    /// Uses the time index (CF_EVENT_TIME) to iterate in reverse chronological
    /// order and only deserializes the `limit` events needed — O(limit), not O(N).
    pub fn recent(&self, limit: usize) -> Result<Vec<Event>> {
        let mut events = Vec::with_capacity(limit);

        // Time index keys are: ts_ms(BE) | event_id — reverse iter = newest first
        for (_, event_key) in self.storage.iter_tree_rev(CF_EVENT_TIME)? {
            if events.len() >= limit {
                break;
            }
            match self.storage.get(CF_EVENTS, &event_key)? {
                Some(value) => match crate::types::event::deserialize_event(&value) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize event: {}", e);
                        continue;
                    }
                },
                None => continue,
            }
        }

        Ok(events)
    }

    /// Get the oldest event in the ledger, if any.
    ///
    /// Uses the time index in chronological order and only deserializes the
    /// first readable event encountered.
    pub fn oldest(&self) -> Result<Option<Event>> {
        for (_, event_key) in self.storage.iter_tree(CF_EVENT_TIME)? {
            match self.storage.get(CF_EVENTS, &event_key)? {
                Some(value) => match crate::types::event::deserialize_event(&value) {
                    Ok(event) => return Ok(Some(event)),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize event (oldest): {}", e);
                        continue;
                    }
                },
                None => continue,
            }
        }

        Ok(None)
    }

    /// Get events for a specific project.
    ///
    /// Uses the project index (CF_EVENT_PROJECT) with prefix scan.
    /// Keys are: project|0xFF|ts_ms(BE)|event_id — reverse gives newest first.
    pub fn by_project(&self, project: &str, limit: usize) -> Result<Vec<Event>> {
        // Build prefix: project bytes + 0xFF separator
        let mut prefix = project.as_bytes().to_vec();
        prefix.push(0xFF);

        let mut events = Vec::new();

        // Cargar TODOS los eventos del proyecto (no break prematuro)
        for (_, event_key) in self.storage.prefix_iter(CF_EVENT_PROJECT, &prefix)? {
            if let Some(bytes) = self.storage.get(CF_EVENTS, &event_key)? {
                match crate::types::event::deserialize_event(&bytes) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize event: {}", e);
                        continue;
                    }
                }
            }
        }

        // PRIMERO ordenar por timestamp descending
        events.sort_by(|a, b| b.ts.cmp(&a.ts));
        // LUEGO truncar al limit
        events.truncate(limit);

        Ok(events)
    }

    /// Get events in a time range (inclusive).
    ///
    /// Uses the time index (CF_EVENT_TIME) with range scan — O(range_size), not O(N).
    /// Time index keys are: ts_ms(BE) | event_id.
    pub fn in_range(&self, from_ms: i64, to_ms: i64, limit: usize) -> Result<Vec<Event>> {
        let mut events = Vec::with_capacity(limit);

        // Build key range: [from_ms..to_ms+1) (BE bytes ensure lexicographic = chronological)
        let from_key = from_ms.to_be_bytes().to_vec();
        let to_key = (to_ms + 1).to_be_bytes().to_vec();

        for (_, event_key) in self.storage.range_iter(CF_EVENT_TIME, &from_key, &to_key)? {
            if events.len() >= limit {
                break;
            }
            match self.storage.get(CF_EVENTS, &event_key)? {
                Some(value) => match crate::types::event::deserialize_event(&value) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize event (in_range): {}", e);
                        continue;
                    }
                },
                None => continue,
            }
        }

        // Already in chronological order from the time index
        Ok(events)
    }

    /// Count total events.
    pub fn count(&self) -> Result<u64> {
        self.storage.count(CF_EVENTS)
    }

    /// Get all events sorted by timestamp (oldest first).
    pub fn all(&self) -> Result<Vec<Event>> {
        let mut events = Vec::new();

        // Use time index for guaranteed chronological order (no post-sort needed)
        for (_, event_key) in self.storage.iter_tree(CF_EVENT_TIME)? {
            match self.storage.get(CF_EVENTS, &event_key)? {
                Some(value) => match crate::types::event::deserialize_event(&value) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize event (all): {}", e);
                        continue;
                    }
                },
                None => continue,
            }
        }

        Ok(events)
    }

    /// Alias for `all()` for clarity in sync contexts.
    pub fn all_events(&self) -> Result<Vec<Event>> {
        self.all()
    }

    /// Reconstruct the append order from the hash chain itself.
    ///
    /// The time index only keeps millisecond precision, so events written in the
    /// same millisecond can appear in a different order than they were appended.
    pub(crate) fn ordered_chain_events(&self) -> Result<Vec<Event>> {
        let mut events_by_hash: HashMap<[u8; 32], Event> = HashMap::new();
        let mut next_by_prev: HashMap<Option<[u8; 32]>, [u8; 32]> = HashMap::new();

        for (_, value) in self.storage.iter_tree(CF_EVENTS)? {
            let event: Event = match crate::types::event::deserialize_event(&value) {
                Ok(event) => event,
                Err(e) => {
                    tracing::warn!("Failed to deserialize event (ordered_chain_events): {}", e);
                    continue;
                }
            };

            let this_hash = event.this_hash.ok_or_else(|| {
                BrainError::Internal(anyhow::anyhow!(
                    "Event {} is missing this_hash; cannot reconstruct append order",
                    event.id
                ))
            })?;

            if let Some(existing) = events_by_hash.insert(this_hash, event.clone()) {
                return Err(BrainError::Internal(anyhow::anyhow!(
                    "Duplicate this_hash detected for events {} and {}",
                    existing.id,
                    event.id
                )));
            }

            if let Some(existing_next) = next_by_prev.insert(event.prev_hash, this_hash) {
                let existing = events_by_hash.get(&existing_next).ok_or_else(|| {
                    BrainError::Internal(anyhow::anyhow!(
                        "Broken chain index while linking event {}",
                        event.id
                    ))
                })?;
                return Err(BrainError::Internal(anyhow::anyhow!(
                    "Branching hash chain detected: events {} and {} share prev_hash {:?}",
                    existing.id,
                    event.id,
                    event.prev_hash
                )));
            }
        }

        if events_by_hash.is_empty() {
            return Ok(Vec::new());
        }

        let mut ordered = Vec::with_capacity(events_by_hash.len());
        let mut expected_prev: Option<[u8; 32]> = None;

        loop {
            let Some(next_hash) = next_by_prev.get(&expected_prev).copied() else {
                break;
            };
            let event = events_by_hash.get(&next_hash).ok_or_else(|| {
                BrainError::Internal(anyhow::anyhow!(
                    "Hash chain references missing event hash {}",
                    hex::encode(next_hash)
                ))
            })?;
            ordered.push(event.clone());
            expected_prev = Some(next_hash);
        }

        if ordered.len() != events_by_hash.len() {
            return Err(BrainError::Internal(anyhow::anyhow!(
                "Hash chain is disconnected: reconstructed {} of {} events",
                ordered.len(),
                events_by_hash.len()
            )));
        }

        Ok(ordered)
    }

    /// Get all events after a given event ID (for incremental sync).
    ///
    /// Follows append order from the hash chain so same-millisecond events are not skipped.
    pub fn events_after(&self, last_id: &EventId) -> Result<Vec<Event>> {
        let ordered = self.ordered_chain_events()?;

        let Some(position) = ordered.iter().position(|event| event.id == *last_id) else {
            tracing::warn!("events_after: reference event {} not found", last_id);
            return Ok(vec![]);
        };

        Ok(ordered.into_iter().skip(position + 1).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::LedgerWriter;
    use crate::types::EventKind;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    fn fixed_event(id: &str, nanos: u32, description: &str) -> Event {
        let mut event = Event::new(EventKind::Observation, description);
        event.id = EventId(ulid::Ulid::from_string(id).unwrap());
        event.ts = Utc.timestamp_opt(1_710_000_000, nanos).unwrap();
        event
    }

    #[test]
    fn test_write_and_read() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let reader = LedgerReader::new(storage);

        // Write event
        let event = Event::new(EventKind::Decision, "Test decision").with_project("test-project");
        let saved = writer.append(event).unwrap();

        // Read back
        let loaded = reader.get(&saved.id).unwrap().unwrap();
        assert_eq!(loaded.description, "Test decision");
        assert_eq!(loaded.project_id, Some("test-project".to_string()));
    }

    #[test]
    fn test_by_project() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let reader = LedgerReader::new(storage);

        // Write events for different projects
        writer
            .append(Event::new(EventKind::Action, "A1").with_project("proj-a"))
            .unwrap();
        writer
            .append(Event::new(EventKind::Action, "B1").with_project("proj-b"))
            .unwrap();
        writer
            .append(Event::new(EventKind::Action, "A2").with_project("proj-a"))
            .unwrap();

        // Query by project
        let proj_a = reader.by_project("proj-a", 10).unwrap();
        assert_eq!(proj_a.len(), 2);

        let proj_b = reader.by_project("proj-b", 10).unwrap();
        assert_eq!(proj_b.len(), 1);
    }

    #[test]
    fn test_events_after_preserves_append_order_with_same_millisecond_events() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let reader = LedgerReader::new(storage);

        let first = writer
            .append(fixed_event(
                "01KKCBRNX8G3P36C357E9QA4RT",
                123_100_000,
                "first",
            ))
            .unwrap();
        let second = writer
            .append(fixed_event(
                "01KKCBRNX8PSSGBXRPSNAZPM2E",
                123_200_000,
                "second",
            ))
            .unwrap();
        let third = writer
            .append(fixed_event(
                "01KKCBRNX80C4XNJE1N3P4X4Q4",
                123_300_000,
                "third",
            ))
            .unwrap();

        let after_first = reader.events_after(&first.id).unwrap();
        let ids: Vec<String> = after_first
            .into_iter()
            .map(|event| event.id.to_string())
            .collect();

        assert_eq!(ids, vec![second.id.to_string(), third.id.to_string()]);
    }

    #[test]
    fn test_oldest() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let reader = LedgerReader::new(storage);

        writer
            .append(Event::new(EventKind::Action, "first"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        writer
            .append(Event::new(EventKind::Action, "second"))
            .unwrap();

        let oldest = reader.oldest().unwrap().expect("oldest should exist");
        assert_eq!(oldest.description, "first");
    }
}
