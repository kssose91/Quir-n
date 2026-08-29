use crate::error::{Result, ServiceError};
use fastembed::{EmbeddingModel, RerankerModel, TextEmbedding, TextRerank};
use ort::ep::{CPUExecutionProvider, CUDAExecutionProvider};
use ort::execution_providers::ExecutionProviderDispatch;
use std::{env, net::SocketAddr, path::PathBuf, str::FromStr};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub device_label: String,
    pub cuda_device_id: i32,
    pub execution_provider_specs: Vec<ExecutionProviderSpec>,
    pub execution_provider_summary: Vec<String>,
    pub embed_model: EmbeddingModel,
    pub embed_model_code: String,
    pub embed_dimension: usize,
    pub rerank_model: RerankerModel,
    pub rerank_model_code: String,
    pub model_cache_dir: PathBuf,
    pub show_download_progress: bool,
    pub warmup: bool,
    pub max_embed_batch_size: usize,
    pub max_rerank_batch_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionProviderSpec {
    Cuda { device_id: i32 },
    Cpu,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind_addr = parse_bind_addr()?
            .parse::<SocketAddr>()
            .map_err(|e| ServiceError::Config(format!("invalid bind address: {e}")))?;
        let device_label = parse_env("SEMANTIC_IA_DEVICE_LABEL", "CUDA device 0");
        let cuda_device_id = parse_env("SEMANTIC_IA_CUDA_DEVICE_ID", "0")
            .parse::<i32>()
            .map_err(|e| ServiceError::Config(format!("invalid CUDA device id: {e}")))?;
        let execution_provider_specs = parse_execution_providers(
            &parse_env("SEMANTIC_IA_EXECUTION_PROVIDERS", "cuda,cpu"),
            cuda_device_id,
        )?;
        let execution_provider_summary = execution_provider_specs
            .iter()
            .map(|spec| match spec {
                ExecutionProviderSpec::Cuda { device_id } => format!("cuda:{device_id}"),
                ExecutionProviderSpec::Cpu => "cpu".to_string(),
            })
            .collect::<Vec<_>>();

        let embed_model = parse_embedding_model(&parse_env_alias(
            &[
                "SEMANTIC_IA_EMBED_MODEL",
                "QUIRON_SEMANTIC_IA_LOCAL_MODEL_EMBED",
            ],
            "BAAI/bge-m3",
        ))?;
        let embed_model_for_info = embed_model.clone();
        let embed_info = TextEmbedding::get_model_info(&embed_model_for_info)
            .map_err(|err| ServiceError::Config(err.to_string()))?;
        let rerank_model = parse_rerank_model(&parse_env_alias(
            &[
                "SEMANTIC_IA_RERANK_MODEL",
                "QUIRON_SEMANTIC_IA_LOCAL_MODEL_RERANK",
            ],
            "rozgo/bge-reranker-v2-m3",
        ))?;
        let rerank_info = TextRerank::get_model_info(&rerank_model);

        let model_cache_dir = parse_env(
            "SEMANTIC_IA_MODEL_CACHE_DIR",
            "/home/kssose/.cache/semantic-ia-local",
        )
        .into();
        let show_download_progress = parse_bool("SEMANTIC_IA_SHOW_DOWNLOAD_PROGRESS", false);
        let warmup = parse_bool("SEMANTIC_IA_WARMUP", false);
        let max_embed_batch_size = parse_env("SEMANTIC_IA_MAX_EMBED_BATCH_SIZE", "128")
            .parse::<usize>()
            .map_err(|e| ServiceError::Config(format!("invalid embed batch size: {e}")))?;
        let max_rerank_batch_size = parse_env("SEMANTIC_IA_MAX_RERANK_BATCH_SIZE", "32")
            .parse::<usize>()
            .map_err(|e| ServiceError::Config(format!("invalid rerank batch size: {e}")))?;

        Ok(Self {
            bind_addr,
            device_label,
            cuda_device_id,
            execution_provider_specs,
            execution_provider_summary,
            embed_model,
            embed_model_code: embed_info.model_code.clone(),
            embed_dimension: embed_info.dim,
            rerank_model,
            rerank_model_code: rerank_info.model_code.clone(),
            model_cache_dir,
            show_download_progress,
            warmup,
            max_embed_batch_size,
            max_rerank_batch_size,
        })
    }

    pub fn execution_providers(&self) -> Vec<ExecutionProviderDispatch> {
        self.execution_provider_specs
            .iter()
            .map(|spec| match spec {
                ExecutionProviderSpec::Cuda { device_id } => CUDAExecutionProvider::default()
                    .with_device_id(*device_id)
                    .build(),
                ExecutionProviderSpec::Cpu => CPUExecutionProvider::default().build(),
            })
            .collect()
    }
}

