//! # LLM Enricher — Codex-powered event metadata extraction
//!
//! Uses vertex-gateway to call Codex and extract structured metadata from events.
//! The enrichment result is written to `MemoryEnvelope` via `revise()`,
//! **never** modifying the immutable ledger event.

use crate::error::Result;
use crate::llm_client::{LlmClient, Message, MessagesRequest};
use crate::types::Event;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Structured metadata extracted by Codex from an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnrichment {
    /// Project identifier (e.g., "quiron-brain", "gloriosa", "quantum")
    #[serde(default)]
    pub project: Option<String>,
    /// Module or folder path (e.g., "src/semantic/sync")
    #[serde(default)]
    pub module_path: Option<String>,
    /// Absolute or relative file paths mentioned or involved
    #[serde(default)]
    pub files: Vec<String>,
    /// Code symbols: functions, structs, traits, methods
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Semantic tags for categorization and search
    #[serde(default)]
    pub tags: Vec<String>,
    /// Importance override (0.0–1.0), if Codex thinks the heuristic is off
    #[serde(default)]
    pub importance: Option<f32>,
    /// One-line distilled summary of what this event actually means
    #[serde(default)]
    pub summary: Option<String>,
}

/// Batch enrichment response from Codex.
#[derive(Debug, Serialize, Deserialize)]
struct EnrichmentBatchResponse {
    enrichments: Vec<EventEnrichmentEntry>,
}

/// One entry pairing event id with its enrichment.
#[derive(Debug, Serialize, Deserialize)]
struct EventEnrichmentEntry {
    event_id: String,
    #[serde(flatten)]
    enrichment: EventEnrichment,
}

pub struct LlmEnricher {
    client: Arc<LlmClient>,
    model: Option<String>,
}

impl LlmEnricher {
    pub fn new(client: Arc<LlmClient>) -> Self {
        Self {
            client,
            model: enricher_model_from_env(),
        }
    }

    /// Enrich a batch of events via Codex. Returns (event_id, enrichment) pairs.
    /// Events that fail to enrich are silently skipped (no partial failure).
    pub async fn enrich_batch(&self, events: &[Event]) -> Result<Vec<(String, EventEnrichment)>> {
        if events.is_empty() {
            return Ok(vec![]);
        }

        let events_text = Self::format_events(events);

        let system_prompt = ENRICHMENT_SYSTEM_PROMPT;

        let user_prompt = format!(
            "Enrich the following {} events:\n\n{}",
            events.len(),
            events_text
        );

        let req = MessagesRequest {
            provider: None,
            reasoning_effort: None,
            model: self.model.clone().unwrap_or_default(),
            route: Some("worker".to_string()),
            messages: vec![Message {
                role: "user".to_string(),
                content: user_prompt,
            }],
            max_tokens: 3000,
            temperature: Some(0.1),
            system: Some(system_prompt.to_string()),
            stream: Some(false),
            tools: Vec::new(),
            items: Vec::new(),
        };

        match self.client.send_message(&req).await {
            Ok(response) => {
                let content = if let Some(block) = response.content.first() {
                    block.text.clone()
                } else {
                    return Ok(vec![]);
                };

                Self::parse_enrichment_response(&content)
            }
            Err(e) => {
                tracing::error!("LLM enrichment call failed: {}", e);
                Ok(vec![])
            }
        }
    }

    fn format_events(events: &[Event]) -> String {
        let mut text = String::new();
        for event in events {
            text.push_str(&format!(
                "EVENT_ID: {}\nKIND: {:?}\nDESCRIPTION: {}\nPROJECT: {}\nINPUTS: {}\nOUTPUTS: {}\nTAGS: {}\n---\n",
                event.id,
                event.kind,
                event.description,
                event.project_id.as_deref().unwrap_or("unknown"),
                event.inputs.join(", "),
                event.outputs.join(", "),
                event.tags.join(", "),
            ));
        }
        text
    }

