//! # LLM Client - Via Vertex Gateway
//!
//! Cliente para llamar a modelos LLM via el binario inmutable vertex-gateway.
//!
//! ## Principios de Seguridad
//!
//! 1. **NO conecta directamente a internet** — Todo pasa por vertex-gateway
//! 2. **Comunicación via stdin/stdout** — Pipes locales, no HTTP
//! 3. **Config centralizada** — Solo vertex-gateway decide endpoint/modelo real
//! 4. **Binario auditable** — ~317 líneas de código verificable
//!
//! ## Arquitectura
//!
//! ```text
//! quiron-brain  ──stdin──►  vertex-gateway  ──HTTP──►  LLM local / remoto
//!               ◄─stdout──                  ◄────────
//! ```
//!
//! ## Notas de Implementación
//!
//! - **Timeout = Reset**: Si hay timeout después de escribir, el gateway puede
//!   responder tarde. La siguiente lectura leería esa respuesta "antigua".
//!   Por eso, cualquier error de timeout/parse/EOF mata y respawnea el proceso.
//!
//! - **Model + route son intención**: `model` y `route` son preferencia del planner.
//!   El modelo real final lo resuelve vertex-gateway.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// Error del cliente LLM
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("Gateway error: {0}")]
    Gateway(String),
    #[error("Gateway not found at: {0}")]
    GatewayNotFound(String),
    #[error("Gateway spawn failed: {0}")]
    SpawnFailed(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Invalid response: {0}")]
    Response(String),
    #[error("Timeout waiting for gateway (protocol reset required)")]
    Timeout,
    #[error("Gateway EOF (process died)")]
    GatewayEof,
}

/// Resultado del cliente LLM
pub type Result<T> = std::result::Result<T, LlmError>;

/// Mensaje en formato compatible Anthropic (para API de quiron-brain)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Request compatible con API Claude/Messages (para API de quiron-brain)
///
/// **NOTA**: `model` y `route` son intención del planner. El modelo real
/// final es decidido por vertex-gateway según su configuración.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Modelo preferido para esta llamada
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Ruta de ejecución preferida (`primary` o `worker`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    /// Herramientas ofrecidas al modelo, en formato Anthropic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    /// Conversación estructurada (capa 4). Si viene vacía, se aplana
    /// `messages` a un prompt de texto, como siempre.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ChatItem>,
}

/// Definición de una herramienta, en formato Anthropic (`input_schema`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Esquema JSON de los parámetros.
    pub input_schema: serde_json::Value,
}

/// Un item de la conversación multi-turno con herramientas.
///
/// Es el formato de cable que entiende vertex-gateway (`input_items`): el tag
/// `type` serializa como `message` | `function_call` | `function_call_output`.
/// Cuando una petición trae items, la conversación viaja estructurada y el
/// gateway puede realimentar al modelo los resultados de herramientas por su
/// `call_id`; el aplanado a texto solo sirve para peticiones sin herramientas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatItem {
    Message {
        role: String,
        text: String,
    },
    FunctionCall {
        call_id: String,
        name: String,
        /// Argumentos como cadena JSON, tal cual los emitió el modelo.
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

fn default_max_tokens() -> u32 {
    4096
}

/// Respuesta compatible con API Claude (para API de quiron-brain)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Identificador de la llamada, para un bloque `tool_use`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Nombre de la herramienta, para un bloque `tool_use`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Argumentos de la herramienta ya parseados, para un bloque `tool_use`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

// ============================================================================
// GATEWAY PROTOCOL - Tipos internos para comunicación con vertex-gateway
// ============================================================================

/// Request a vertex-gateway (stdin)
#[derive(Debug, Serialize)]
struct GatewayRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<String>,
    /// Herramientas en el formato del gateway (`parameters`, no `input_schema`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<GatewayTool>,
    /// Conversación estructurada. El gateway la usa en lugar de `prompt`
    /// cuando no viene vacía; se omite del JSON en el resto de casos para que
    /// el cuerpo de las peticiones legadas no cambie.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    input_items: Vec<ChatItem>,
}

