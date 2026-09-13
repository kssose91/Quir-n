use crate::error::Result;
use crate::llm_client::{LlmClient, Message, MessagesRequest};
use crate::types::Event;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct DistilledInsight {
    pub title: String,
    pub description: String,
    pub kind: String, // e.g., "Claim", "Decision", "Pattern", "Bug"
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DistillationResult {
    pub insights: Vec<DistilledInsight>,
}

pub struct LlmDistiller {
    client: Arc<LlmClient>,
    model: Option<String>,
}

impl LlmDistiller {
    pub fn new(client: Arc<LlmClient>) -> Self {
        Self {
            client,
            model: distiller_model_from_env(),
        }
    }

    pub async fn distill_events(&self, events: &[Event]) -> Result<Vec<DistilledInsight>> {
        if events.is_empty() {
            return Ok(vec![]);
        }

        // Prepare the events digest
        let mut events_text = String::new();
        for event in events {
            events_text.push_str(&format!(
                "[{}] {:?}: {} (Inputs: {:?}, Outputs: {:?})\n",
                event.ts.format("%Y-%m-%d %H:%M:%S"),
                event.kind,
                event.description,
                event.inputs,
                event.outputs
            ));
        }

        let system_prompt = "You are Quirón's Asynchronous Cognitive Distiller. \
            Your job is to read raw telemetry and editor events (Sled Ledger) and distill them into \
            valuable, structured architectural insights, claims, and decisions. \
            Ignore noise (like typos or meaningless fast edits). Focus on \"Eureka\" moments, \
            architectural shifts, identified bugs, or confirmed decisions. \
            Output ONLY valid JSON matching this schema: \
            { \"insights\": [ { \"title\": \"...\", \"description\": \"...\", \"kind\": \"Claim|Decision|Pattern|Bug\", \"tags\": [\"...\"] } ] }";

        let user_prompt = format!(
            "Distill the following raw events into insights:\n\n{}",
            events_text
        );

        let req = MessagesRequest {
            provider: None,
            reasoning_effort: None,
            // Si no se configura un modelo explícito, delegamos la resolución al gateway.
            model: self.model.clone().unwrap_or_default(),
            route: Some("primary".to_string()),
            messages: vec![Message {
                role: "user".to_string(),
                content: user_prompt,
            }],
            max_tokens: 2000,
            temperature: Some(0.2), // Low temp for structured extraction
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

                // Naive extraction of JSON block
                let json_str = if let Some(start) = content.find('{') {
                    if let Some(end) = content.rfind('}') {
                        &content[start..=end]
                    } else {
                        &content
                    }
                } else {
                    &content
                };

                match serde_json::from_str::<DistillationResult>(json_str) {
                    Ok(parsed) => Ok(parsed.insights),
                    Err(e) => {
                        tracing::warn!("Failed to parse LLM distillation JSON: {}", e);
                        Ok(vec![])
                    }
                }
            }
            Err(e) => {
                tracing::error!("LLM Distillation failed: {}", e);
                Ok(vec![])
            }
        }
    }
}

fn distiller_model_from_env() -> Option<String> {
    normalized_distiller_model(std::env::var("QUIRON_LLM_MODEL_DISTILLER").ok())
}

fn normalized_distiller_model(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_distiller_model_ignores_blank_values() {
        assert_eq!(
            normalized_distiller_model(Some(" gemini-2.5-pro ".to_string())),
            Some("gemini-2.5-pro".to_string())
        );
        assert_eq!(normalized_distiller_model(Some("   ".to_string())), None);
        assert_eq!(normalized_distiller_model(None), None);
    }
}
