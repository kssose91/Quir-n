//! Semantic client wrapper for Quirón Brain
//!
//! Keeps Qdrant as the canonical vector store and delegates semantic compute
//! either to local in-process embeddings or to the external semantic_ia_local
//! service.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, SearchPointsBuilder, UpsertPointsBuilder,
    VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::sync::Arc;

use super::embed::EmbedService;
use super::search::{CragResult, RankedResult, SearchResult};
use super::{SemanticBackendKind, SemanticConfig};
use crate::memory::MemoryEnvelope;
use crate::types::Event;

enum SemanticComputeBackend {
    InProcess(EmbedService),
}

/// Semantic client combining the in-process embedding runtime with local Qdrant.
pub struct SemanticClient {
    qdrant: Qdrant,
    compute: SemanticComputeBackend,
    config: SemanticConfig,
}

impl SemanticClient {
    pub async fn connect(config: SemanticConfig) -> Result<Self> {
        let qdrant = Qdrant::from_url(&config.qdrant_url)
            .skip_compatibility_check()
            .build()
            .context("Failed to connect to Qdrant")?;

        let compute = match config.backend {
            SemanticBackendKind::InProcess => {
                SemanticComputeBackend::InProcess(EmbedService::new(&config.embed_model)?)
            }
        };

        let client = Self {
            qdrant,
            compute,
            config,
        };

        client.ensure_collection().await?;
        Ok(client)
    }

    async fn ensure_collection(&self) -> Result<()> {
        let exists = self
            .qdrant
            .collection_exists(&self.config.collection)
            .await
            .context("Failed to check collection existence")?;

        if !exists {
            tracing::info!(
                collection = %self.config.collection,
                vector_dim = self.vector_dimension(),
                backend = self.config.backend_label(),
                "Creating Qdrant collection"
            );

            self.qdrant
                .create_collection(
                    CreateCollectionBuilder::new(&self.config.collection).vectors_config(
                        VectorParamsBuilder::new(self.vector_dimension() as u64, Distance::Cosine),
                    ),
                )
                .await
                .context("Failed to create collection")?;
        }

        Ok(())
    }

    pub async fn recreate_collection(&self) -> Result<()> {
        let exists = self
            .qdrant
            .collection_exists(&self.config.collection)
            .await
            .context("Failed to check collection existence before recreate")?;

        if exists {
            self.qdrant
                .delete_collection(&self.config.collection)
                .await
                .context("Failed to delete collection before recreate")?;
        }

        self.ensure_collection().await
    }

    pub async fn upsert_event(&self, event: &Event) -> Result<()> {
        self.upsert_event_with_envelope(event, None).await
    }