/// Herramienta en el formato que espera el gateway.
#[derive(Debug, Serialize)]
struct GatewayTool {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

/// Una llamada a herramienta devuelta por el gateway.
#[derive(Debug, Clone, Deserialize)]
struct GatewayToolCall {
    call_id: String,
    name: String,
    arguments: String,
}

/// Response de vertex-gateway (stdout)
#[derive(Debug, Deserialize)]
struct GatewayResponse {
    ok: bool,
    content: Option<String>,
    error: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<GatewayToolCall>>,
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    tokens_used: Option<u32>,
    model: Option<String>,
}

fn normalized_model_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Traduce la respuesta del gateway a bloques de contenido en formato Anthropic.
///
/// El texto se convierte en un bloque `text`; cada llamada a herramienta en un
/// bloque `tool_use` con sus argumentos ya parseados. Si los argumentos no son
/// JSON válido, se preserva la cadena original bajo la clave `_raw` en lugar de
/// perderlos.
fn content_blocks_from_gateway(response: &GatewayResponse) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    if let Some(text) = response.content.as_ref().filter(|t| !t.is_empty()) {
        blocks.push(ContentBlock {
            content_type: "text".into(),
            text: text.clone(),
            ..Default::default()
        });
    }

    for call in response.tool_calls.iter().flatten() {
        let input = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .unwrap_or_else(|_| serde_json::json!({ "_raw": call.arguments }));
        blocks.push(ContentBlock {
            content_type: "tool_use".into(),
            id: Some(call.call_id.clone()),
            name: Some(call.name.clone()),
            input: Some(input),
            ..Default::default()
        });
    }

    // Una respuesta sin texto ni herramientas produce al menos un bloque vacío,
    // para no romper a los consumidores que asumen contenido.
    if blocks.is_empty() {
        blocks.push(ContentBlock {
            content_type: "text".into(),
            ..Default::default()
        });
    }

    blocks
}

fn resolve_gateway_usage(response: &GatewayResponse) -> Usage {
    let input_tokens = response.input_tokens.or_else(|| {
        response
            .tokens_used
            .zip(response.output_tokens)
            .map(|(total, output)| total.saturating_sub(output))
    });
    let output_tokens = response.output_tokens.or_else(|| {
        response
            .tokens_used
            .zip(response.input_tokens)
            .map(|(total, input)| total.saturating_sub(input))
    });
    let resolved_input = input_tokens.unwrap_or(0);
    let resolved_output = output_tokens.unwrap_or(0);

    Usage {
        input_tokens: resolved_input,
        output_tokens: resolved_output,
        total_tokens: resolved_input.saturating_add(resolved_output),
    }
}

// ============================================================================
// LLM CLIENT - Via binario inmutable
// ============================================================================

/// Cliente LLM que se comunica via el binario inmutable vertex-gateway.
///
/// **Seguridad**: Este cliente NO hace conexiones HTTP directas.
/// Todo el tráfico pasa por el binario vertex-gateway que:
/// - Resuelve endpoint/modelo usando configuración auditable
/// - No lee variables de entorno con secretos
/// - Es auditable (~317 líneas)
///
/// **Protocolo**: Line-based JSON sobre stdin/stdout.
/// En caso de timeout/error, el proceso gateway se mata y respawnea
/// para evitar desincronización del protocolo.
pub struct LlmClient {
    /// Path al binario vertex-gateway
    gateway_path: PathBuf,
    /// Proceso del gateway (mantenido vivo para múltiples requests)
    /// El Mutex serializa acceso a stdin/stdout
    gateway_process: Mutex<Option<GatewayProcess>>,
}

/// Proceso del gateway con sus streams
struct GatewayProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout_reader: BufReader<tokio::process::ChildStdout>,
}

/// Alias para compartir entre handlers.
/// Solo un Mutex interno (gateway_process) - no envolver en otro.
pub type SharedLlmClient = Arc<LlmClient>;

impl LlmClient {
    /// Timeout por defecto para operaciones con el gateway.
    /// Si el backend real declara timeouts mayores por ruta, el cliente debe
    /// respetarlos para no resetear el protocolo antes de tiempo.
    const DEFAULT_GATEWAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

    /// Margen adicional para cubrir serialización, parseo y latencia del hijo.
    const GATEWAY_TIMEOUT_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

    /// Límite de tamaño de prompt (evitar DoS accidental)
    const MAX_PROMPT_SIZE: usize = 1_000_000; // 1MB

    /// Crear cliente desde path al gateway
    pub fn new(gateway_path: PathBuf) -> Result<Self> {
        // Verificar que existe el binario
        if !gateway_path.exists() {
            return Err(LlmError::GatewayNotFound(
                gateway_path.display().to_string(),
            ));
        }

        Ok(Self {
            gateway_path,
            gateway_process: Mutex::new(None),
        })
    }

