use crate::{
    config::Config,
    engine::{
        EmbedMode, EmbedRequest, EmbedResponse, RerankRequest, RerankResponse, SemanticEngines,
    },
    error::Result,
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub async fn serve(config: Arc<Config>, engines: Arc<SemanticEngines>) -> Result<()> {
    init_tracing();

    tracing::info!(
        bind_addr = %config.bind_addr,
        device_label = %config.device_label,
        embed_model = %config.embed_model_code,
        rerank_model = %config.rerank_model_code,
        execution_providers = ?config.execution_provider_summary,
        "Starting semantic IA local service"
    );

    if config.warmup {
        let warmup_engines = Arc::clone(&engines);
        tokio::spawn(async move {
            if let Err(err) = warmup_engines.warmup().await {
                tracing::warn!(error = %err, "Warmup failed");
            }
        });
    }

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/embed", post(embed))
        .route("/v1/rerank", post(rerank))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(Arc::clone(&engines));

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .map_err(|err| crate::error::ServiceError::Config(format!("failed to bind: {err}")))?;
    axum::serve(listener, app.into_make_service())
        .await
        .map_err(|err| crate::error::ServiceError::Internal(err.to_string()))?;
    Ok(())
}

async fn health(State(engines): State<Arc<SemanticEngines>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        health: engines.health(),
    })
}

async fn embed(
    State(engines): State<Arc<SemanticEngines>>,
    Json(request): Json<EmbedHttpRequest>,
) -> Result<Json<EmbedResponse>> {
    let internal = EmbedRequest {
        inputs: request.inputs,
        mode: request.mode.unwrap_or(EmbedMode::Raw),
    };
    Ok(Json(engines.embed(internal).await?))
}

async fn rerank(
    State(engines): State<Arc<SemanticEngines>>,
    Json(request): Json<RerankHttpRequest>,
) -> Result<Json<RerankResponse>> {
    let internal = RerankRequest {
        query: request.query,
        documents: request.documents,
        top_k: request.top_k.unwrap_or(5),
        return_documents: request.return_documents.unwrap_or(true),
    };
    Ok(Json(engines.rerank(internal).await?))
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    health: crate::engine::ServiceHealth,
}

#[derive(Debug, Deserialize)]
struct EmbedHttpRequest {
    inputs: Vec<String>,
    #[serde(default)]
    mode: Option<EmbedMode>,
}

#[derive(Debug, Deserialize)]
struct RerankHttpRequest {
    query: String,
    documents: Vec<String>,
    #[serde(default)]
    top_k: Option<usize>,
    #[serde(default)]
    return_documents: Option<bool>,
}

impl SemanticEngines {
    async fn warmup(&self) -> Result<()> {
        let _ = self
            .embed(EmbedRequest {
                inputs: vec!["warmup".to_string()],
                mode: EmbedMode::Raw,
            })
            .await?;
        let _ = self
            .rerank(RerankRequest {
                query: "warmup".to_string(),
                documents: vec!["warmup document".to_string()],
                top_k: 1,
                return_documents: false,
            })
            .await?;
        Ok(())
    }
}
