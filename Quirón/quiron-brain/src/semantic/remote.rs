//! HTTP client for the external semantic_ia_local compute service.
//!
//! The remote service owns GPU-heavy semantic compute (embeddings + rerank),
//! while Quirón Brain keeps Qdrant as the canonical vector store.

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::SemanticConfig;

#[derive(Debug, Clone)]
pub struct RemoteSemanticClient {
    http: Client,
    config: SemanticConfig,
    embed_dimension: usize,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum RemoteEmbedMode {
    Query,
    Passage,
}

#[derive(Debug, Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
    mode: RemoteEmbedMode,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    vectors: Vec<EmbedVector>,
}

#[derive(Debug, Deserialize)]
struct EmbedVector {
    values: Vec<f32>,
}

#[derive(Debug, Serialize)]
struct RerankRequest<'a> {
    query: &'a str,
    documents: &'a [String],
    top_k: usize,
    return_documents: bool,
}

#[derive(Debug, Deserialize)]
struct RerankResponse {
    results: Vec<RemoteRerankItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteRerankItem {
    pub index: usize,
    pub score: f32,
}

#[derive(Debug, Deserialize)]
struct HealthEnvelope {
    health: HealthPayload,
}

#[derive(Debug, Deserialize)]
struct HealthPayload {
    embedding: HealthModel,
}

#[derive(Debug, Deserialize)]
struct HealthModel {
    dimension: Option<usize>,
}

impl RemoteSemanticClient {
    pub async fn connect(config: SemanticConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        if let Some(token) = config.remote_token.as_ref() {
            let value = HeaderValue::from_str(&format!("Bearer {}", token))
                .context("invalid remote semantic token")?;
            headers.insert(AUTHORIZATION, value);
        }

        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(config.remote_timeout_secs))
            .build()
            .context("failed to build remote semantic HTTP client")?;

        let client = Self {
            http,
            config,
            embed_dimension: 0,
        };

        let health = client.fetch_health().await?;
        let embed_dimension = health
            .health
            .embedding
            .dimension
            .context("remote semantic health did not expose embedding dimension")?;

        Ok(Self {
            embed_dimension,
            ..client
        })
    }

    pub fn dimension(&self) -> usize {
        self.embed_dimension
    }

    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.embed_many(&[query.to_string()], RemoteEmbedMode::Query)
            .await?
            .into_iter()
            .next()
            .context("remote semantic embed returned no vector for query")
    }

    pub async fn embed_passage(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_many(&[text.to_string()], RemoteEmbedMode::Passage)
            .await?
            .into_iter()
            .next()
            .context("remote semantic embed returned no vector for passage")
    }

    pub async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_k: usize,
    ) -> Result<Vec<RemoteRerankItem>> {
        let response: RerankResponse = self
            .request_json(
                "/v1/rerank",
                &RerankRequest {
                    query,
                    documents,
                    top_k,
                    return_documents: false,
                },
            )
            .await?;
        Ok(response.results)
    }

    pub async fn health_check(&self) -> bool {
        self.fetch_health().await.is_ok()
    }

    async fn embed_many(&self, inputs: &[String], mode: RemoteEmbedMode) -> Result<Vec<Vec<f32>>> {
        let response: EmbedResponse = self
            .request_json("/v1/embed", &EmbedRequest { inputs, mode })
            .await?;
        Ok(response
            .vectors
            .into_iter()
            .map(|vector| vector.values)
            .collect())
    }

    async fn fetch_health(&self) -> Result<HealthEnvelope> {
        let url = self.endpoint("/health");
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to GET {}", url))?;
        let response = Self::ensure_success(response, &url).await?;
        response
            .json::<HealthEnvelope>()
            .await
            .with_context(|| format!("failed to decode response from {}", url))
    }

    async fn request_json<B: Serialize + ?Sized, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.endpoint(path);
        let response = self
            .http
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("failed to POST {}", url))?;
        let response = Self::ensure_success(response, &url).await?;
        response
            .json::<T>()
            .await
            .with_context(|| format!("failed to decode response from {}", url))
    }

    async fn ensure_success(response: reqwest::Response, url: &str) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("{} returned {}: {}", url, status, body);
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.remote_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}
