//! # Chat Panel
//!
//! Panel de chat que conecta con quiron-brain.
//!
//! > **Arquitectura**: UI efímera → Quirón (orchestrator) → quiron-brain:8766

use llore_brain::orchestrator::ResponseKind;
use llore_brain::{ClientError, DelegationMetrics, OrchestratorResult, Quiron, QuironConfig};

/// Mensaje en el historial de chat
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// true = usuario, false = Quirón
    pub is_user: bool,
    /// Contenido del mensaje
    pub content: String,
}

/// Estado del chat
pub struct ChatState {
    /// Runtime cognitivo completo de Quirón
    quiron: Quiron,
    /// Historial de mensajes
    pub messages: Vec<ChatMessage>,
    /// Si está esperando respuesta
    pub loading: bool,
}

impl ChatState {
    pub fn new() -> Self {
        Self {
            quiron: Quiron::new(QuironConfig::default()),
            messages: Vec::new(),
            loading: false,
        }
    }

    /// Envía un mensaje a Quirón
    pub async fn send_message(&mut self, content: &str) -> Result<String, ClientError> {
        // Añadir mensaje del usuario
        self.messages.push(ChatMessage {
            is_user: true,
            content: content.to_string(),
        });

        self.loading = true;

        // Ejecutar pipeline real (deliberación + gates + recall + LLM via gateway)
        let result = self.quiron.process(content).await;
        let metrics = self.quiron.delegation_metrics();
        let response = format_orchestrator_result(result, &metrics);

        // Añadir respuesta
        self.messages.push(ChatMessage {
            is_user: false,
            content: response.clone(),
        });

        self.loading = false;

        Ok(response)
    }

    /// Verifica conexión con quiron-brain
    pub async fn check_health(&self) -> bool {
        self.quiron.check_health().await
    }
}

impl Default for ChatState {
    fn default() -> Self {
        Self::new()
    }
}

fn format_metrics_tag(metrics: &DelegationMetrics) -> String {
    let w_ewma = metrics
        .worker_tokens_ewma
        .map(|v| format!("{:.0}", v))
        .unwrap_or_else(|| "-".to_string());
    let p_ewma = metrics
        .primary_tokens_ewma
        .map(|v| format!("{:.0}", v))
        .unwrap_or_else(|| "-".to_string());
    format!(
        "budget={}/{} rem={} fb={:.2} par={}/{} w_ewma={} p_ewma={} min_w={} min_p={} scale_w={:.2} scale_p={:.2}",
        metrics.model_tokens_used,
        metrics.token_budget,
        metrics.token_budget_remaining,
        metrics.llm_fallback_rate,
        metrics.parallel_subtasks_current,
        metrics.parallel_subtasks_cap,
        w_ewma,
        p_ewma,
        metrics.worker_min_tokens_threshold,
        metrics.primary_min_tokens_threshold,
        metrics.worker_threshold_scale,
        metrics.primary_threshold_scale
    )
}

fn format_orchestrator_result(result: OrchestratorResult, metrics: &DelegationMetrics) -> String {
    let metrics_tag = format_metrics_tag(metrics);
    match result {
        OrchestratorResult::Complete {
            response,
            iterations,
            tokens_used,
        } => match response {
            ResponseKind::VerifiedClaim {
                content,
                evidence,
                confidence,
            } => format!(
                "{}\n\n[verified | evidence={} | conf={:.2} | iter={} | tok={} | {}]",
                content,
                evidence.len(),
                confidence,
                iterations,
                tokens_used,
                metrics_tag.as_str()
            ),
            ResponseKind::Proposal {
                content,
                citations,
                ..
            } => format!(
                "{}\n\n[proposal | citations={} | iter={} | tok={} | {}]",
                content,
                citations.len(),
                iterations,
                tokens_used,
                metrics_tag.as_str()
            ),
            ResponseKind::Hypothesis {
                content,
                plausibility,
                what_would_verify,
            } => format!(
                "{}\n\n[hypothesis | plaus={:.2} | verify_items={} | iter={} | tok={} | {}]",
                content,
                plausibility,
                what_would_verify.len(),
                iterations,
                tokens_used,
                metrics_tag.as_str()
            ),
            ResponseKind::NeedMoreContext {
                reason,
                what_missing,
                attempts,
            } => format!(
                "{}\n\n[need_more_context | missing={} | attempts={} | iter={} | tok={} | {}]",
                reason,
                what_missing.len(),
                attempts.len(),
                iterations,
                tokens_used,
                metrics_tag.as_str()
            ),
            ResponseKind::NoEvidence {
                attempts,
                scope_was_bounded,
                confidence_in_absence,
            } => format!(
                "No encontré información sobre eso.\n\n[no_evidence | attempts={} | bounded={} | conf_abs={} | iter={} | tok={} | {}]",
                attempts.len(),
                scope_was_bounded,
                confidence_in_absence
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "-".to_string()),
                iterations,
                tokens_used,
                metrics_tag.as_str()
            ),
        },
        OrchestratorResult::NeedsUserInput { question, options } => format!(
            "{}\n\n[needs_user_input | options={} | {}]",
            question,
            options.as_ref().map(|v| v.len()).unwrap_or(0),
            metrics_tag.as_str()
        ),
        OrchestratorResult::AskUserToContinue {
            iterations,
            question,
            ..
        } => format!(
            "{}\n\n[ask_user_continue | iter={} | {}]",
            question,
            iterations,
            metrics_tag.as_str()
        ),
        OrchestratorResult::TokenBudgetExhausted { tokens_used, .. } => format!(
            "Límite de tokens alcanzado.\n\n[token_budget_exhausted | tok={} | {}]",
            tokens_used,
            metrics_tag.as_str()
        ),
    }
}
