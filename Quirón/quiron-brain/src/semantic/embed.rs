//! Local embedding runtime in Rust.

use anyhow::{anyhow, bail, Context, Result};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

pub struct EmbedService {
    model: Arc<Mutex<TextEmbedding>>,
    model_name: String,
    dimension: usize,
}

impl EmbedService {
    pub fn new(requested_model: &str) -> Result<Self> {
        let model = parse_embedding_model(requested_model)?;
        let info = TextEmbedding::get_model_info(&model)
            .context("failed to resolve embedding model metadata")?;
        let model_name = info.model_code.clone();
        let dimension = info.dim;
        let instance =
            TextEmbedding::try_new(TextInitOptions::new(model).with_show_download_progress(false))
                .with_context(|| format!("failed to initialize embedding model {model_name}"))?;

        Ok(Self {
            model: Arc::new(Mutex::new(instance)),
            model_name,
            dimension,
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let text = text.to_string();
        let model = Arc::clone(&self.model);
        tokio::task::spawn_blocking(move || {
            let mut model = model
                .lock()
                .map_err(|_| anyhow!("embedding model mutex poisoned"))?;
            let vectors = model
                .embed(vec![text], Some(1))
                .context("failed to generate embedding")?;
            vectors
                .into_iter()
                .next()
                .context("embedding model returned no vector")
        })
        .await
        .context("embedding task panicked")?
    }

    pub async fn health_check(&self) -> bool {
        !self.model_name.is_empty() && self.dimension > 0
    }
}

fn parse_embedding_model(raw: &str) -> Result<EmbeddingModel> {
    if let Ok(model) = EmbeddingModel::from_str(raw.trim()) {
        return Ok(model);
    }

    let lowered = raw.trim().to_ascii_lowercase();
    let model = match lowered.as_str() {
        "baai/bge-large-en-v1.5" | "bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
        "qdrant/bge-large-en-v1.5-onnx-q" | "bge-large-en-v1.5-q" => EmbeddingModel::BGELargeENV15Q,
        "baai/bge-base-en-v1.5" | "bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
        "qdrant/bge-base-en-v1.5-onnx-q" | "bge-base-en-v1.5-q" => EmbeddingModel::BGEBaseENV15Q,
        "baai/bge-small-en-v1.5" | "bge-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
        "qdrant/bge-small-en-v1.5-onnx-q" | "bge-small-en-v1.5-q" => EmbeddingModel::BGESmallENV15Q,
        "baai/bge-m3" | "bge-m3" => EmbeddingModel::BGEM3,
        _ => {
            bail!(
                "unknown embedding model '{}'; use a fastembed enum name or a supported BGE identifier",
                raw
            );
        }
    };

    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_bge_large_name() {
        let model = parse_embedding_model("BAAI/bge-large-en-v1.5").unwrap();
        assert_eq!(model, EmbeddingModel::BGELargeENV15);
    }

    #[test]
    fn parses_fastembed_enum_name() {
        let model = parse_embedding_model("BGESmallENV15").unwrap();
        assert_eq!(model, EmbeddingModel::BGESmallENV15);
    }
}