    /// Vectoriza un texto y sube un punto arbitrario a la colección configurada.
    ///
    /// Genérico y ajeno a los eventos: lo usa el índice de código, que corre
    /// sobre su propia colección (`quiron_code`) para no mezclarse con la
    /// memoria de eventos. El `point_id` estable hace el upsert idempotente.
    pub async fn upsert_text_point(
        &self,
        point_id: impl Into<qdrant_client::qdrant::PointId>,
        text: &str,
        payload: HashMap<String, qdrant_client::qdrant::Value>,
    ) -> Result<()> {
        let vector = self
            .embed_passage(text)
            .await
            .context("Failed to embed code unit text")?;
        let point = PointStruct::new(point_id, vector, payload);
        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(&self.config.collection, vec![point]))
            .await
            .context("Failed to upsert code unit point")?;
        Ok(())
    }

    pub async fn upsert_event_with_envelope(
        &self,
        event: &Event,
        envelope: Option<&MemoryEnvelope>,
    ) -> Result<()> {
        let vector = self
            .embed_passage(&event.description)
            .await
            .context("Failed to generate embedding")?;

        let point = PointStruct::new(
            event_point_id(event),
            vector,
            build_payload(event, envelope),
        );

        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(
                &self.config.collection,
                vec![point],
            ))
            .await
            .context("Failed to upsert point")?;

        tracing::debug!(
            event_id = %event.id,
            backend = self.config.backend_label(),
            "Indexed event in Qdrant"
        );

        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        limit: u64,
        threshold: Option<f32>,
    ) -> Result<Vec<SearchResult>> {
        let threshold = threshold.unwrap_or(self.config.min_threshold);
        let vector = self
            .embed_query(query)
            .await
            .context("Failed to generate query embedding")?;

        let results = match self
            .qdrant
            .search_points(
                SearchPointsBuilder::new(&self.config.collection, vector.clone(), limit)
                    .score_threshold(threshold)
                    .with_payload(true),
            )
            .await
        {
            Ok(results) => results,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    qdrant_url = %self.config.qdrant_url,
                    collection = %self.config.collection,
                    "Qdrant gRPC search failed; falling back to REST search"
                );
                return self
                    .search_via_rest(vector, limit, threshold)
                    .await
                    .context("Failed to search Qdrant");
            }
        };

        Ok(results
            .result
            .into_iter()
            .map(search_result_from_point)
            .collect())
    }

    pub async fn crag_search(
        &self,
        query: &str,
        limit: u64,
        max_attempts: u32,
    ) -> Result<CragResult> {
        let mut attempts = 0;
        let mut current_query = query.to_string();

        loop {
            attempts += 1;

            let results = self.search(&current_query, limit, None).await?;

            if !results.is_empty() {
                let confidence = if results[0].score >= self.config.high_confidence {
                    "HIGH".to_string()
                } else {
                    "MEDIUM".to_string()
                };

                return Ok(CragResult {
                    results,
                    confidence,
                    blocked: false,
                    attempts,
                    original_query: query.to_string(),
                    reformulated_query: if attempts > 1 {
                        Some(current_query)
                    } else {
                        None
                    },
                });
            }

            if attempts >= max_attempts {
                break;
            }

            current_query = Self::reformulate_query(&current_query, attempts);
            tracing::debug!(
                attempt = attempts,
                query = %current_query,
                "CRAG: Reformulating query"
            );
        }

        Ok(CragResult {
            results: vec![],
            confidence: "BLOCKED".to_string(),
            blocked: true,
            attempts,
            original_query: query.to_string(),
            reformulated_query: None,
        })
    }

    fn reformulate_query(query: &str, attempt: u32) -> String {
        let words: Vec<&str> = query.split_whitespace().collect();

        match attempt {
            1 => {
                let stopwords = [
                    "el", "la", "los", "las", "de", "del", "en", "a", "para", "por", "con", "the",
                    "a", "an", "in", "on", "for", "to", "with", "of", "and", "or",
                ];
                words
                    .into_iter()
                    .filter(|w| !stopwords.contains(&w.to_lowercase().as_str()))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            2 => {
                let mut sorted_words = words.clone();
                sorted_words.sort_by(|a, b| b.len().cmp(&a.len()));
                sorted_words
                    .into_iter()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            _ => query.to_string(),
        }
    }

    pub async fn search_ranked(&self, query: &str, limit: u64) -> Result<Vec<RankedResult>> {
        let candidates = self.search(query, limit * 3, None).await?;

        let mut ranked: Vec<RankedResult> = candidates
            .into_iter()
            .map(|result| {
                let importance = result.importance.unwrap_or(0.5);
                let freshness = calculate_freshness(&result.timestamp);
                let base_score = result.score;
                let final_score = base_score * importance * freshness;

                let mut reasons = vec![];
                if importance > 0.8 {
                    reasons.push("high importance".to_string());
                }
                if freshness > 0.9 {
                    reasons.push("recent".to_string());
                }
                if base_score > self.config.high_confidence {
                    reasons.push("high similarity".to_string());
                }

                RankedResult {
                    result,
                    final_score,
                    reasons,
                }
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked.truncate(limit as usize);

        Ok(ranked)
    }

    pub async fn health_check(&self) -> bool {
        let compute_ok = match &self.compute {
            SemanticComputeBackend::InProcess(embed) => embed.health_check().await,
        };

        let qdrant_ok = self
            .qdrant
            .collection_exists(&self.config.collection)
            .await
            .unwrap_or(false);

        compute_ok && qdrant_ok
    }

    pub(crate) fn embedding_model(&self) -> &str {
        &self.config.embed_model
    }

    pub(crate) fn vector_dimension(&self) -> usize {
        match &self.compute {
            SemanticComputeBackend::InProcess(embed) => embed.dimension(),
        }
    }

    pub(crate) async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        match &self.compute {
            SemanticComputeBackend::InProcess(embed) => embed.embed(query).await,
        }
    }

    pub(crate) async fn embed_passage(&self, text: &str) -> Result<Vec<f32>> {
        match &self.compute {
            SemanticComputeBackend::InProcess(embed) => embed.embed(text).await,
        }
    }

    async fn search_via_rest(
        &self,
        vector: Vec<f32>,
        limit: u64,
        threshold: f32,
    ) -> Result<Vec<SearchResult>> {
        let rest_url = rest_qdrant_url(&self.config.qdrant_url);
        let url = format!(
            "{}/collections/{}/points/search",
            rest_url, self.config.collection
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("Failed to build REST fallback client")?;

        let response = client
            .post(&url)
            .json(&json!({
                "vector": vector,
                "limit": limit,
                "with_payload": true,
                "score_threshold": threshold
            }))
            .send()
            .await
            .context("Failed to call Qdrant REST search")?;

        let response = response
            .error_for_status()
            .context("Qdrant REST search returned error status")?;

        let body: RestSearchResponse = response
            .json()
            .await
            .context("Failed to decode Qdrant REST search response")?;

        Ok(body.result.into_iter().map(search_result_from_rest_point).collect())
    }
}

pub async fn create_semantic_client(config: SemanticConfig) -> Result<Arc<SemanticClient>> {
    let client = SemanticClient::connect(config).await?;
    Ok(Arc::new(client))
}

fn event_point_id(event: &Event) -> u64 {
    let ulid_bytes = event.id.0.to_bytes();
    u64::from_be_bytes([
        ulid_bytes[0],
        ulid_bytes[1],
        ulid_bytes[2],
        ulid_bytes[3],
        ulid_bytes[4],
        ulid_bytes[5],
        ulid_bytes[6],
        ulid_bytes[7],
    ])
}

fn build_payload(
    event: &Event,
    envelope: Option<&MemoryEnvelope>,
) -> HashMap<String, qdrant_client::qdrant::Value> {
    use qdrant_client::qdrant::Value;

    let mut payload: HashMap<String, Value> = HashMap::new();
    payload.insert("event_id".to_string(), event.id.to_string().into());
    payload.insert("kind".to_string(), format!("{:?}", event.kind).into());
    payload.insert("description".to_string(), event.description.clone().into());
    payload.insert("timestamp".to_string(), event.ts.to_rfc3339().into());
    payload.insert(
        "project".to_string(),
        event.project_id.clone().unwrap_or_default().into(),
    );
    payload.insert("importance".to_string(), (event.importance as f64).into());

    if let Some(envelope) = envelope {
        payload.insert(
            "truth_status".to_string(),
            format!("{:?}", envelope.truth_status).to_lowercase().into(),
        );
        payload.insert(
            "memory_source".to_string(),
            format!("{:?}", envelope.source).to_lowercase().into(),
        );
        payload.insert(
            "memory_scope".to_string(),
            format!("{:?}", envelope.scope).to_lowercase().into(),
        );
        payload.insert(
            "promotion_status".to_string(),
            format!("{:?}", envelope.promotion_status)
                .to_lowercase()
                .into(),
        );
        payload.insert(
            "confidence".to_string(),
            (envelope.confidence as f64).into(),
        );
        payload.insert(
            "promotion_targets".to_string(),
            envelope
                .promotion_targets
                .iter()
                .map(|target| format!("{:?}", target).to_lowercase())
                .collect::<Vec<_>>()
                .join(",")
                .into(),
        );
        payload.insert(
            "module_id".to_string(),
            envelope.module_id.clone().unwrap_or_default().into(),
        );
        payload.insert(
            "memory_kind".to_string(),
            format!("{:?}", envelope.memory_kind).to_lowercase().into(),
        );
        payload.insert("file_refs".to_string(), envelope.file_refs.join(",").into());
        payload.insert(
            "symbol_refs".to_string(),
            envelope.symbol_refs.join(",").into(),
        );
        payload.insert(
            "logic_tags".to_string(),
            envelope.logic_tags.join(",").into(),
        );
    }

    payload
}

fn search_result_from_point(point: qdrant_client::qdrant::ScoredPoint) -> SearchResult {
    SearchResult {
        id: point.id.map(|id| format!("{:?}", id)).unwrap_or_default(),
        score: point.score,
        event_id: point.payload.get("event_id").and_then(extract_string),
        kind: point.payload.get("kind").and_then(extract_string),
        description: point.payload.get("description").and_then(extract_string),
        timestamp: point.payload.get("timestamp").and_then(extract_string),
        importance: point.payload.get("importance").and_then(extract_f32),
    }
}

fn extract_string(v: &qdrant_client::qdrant::Value) -> Option<String> {
    use qdrant_client::qdrant::value::Kind;
    v.kind.as_ref().and_then(|kind| match kind {
        Kind::StringValue(value) => Some(value.clone()),
        _ => None,
    })
}

fn extract_f32(v: &qdrant_client::qdrant::Value) -> Option<f32> {
    match &v.kind {
        Some(qdrant_client::qdrant::value::Kind::DoubleValue(value)) => Some(*value as f32),
        Some(qdrant_client::qdrant::value::Kind::IntegerValue(value)) => Some(*value as f32),
        _ => None,
    }
}

fn rest_qdrant_url(grpc_url: &str) -> String {
    grpc_url
        .trim_end_matches('/')
        .replacen(":6334", ":6333", 1)
}

#[derive(Debug, Deserialize)]
struct RestSearchResponse {
    result: Vec<RestScoredPoint>,
}

#[derive(Debug, Deserialize)]
struct RestScoredPoint {
    #[allow(dead_code)]
    id: JsonValue,
    score: f32,
    #[serde(default)]
    payload: HashMap<String, JsonValue>,
}

fn search_result_from_rest_point(point: RestScoredPoint) -> SearchResult {
    SearchResult {
        id: String::new(),
        score: point.score,
        event_id: point
            .payload
            .get("event_id")
            .and_then(extract_rest_string),
        kind: point.payload.get("kind").and_then(extract_rest_string),
        description: point
            .payload
            .get("description")
            .and_then(extract_rest_string),
        timestamp: point
            .payload
            .get("timestamp")
            .and_then(extract_rest_string),
        importance: point.payload.get("importance").and_then(extract_rest_f32),
    }
}

fn extract_rest_string(v: &JsonValue) -> Option<String> {
    v.as_str().map(ToOwned::to_owned)
}

fn extract_rest_f32(v: &JsonValue) -> Option<f32> {
    v.as_f64()
        .map(|value| value as f32)
        .or_else(|| v.as_i64().map(|value| value as f32))
}

/// Calculate freshness score based on timestamp
/// Returns 1.0 for today, decays to 0.5 at 30 days, min 0.1
fn calculate_freshness(timestamp: &Option<String>) -> f32 {
    match timestamp {
        Some(ts) => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
                let age = Utc::now().signed_duration_since(dt.with_timezone(&Utc));
                let days = age.num_days() as f32;
                (0.5_f32).powf(days / 30.0).max(0.1)
            } else {
                0.5
            }
        }
        None => 0.5,
    }
}
