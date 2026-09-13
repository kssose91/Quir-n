//! # Cliente HTTP para quiron-brain
//!
//! Conecta llore_brain con quiron-brain API.
//! Este es el puente que hace que Quirón sea el gateway obligatorio.

// Nota: Request, Response, ResponseStatus se usarán cuando se implemente el protocolo completo
#[allow(unused_imports)]
use crate::protocol::{Request, Response, ResponseStatus, WorkerResult, WorkerTask};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// URL base de quiron-brain (local por defecto)
const DEFAULT_BASE_URL: &str = "http://localhost:8766";
const API_TOKEN_ENV: &str = "QUIRON_API_TOKEN";
const PRIMARY_MODEL_ENV: &str = "QUIRON_LLM_MODEL_PRIMARY";
const WORKER_MODEL_ENV: &str = "QUIRON_LLM_MODEL_WORKER";
const FALLBACK_MODEL_ENV: &str = "QUIRON_LLM_MODEL";
const DEFAULT_LLM_MODEL: &str = "gpt-5.6-sol";

/// Cliente HTTP para quiron-brain
#[derive(Clone)]
pub struct QuironClient {
    /// URL base de quiron-brain
    pub base_url: String,
    timeout: Duration,
    api_token: Option<String>,
    project_id: Option<String>,
    /// Tope de tokens de salida por turno del chat (la barra lo cambia).
    response_max_tokens: u32,
    reasoning_effort: Option<String>,
    chat_provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IndexProgress {
    pub project_id: String,
    pub phase: String,
    pub files_total: usize,
    pub files_done: usize,
    pub units_written: usize,
    pub summaries_generated: usize,
    pub current_path: String,
    pub error: Option<String>,
}

impl QuironClient {
    /// Crear cliente con URL por defecto
    pub fn new() -> Self {
        Self::with_url_and_token(DEFAULT_BASE_URL, env_api_token())
    }

    /// Crear cliente con URL personalizada
    pub fn with_url(base_url: &str) -> Self {
        Self::with_url_and_token(base_url, env_api_token())
    }

    /// Crear cliente con URL y token explícitos.
    pub fn with_url_and_token(base_url: &str, api_token: Option<String>) -> Self {
        Self {
            base_url: base_url.to_string(),
            timeout: Duration::from_secs(30),
            api_token: api_token
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
            project_id: None,
            response_max_tokens: 4096,
            reasoning_effort: None,
            chat_provider: None,
        }
    }

    pub fn with_project_id(mut self, project_id: String) -> Self {
        self.set_project_id(Some(project_id));
        self
    }

    /// Fija la identidad del proyecto activo.
    ///
    /// El cliente se construye antes de que exista un proyecto: el editor
    /// arranca sin carpeta abierta. Conceder acceso a una carpeta resuelve su
    /// identificador y lo instala aquí.
    pub fn set_project_id(&mut self, project_id: Option<String>) {
        self.project_id = project_id.filter(|value| !value.trim().is_empty());
    }

    pub fn set_reasoning_effort(&mut self, effort: Option<String>) {
        self.reasoning_effort = effort;
    }

    pub fn set_chat_provider(&mut self, provider: Option<String>) {
        self.chat_provider = provider;
    }

    /// Tope de tokens de salida por turno (corta / normal / larga).
    pub fn set_response_max_tokens(&mut self, max_tokens: u32) {
        self.response_max_tokens = max_tokens.clamp(256, 32_000);
    }

    pub fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }

    /// Apertura idempotente: mantiene el monitor del proyecto en el cerebro.
    pub async fn start_project_index(&self, root: &std::path::Path, project_id: &str) -> Result<IndexProgress, ClientError> {
        self.post(&format!("{}/index/project", self.base_url),
            &serde_json::json!({"root":root,"project_id":project_id})).await
    }

