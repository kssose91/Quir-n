//! Quiron Brain - Neural Memory System
//!
//! An obligatory local brain for AI agents where everything passes through:
//! - Ledger (immutable event log)
//! - Graph (derived knowledge network)
//! - Gates (invariants that block errors)
//! - Evidence (proof of every action)

// Use the library instead of duplicating modules
use quiron_brain::api::server::{create_router, AppState};
use quiron_brain::{api, graph, invariants, ledger, storage};

#[cfg(feature = "semantic")]
use quiron_brain::semantic;

#[cfg(feature = "neo4j")]
use quiron_brain::neo4j;

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

async fn bind_loopback_listener(addr: SocketAddr) -> anyhow::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind {}: {}", addr, e))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Set process name for system monitors
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        if let Ok(name) = CString::new("Quiron") {
            unsafe {
                libc::prctl(libc::PR_SET_NAME, name.as_ptr());
            }
        }
    }

    // Get data path
    let data_path = std::env::var("QUIRON_DATA").unwrap_or_else(|_| "./data".to_string());

    tracing::info!("Opening storage at {}", data_path);

    // Open storage
    let storage = storage::Storage::open(&data_path)?;

    #[cfg(feature = "semantic")]
    let (semantic_client, _semantic_shutdown) = {
        let config = semantic::SemanticConfig::from_env();
        let backend_label = config.backend_label();
        let collection = config.collection.clone();
        let remote_url = config.remote_url.clone();

        match semantic::create_semantic_client(config).await {
            Ok(client) => {
                tracing::info!(
                    backend = backend_label,
                    collection = %collection,
                    remote_url = %remote_url,
                    "✨ Semantic search enabled"
                );

                // Create sync service for active Qdrant insertion
                let sync_service =
                    semantic::SemanticSyncService::new(storage.clone(), client.clone());
                let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

                // Spawn background sync task (every 500ms)
                tokio::spawn(async move {
                    if let Err(e) = sync_service.run_continuous(500, shutdown_rx).await {
                        tracing::error!("Semantic sync service error: {}", e);
                    }
                });

                (Some(client), Some(shutdown_tx))
            }
            Err(e) => {
                tracing::warn!(
                    backend = backend_label,
                    remote_url = %remote_url,
                    "Semantic search disabled: {}",
                    e
                );
                (None, None)
            }
        }
    };

    // Initialize Neo4j connection if feature enabled
    #[cfg(feature = "neo4j")]
    let (neo4j_connector, _neo4j_shutdown) = {
        let neo4j_uri =
            std::env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://127.0.0.1:7687".to_string());
        let neo4j_user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
        let neo4j_pass = std::env::var("NEO4J_PASSWORD").map_err(|_| {
            anyhow::anyhow!("NEO4J_PASSWORD is required when neo4j feature is enabled")
        })?;

        match neo4j::Neo4jConnector::connect(&neo4j_uri, &neo4j_user, &neo4j_pass).await {
            Ok(connector) => {
                tracing::info!("✨ Neo4j connected ({})", neo4j_uri);

                // Apply schema (constraints + indexes)
                if let Err(e) = neo4j::schema::Schema::ensure(&connector).await {
                    tracing::warn!("Neo4j schema error: {}", e);
                }

                // Create sync service and spawn background task
                let sync_service = neo4j::SyncService::new(storage.clone(), connector.clone());
                let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

                // Do initial sync
                match sync_service.sync_pending().await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!("📊 Synced {} pending events to Neo4j", count);
                        }
                    }
                    Err(e) => tracing::warn!("Initial Neo4j sync failed: {}", e),
                }

                // Spawn background sync task (every 500ms)
                tokio::spawn(async move {
                    if let Err(e) = sync_service.run_continuous(500, shutdown_rx).await {
                        tracing::error!("Neo4j sync service error: {}", e);
                    }
                });

                (Some(connector), Some(shutdown_tx))
            }
            Err(e) => {
                tracing::warn!("Neo4j disabled: {}", e);
                (None, None)
            }
        }
    };

    // Initialize LLM client via vertex-gateway (binario inmutable de seguridad)
    let llm_client = match quiron_brain::llm_client::LlmClient::shared_from_default() {
        Ok(client) => {
            tracing::info!("🔐 LLM client ready (via vertex-gateway - stdin/stdout)");
            Some(client)
        }
        Err(e) => {
            tracing::warn!("LLM client disabled: {} (vertex-gateway not found)", e);
            None
        }
    };

    let api_token = std::env::var("QUIRON_API_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let require_auth = env_bool("QUIRON_REQUIRE_AUTH", api_token.is_some());

    if require_auth && api_token.is_none() {
        anyhow::bail!("QUIRON_REQUIRE_AUTH=true but QUIRON_API_TOKEN is not set");
    }

    if require_auth {
        tracing::info!("🔒 API bearer auth enabled for mutating endpoints");
    } else {
        tracing::warn!("⚠️ API bearer auth disabled (set QUIRON_API_TOKEN to enable)");
    }

    // Create application state
    let state = Arc::new(AppState {
        storage: storage.clone(),
        writer: tokio::sync::Mutex::new(ledger::LedgerWriter::new(storage.clone())),
        reader: ledger::LedgerReader::new(storage.clone()),
        graph: tokio::sync::RwLock::new(graph::GraphBuilder::new(storage.clone())),
        invariants: invariants::InvariantEngine::new(storage.clone()),
        chain_valid_cache: std::sync::atomic::AtomicBool::new(true),
        last_chain_verify: std::sync::atomic::AtomicU64::new(0),
        startup_time: std::time::Instant::now(),
        last_activity_secs: std::sync::atomic::AtomicU64::new(0),
        api_token,
        require_auth,
        llm: llm_client.clone(),
        #[cfg(feature = "semantic")]
        indexing_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        #[cfg(feature = "semantic")]
        semantic: semantic_client.clone(),
        #[cfg(feature = "semantic")]
        code_worker: Arc::new(quiron_brain::index::worker::ProjectWorker::default()),
        #[cfg(feature = "neo4j")]
        neo4j: neo4j_connector,
    });

    // Register default anti-smoke gates
    if let Err(e) = api::server::register_default_gates(&state.invariants) {
        tracing::warn!("Failed to register default gates: {}", e);
    }

    // Create router
    let app = create_router(state.clone());

    // Bind address
    let port: u16 = std::env::var("QUIRON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8766);

    let ipv4_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let ipv6_addr = SocketAddr::from((Ipv6Addr::LOCALHOST, port));

    let ipv4_listener = bind_loopback_listener(ipv4_addr).await?;
    let ipv6_listener = match bind_loopback_listener(ipv6_addr).await {
        Ok(listener) => Some(listener),
        Err(err) => {
            tracing::warn!(
                "⚠️ IPv6 loopback listener unavailable, continuing with IPv4 only: {}",
                err
            );
            None
        }
    };

    tracing::info!("🧠 Quiron Brain listening on http://{}", ipv4_addr);
    if ipv6_listener.is_some() {
        tracing::info!("🧠 Quiron Brain listening on http://{}", ipv6_addr);
    }
    tracing::info!("Endpoints:");
    tracing::info!("  GET  /health              - Health check");
    tracing::info!("  GET  /chain/verify        - Verify hash chain");
    tracing::info!("  POST /event               - Create event");
    tracing::info!("  GET  /events              - List events");
    tracing::info!("  POST /action/request      - Validate action");
    tracing::info!("  GET  /invariants          - List invariants");
    tracing::info!("  POST /invariants          - Create invariant");
    tracing::info!("  GET  /project/:id/timeline - Project timeline");

    // Create LLM distiller and asynchronous Background Distillation Worker if we have an LLM client
    if let Some(client) = llm_client.clone() {
        let enable_distillation_worker = env_bool("QUIRON_ENABLE_DISTILLATION_WORKER", false);
        let enable_enrichment_worker = env_bool("QUIRON_ENABLE_ENRICHMENT_WORKER", false);
        let distillation_interval_secs = std::env::var("QUIRON_DISTILLATION_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(60);
        let enrichment_interval_secs = std::env::var("QUIRON_ENRICHMENT_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(30);

        let llm_distiller = quiron_brain::llm_distiller::LlmDistiller::new(client.clone());
        let distillation_worker = quiron_brain::distillation_worker::DistillationWorker::new(
            storage.clone(),
            quiron_brain::ledger::LedgerReader::new(storage.clone()),
            Arc::new(tokio::sync::Mutex::new(
                quiron_brain::ledger::LedgerWriter::new(storage.clone()),
            )),
            llm_distiller,
        );

        if enable_distillation_worker {
            tokio::spawn(async move {
                distillation_worker
                    .run_continuous(distillation_interval_secs)
                    .await;
            });
            tracing::info!(
                "🧠 Distillation worker ENABLED (interval: {}s).",
                distillation_interval_secs
            );
        } else {
            tracing::info!("🧠 Distillation worker DISABLED (cost saving).");
        }

        // Spawn enrichment worker with its OWN LlmClient instance
        // to avoid Mutex contention with other server handlers.
        // The worker writes to MemoryEnvelopes only — no Qdrant re-upsert
        // to avoid embedding Mutex contention that blocks the server.
        let enricher_client = quiron_brain::llm_client::LlmClient::shared_from_default()
            .expect("enricher LlmClient must init from same path");
        let enricher = quiron_brain::llm_enricher::LlmEnricher::new(enricher_client);
        let enrichment_worker = quiron_brain::enrichment_worker::EnrichmentWorker::new(
            storage.clone(),
            quiron_brain::ledger::LedgerReader::new(storage.clone()),
            enricher,
        );
        if enable_enrichment_worker {
            tokio::spawn(async move {
                enrichment_worker.run_continuous(enrichment_interval_secs).await;
            });
            tracing::info!(
                "🏷️  Enrichment worker ENABLED (interval: {}s).",
                enrichment_interval_secs
            );
        } else {
            tracing::info!("🏷️  Enrichment worker DISABLED (cost saving).");
        }
    } else {
        tracing::warn!("⚠️ LLM client not available, distillation worker will NOT run.");
    }

    // Apagado por inactividad: sin peticiones ni barridos durante
    // QUIRON_IDLE_EXIT_SECS (900 por defecto; 0 lo desactiva) el cerebro
    // termina limpiamente y systemd para los almacenes con él (ExecStopPost).
    // El editor sondea /health mientras está abierto: en uso nunca se apaga.
    let idle_exit_secs = std::env::var("QUIRON_IDLE_EXIT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(900);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    if idle_exit_secs > 0 {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                #[cfg(feature = "semantic")]
                let busy = state.code_worker.busy();
                #[cfg(not(feature = "semantic"))]
                let busy = false;
                if state.idle_secs() >= idle_exit_secs && !busy {
                    tracing::info!(
                        "💤 {}s sin peticiones ni barridos: el cerebro se apaga y los almacenes con él",
                        idle_exit_secs
                    );
                    let _ = shutdown_tx.send(true);
                    break;
                }
            }
        });
        tracing::info!("💤 Apagado por inactividad tras {}s sin uso", idle_exit_secs);
    }
    // Si el emisor desaparece (apagado desactivado) no se termina nunca.
    let wait_shutdown = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    };

    // Start server
    if let Some(ipv6_listener) = ipv6_listener {
        tokio::try_join!(
            axum::serve(ipv4_listener, app.clone())
                .with_graceful_shutdown(wait_shutdown(shutdown_rx.clone())),
            axum::serve(ipv6_listener, app).with_graceful_shutdown(wait_shutdown(shutdown_rx)),
        )?;
    } else {
        axum::serve(ipv4_listener, app)
            .with_graceful_shutdown(wait_shutdown(shutdown_rx))
            .await?;
    }
    tracing::info!("🧠 Quiron Brain detenido");

    Ok(())
}