    /// Crear cliente con path por defecto
    pub fn from_default_path() -> Result<Self> {
        if let Some(configured_path) = std::env::var("QUIRON_VERTEX_GATEWAY_PATH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Self::new(PathBuf::from(configured_path));
        }

        // Buscar en ubicaciones conocidas
        let paths = [
            PathBuf::from("/home/kssose/Quirón/vertex-gateway/target/release/vertex-gateway"),
            PathBuf::from("./vertex-gateway/target/release/vertex-gateway"),
            PathBuf::from("../vertex-gateway/target/release/vertex-gateway"),
        ];

        for path in paths {
            if path.exists() {
                return Self::new(path);
            }
        }

        Err(LlmError::GatewayNotFound(
            "vertex-gateway not found in expected locations".into(),
        ))
    }

    /// Crear cliente compartido
    pub fn shared(gateway_path: PathBuf) -> Result<SharedLlmClient> {
        Ok(Arc::new(Self::new(gateway_path)?))
    }

    /// Crear cliente compartido con path por defecto
    pub fn shared_from_default() -> Result<SharedLlmClient> {
        Ok(Arc::new(Self::from_default_path()?))
    }

    /// Matar y limpiar proceso gateway (para reset de protocolo)
    async fn reset_gateway_locked(guard: &mut Option<GatewayProcess>) {
        if let Some(mut proc) = guard.take() {
            // Best-effort: matar el proceso para evitar respuestas tardías
            let _ = proc.child.kill().await;
            tracing::info!("Gateway process killed for protocol reset");
        }
    }

    /// Reset público del gateway
    pub async fn reset_gateway(&self) {
        let mut guard = self.gateway_process.lock().await;
        Self::reset_gateway_locked(&mut *guard).await;
    }