    fn parse_enrichment_response(content: &str) -> Result<Vec<(String, EventEnrichment)>> {
        // Extract JSON block from LLM response (may be wrapped in markdown code fence)
        let json_str = extract_json_block(content);

        match serde_json::from_str::<EnrichmentBatchResponse>(json_str) {
            Ok(parsed) => Ok(parsed
                .enrichments
                .into_iter()
                .map(|entry| (entry.event_id, entry.enrichment))
                .collect()),
            Err(e) => {
                tracing::warn!(
                    "Failed to parse LLM enrichment JSON: {} — raw: {}",
                    e,
                    &content[..content.len().min(200)]
                );
                Ok(vec![])
            }
        }
    }
}

/// Extract JSON block from LLM response, handling markdown fences.
fn extract_json_block(content: &str) -> &str {
    // Try to find ```json ... ``` first
    if let Some(start) = content.find("```json") {
        let after = &content[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    // Fallback: find outermost { ... }
    if let Some(start) = content.find('{') {
        if let Some(end) = content.rfind('}') {
            return &content[start..=end];
        }
    }
    content
}

fn enricher_model_from_env() -> Option<String> {
    std::env::var("QUIRON_LLM_MODEL_ENRICHER")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            // Fallback to distiller model (same tier of work)
            std::env::var("QUIRON_LLM_MODEL_DISTILLER")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
}

const ENRICHMENT_SYSTEM_PROMPT: &str = r#"You are Quirón's Memory Enricher. Your job is to analyze raw events from a software engineering memory ledger and extract structured metadata.

For each event, extract:
- **project**: Which project this belongs to (e.g., "quiron-brain", "gloriosa", "quantum", "openclaw"). Use "unknown" if unclear.
- **module_path**: The module or folder path if identifiable (e.g., "src/semantic/sync", "src/api/server").
- **files**: Any file paths mentioned or implied.
- **symbols**: Code symbols: function names, struct names, trait names (e.g., "SemanticClient::connect", "EmbedService").
- **tags**: 3-5 semantic tags for categorization (e.g., ["neo4j", "performance", "refactoring", "async"]).
- **importance**: Float 0.0-1.0. Rate based on long-term value: architectural decisions=0.9, routine logs=0.3, bugs found=0.8.
- **summary**: One concise sentence capturing the essence.

Output ONLY valid JSON matching this schema:
{
  "enrichments": [
    {
      "event_id": "<the EVENT_ID>",
      "project": "...",
      "module_path": "...",
      "files": ["..."],
      "symbols": ["..."],
      "tags": ["..."],
      "importance": 0.7,
      "summary": "..."
    }
  ]
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_enrichment() {
        let json = r#"{"enrichments":[{"event_id":"ABC123","project":"quiron-brain","module_path":"src/semantic","files":["src/semantic/sync.rs"],"symbols":["SemanticSyncService"],"tags":["qdrant","sync","performance"],"importance":0.7,"summary":"Fixed sync blocking issue"}]}"#;
        let result = LlmEnricher::parse_enrichment_response(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "ABC123");
        assert_eq!(result[0].1.project, Some("quiron-brain".to_string()));
        assert_eq!(result[0].1.tags.len(), 3);
    }

    #[test]
    fn parse_wrapped_in_markdown_fence() {
        let content = "Here is the enrichment:\n```json\n{\"enrichments\":[{\"event_id\":\"X\",\"tags\":[\"test\"]}]}\n```\n";
        let result = LlmEnricher::parse_enrichment_response(content).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "X");
    }

    #[test]
    fn parse_empty_returns_empty() {
        let result = LlmEnricher::parse_enrichment_response("no json here").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn extract_json_block_works() {
        assert_eq!(extract_json_block("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(
            extract_json_block("text ```json\n{\"a\":1}\n``` more"),
            "{\"a\":1}"
        );
        assert_eq!(extract_json_block("prefix {\"a\":1} suffix"), "{\"a\":1}");
    }
}
