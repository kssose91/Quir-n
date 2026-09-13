//! Durable event transactions and a versioned, full-content hash chain.
use crate::error::{BrainError, Result};
use crate::memory::MemoryEnvelopeStore;
use crate::storage::cf::*;
use crate::storage::Storage;
use crate::types::Event;
use serde::Serialize;
use sled::transaction::{ConflictableTransactionError as TxError, Transactional};

const CHAIN_HEAD_KEY: &[u8] = b"__chain_head__";
const LEGACY_ANCHOR_KEY: &[u8] = b"__legacy_anchor_v2__";

#[derive(Debug, Serialize)]
pub struct ChainVerification {
    pub valid: bool,
    pub event_count: u64,
    pub legacy_events: u64,
    pub v2_events: u64,
    pub legacy_snapshot_protected: bool,
    pub message: String,
}

pub struct LedgerWriter { storage: Storage }

fn version_key(event: &Event) -> Vec<u8> {
    let mut key = b"event-hash-version:".to_vec();
    key.extend_from_slice(&event.id.to_bytes());
    key
}

fn digest(bytes: Option<Vec<u8>>) -> Result<Option<[u8; 32]>> {
    bytes.map(|b| b.try_into().map_err(|_| anyhow::anyhow!("Invalid chain digest length").into())).transpose()
}

/// Domain separation and bincode's length prefixes cover every Event field.
/// Event's on-disk layout is unchanged; this_hash is excluded from its own hash.
fn hash_v2(event: &Event, anchor: &[u8; 32]) -> Result<[u8; 32]> {
    let mut canonical = event.clone();
    canonical.this_hash = None;
    let mut h = blake3::Hasher::new();
    h.update(b"quiron:ledger:event:v2\0");
    h.update(anchor);
    h.update(&bincode::serialize(&canonical)?);
    Ok(*h.finalize().as_bytes())
}

/// Binds the full legacy content observed at transition, without rewriting it.
/// This cannot attest to changes that happened before that transition.
fn legacy_anchor(events: &[Event]) -> Result<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    h.update(b"quiron:ledger:legacy-snapshot:v2\0");
    h.update(&(events.len() as u64).to_be_bytes());
    for event in events {
        let bytes = bincode::serialize(event)?;
        h.update(&(bytes.len() as u64).to_be_bytes());
        h.update(&bytes);
    }
    Ok(*h.finalize().as_bytes())
}

impl LedgerWriter {
    pub fn new(storage: Storage) -> Self { Self { storage } }