    /// Spawn o reusar el proceso gateway
    async fn ensure_gateway(&self) -> Result<()> {
        let mut guard = self.gateway_process.lock().await;

        // Verificar si el proceso existente sigue vivo
        if let Some(ref mut proc) = *guard {
            match proc.child.try_wait() {
                Ok(None) => return Ok(()), // Sigue corriendo
                Ok(Some(status)) => {
                    tracing::info!("Gateway process exited with {:?}, respawning...", status);
                }
                Err(e) => {
                    tracing::warn!("Failed to check gateway status: {}", e);
                }
            }
            // Limpiar proceso muerto
            *guard = None;
        }

        // Spawn nuevo proceso
        tracing::info!("Spawning vertex-gateway: {:?}", self.gateway_path);

        // Reenviar variables LLM relevantes al proceso hijo.
        // Nota: por defecto el proceso hereda el entorno, pero lo hacemos explícito
        // para evitar ambigüedad en despliegues con entornos sanitizados.
        let mut cmd = Command::new(&self.gateway_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // Ver errores del gateway en logs

        for key in [
            "QUIRON_GATEWAY_BACKEND",
            "QUIRON_LLM_ENDPOINT",
            "QUIRON_LLM_ENDPOINT_PRIMARY",
            "QUIRON_LLM_ENDPOINT_WORKER",
            "QUIRON_LLM_MODEL_PRIMARY",
            "QUIRON_LLM_MODEL_WORKER",
            "QUIRON_LLM_API_KEY_PRIMARY",
            "QUIRON_LLM_API_KEY_WORKER",
            "QUIRON_LLM_API_KEY_PRIMARY_FILE",
            "QUIRON_LLM_API_KEY_WORKER_FILE",
            "QUIRON_LLM_TIMEOUT_PRIMARY_SECS",
            "QUIRON_LLM_TIMEOUT_WORKER_SECS",
            "QUIRON_OPENCLAW_CMD",
            "QUIRON_OPENCLAW_AGENT_PRIMARY",
            "QUIRON_OPENCLAW_AGENT_WORKER",
            "QUIRON_OPENCLAW_THINKING",
            "QUIRON_OPENCLAW_LOCAL",
            // Compatibilidad histórica
            "OPENAI_API_KEY",
        ] {
            if let Ok(value) = std::env::var(key) {
                cmd.env(key, value);
            }
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| LlmError::SpawnFailed(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LlmError::SpawnFailed("Failed to get stdin".into()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LlmError::SpawnFailed("Failed to get stdout".into()))?;

        *guard = Some(GatewayProcess {
            child,
            stdin,
            stdout_reader: BufReader::new(stdout),
        });

        Ok(())
    }

    /// Enviar request al gateway y obtener respuesta.
    /// En caso de timeout/error, hace reset del gateway para evitar desincronización.
    async fn call_gateway(&self, req: GatewayRequest) -> Result<GatewayResponse> {
        // Validar tamaño de prompt
        if req.prompt.len() > Self::MAX_PROMPT_SIZE {
            return Err(LlmError::Io(format!(
                "Prompt too large: {} bytes (max {})",
                req.prompt.len(),
                Self::MAX_PROMPT_SIZE
            )));
        }

        let gateway_timeout = Self::gateway_timeout_for_route(req.route.as_deref());

        self.ensure_gateway().await?;

        let mut guard = self.gateway_process.lock().await;
        let proc = guard
            .as_mut()
            .ok_or_else(|| LlmError::Gateway("Gateway not initialized".into()))?;

        // Serializar request
        let mut request_line = serde_json::to_string(&req)
            .map_err(|e| LlmError::Io(format!("Serialize error: {}", e)))?;
        request_line.push('\n');

        // WRITE con timeout
        let write_res = tokio::time::timeout(gateway_timeout, async {
            proc.stdin
                .write_all(request_line.as_bytes())
                .await
                .map_err(|e| LlmError::Io(format!("Write error: {}", e)))?;
            proc.stdin
                .flush()
                .await
                .map_err(|e| LlmError::Io(format!("Flush error: {}", e)))?;
            Ok::<(), LlmError>(())
        })
        .await;

        // Si timeout en write: protocolo potencialmente corrupto → reset
        if write_res.is_err() {
            Self::reset_gateway_locked(&mut *guard).await;
            return Err(LlmError::Timeout);
        }
        // Propagar error de IO si hubo
        write_res.unwrap()?;

        // READ con timeout
        let read_res = tokio::time::timeout(gateway_timeout, async {
            let mut line = String::new();
            let n = proc
                .stdout_reader
                .read_line(&mut line)
                .await
                .map_err(|e| LlmError::Io(format!("Read error: {}", e)))?;
            if n == 0 {
                return Err(LlmError::GatewayEof);
            }
            Ok::<String, LlmError>(line)
        })
        .await;

        let response_line = match read_res {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => {
                // Error de IO o EOF → reset
                Self::reset_gateway_locked(&mut *guard).await;
                return Err(e);
            }
            Err(_) => {
                // Timeout → reset
                Self::reset_gateway_locked(&mut *guard).await;
                return Err(LlmError::Timeout);
            }
        };

        // Parsear respuesta
        let response_line = response_line.trim();

        // Saltear líneas vacías (por si el gateway imprime algo accidentalmente)
        if response_line.is_empty() {
            // Intentar leer una línea más
            let retry_res = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut line = String::new();
                let n = proc
                    .stdout_reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| LlmError::Io(format!("Read retry error: {}", e)))?;
                if n == 0 {
                    return Err(LlmError::GatewayEof);
                }
                Ok::<String, LlmError>(line)
            })
            .await;

            match retry_res {
                Ok(Ok(line)) => {
                    return self.parse_gateway_response(&line);
                }
                _ => {
                    Self::reset_gateway_locked(&mut *guard).await;
                    return Err(LlmError::Gateway("Empty response from gateway".into()));
                }
            }
        }

        self.parse_gateway_response(response_line)
    }

    fn gateway_timeout_for_route(route: Option<&str>) -> std::time::Duration {
        if let Some(explicit) = timeout_from_env("QUIRON_LLM_GATEWAY_TIMEOUT_SECS") {
            return explicit;
        }

        let route_timeout = match route {
            Some("worker") => timeout_from_env("QUIRON_LLM_TIMEOUT_WORKER_SECS"),
            Some("primary") => timeout_from_env("QUIRON_LLM_TIMEOUT_PRIMARY_SECS"),
            _ => timeout_from_env("QUIRON_LLM_TIMEOUT_WORKER_SECS")
                .into_iter()
                .chain(timeout_from_env("QUIRON_LLM_TIMEOUT_PRIMARY_SECS"))
                .max(),
        };

        route_timeout
            .map(|timeout| timeout.saturating_add(Self::GATEWAY_TIMEOUT_GRACE))
            .unwrap_or(Self::DEFAULT_GATEWAY_TIMEOUT)
    }

