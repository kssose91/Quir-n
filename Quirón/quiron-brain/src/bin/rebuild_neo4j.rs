//! One-shot tool: rebuild Neo4j graph from all ledger events.
//!
//! Usage: cargo run --release --features neo4j --bin rebuild_neo4j
//!
//! Clears Neo4j, re-applies schema, replays all events.

use quiron_brain::neo4j::{Neo4jConnector, SyncService};
use quiron_brain::storage::Storage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Init tracing
    tracing_subscriber::fmt::init();

    let data_path = std::env::var("QUIRON_DATA").unwrap_or_else(|_| "./data".to_string());
    let neo4j_uri =
        std::env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://127.0.0.1:7687".to_string());
    let neo4j_user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
    let neo4j_pass = std::env::var("NEO4J_PASSWORD").expect("NEO4J_PASSWORD must be set");

    println!("Opening storage at: {}", data_path);
    let storage = Storage::open(&data_path)?;

    println!("Connecting to Neo4j at: {}", neo4j_uri);
    let neo4j = Neo4jConnector::connect(&neo4j_uri, &neo4j_user, &neo4j_pass).await?;

    let sync = SyncService::new(storage, neo4j);

    println!("Rebuilding Neo4j graph from scratch...");
    let count = sync.rebuild_from_scratch().await?;

    println!("\n✅ Rebuild complete! Projected {} events to Neo4j", count);

    Ok(())
}