fn parse_env(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn parse_env_alias(keys: &[&str], fallback: &str) -> String {
    for key in keys {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }

    fallback.to_string()
}

fn parse_bind_addr() -> Result<String> {
    if let Ok(value) = env::var("SEMANTIC_IA_BIND_ADDR") {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }

    if let Ok(endpoint) = env::var("QUIRON_SEMANTIC_IA_LOCAL_ENDPOINT") {
        let endpoint = endpoint.trim().trim_end_matches('/');
        if let Some(rest) = endpoint
            .strip_prefix("http://")
            .or_else(|| endpoint.strip_prefix("https://"))
        {
            return Ok(rest.to_string());
        }
    }

    if let Ok(port) = env::var("QUIRON_SEMANTIC_IA_LOCAL_PORT") {
        let port = port.trim();
        if !port.is_empty() {
            return Ok(format!("127.0.0.1:{port}"));
        }
    }

    Ok("127.0.0.1:8091".to_string())
}

fn parse_bool(key: &str, fallback: bool) -> bool {
    match env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => fallback,
        },
        Err(_) => fallback,
    }
}

fn parse_execution_providers(
    raw: &str,
    default_cuda_device_id: i32,
) -> Result<Vec<ExecutionProviderSpec>> {
    let mut specs = Vec::new();
    let normalized = raw.trim().to_ascii_lowercase();
    let tokens = if normalized.is_empty() || normalized == "auto" {
        vec!["cuda", "cpu"]
    } else {
        normalized
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>()
    };

    for token in tokens {
        let spec = if token == "cpu" {
            ExecutionProviderSpec::Cpu
        } else if token == "cuda" {
            ExecutionProviderSpec::Cuda {
                device_id: default_cuda_device_id,
            }
        } else if let Some(rest) = token.strip_prefix("cuda:") {
            let device_id = rest.parse::<i32>().map_err(|e| {
                ServiceError::Config(format!("invalid cuda device id in provider list: {e}"))
            })?;
            ExecutionProviderSpec::Cuda { device_id }
        } else {
            return Err(ServiceError::Config(format!(
                "unknown execution provider '{token}'"
            )));
        };

        if !specs.contains(&spec) {
            specs.push(spec);
        }
    }

    if specs.is_empty() {
        specs.push(ExecutionProviderSpec::Cuda {
            device_id: default_cuda_device_id,
        });
        specs.push(ExecutionProviderSpec::Cpu);
    }

    Ok(specs)
}

fn parse_embedding_model(raw: &str) -> Result<EmbeddingModel> {
    if let Ok(model) = EmbeddingModel::from_str(raw.trim()) {
        return Ok(model);
    }

    let lowered = raw.trim().to_ascii_lowercase();
    let model = match lowered.as_str() {
        "baai/bge-m3" | "bge-m3" => EmbeddingModel::BGEM3,
        "baai/bge-large-en-v1.5" | "bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
        "baai/bge-base-en-v1.5" | "bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
        "baai/bge-small-en-v1.5" | "bge-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
        "intfloat/multilingual-e5-large" | "multilingual-e5-large" => {
            EmbeddingModel::MultilingualE5Large
        }
        "intfloat/multilingual-e5-base" | "multilingual-e5-base" => {
            EmbeddingModel::MultilingualE5Base
        }
        "intfloat/multilingual-e5-small" | "multilingual-e5-small" => {
            EmbeddingModel::MultilingualE5Small
        }
        "alibaba-nlp/gte-large-en-v1.5" | "gte-large-en-v1.5" => EmbeddingModel::GTELargeENV15,
        "alibaba-nlp/gte-base-en-v1.5" | "gte-base-en-v1.5" => EmbeddingModel::GTEBaseENV15,
        _ => {
            return Err(ServiceError::Config(format!(
                "unknown embedding model '{raw}'"
            )));
        }
    };

    Ok(model)
}

fn parse_rerank_model(raw: &str) -> Result<RerankerModel> {
    if let Ok(model) = RerankerModel::from_str(raw.trim()) {
        return Ok(model);
    }

    let lowered = raw.trim().to_ascii_lowercase();
    let model = match lowered.as_str() {
        "bge-reranker-v2-m3" | "rozgo/bge-reranker-v2-m3" => RerankerModel::BGERerankerV2M3,
        "bge-reranker-base" | "baai/bge-reranker-base" => RerankerModel::BGERerankerBase,
        "jina-reranker-v1-turbo-en" | "jinaai/jina-reranker-v1-turbo-en" => {
            RerankerModel::JINARerankerV1TurboEn
        }
        "jina-reranker-v2-base-multilingual" | "jinaai/jina-reranker-v2-base-multilingual" => {
            RerankerModel::JINARerankerV2BaseMultiligual
        }
        _ => {
            return Err(ServiceError::Config(format!(
                "unknown rerank model '{raw}'"
            )));
        }
    };

    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bge_embedding_alias() {
        assert_eq!(
            parse_embedding_model("bge-m3").unwrap(),
            EmbeddingModel::BGEM3
        );
    }

    #[test]
    fn parses_rerank_alias() {
        assert_eq!(
            parse_rerank_model("rozgo/bge-reranker-v2-m3").unwrap(),
            RerankerModel::BGERerankerV2M3
        );
    }
}
