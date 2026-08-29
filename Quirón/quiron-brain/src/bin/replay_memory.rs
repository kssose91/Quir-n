//! Canonical one-shot replay tool for memory projections.
//!
//! Usage:
//!   cargo run --release --bin replay_memory
//!   cargo run --release --features semantic --bin replay_memory -- --semantic
//!   cargo run --release --features full --bin replay_memory -- --semantic --neo4j

use quiron_brain::replay::ReplayService;
use quiron_brain::storage::Storage;
use serde_json::json;

#[cfg(feature = "neo4j")]
use quiron_brain::neo4j::Neo4jConnector;
#[cfg(feature = "semantic")]
use quiron_brain::semantic;

#[derive(Debug, Default)]
struct ReplayOptions {
    semantic: bool,
    neo4j: bool,
}

fn usage(program: &str) -> String {
    format!(
        "Usage: {program} [--semantic] [--neo4j]\n\n\
         Replays canonical memory layers from the ledger.\n\
         Base run always backfills envelopes and rebuilds the local graph.\n\
         --semantic rebuilds Qdrant if compiled with the `semantic` feature.\n\
         --neo4j rebuilds Neo4j if compiled with the `neo4j` feature."
    )
}

fn parse_options() -> anyhow::Result<ReplayOptions> {
    let args: Vec<String> = std::env::args().collect();
    let mut options = ReplayOptions::default();

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--semantic" => options.semantic = true,
            "--neo4j" => options.neo4j = true,
            "--help" | "-h" => {
                println!("{}", usage(&args[0]));
                std::process::exit(0);
            }
            other => anyhow::bail!("Unknown argument: {other}\n\n{}", usage(&args[0])),
        }
    }

    Ok(options)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let options = parse_options()?;
    let data_path = std::env::var("QUIRON_DATA").unwrap_or_else(|_| "./data".to_string());
    let storage = Storage::open(&data_path)?;
    let replay = ReplayService::new(storage.clone());

    let envelope_backfill = replay.backfill_memory_envelopes()?;
    let local_graph = replay.rebuild_local_graph()?;

    #[cfg(feature = "semantic")]
    let semantic_report = if options.semantic {
        let config = semantic::SemanticConfig::from_env();
        let client = semantic::create_semantic_client(config).await?;
        Some(replay.rebuild_semantic_index(client).await?)
    } else {
        None
    };

    #[cfg(not(feature = "semantic"))]
    let semantic_report = {
        if options.semantic {
            anyhow::bail!(
                "--semantic requires compiling replay_memory with the `semantic` feature"
            );
        }
        Option::<serde_json::Value>::None
    };

    #[cfg(feature = "neo4j")]
    let neo4j_report = if options.neo4j {
        let neo4j_uri =
            std::env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://127.0.0.1:7687".to_string());
        let neo4j_user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
        let neo4j_pass = std::env::var("NEO4J_PASSWORD")
            .map_err(|_| anyhow::anyhow!("NEO4J_PASSWORD must be set"))?;
        let connector = Neo4jConnector::connect(&neo4j_uri, &neo4j_user, &neo4j_pass).await?;
        Some(replay.rebuild_neo4j_projection(connector).await?)
    } else {
        None
    };

    #[cfg(not(feature = "neo4j"))]
    let neo4j_report = {
        if options.neo4j {
            anyhow::bail!("--neo4j requires compiling replay_memory with the `neo4j` feature");
        }
        Option::<serde_json::Value>::None
    };

    let report = json!({
        "data_path": data_path,
        "envelope_backfill": envelope_backfill,
        "local_graph": local_graph,
        "semantic": semantic_report,
        "neo4j": neo4j_report,
    });

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