    /// Historical algorithm: retained solely to read and validate legacy data.
    fn hash_event(&self, event: &Event, prev_hash: Option<[u8; 32]>) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        if let Some(prev) = prev_hash { h.update(&prev); }
        h.update(event.id.0.to_bytes().as_slice());
        h.update(&event.ts.timestamp_millis().to_be_bytes());
        h.update(event.agent_id.as_bytes());
        h.update(event.description.as_bytes());
        h.update(&[event.kind as u8]);
        if let Some(project) = &event.project_id { h.update(project.as_bytes()); }
        for value in event.inputs.iter().chain(&event.outputs).chain(&event.tags) { h.update(value.as_bytes()); }
        *h.finalize().as_bytes()
    }

    /// Success means the multi-tree transaction has been flushed to storage.
    /// An I/O error after commit is an uncertain outcome; a duplicate ID is
    /// rejected on retry instead of overwriting history.
    pub fn append(&self, event: Event) -> Result<Event> {
        Ok(self.append_batch(vec![event])?.remove(0))
    }

    /// Entire batch, indexes, versions, anchor and head commit atomically.
    pub fn append_batch(&self, events: Vec<Event>) -> Result<Vec<Event>> {
        if events.is_empty() { return Ok(Vec::new()); }
        let _guard = self.storage.lock_ledger()?;
        let anchor = match digest(self.storage.get(CF_CHAIN, LEGACY_ANCHOR_KEY)?)? {
            Some(anchor) => anchor,
            None => {
                let report = self.verify_locked()?;
                if !report.valid { return Err(anyhow::anyhow!("Cannot extend invalid ledger: {}", report.message).into()); }
                legacy_anchor(&crate::ledger::LedgerReader::new(self.storage.clone()).ordered_chain_events()?)?
            }
        };
        let tree = |name| self.storage.tree(name).ok_or_else(|| BrainError::Internal(anyhow::anyhow!("Missing tree {name}")));
        let committed = (tree(CF_EVENTS)?, tree(CF_EVENT_TIME)?, tree(CF_EVENT_PROJECT)?, tree(CF_CHAIN)?)
            .transaction(|(data, time, projects, chain)| {
                let mut prev = digest(chain.get(CHAIN_HEAD_KEY)?.map(|v| v.to_vec())).map_err(TxError::Abort)?;
                if let Some(stored) = chain.get(LEGACY_ANCHOR_KEY)? {
                    if stored.as_ref() != anchor.as_slice() {
                        return Err(TxError::Abort(BrainError::Internal(anyhow::anyhow!("Legacy anchor changed"))));
                    }
                }
                let mut saved = Vec::with_capacity(events.len());
                for original in &events {
                    let mut event = original.clone();
                    let key = event.id.to_bytes();
                    if data.get(key)?.is_some() {
                        return Err(TxError::Abort(BrainError::InvalidId(format!("Duplicate ledger event {}", event.id))));
                    }
                    event.prev_hash = prev;
                    let hash = hash_v2(&event, &anchor).map_err(TxError::Abort)?;
                    event.this_hash = Some(hash);
                    let encoded = bincode::serialize(&event).map_err(BrainError::from).map_err(TxError::Abort)?;
                    let mut time_key = event.ts.timestamp_millis().to_be_bytes().to_vec();
                    time_key.extend_from_slice(&key);
                    data.insert(key.as_slice(), encoded)?;
                    time.insert(time_key, key.as_slice())?;
                    if let Some(project) = &event.project_id {
                        let mut project_key = project.as_bytes().to_vec();
                        project_key.push(0xff);
                        project_key.extend_from_slice(&event.ts.timestamp_millis().to_be_bytes());
                        project_key.extend_from_slice(&key);
                        projects.insert(project_key, key.as_slice())?;
                    }
                    chain.insert(version_key(&event), vec![2])?;
                    prev = Some(hash);
                    saved.push(event);
                }
                chain.insert(LEGACY_ANCHOR_KEY, anchor.as_slice())?;
                chain.insert(CHAIN_HEAD_KEY, prev.as_ref().unwrap().as_slice())?;
                chain.flush(); // sled flushes the committed transaction before returning.
                Ok(saved)
            }).map_err(|e| BrainError::Internal(anyhow::anyhow!("Ledger transaction failed: {e}")))?;
        // Derived metadata is best-effort; the authoritative event is durable.
        for event in &committed {
            if let Err(e) = MemoryEnvelopeStore::new(self.storage.clone()).save_default_for_event(event) {
                tracing::warn!("Failed to persist memory envelope for {}: {}", event.id, e);
            }
        }
        Ok(committed)
    }

    pub fn verify_chain(&self) -> Result<bool> { Ok(self.verify_chain_detailed()?.valid) }

    pub fn verify_chain_detailed(&self) -> Result<ChainVerification> {
        let _guard = self.storage.lock_ledger()?;
        self.verify_locked()
    }

    fn verify_locked(&self) -> Result<ChainVerification> {
        let mut report = ChainVerification { valid: false, event_count: self.storage.count(CF_EVENTS)?,
            legacy_events: 0, v2_events: 0, legacy_snapshot_protected: false, message: String::new() };
        let ordered = match crate::ledger::LedgerReader::new(self.storage.clone()).ordered_chain_events() {
            Ok(events) => events,
            Err(e) => { report.message = e.to_string(); return Ok(report); }
        };
        let anchor = digest(self.storage.get(CF_CHAIN, LEGACY_ANCHOR_KEY)?)?;
        let mut previous = None;
        let mut legacy = Vec::new();
        for event in &ordered {
            let version = self.storage.get(CF_CHAIN, &version_key(event))?;
            let computed = match version.as_deref() {
                None if report.v2_events == 0 => {
                    report.legacy_events += 1;
                    legacy.push(event.clone());
                    self.hash_event(event, previous)
                }
                Some([2]) if anchor.is_some() => {
                    report.v2_events += 1;
                    hash_v2(event, &anchor.unwrap())?
                }
                _ => { report.message = format!("Invalid hash version or missing anchor at {}", event.id); return Ok(report); }
            };
            if event.prev_hash != previous || event.this_hash != Some(computed) {
                report.message = format!("Hash mismatch at {}", event.id); return Ok(report);
            }
            previous = event.this_hash;
        }
        if let Some(anchor) = anchor {
            if report.v2_events == 0 || anchor != legacy_anchor(&legacy)? {
                report.message = "Legacy snapshot mismatch".into(); return Ok(report);
            }
            report.legacy_snapshot_protected = !legacy.is_empty();
        }
        if digest(self.storage.get(CF_CHAIN, CHAIN_HEAD_KEY)?)? != previous {
            report.message = "Chain head mismatch".into(); return Ok(report);
        }
        report.valid = true;
        report.message = if report.v2_events == 0 {
            "Legacy chain verified with limited field coverage; next append creates a full-content anchor".into()
        } else { "V2 full-content chain and legacy snapshot verified".into() };
        Ok(report)
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
    fn v2_covers_previously_omitted_fields_and_delimits_strings() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let mut event = Event::new(EventKind::Action, "check every field");
        event.inputs = vec!["ab".into(), "c".into()];
        let saved = writer.append(event).unwrap();
        let mut variants = Vec::new();
        let mut changed = saved.clone(); changed.inputs = vec!["a".into(), "bc".into()]; variants.push(changed);
        let mut changed = saved.clone(); changed.importance = 0.123; variants.push(changed);
        let mut changed = saved.clone(); changed.parent_event_id = Some(crate::types::EventId::new()); variants.push(changed);
        let mut changed = saved.clone(); changed.metrics = Some(crate::types::event::Metrics { latency_ms: Some(1), tokens_in: None, tokens_out: None, cpu_percent: None, gpu_percent: None }); variants.push(changed);
        let mut changed = saved.clone(); changed.artifacts.push(crate::types::event::ArtifactRef { path: "a.rs".into(), hash: Some("altered".into()), op: crate::types::event::ArtifactOp::Read }); variants.push(changed);
        let mut changed = saved.clone(); changed.ts += chrono::Duration::nanoseconds(1); variants.push(changed);
        for changed in variants {
            storage.put(CF_EVENTS, &saved.id.to_bytes(), &bincode::serialize(&changed).unwrap()).unwrap();
            assert!(!writer.verify_chain().unwrap(), "mutation was not detected: {changed:?}");
        }
        storage.put(CF_EVENTS, &saved.id.to_bytes(), &bincode::serialize(&saved).unwrap()).unwrap();
        assert!(writer.verify_chain().unwrap());
    }

    #[test]
    fn legacy_transition_preserves_bytes_and_anchors_all_old_fields() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let mut old = Event::new(EventKind::Action, "legacy");
        old.this_hash = Some(writer.hash_event(&old, None));
        let bytes = bincode::serialize(&old).unwrap();
        storage.put(CF_EVENTS, &old.id.to_bytes(), &bytes).unwrap();
        storage.put(CF_CHAIN, CHAIN_HEAD_KEY, &old.this_hash.unwrap()).unwrap();
        assert!(writer.verify_chain().unwrap());
        let new = writer.append(Event::new(EventKind::Observation, "v2")).unwrap();
        assert_eq!(new.prev_hash, old.this_hash);
        assert_eq!(storage.get(CF_EVENTS, &old.id.to_bytes()).unwrap().unwrap(), bytes);
        let report = writer.verify_chain_detailed().unwrap();
        assert!(report.valid && report.legacy_snapshot_protected);
        assert_eq!((report.legacy_events, report.v2_events), (1,1));
        old.importance = 0.123;
        storage.put(CF_EVENTS, &old.id.to_bytes(), &bincode::serialize(&old).unwrap()).unwrap();
        assert!(!writer.verify_chain().unwrap());
    }

    #[test]
    fn failed_batch_does_not_write_partial_events_indexes_or_head() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let event = Event::new(EventKind::Action, "duplicate");
        assert!(writer.append_batch(vec![event.clone(), event.clone()]).is_err());
        for tree in [CF_EVENTS, CF_EVENT_TIME, CF_EVENT_PROJECT, CF_CHAIN] {
            assert_eq!(storage.count(tree).unwrap(), 0, "partial transaction: {tree}");
        }
        let saved = writer.append(event.clone()).unwrap();
        assert!(writer.append(event).is_err());
        assert_eq!(storage.get(CF_EVENTS, &saved.id.to_bytes()).unwrap().unwrap(), bincode::serialize(&saved).unwrap());
        assert!(writer.verify_chain().unwrap());
    }

    #[test]
    fn independent_writers_share_one_chain() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let threads: Vec<_> = (0..4).map(|_| {
            let storage = storage.clone();
            std::thread::spawn(move || {
                let writer = LedgerWriter::new(storage);
                for _ in 0..12 { writer.append(Event::new(EventKind::Action, "concurrent")).unwrap(); }
            })
        }).collect();
        for thread in threads { thread.join().unwrap(); }
        assert_eq!(storage.count(CF_EVENTS).unwrap(), 48);
        assert!(LedgerWriter::new(storage).verify_chain().unwrap());
    }

    #[test]
    fn corrupt_records_and_version_downgrades_cannot_be_skipped() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let writer = LedgerWriter::new(storage.clone());
        let event = writer.append(Event::new(EventKind::Action, "test")).unwrap();
        storage.delete(CF_CHAIN, &version_key(&event)).unwrap();
        assert!(!writer.verify_chain().unwrap());
        storage.put(CF_CHAIN, &version_key(&event), &[2]).unwrap();
        storage.put(CF_EVENTS, b"broken", b"unreadable").unwrap();
        assert!(!writer.verify_chain().unwrap());
    }

    #[test]
    fn committed_batches_survive_process_kill() {
        use std::io::{BufRead, Write};
        if let Ok(path) = std::env::var("QUIRON_CRASH_TEST_DB") {
            let storage = Storage::open(path).unwrap();
            let writer = LedgerWriter::new(storage);
            for batch in 0..10000 {
                let events = (0..5).map(|_| {
                    let mut event = Event::new(EventKind::Action, format!("batch-{batch}"));
                    event.project_id = Some("crash-fixture".into()); event
                }).collect();
                writer.append_batch(events).unwrap();
                println!("ACK {}", (batch+1)*5);
                std::io::stdout().flush().unwrap();
            }
            return;
        }
        let dir = TempDir::new().unwrap();
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
        let mut child = ChildGuard(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ledger::writer::tests::committed_batches_survive_process_kill", "--nocapture"])
            .env("QUIRON_CRASH_TEST_DB", dir.path()).stdout(std::process::Stdio::piped()).spawn().unwrap());
        let stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
                if let Some(n) = line.strip_prefix("ACK ").and_then(|s| s.parse::<u64>().ok()) { let _ = tx.send(n); }
            }
        });
        let mut acknowledged = 0;
        for _ in 0..3 { acknowledged = rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap(); }
        child.0.kill().unwrap(); child.0.wait().unwrap(); reader.join().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let count = storage.count(CF_EVENTS).unwrap();
        assert!(count >= acknowledged);
        assert_eq!(count % 5, 0);
        assert_eq!(storage.count(CF_EVENT_TIME).unwrap(), count);
        assert_eq!(storage.count(CF_EVENT_PROJECT).unwrap(), count);
        assert!(LedgerWriter::new(storage.clone()).verify_chain().unwrap());
        let mut groups = std::collections::HashMap::new();
        for event in crate::ledger::LedgerReader::new(storage).all().unwrap() {
            *groups.entry(event.description).or_insert(0) += 1;
        }
        assert!(groups.values().all(|n| *n == 5));
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
