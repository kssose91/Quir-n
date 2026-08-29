use crate::{
    config::Config,
    error::{Result, ServiceError},
};
use fastembed::{RerankResult, TextEmbedding, TextInitOptions, TextRerank};
use once_cell::sync::OnceCell;
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

pub struct SemanticEngines {
    config: Arc<Config>,
    embedding: LazyModel<TextEmbedding>,
    reranker: LazyModel<TextRerank>,
    started_at: Instant,
}

struct LazyModel<T> {
    cell: OnceCell<Arc<Mutex<T>>>,
    last_error: Mutex<Option<String>>,
    loaded_at: Mutex<Option<String>>,
}

impl<T> Default for LazyModel<T> {
    fn default() -> Self {
        Self {
            cell: OnceCell::new(),
            last_error: Mutex::new(None),
            loaded_at: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceHealth {
    pub status: String,
    pub service: String,
    pub version: String,
    pub bind_addr: String,
    pub device_label: String,
    pub cuda_device_id: i32,
    pub execution_providers: Vec<String>,
    pub uptime_seconds: u64,
    pub embedding: ModelHealth,
    pub rerank: ModelHealth,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelHealth {
    pub model_code: String,
    pub ready: bool,
    pub loaded_at: Option<String>,
    pub last_error: Option<String>,
    pub batch_size_limit: usize,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbedVector {
    pub index: usize,
    pub values: Vec<f32>,
    pub dimension: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbedResponse {
    pub model: String,
    pub dimension: usize,
    pub device_label: String,
    pub mode: String,
    pub vectors: Vec<EmbedVector>,
    pub latency_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerankItem {
    pub index: usize,
    pub score: f32,
    pub document: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerankResponse {
    pub model: String,
    pub query: String,
    pub device_label: String,
    pub results: Vec<RerankItem>,
    pub latency_ms: u128,
}

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedMode {
    Raw,
    Query,
    Passage,
}

#[derive(Debug, Clone)]
pub struct EmbedRequest {
    pub inputs: Vec<String>,
    pub mode: EmbedMode,
}

#[derive(Debug, Clone)]
pub struct RerankRequest {
    pub query: String,
    pub documents: Vec<String>,
    pub top_k: usize,
    pub return_documents: bool,
}

impl SemanticEngines {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            embedding: LazyModel::default(),
            reranker: LazyModel::default(),
            started_at: Instant::now(),
        }
    }

    pub fn health(&self) -> ServiceHealth {
        let embedding = self.embedding.health(
            self.config.embed_model_code.clone(),
            Some(self.config.embed_dimension),
            self.config.max_embed_batch_size,
            "embedding",
        );
        let rerank = self.reranker.health(
            self.config.rerank_model_code.clone(),
            None,
            self.config.max_rerank_batch_size,
            "rerank",
        );
        let ready = embedding.ready && rerank.ready;
        ServiceHealth {
            status: if ready {
                "ok".to_string()
            } else {
                "degraded".to_string()
            },
            service: "semantic-ia-local".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            bind_addr: self.config.bind_addr.to_string(),
            device_label: self.config.device_label.clone(),
            cuda_device_id: self.config.cuda_device_id,
            execution_providers: self.config.execution_provider_summary.clone(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            embedding,
            rerank,
        }
    }

    pub async fn embed(&self, request: EmbedRequest) -> Result<EmbedResponse> {
        if request.inputs.is_empty() {
            return Err(ServiceError::BadRequest(
                "inputs must not be empty".to_string(),
            ));
        }
        if request.inputs.len() > self.config.max_embed_batch_size {
            return Err(ServiceError::BadRequest(format!(
                "inputs exceeds max_embed_batch_size={}",
                self.config.max_embed_batch_size
            )));
        }

        let prepared_inputs = request
            .inputs
            .iter()
            .map(|input| apply_embed_mode(request.mode, input))
            .collect::<Vec<_>>();

        let model = self.embedding.load(|| {
            build_embedding_model(&self.config)
                .map_err(|err| ServiceError::ModelLoad(err.to_string()))
        })?;

        let dimension = self.config.embed_dimension;
        let model_code = self.config.embed_model_code.clone();
        let device_label = self.config.device_label.clone();
        let started = Instant::now();
        let inputs = prepared_inputs.clone();
        let batch_size = self.config.max_embed_batch_size;
        let vectors = tokio::task::spawn_blocking(move || -> Result<Vec<EmbedVector>> {
            let mut model = model
                .lock()
                .map_err(|_| ServiceError::Internal("embedding mutex poisoned".to_string()))?;
            let output = model
                .embed(inputs, Some(batch_size))
                .map_err(|err| ServiceError::Inference(err.to_string()))?;
            Ok(output
                .into_iter()
                .enumerate()
                .map(|(index, values)| EmbedVector {
                    index,
                    dimension,
                    values,
                })
                .collect())
        })
        .await
        .map_err(|err| ServiceError::Internal(format!("embedding task panicked: {err}")))??;

        Ok(EmbedResponse {
            model: model_code,
            dimension,
            device_label,
            mode: format!("{:?}", request.mode).to_lowercase(),
            vectors,
            latency_ms: started.elapsed().as_millis(),
        })
    }

    pub async fn rerank(&self, request: RerankRequest) -> Result<RerankResponse> {
        if request.query.trim().is_empty() {
            return Err(ServiceError::BadRequest(
                "query must not be empty".to_string(),
            ));
        }
        if request.documents.is_empty() {
            return Err(ServiceError::BadRequest(
                "documents must not be empty".to_string(),
            ));
        }
        if request.documents.len() > self.config.max_rerank_batch_size {
            return Err(ServiceError::BadRequest(format!(
                "documents exceeds max_rerank_batch_size={}",
                self.config.max_rerank_batch_size
            )));
        }
        let top_k = request.top_k.clamp(1, request.documents.len());

        let model = self.reranker.load(|| {
            build_rerank_model(&self.config).map_err(|err| ServiceError::ModelLoad(err.to_string()))
        })?;

        let started = Instant::now();
        let query = request.query.clone();
        let documents = request.documents.clone();
        let return_documents = request.return_documents;
        let batch_size = self.config.max_rerank_batch_size;

        let results = tokio::task::spawn_blocking(move || -> Result<Vec<RerankItem>> {
            let mut model = model
                .lock()
                .map_err(|_| ServiceError::Internal("rerank mutex poisoned".to_string()))?;
            let output: Vec<RerankResult> = model
                .rerank(query, &documents, return_documents, Some(batch_size))
                .map_err(|err| ServiceError::Inference(err.to_string()))?;

            Ok(output
                .into_iter()
                .take(top_k)
                .map(|item| RerankItem {
                    index: item.index,
                    score: item.score,
                    document: item.document,
                })
                .collect())
        })
        .await
        .map_err(|err| ServiceError::Internal(format!("rerank task panicked: {err}")))??;

        Ok(RerankResponse {
            model: self.config.rerank_model_code.clone(),
            query: request.query,
            device_label: self.config.device_label.clone(),
            results,
            latency_ms: started.elapsed().as_millis(),
        })
    }
}

impl<T> LazyModel<T> {
    fn load<F>(&self, loader: F) -> Result<Arc<Mutex<T>>>
    where
        F: FnOnce() -> Result<T>,
    {
        match self
            .cell
            .get_or_try_init(|| loader().map(|model| Arc::new(Mutex::new(model))))
        {
            Ok(model) => {
                *self
                    .last_error
                    .lock()
                    .map_err(|_| ServiceError::Internal("status mutex poisoned".to_string()))? =
                    None;
                *self
                    .loaded_at
                    .lock()
                    .map_err(|_| ServiceError::Internal("status mutex poisoned".to_string()))? =
                    Some(chrono::Utc::now().to_rfc3339());
                Ok(model.clone())
            }
            Err(err) => {
                *self
                    .last_error
                    .lock()
                    .map_err(|_| ServiceError::Internal("status mutex poisoned".to_string()))? =
                    Some(err.to_string());
                Err(err)
            }
        }
    }

    fn health(
        &self,
        model_code: String,
        dimension: Option<usize>,
        batch_size_limit: usize,
        kind: &str,
    ) -> ModelHealth {
        let ready = self.cell.get().is_some();
        let loaded_at = self.loaded_at.lock().ok().and_then(|guard| guard.clone());
        let last_error = self.last_error.lock().ok().and_then(|guard| guard.clone());
        ModelHealth {
            model_code,
            ready,
            loaded_at,
            last_error,
            batch_size_limit,
            kind: kind.to_string(),
            dimension,
        }
    }
}

fn build_embedding_model(config: &Config) -> Result<TextEmbedding> {
    let options = TextInitOptions::new(config.embed_model.clone())
        .with_cache_dir(config.model_cache_dir.clone())
        .with_execution_providers(config.execution_providers())
        .with_show_download_progress(config.show_download_progress);
    TextEmbedding::try_new(options).map_err(|err| ServiceError::ModelLoad(err.to_string()))
}

fn build_rerank_model(config: &Config) -> Result<TextRerank> {
    let options = fastembed::RerankInitOptions::new(config.rerank_model.clone())
        .with_cache_dir(config.model_cache_dir.clone())
        .with_execution_providers(config.execution_providers())
        .with_show_download_progress(config.show_download_progress);
    TextRerank::try_new(options).map_err(|err| ServiceError::ModelLoad(err.to_string()))
}

fn apply_embed_mode(mode: EmbedMode, text: &str) -> String {
    match mode {
        EmbedMode::Raw => text.to_string(),
        EmbedMode::Query => format!("query: {text}"),
        EmbedMode::Passage => format!("passage: {text}"),
    }
}