    /// Parsear respuesta JSON del gateway
    fn parse_gateway_response(&self, line: &str) -> Result<GatewayResponse> {
        let response: GatewayResponse = serde_json::from_str(line).map_err(|e| {
            // Parse error = posible desync, pero ya tenemos la línea completa
            LlmError::Response(format!("Parse error: {} (response: {})", e, line))
        })?;

        // Verificar errores del gateway
        if !response.ok {
            return Err(LlmError::Gateway(
                response
                    .error
                    .unwrap_or_else(|| "Unknown gateway error".into()),
            ));
        }

        Ok(response)
    }

    /// Enviar request en formato API Claude (convierte a formato gateway)
    pub async fn send_message(&self, req: &MessagesRequest) -> Result<MessagesResponse> {
        // Construir prompt desde mensajes
        let prompt = req
            .messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        let requested_model = normalized_model_name(&req.model);

        // Traducir las herramientas del formato Anthropic (`input_schema`) al del
        // gateway (`parameters`).
        let tools = req
            .tools
            .iter()
            .map(|tool| GatewayTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.input_schema.clone(),
            })
            .collect();

        // Llamar al gateway
        let gateway_req = GatewayRequest {
            provider: req.provider.clone(),
            reasoning_effort: req.reasoning_effort.clone(),
            prompt,
            system: req.system.clone(),
            max_tokens: req.max_tokens,
            model: requested_model.clone(),
            route: req.route.clone(),
            tools,
            input_items: req.items.clone(),
        };

        let gateway_resp = self.call_gateway(gateway_req).await?;

