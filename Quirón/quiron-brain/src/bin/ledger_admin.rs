//! Offline verification and explicit legacy anchoring. Stop the service and
//! back up its database before anchoring; opening a live Sled database fails.
use quiron_brain::{ledger::LedgerWriter, storage::Storage, types::{Event, EventKind}};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_default();
    if action == "--help" {
        println!("ledger_admin verify|anchor --data PATH\nStop the brain and back up PATH before anchor. Legacy events are not rewritten.");
        return Ok(());
    }
    if !matches!(action.as_str(), "verify" | "anchor") || args.next().as_deref() != Some("--data") {
        anyhow::bail!("Usage: ledger_admin verify|anchor --data PATH");
    }
    let path = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("Missing data path"))?);
    if args.next().is_some() || !path.join("conf").is_file() {
        anyhow::bail!("Expected an existing Sled database, not a new directory");
    }
    let storage = Storage::open(&path)?;
    let original: Vec<_> = storage.tree(quiron_brain::storage::cf::CF_EVENTS)
        .ok_or_else(|| anyhow::anyhow!("Missing events tree"))?.iter().collect::<std::result::Result<_,_>>()?;
    let writer = LedgerWriter::new(storage.clone());
    let before = writer.verify_chain_detailed()?;
    if !before.valid {
        println!("{}", serde_json::to_string_pretty(&before)?);
        anyhow::bail!("Ledger verification failed; no anchor written");
    }
    let mut migration_event = None;
    if action == "anchor" && before.v2_events == 0 {
        let mut event = Event::new(EventKind::Observation, "Ledger v2: full-content legacy snapshot anchored; prior event bytes preserved.");
        event.agent_id = "ledger-admin".into();
        event.tags = vec!["ledger-v2-migration".into()];
        migration_event = Some(writer.append(event)?.id.to_string());
    }
    let after = writer.verify_chain_detailed()?;
    let mut legacy_bytes_preserved = true;
    for (key, value) in &original {
        if storage.get(quiron_brain::storage::cf::CF_EVENTS, key)?.as_deref() != Some(value.as_ref()) {
            legacy_bytes_preserved = false;
        }
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "before": before, "after": after, "migration_event": migration_event, "legacy_bytes_preserved": legacy_bytes_preserved,
        "note": "The anchor protects legacy content observed at transition; it does not prove originality before transition."
    }))?);
    if !after.valid || !legacy_bytes_preserved { anyhow::bail!("Post-transition verification failed"); }
    Ok(())
}
