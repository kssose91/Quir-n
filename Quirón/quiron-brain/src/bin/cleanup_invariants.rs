//! One-shot tool: deduplicate invariants in Sled.
//!
//! Usage: cargo run --release --bin cleanup_invariants [-- --data-path ./data]
//!
//! Keeps only ONE invariant per unique name (the newest one).
//! Deletes all duplicates. No mock data, no smoke.

use quiron_brain::invariants::InvariantEngine;
use quiron_brain::storage::Storage;
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse optional --data-path argument
    let args: Vec<String> = std::env::args().collect();
    let data_path = if let Some(pos) = args.iter().position(|a| a == "--data-path") {
        args.get(pos + 1).map(|s| s.as_str()).unwrap_or("./data")
    } else {
        "./data"
    };

    println!("Opening Sled database at: {}", data_path);
    let storage = Storage::open(data_path)?;
    let engine = InvariantEngine::new(storage.clone());

    let all = engine.all()?;

    println!("\n=== SCAN RESULTS ===");
    println!("Total invariants in store: {}", all.len());

    // Group by name, keeping (id_bytes, index) for each
    let mut by_name: HashMap<String, Vec<(usize, [u8; 16])>> = HashMap::new();
    for (idx, inv) in all.iter().enumerate() {
        by_name
            .entry(inv.name.clone())
            .or_default()
            .push((idx, inv.id.to_bytes()));
    }

    println!("Unique names found: {}", by_name.len());
    for (name, entries) in &by_name {
        println!("  {} — {} entries", name, entries.len());
    }
    println!();

    // For each name, keep the LAST one (newest ULID), delete the rest
    let mut to_delete: Vec<[u8; 16]> = Vec::new();

    for (name, entries) in &by_name {
        if entries.len() <= 1 {
            println!("  ✅ {} — no dups", name);
            continue;
        }

        // ULIDs are time-ordered — sort by bytes, keep last
        let mut sorted = entries.clone();
        sorted.sort_by(|a, b| a.1.cmp(&b.1));

        let keep_bytes = sorted.last().unwrap().1;
        println!(
            "  🔧 {} — {} dups to remove (keeping ID ending ...{:02x}{:02x})",
            name,
            sorted.len() - 1,
            keep_bytes[14],
            keep_bytes[15],
        );

        for (_, key_bytes) in sorted.iter().take(sorted.len() - 1) {
            to_delete.push(*key_bytes);
        }
    }

    println!("\n=== CLEANUP ===");
    println!("Entries to delete: {}", to_delete.len());

    if to_delete.is_empty() {
        println!("Nothing to clean up!");
        return Ok(());
    }

    // Delete using Storage API
    for key_bytes in &to_delete {
        storage.delete("invariants", key_bytes)?;
    }
    storage.flush()?;

    let remaining = engine.all()?.len();
    println!("\n=== RESULT ===");
    println!("Deleted: {} duplicate invariants", to_delete.len());
    println!("Remaining: {} unique invariants", remaining);
    println!("\nRestart quiron-brain to pick up clean state.");

    Ok(())
}