    /// Pide al cerebro que pare el monitor del proyecto (conserva el índice).
    pub async fn stop_project_index(&self, project_id: &str) -> Result<(), ClientError> {
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let response = client.delete(format!("{}/index/project/{}", self.base_url, project_id));
        let response = self
            .apply_auth(response)
            .send()
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        if !response.status().is_success() {
            return Err(ClientError::Http {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    /// Obtener contexto de startup (identidad, gates, eventos recientes)
    pub async fn get_context(&self) -> Result<ContextResponse, ClientError> {
        let url = format!("{}/context", self.base_url);
        self.get(&url).await
    }

    /// Crear un evento en el ledger
    pub async fn create_event(
        &self,
        event: CreateEventRequest,
    ) -> Result<CreateEventResponse, ClientError> {
        let url = format!("{}/event", self.base_url);
        self.post(&url, &event).await
    }

    /// Escribir checkpoint de telemetría de sesión en namespace dedicado.
    pub async fn create_session_telemetry_checkpoint(
        &self,
        checkpoint: CreateSessionTelemetryCheckpointRequest,
    ) -> Result<TelemetryWriteResponse, ClientError> {
        let url = format!("{}/telemetry/session/checkpoint", self.base_url);
        self.post(&url, &checkpoint).await
    }

    /// Escribir anomalía de telemetría de sesión en namespace dedicado.
    pub async fn create_session_telemetry_anomaly(
        &self,
        anomaly: CreateSessionTelemetryAnomalyRequest,
    ) -> Result<TelemetryWriteResponse, ClientError> {
        let url = format!("{}/telemetry/session/anomaly", self.base_url);
        self.post(&url, &anomaly).await
    }

    /// Leer telemetría de una sesión (checkpoints + anomalías).
    pub async fn get_session_telemetry(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<SessionTelemetryResponse, ClientError> {
        self.get_session_telemetry_page(session_id, limit, 0, 0)
            .await
    }

    /// Leer una página de telemetría de sesión con offsets independientes.
    pub async fn get_session_telemetry_page(
        &self,
        session_id: &str,
        limit: usize,
        checkpoints_offset: usize,
        anomalies_offset: usize,
    ) -> Result<SessionTelemetryResponse, ClientError> {
        let url = format!(
            "{}/telemetry/session/{}?limit={}&checkpoints_offset={}&anomalies_offset={}",
            self.base_url, session_id, limit, checkpoints_offset, anomalies_offset
        );
        self.get(&url).await
    }

    /// Validar una acción contra los gates
    pub async fn validate_action(
        &self,
        action: ActionRequest,
    ) -> Result<ValidationResponse, ClientError> {
        let url = format!("{}/action/request", self.base_url);
        self.post(&url, &action).await
    }

    /// Verificar integridad de la cadena de hash
    pub async fn verify_chain(&self) -> Result<ChainVerifyResponse, ClientError> {
        let url = format!("{}/chain/verify", self.base_url);
        self.get(&url).await
    }

    /// Obtener eventos recientes
    pub async fn list_events(&self, limit: usize) -> Result<Vec<EventSummary>, ClientError> {
        let url = format!("{}/events?limit={}", self.base_url, limit);
        self.get(&url).await
    }

    /// Búsqueda semántica (si está habilitada)
    pub async fn search(&self, query: &str, limit: usize) -> Result<SearchResponse, ClientError> {
        let url = format!("{}/search?q={}&limit={}", self.base_url, query, limit);
        self.get(&url).await
    }

    /// Recall con Virtual Context Tools (respeta scope real del backend).
    pub async fn recall(
        &self,
        query: &str,
        limit: usize,
        scope: &str,
    ) -> Result<RecallResponse, ClientError> {
        let url = format!("{}/recall", self.base_url);
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        let response = self
            .apply_auth(client.get(&url))
            .query(&[
                ("q", query),
                ("limit", &limit.to_string()),
                ("scope", scope),
            ])
            .send()
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::Http {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        response
            .json()
            .await
            .map_err(|e| ClientError::Parse(e.to_string()))
    }

    /// Health check
    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        let url = format!("{}/health", self.base_url);
        self.get(&url).await
    }

    /// Llamar al LLM via quiron-brain /v1/messages
    ///
    /// quiron-brain actúa como proxy y llama al vertex-gateway (binario inmutable).
    /// Todo el tráfico LLM pasa por aquí — NUNCA conexión directa a internet.
    pub async fn send_to_llm(
        &self,
        prompt: &str,
        context: &str,
        system: Option<&str>,
        model: Option<&str>,
    ) -> Result<LlmResponse, ClientError> {
        let url = format!("{}/v1/messages", self.base_url);

        let mut system_sections = Vec::new();
        if let Some(system) = system.map(str::trim).filter(|value| !value.is_empty()) {
            system_sections.push(system.to_string());
        }
        if !context.trim().is_empty() {
            system_sections.push(format!("## Contexto del proyecto\n{}", context));
        }
        let full_system = (!system_sections.is_empty()).then(|| system_sections.join("\n\n"));

        let request = LlmRequest {
            model: model
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(preferred_primary_model),
            messages: vec![LlmMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            system: full_system,
            max_tokens: 4096,
            // Política Quirón: respuesta final siempre en ruta primary (LLM grande de pago).
            route: Some("primary".to_string()),
            worker_task: None,
            project_id: self.project_id.clone(),
        };

        self.post_llm_turn(&url, &request).await
    }

    /// Un turno del chat con herramientas: la conversación completa viaja cada
    /// vez (el cerebro no guarda estado de chat) y la respuesta puede pedir
    /// herramientas (`stop_reason: "tool_use"`) en lugar de cerrar con texto.
    ///
    /// Quien decide QUÉ se ejecuta no es este cliente: las llamadas vuelven al
    /// editor, que las pasa por el arnés. Aquí solo se transporta.
    pub async fn send_chat_turn(
        &self,
        messages: Vec<ChatTurnMessage>,
        tools: Vec<LlmToolDef>,
        context: &str,
        system: Option<&str>,
        model: Option<&str>,
    ) -> Result<LlmResponse, ClientError> {
        let url = format!("{}/v1/messages", self.base_url);

        let mut system_sections = Vec::new();
        if let Some(system) = system.map(str::trim).filter(|value| !value.is_empty()) {
            system_sections.push(system.to_string());
        }
        if !context.trim().is_empty() {
            system_sections.push(format!("## Contexto del proyecto\n{}", context));
        }
        let full_system = (!system_sections.is_empty()).then(|| system_sections.join("\n\n"));

        let request = ChatTurnRequest {
            provider: self.chat_provider.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            model: model
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(preferred_primary_model),
            messages,
            system: full_system,
            max_tokens: self.response_max_tokens,
            route: Some("primary".to_string()),
            project_id: self.project_id.clone(),
            tools,
        };

        self.post_llm_turn(&url, &request).await
    }

    /// Delegar tarea operativa al worker local (subordinado al planner).
    ///
    /// Reglas:
    /// - ruta `worker`
    /// - salida JSON `WorkerResult`
    /// - `claims` debe ser vacío (no-claim-from-worker)
    pub async fn send_to_worker(
        &self,
        task: &WorkerTask,
        prompt: &str,
        context: &str,
        system: Option<&str>,
    ) -> Result<WorkerResult, ClientError> {
        let out = self
            .send_to_worker_with_usage(task, prompt, context, system)
            .await?;
        Ok(out.result)
    }

    /// Igual que `send_to_worker`, pero retorna además tokens consumidos.
    pub async fn send_to_worker_with_usage(
        &self,
        task: &WorkerTask,
        prompt: &str,
        context: &str,
        system: Option<&str>,
    ) -> Result<WorkerDispatchResult, ClientError> {
        let url = format!("{}/v1/messages", self.base_url);
        let full_system = format!(
            "{}\n\n## Worker Task\n\
            task_id: {}\n\
            kind: {}\n\
            objective: {}\n\
            constraints: {:?}\n\n\
            ## Contexto de Memoria\n{}\n\n\
            ## Contract (STRICT)\n\
            - Return ONLY valid JSON (no markdown)\n\
            - Schema: {{\"task_id\":\"...\",\"summary\":\"...\",\"citations\":[\"...\"],\"claims\":[],\"confidence\":0.0}}\n\
            - task_id must match exactly\n\
            - claims must be empty",
            system.unwrap_or("Analiza exclusivamente la tarea y el contexto proporcionados."),
            task.task_id,
            task.kind,
            task.objective,
            task.constraints,
            context
        );
        let system_tokens = estimate_tokens_heuristic(&full_system);

        let request = LlmRequest {
            model: preferred_worker_model(),
            messages: vec![LlmMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            system: Some(full_system),
            max_tokens: 2048,
            route: Some("worker".to_string()),
            worker_task: Some(task.clone()),
            project_id: self.project_id.clone(),
        };

        let response: LlmResponse = self
            .post_llm_turn(&url, &request)
            .await?;
        let text = response.text();
        let usage_tokens = response
            .usage
            .as_ref()
            .map(|u| u.input_tokens.saturating_add(u.output_tokens));
        let fallback_tokens = system_tokens
            .saturating_add(estimate_tokens_heuristic(prompt))
            .saturating_add(estimate_tokens_heuristic(&text));
        let result = parse_worker_result(&text)
            .map_err(|e| ClientError::Parse(format!("worker contract parse failed: {}", e)))?;

        if result.task_id != task.task_id {
            return Err(ClientError::Parse(format!(
                "worker task_id mismatch: expected '{}' got '{}'",
                task.task_id, result.task_id
            )));
        }

        if !result.claims.is_empty() {
            return Err(ClientError::Parse(
                "worker contract violation: claims must be empty".to_string(),
            ));
        }

        Ok(WorkerDispatchResult {
            result,
            tokens_used: usage_tokens.unwrap_or(fallback_tokens),
        })
    }

    /// Obtener un evento por ID (para verificar existencia de evidencia)
    /// Devuelve None si 404
    pub async fn get_event(&self, id: &str) -> Result<Option<EventSummary>, ClientError> {
        let url = format!("{}/events/{}", self.base_url, id);
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        let response = self
            .apply_auth(client.get(&url))
            .send()
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        match response.status().as_u16() {
            404 => Ok(None),
            200 => {
                let event = response
                    .json()
                    .await
                    .map_err(|e| ClientError::Parse(e.to_string()))?;
                Ok(Some(event))
            }
            status => Err(ClientError::Http {
                status,
                message: response.text().await.unwrap_or_default(),
            }),
        }
    }

    /// Obtener múltiples eventos por IDs (batch optimizado)
    /// Eventos no encontrados se omiten del resultado
    pub async fn get_events(&self, ids: &[String]) -> Result<Vec<EventSummary>, ClientError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/events/batch", self.base_url);
        let request = EventsBatchRequest { ids: ids.to_vec() };
        self.post(&url, &request).await
    }

    // === Helpers HTTP ===

    async fn get<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, ClientError> {
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        let response = client.get(url);
        let response = self
            .apply_auth(response)
            .send()
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::Http {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        response
            .json()
            .await
            .map_err(|e| ClientError::Parse(e.to_string()))
    }

    async fn post<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<R, ClientError> {
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        self.dispatch_post(client, url, body).await
    }

    /// POST para turnos de modelo: SIN plazo total.
    ///
    /// Un modelo razonando puede tardar minutos o una hora; cortarlo con un
    /// deadline es perder la respuesta a mitad de generación. Solo se acota el
    /// CONECTAR: un cerebro caído debe dar error en segundos, no colgar. En
    /// loopback no hay conexiones medio muertas: si el cerebro muere durante
    /// el turno, el socket se cierra y el error llega solo.
    async fn post_llm_turn<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<R, ClientError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        self.dispatch_post(client, url, body).await
    }

    async fn dispatch_post<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        client: reqwest::Client,
        url: &str,
        body: &T,
    ) -> Result<R, ClientError> {
        let response = self
            .apply_auth(client.post(url))
            .json(body)
            .send()
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::Http {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        response
            .json()
            .await
            .map_err(|e| ClientError::Parse(e.to_string()))
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkerDispatchResult {
    pub result: WorkerResult,
    pub tokens_used: u32,
}

impl Default for QuironClient {
    fn default() -> Self {
        Self::new()
    }
}

// === Tipos de Request/Response ===

/// Respuesta de /context
#[derive(Debug, Clone, Deserialize)]
pub struct ContextResponse {
    pub identity: IdentityInfo,
    pub brain_status: BrainStatus,
    pub active_gates: Vec<GateInfo>,
    pub recent_events: Vec<RecentEventInfo>,
    pub instructions: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdentityInfo {
    pub name: String,
    pub role: String,
    pub principles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrainStatus {
    pub chain_valid: bool,
    pub event_count: u64,
    pub invariant_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GateInfo {
    pub name: String,
    pub severity: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecentEventInfo {
    pub id: String,
    pub kind: String,
    pub description: String,
    pub ts: String,
}

/// Request para crear evento
#[derive(Debug, Clone, Serialize)]
pub struct CreateEventRequest {
    pub kind: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<String>>,
}

/// Respuesta de crear evento
#[derive(Debug, Clone, Deserialize)]
pub struct CreateEventResponse {
    pub ok: bool,
    pub id: String,
}

/// Request para checkpoint de telemetría de sesión.
#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionTelemetryCheckpointRequest {
    pub schema_version: u16,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub segment_seq: u32,
    pub step_start: u64,
    pub step_end: u64,
    pub ts_start: String,
    pub ts_end: String,
    pub tasks_total: u32,
    pub worker_tasks_total: u32,
    pub primary_calls_total: u32,
    pub model_tokens_used_delta: u32,
    pub model_tokens_used_total: u32,
    pub token_budget: u32,
    pub token_budget_remaining: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_tokens_ewma: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_tokens_ewma: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_latency_ewma_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall_latency_ewma_ms: Option<f64>,
    pub llm_fallback_rate: f32,
    pub parallel_subtasks_current: usize,
    pub parallel_subtasks_cap: usize,
    pub worker_threshold_scale: f32,
    pub primary_threshold_scale: f32,
    pub anomaly_flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_change: Option<String>,
}

/// Request para anomalía de telemetría de sesión.
#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionTelemetryAnomalyRequest {
    pub schema_version: u16,
    pub session_id: String,
    pub segment_seq: u32,
    pub step: u64,
    pub request_iteration: u32,
    pub timestamp: String,
    pub kind: String,
    pub detail: String,
}

/// Respuesta genérica de escritura de telemetría.
#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryWriteResponse {
    pub ok: bool,
    pub key: String,
}

/// Respuesta de lectura por sesión.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionTelemetryResponse {
    pub schema_version: u16,
    pub session_id: String,
    #[serde(default)]
    pub checkpoints_total: usize,
    #[serde(default)]
    pub anomalies_total: usize,
    #[serde(default)]
    pub checkpoints_offset: usize,
    #[serde(default)]
    pub anomalies_offset: usize,
    #[serde(default)]
    pub has_more_checkpoints: bool,
    #[serde(default)]
    pub has_more_anomalies: bool,
    pub checkpoints: Vec<SessionTelemetryCheckpointSummary>,
    pub anomalies: Vec<SessionTelemetryAnomalySummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionTelemetryCheckpointSummary {
    pub segment_seq: u32,
    pub step_start: u64,
    pub step_end: u64,
    #[serde(default)]
    pub ts_start: String,
    #[serde(default)]
    pub ts_end: String,
    #[serde(default)]
    pub model_tokens_used_delta: u32,
    #[serde(default)]
    pub model_tokens_used_total: u32,
    #[serde(default)]
    pub token_budget: u32,
    #[serde(default)]
    pub token_budget_remaining: u32,
    #[serde(default)]
    pub llm_fallback_rate: f32,
    #[serde(default)]
    pub anomaly_flags: Vec<String>,
    #[serde(default)]
    pub preset_change: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionTelemetryAnomalySummary {
    pub segment_seq: u32,
    pub step: u64,
    pub kind: String,
    #[serde(default)]
    pub request_iteration: u32,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub detail: String,
}

/// Request para validar acción  
#[derive(Debug, Clone, Serialize)]
pub struct ActionRequest {
    /// Tipo de acción (write, patch, delete, read, etc.)
    pub action: String,
    /// Target de la acción (path de archivo, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// ID del proyecto
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// ID del agente
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Contexto adicional (files_read, evidence, etc.)
    #[serde(default)]
    pub context: serde_json::Value,
}

/// Respuesta de validación
#[derive(Debug, Clone, Deserialize)]
pub struct ValidationResponse {
    pub allowed: bool,
    pub reason: Option<String>,
    pub blocking_invariants: Vec<String>,
    pub warnings: Vec<String>,
}

/// Respuesta de verificar cadena
#[derive(Debug, Clone, Deserialize)]
pub struct ChainVerifyResponse {
    pub valid: bool,
    pub event_count: u64,
    pub message: String,
}

/// Evento resumido
#[derive(Debug, Clone, Deserialize)]
pub struct EventSummary {
    #[serde(alias = "event_id")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_kind_as_string")]
    pub kind: String,
    pub description: String,
}

fn deserialize_kind_as_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let kind = match value {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => "unknown".to_string(),
        other => other.to_string(),
    };
    Ok(kind)
}

#[derive(Debug, Clone, Serialize)]
struct EventsBatchRequest {
    ids: Vec<String>,
}

/// Respuesta de búsqueda
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub blocked: bool,
    pub confidence: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub event_id: Option<String>,
    pub kind: Option<String>,
    pub description: Option<String>,
    pub score: f32,
}

/// Respuesta de /recall (Virtual Context Tools).
#[derive(Debug, Clone, Deserialize)]
pub struct RecallResponse {
    pub query: String,
    pub events: Vec<RecallEvent>,
    pub search_time_ms: u64,
    pub strategy: String,
}

/// Match individual de /recall.
#[derive(Debug, Clone, Deserialize)]
pub struct RecallEvent {
    pub event_id: String,
    pub description: String,
    pub kind: String,
    pub timestamp: String,
    pub score: f32,
    pub why_selected: String,
}

/// Respuesta de health
///
/// `node_count` e `invariant_count` llevan `default` porque una versión antigua
/// de quiron-brain puede no enviarlos; su ausencia no debe invalidar la lectura
/// del estado.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub event_count: u64,
    #[serde(default)]
    pub node_count: u64,
    #[serde(default)]
    pub invariant_count: u64,
}

impl HealthResponse {
    pub fn is_ok(&self) -> bool {
        self.status == "ok"
    }
}

// ============================================================================
// TIPOS PARA LLM (comunicación con /v1/messages via vertex-gateway)
// ============================================================================

/// Request para llamar al LLM
#[derive(Debug, Clone, Serialize)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_task: Option<WorkerTask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// Mensaje individual para el LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

/// Request de un turno del chat con herramientas.
///
/// A diferencia de [`LlmRequest`], los mensajes llevan bloques estructurados
/// (formato Anthropic): así los `tool_use` del asistente y los `tool_result`
/// del editor conservan su `id` de punta a punta y quiron-brain puede
/// realimentarlos al modelo por el gateway.
#[derive(Debug, Clone, Serialize)]
pub struct ChatTurnRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub model: String,
    pub messages: Vec<ChatTurnMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<LlmToolDef>,
}

/// Mensaje del chat con contenido por bloques.
#[derive(Debug, Clone, Serialize)]
pub struct ChatTurnMessage {
    pub role: String,
    pub content: Vec<ChatBlock>,
}

impl ChatTurnMessage {
    /// Mensaje de un solo bloque de texto.
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: vec![ChatBlock::Text { text: text.into() }],
        }
    }
}

/// Un bloque de contenido, en formato Anthropic.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatBlock {
    Text {
        text: String,
    },
    /// El modelo pidió una herramienta (se conserva en el historial tal cual).
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Lo que la herramienta devolvió tras pasar por el arnés.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// Definición de una herramienta ofrecida al modelo (formato Anthropic).
#[derive(Debug, Clone, Serialize)]
pub struct LlmToolDef {
    pub name: String,
    pub description: String,
    /// Esquema JSON de los parámetros.
    pub input_schema: serde_json::Value,
}

/// Ficha de código que el cerebro inyectó en la respuesta: ruta, símbolo, rango
/// y hash del archivo tal como estaba al responder. Sirve para enseñar la fuente
/// y abrirla, sin fiarse de lo que el modelo copió en su texto.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeHint {
    pub path: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub start_line: usize,
    #[serde(default)]
    pub end_line: usize,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub summary_origin: String,
}

/// Lo que el cerebro añade a la respuesta del modelo.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LlmQuironContext {
    #[serde(default)]
    pub code_hints: Vec<CodeHint>,
}

/// Respuesta del LLM
#[derive(Debug, Clone, Deserialize)]
pub struct LlmResponse {
    pub content: Vec<LlmContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<LlmUsage>,
    /// Fichas y demás contexto que el cerebro adjunta; ausente en otras rutas.
    #[serde(default)]
    pub quiron_context: Option<LlmQuironContext>,
}

impl LlmResponse {
    /// Extraer texto de la respuesta
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| {
                if c.content_type == "text" {
                    Some(c.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: String,
    /// Identificador de la llamada, si el bloque es `tool_use`.
    #[serde(default)]
    pub id: Option<String>,
    /// Nombre de la herramienta pedida, si el bloque es `tool_use`.
    #[serde(default)]
    pub name: Option<String>,
    /// Argumentos ya parseados, si el bloque es `tool_use`.
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

/// Errores del cliente
#[derive(Debug, Clone)]
pub enum ClientError {
    /// Error de transporte (red, timeout)
    Transport(String),
    /// Error HTTP (status code)
    Http { status: u16, message: String },
    /// Error al parsear respuesta
    Parse(String),
    /// quiron-brain no está disponible
    Unavailable,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Transport(msg) => write!(f, "Transport error: {}", msg),
            ClientError::Http { status, message } => write!(f, "HTTP {}: {}", status, message),
            ClientError::Parse(msg) => write!(f, "Parse error: {}", msg),
            ClientError::Unavailable => write!(f, "quiron-brain not available"),
        }
    }
}

impl std::error::Error for ClientError {}

fn extract_json_object(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(stripped) = trimmed.strip_prefix("```json") {
        return stripped.trim().trim_end_matches("```").trim();
    }
    if let Some(stripped) = trimmed.strip_prefix("```") {
        return stripped.trim().trim_end_matches("```").trim();
    }
    trimmed
}

fn parse_worker_result(text: &str) -> Result<WorkerResult, serde_json::Error> {
    serde_json::from_str(extract_json_object(text))
}

fn estimate_tokens_heuristic(text: &str) -> u32 {
    let chars = text.chars().count() as u32;
    (chars.saturating_add(3) / 4).max(1)
}

fn env_api_token() -> Option<String> {
    std::env::var(API_TOKEN_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn preferred_primary_model() -> String {
    env_non_empty(PRIMARY_MODEL_ENV)
        .or_else(|| env_non_empty(FALLBACK_MODEL_ENV))
        .unwrap_or_else(|| DEFAULT_LLM_MODEL.to_string())
}

fn preferred_worker_model() -> String {
    env_non_empty(WORKER_MODEL_ENV)
        .or_else(|| env_non_empty(PRIMARY_MODEL_ENV))
        .or_else(|| env_non_empty(FALLBACK_MODEL_ENV))
        .unwrap_or_else(|| DEFAULT_LLM_MODEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = QuironClient::new();
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn test_custom_url() {
        let client = QuironClient::with_url("http://custom:9000");
        assert_eq!(client.base_url, "http://custom:9000");
    }

    #[test]
    fn test_client_with_explicit_token() {
        let client = QuironClient::with_url_and_token("http://custom:9000", Some("abc123".into()));
        assert_eq!(client.base_url, "http://custom:9000");
        assert_eq!(client.api_token.as_deref(), Some("abc123"));
    }

    #[test]
    fn client_carries_workspace_project_id() {
        let client = QuironClient::with_url("http://custom:9000")
            .with_project_id("/workspace/project-a".to_string());
        assert_eq!(client.project_id(), Some("/workspace/project-a"));
    }

    #[test]
    fn test_parse_worker_result_from_json() {
        let text =
            r#"{"task_id":"t1","summary":"ok","citations":["evt-1"],"claims":[],"confidence":0.8}"#;
        let parsed = parse_worker_result(text).unwrap();
        assert_eq!(parsed.task_id, "t1");
        assert!(parsed.claims.is_empty());
    }

    #[test]
    fn test_parse_worker_result_from_fenced_json() {
        let text = r#"```json
        {"task_id":"t2","summary":"ok","citations":[],"claims":[]}
        ```"#;
        let parsed = parse_worker_result(text).unwrap();
        assert_eq!(parsed.task_id, "t2");
    }

    #[test]
    fn test_event_summary_accepts_numeric_kind() {
        let raw = r#"{"id":"evt-1","kind":3,"description":"hello"}"#;
        let parsed: EventSummary = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.id, "evt-1");
        assert_eq!(parsed.kind, "3");
        assert_eq!(parsed.description, "hello");
    }

    #[test]
    fn test_event_summary_accepts_event_id_alias() {
        let raw = r#"{"event_id":"evt-2","kind":"Run","description":"hello"}"#;
        let parsed: EventSummary = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.id, "evt-2");
        assert_eq!(parsed.kind, "Run");
    }
}