        // Convertir respuesta a formato compatible
        let usage = resolve_gateway_usage(&gateway_resp);
        let blocks = content_blocks_from_gateway(&gateway_resp);
        let stop_reason = if gateway_resp.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
            "tool_use"
        } else {
            "end_turn"
        };

        Ok(MessagesResponse {
            id: format!("gateway-{}", ulid::Ulid::new()),
            // Exponemos el modelo real devuelto por gateway; fallback al solicitado.
            model: gateway_resp.model.or(requested_model).unwrap_or_default(),
            content: blocks,
            stop_reason: Some(stop_reason.into()),
            usage,
        })
    }

    /// Llamada simple (solo prompt y system)
    pub async fn simple_call(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let gateway_req = GatewayRequest {
            provider: None,
            reasoning_effort: None,
            prompt: prompt.to_string(),
            system: system.map(|s| s.to_string()),
            max_tokens: 4096,
            model: None,
            route: None,
            tools: Vec::new(),
            input_items: Vec::new(),
        };

        let resp = self.call_gateway(gateway_req).await?;
        Ok(resp.content.unwrap_or_default())
    }

    /// Extraer texto de la respuesta
    pub fn extract_text(response: &MessagesResponse) -> String {
        response
            .content
            .iter()
            .filter(|c| c.content_type == "text")
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn timeout_from_env(key: &str) -> Option<std::time::Duration> {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(std::time::Duration::from_secs)
}

// ============================================================================
// COMPATIBILIDAD - Para código existente que espera from_env()
// ============================================================================

impl LlmClient {
    /// Compatibilidad con código existente
    /// DEPRECATED: Usar from_default_path() en su lugar
    pub fn from_env() -> Result<Self> {
        Self::from_default_path()
    }

    /// Compatibilidad con código existente
    /// DEPRECATED: Usar shared_from_default() en su lugar
    pub fn shared_from_env() -> Result<SharedLlmClient> {
        Self::shared_from_default()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_request_serialization() {
        let req = GatewayRequest {
            provider: None,
            reasoning_effort: None,
            prompt: "Hola".into(),
            system: Some("Eres un asistente".into()),
            max_tokens: 100,
            model: Some("qwen2.5:1.5b".into()),
            route: Some("primary".into()),
            tools: Vec::new(),
            input_items: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("Hola"));
        assert!(json.contains("system"));
        // Sin conversación estructurada, el campo no aparece: el cuerpo de las
        // peticiones legadas no cambia ni un byte.
        assert!(!json.contains("input_items"), "{json}");
    }

    #[test]
    fn test_chat_items_serializan_el_formato_de_cable_del_gateway() {
        let items = vec![
            ChatItem::Message {
                role: "user".into(),
                text: "¿qué dice el README?".into(),
            },
            ChatItem::FunctionCall {
                call_id: "call_1".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"README.md\"}".into(),
            },
            ChatItem::FunctionCallOutput {
                call_id: "call_1".into(),
                output: "contenido".into(),
            },
        ];
        let json = serde_json::to_value(&items).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                { "type": "message", "role": "user", "text": "¿qué dice el README?" },
                { "type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"README.md\"}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "contenido" }
            ])
        );
    }

    #[test]
    fn test_normalized_model_name_omits_blank_values() {
        assert_eq!(normalized_model_name(""), None);
        assert_eq!(normalized_model_name("   "), None);
        assert_eq!(
            normalized_model_name(" gemini-2.5-pro "),
            Some("gemini-2.5-pro".to_string())
        );
    }

    #[test]
    fn test_from_default_path_prefers_env_override() {
        let dir = tempfile::tempdir().unwrap();
        let gateway_path = dir.path().join("vertex-gateway");
        std::fs::write(&gateway_path, b"#!/bin/sh\n").unwrap();

        std::env::set_var("QUIRON_VERTEX_GATEWAY_PATH", &gateway_path);
        let client = LlmClient::from_default_path().unwrap();
        assert_eq!(client.gateway_path, gateway_path);
        std::env::remove_var("QUIRON_VERTEX_GATEWAY_PATH");
    }

    #[test]
    fn test_gateway_response_parsing() {
        let json = r#"{"ok": true, "content": "Hello!", "input_tokens": 17, "output_tokens": 42}"#;
        let resp: GatewayResponse = serde_json::from_str(json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.content, Some("Hello!".into()));
        assert_eq!(resp.input_tokens, Some(17));
        assert_eq!(resp.output_tokens, Some(42));
    }

    #[test]
    fn test_resolve_gateway_usage_prefers_split_fields() {
        let resp = GatewayResponse {
            ok: true,
            content: Some("Hello!".into()),
            error: None,
            tool_calls: None,
            input_tokens: Some(11),
            output_tokens: Some(7),
            tokens_used: Some(999),
            model: Some("gpt-test".into()),
        };
        let usage = resolve_gateway_usage(&resp);
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.total_tokens, 18);
    }

    #[test]
    fn test_resolve_gateway_usage_derives_missing_side_from_total() {
        let resp = GatewayResponse {
            ok: true,
            content: Some("Hello!".into()),
            error: None,
            tool_calls: None,
            input_tokens: Some(11),
            output_tokens: None,
            tokens_used: Some(18),
            model: Some("gpt-test".into()),
        };
        let usage = resolve_gateway_usage(&resp);
        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.total_tokens, 18);
    }

    #[test]
    fn test_gateway_error_parsing() {
        let json = r#"{"ok": false, "error": "Auth failed"}"#;
        let resp: GatewayResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.error, Some("Auth failed".into()));
    }

    #[test]
    fn test_gateway_timeout_for_worker_uses_worker_timeout_plus_grace() {
        std::env::set_var("QUIRON_LLM_TIMEOUT_WORKER_SECS", "600");
        std::env::remove_var("QUIRON_LLM_GATEWAY_TIMEOUT_SECS");

        let timeout = LlmClient::gateway_timeout_for_route(Some("worker"));

        assert_eq!(timeout, std::time::Duration::from_secs(630));

        std::env::remove_var("QUIRON_LLM_TIMEOUT_WORKER_SECS");
    }

    #[test]
    fn test_gateway_timeout_explicit_override_wins() {
        std::env::set_var("QUIRON_LLM_GATEWAY_TIMEOUT_SECS", "200");
        std::env::set_var("QUIRON_LLM_TIMEOUT_WORKER_SECS", "600");

        let timeout = LlmClient::gateway_timeout_for_route(Some("worker"));

        assert_eq!(timeout, std::time::Duration::from_secs(200));

        std::env::remove_var("QUIRON_LLM_GATEWAY_TIMEOUT_SECS");
        std::env::remove_var("QUIRON_LLM_TIMEOUT_WORKER_SECS");
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message {
            role: "user".into(),
            content: "Hello".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_shared_client_type() {
        // Verificar que SharedLlmClient es Arc<LlmClient> (no Arc<Mutex<LlmClient>>)
        fn _assert_send_sync<T: Send + Sync>() {}
        _assert_send_sync::<SharedLlmClient>();
    }
}
