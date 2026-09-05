//! # LLM Gateway — Quirón
//!
//! Binario que conecta quiron-brain a providers LLM.
//! Soporta backends: openai_compatible, openclaw, codex_direct.
//!
//! ## Uso:
//! ```bash
//! echo '{"prompt": "Hola"}' | QUIRON_GATEWAY_BACKEND=codex_direct vertex-gateway
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
mod claude_cli;
use claude_cli::process_request_claude_cli;

// ============================================================================
// CONFIGURACIÓN
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayBackend {
    OpenAiCompatible,
    OllamaNative,
    OpenClaw,
    CodexDirect,
    ClaudeCli,
}

impl GatewayBackend {
    fn from_env(value: Option<String>) -> Result<Self, String> {
        match value
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            None
            | Some("")
            | Some("openai_compatible")
            | Some("openai-compatible")
            | Some("openai")
            | Some("legacy")
            | Some("http") => Ok(GatewayBackend::OpenAiCompatible),
            Some("ollama") | Some("ollama_native") | Some("ollama-native") => {
                Ok(GatewayBackend::OllamaNative)
            }
            Some("claude_cli") | Some("claude-cli") | Some("claude") => Ok(GatewayBackend::ClaudeCli),
            Some("openclaw") => Ok(GatewayBackend::OpenClaw),
            Some("codex_direct") | Some("codex-direct") | Some("codex") => {
                Ok(GatewayBackend::CodexDirect)
            }
            Some(other) => Err(format!(
                "invalid QUIRON_GATEWAY_BACKEND='{}' (use openai_compatible|ollama_native|openclaw|codex_direct|claude_cli)",
                other
            )),
        }
    }
}

// Sin `Debug`: contiene `primary_api_key` y `worker_api_key`. No se imprime en
// ningún punto y no debe poder imprimirse por accidente.
struct GatewayConfig {
    primary_backend: GatewayBackend,
    worker_backend: GatewayBackend,
    primary_endpoint: String,
    worker_endpoint: String,
    primary_model: Option<String>,
    worker_model: Option<String>,
    primary_api_key: Option<String>,
    worker_api_key: Option<String>,
    primary_timeout_secs: u64,
    worker_timeout_secs: u64,
    worker_temperature: f32,
    worker_ollama_think: Option<Value>,
    worker_ollama_num_ctx: Option<u32>,
    worker_ollama_format_json: bool,
    openclaw_cmd: String,
    openclaw_agent_primary: String,
    openclaw_agent_worker: String,
    openclaw_thinking: Option<String>,
    openclaw_local: bool,
    // Direct provider config
    codex_auth_file: PathBuf,
    codex_model: String,
}

impl GatewayConfig {
    fn from_env() -> Result<Self, String> {
        let default_backend_env = std::env::var("QUIRON_GATEWAY_BACKEND").ok();
        let primary_backend = GatewayBackend::from_env(default_backend_env.clone())?;

        let worker_backend_env = std::env::var("QUIRON_GATEWAY_BACKEND_WORKER")
            .ok()
            .or(default_backend_env);
        let worker_backend = GatewayBackend::from_env(worker_backend_env)?;

        let base_endpoint =
            std::env::var("QUIRON_LLM_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8080".into());

        let primary_endpoint = normalized_or(
            std::env::var("QUIRON_LLM_ENDPOINT_PRIMARY").ok(),
            &base_endpoint,
        );
        let worker_endpoint = normalized_or(
            std::env::var("QUIRON_LLM_ENDPOINT_WORKER").ok(),
            &primary_endpoint,
        );

        let primary_model = normalized(std::env::var("QUIRON_LLM_MODEL_PRIMARY").ok());
        let worker_model = normalized(std::env::var("QUIRON_LLM_MODEL_WORKER").ok());
        let primary_api_key = load_secret(
            "QUIRON_LLM_API_KEY_PRIMARY",
            "QUIRON_LLM_API_KEY_PRIMARY_FILE",
        )?;
        let worker_api_key = load_secret(
            "QUIRON_LLM_API_KEY_WORKER",
            "QUIRON_LLM_API_KEY_WORKER_FILE",
        )?;

        let primary_timeout_secs =
            parse_timeout_secs(std::env::var("QUIRON_LLM_TIMEOUT_PRIMARY_SECS").ok(), 300);
        let worker_timeout_secs = parse_timeout_secs(
            std::env::var("QUIRON_LLM_TIMEOUT_WORKER_SECS").ok(),
            primary_timeout_secs,
        );
        let worker_temperature =
            parse_temperature(std::env::var("QUIRON_LLM_TEMPERATURE_WORKER").ok(), 0.0);
        let worker_ollama_think =
            parse_optional_json_scalar(std::env::var("QUIRON_OLLAMA_WORKER_THINK").ok());
        let worker_ollama_num_ctx =
            parse_optional_u32(std::env::var("QUIRON_OLLAMA_WORKER_NUM_CTX").ok());
        let worker_ollama_format_json = env_bool("QUIRON_OLLAMA_WORKER_FORMAT_JSON", true);
        let openclaw_cmd = normalized(std::env::var("QUIRON_OPENCLAW_CMD").ok())
            .unwrap_or_else(|| "/home/kssose/miniforge3/bin/openclaw".to_string());
        let openclaw_agent_primary =
            normalized(std::env::var("QUIRON_OPENCLAW_AGENT_PRIMARY").ok())
                .unwrap_or_else(|| "main".to_string());
        let openclaw_agent_worker = normalized(std::env::var("QUIRON_OPENCLAW_AGENT_WORKER").ok())
            .unwrap_or_else(|| openclaw_agent_primary.clone());
        let openclaw_thinking = normalized(std::env::var("QUIRON_OPENCLAW_THINKING").ok());
        let openclaw_local = env_bool("QUIRON_OPENCLAW_LOCAL", false);

        // Direct provider config
        let default_auth_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/home/kssose"))
            .join(".codex")
            .join("auth.json");
        let codex_auth_file = normalized(std::env::var("QUIRON_CODEX_AUTH_FILE").ok())
            .map(PathBuf::from)
            .unwrap_or(default_auth_path);
        let codex_model = normalized(std::env::var("QUIRON_CODEX_MODEL").ok())
            .unwrap_or_else(|| "gpt-5.5".to_string());

        Ok(Self {
            primary_backend,
            worker_backend,
            primary_endpoint,
            worker_endpoint,
            primary_model,
            worker_model,
            primary_api_key,
            worker_api_key,
            primary_timeout_secs,
            worker_timeout_secs,
            worker_temperature,
            worker_ollama_think,
            worker_ollama_num_ctx,
            worker_ollama_format_json,
            openclaw_cmd,
            openclaw_agent_primary,
            openclaw_agent_worker,
            openclaw_thinking,
            openclaw_local,
            codex_auth_file,
            codex_model,
        })
    }
}

fn normalized_or(value: Option<String>, fallback: &str) -> String {
    normalized(value).unwrap_or_else(|| fallback.to_string())
}

fn normalized(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn parse_timeout_secs(value: Option<String>, fallback: u64) -> u64 {
    value
        .as_deref()
        .map(str::trim)
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

fn parse_temperature(value: Option<String>, fallback: f32) -> f32 {
    value
        .as_deref()
        .map(str::trim)
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(fallback)
}

fn parse_optional_u32(value: Option<String>) -> Option<u32> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0)
}

fn parse_optional_json_scalar(value: Option<String>) -> Option<Value> {
    let raw = normalized(value)?;
    match raw.to_ascii_lowercase().as_str() {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        _ => Some(Value::String(raw)),
    }
}

fn env_bool(name: &str, fallback: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => fallback,
        },
        Err(_) => fallback,
    }
}

fn load_secret(env_key: &str, file_env_key: &str) -> Result<Option<String>, String> {
    if let Some(secret_file) = normalized(std::env::var(file_env_key).ok()) {
        let secret = read_secret_file(&secret_file).map_err(|err| {
            format!(
                "cannot read {} from {}='{}': {}",
                env_key, file_env_key, secret_file, err
            )
        })?;
        return Ok(Some(secret));
    }

    Ok(normalized(std::env::var(env_key).ok()))
}

fn read_secret_file(path: &str) -> Result<String, String> {
    let symlink_meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("stat failed for '{}': {}", path, e))?;

    if symlink_meta.file_type().is_symlink() {
        return Err("symlink not allowed for secret file".into());
    }

    if !symlink_meta.is_file() {
        return Err("path is not a regular file".into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = symlink_meta.permissions().mode() & 0o777;
        if (mode & 0o077) != 0 {
            return Err(format!(
                "insecure file mode {:o}; require owner-only permissions (e.g. 600)",
                mode
            ));
        }
    }

    let content =
        std::fs::read_to_string(Path::new(path)).map_err(|e| format!("read failed: {}", e))?;
    let secret = content.trim().to_string();
    if secret.is_empty() {
        return Err("empty secret file".into());
    }

    Ok(secret)
}

// ============================================================================
// TIPOS
// ============================================================================

/// Request desde quiron-brain
#[derive(Debug, Deserialize)]
struct GatewayRequest {
    prompt: String,
    #[serde(default)]
    system: Option<String>,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    route: Option<String>,
    /// Herramientas ofrecidas al modelo. Solo las transporta el camino de Codex.
    #[serde(default)]
    tools: Vec<ToolDef>,
    /// Conversación multi-turno como items heterogéneos (mensajes, llamadas a
    /// herramienta y sus resultados). Cuando viene vacía —todos los llamadores
    /// previos a la capa 4— se usa `prompt`, y el cuerpo hacia Codex es
    /// byte-idéntico al de siempre.
    #[serde(default)]
    input_items: Vec<GatewayInputItem>,
}

/// Un item de la conversación que quiron-brain reenvía al modelo.
///
/// Es el mínimo común: texto por rol, la llamada que el modelo emitió en un
/// turno anterior (se le devuelve tal cual la dijo) y el resultado que produjo
/// el editor tras el arnés. El gateway solo lo transporta; no interpreta nada.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GatewayInputItem {
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

/// Definición de una herramienta que el modelo puede invocar.
///
/// El gateway no ejecuta herramientas: solo las transporta hasta el modelo y
/// devuelve las llamadas que este emite. Quien las ejecuta —tras el arnés— es el
/// editor. `parameters` es un JSON Schema opaco para el gateway.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct ToolDef {
    name: String,
    #[serde(default)]
    description: String,
    parameters: serde_json::Value,
}

/// Una llamada a herramienta emitida por el modelo.
#[derive(Debug, Clone, Serialize)]
struct ToolCall {
    call_id: String,
    name: String,
    /// Argumentos como cadena JSON, tal cual los emite el modelo.
    arguments: String,
}

/// Response hacia quiron-brain
#[derive(Debug, Serialize, Default)]
struct GatewayResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Llamadas a herramienta pendientes de ejecutar. Presente solo cuando el
    /// modelo pide una herramienta en lugar de responder con texto.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TokenUsageSplit {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

/// Request a llama.cpp (formato OpenAI compatible)
#[derive(Debug, Serialize)]
struct LlamaRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
    /// Herramientas en el formato de OpenAI (`type: function`). Ausente si no
    /// se ofrece ninguna, para no cambiar el cuerpo de las llamadas de siempre.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDef>>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    /// Llamadas que el asistente emitió en un turno anterior (se le devuelven).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    /// En un mensaje `tool`, la llamada a la que responde.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl Message {
    fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
            thinking: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiToolDef {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiFunctionDef,
}

#[derive(Debug, Serialize)]
struct OpenAiFunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolCall {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default = "function_kind")]
    kind: String,
    function: OpenAiFunctionCall,
}

fn function_kind() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    /// Cadena JSON tal cual la emite el modelo.
    #[serde(default)]
    arguments: String,
}

/// `content` llega como `null` cuando el modelo solo pide herramientas.
fn nullable_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Response de llama.cpp
#[derive(Debug, Deserialize)]
struct LlamaResponse {
    choices: Option<Vec<Choice>>,
    usage: Option<LlamaUsage>,
    #[serde(default)]
    error: Option<LlamaError>,
}

#[derive(Debug, Deserialize)]
struct LlamaError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<Value>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    message: Option<ChoiceMessage>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct LlamaUsage {
    #[serde(default)]
    total_tokens: Option<u32>,
    #[serde(default, alias = "input_tokens", alias = "prompt_eval_count")]
    prompt_tokens: Option<u32>,
    #[serde(default, alias = "output_tokens", alias = "completion_eval_count")]
    completion_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenClawEnvelope {
    #[serde(default)]
    payloads: Vec<OpenClawPayload>,
    #[serde(default)]
    meta: Option<OpenClawMeta>,
    #[serde(default)]
    result: Option<OpenClawResultEnvelope>,
}

#[derive(Debug, Deserialize)]
struct OpenClawResultEnvelope {
    #[serde(default)]
    payloads: Vec<OpenClawPayload>,
    #[serde(default)]
    meta: Option<OpenClawMeta>,
}

#[derive(Debug, Deserialize)]
struct OpenClawPayload {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct OpenClawMeta {
    #[serde(default, rename = "agentMeta")]
    agent_meta: Option<OpenClawAgentMeta>,
}

#[derive(Debug, Deserialize)]
struct OpenClawAgentMeta {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<OpenClawUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenClawUsage {
    #[serde(default)]
    total: Option<u32>,
    #[serde(default)]
    input: Option<u32>,
    #[serde(default)]
    output: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
enum Route {
    Primary,
    Worker,
}

fn parse_route(route: Option<&str>) -> Result<Route, String> {
    match route.map(str::trim).filter(|r| !r.is_empty()) {
        None => Ok(Route::Primary),
        Some(raw) => match raw.to_ascii_lowercase().as_str() {
            "primary" | "planner" | "default" => Ok(Route::Primary),
            "worker" | "local-worker" => Ok(Route::Worker),
            other => Err(format!("invalid route '{}': use primary|worker", other)),
        },
    }
}

// ============================================================================
// MAIN
// ============================================================================

#[tokio::main]
async fn main() {
    let config = match GatewayConfig::from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("❌ vertex-gateway config error: {}", err);
            std::process::exit(2);
        }
    };
    eprintln!("🧠 vertex-gateway initialized (Split-Brain Support)");

    // Print Primary Setup
    match config.primary_backend {
        GatewayBackend::ClaudeCli => eprintln!("   [Primary] Claude CLI: sesión oficial, herramientas de Quirón"),
        GatewayBackend::OpenAiCompatible => {
            let model = config.primary_model.as_deref().unwrap_or("<unset>");
            let auth = if config.primary_api_key.is_some() {
                "api-key:set"
            } else {
                "api-key:unset"
            };
            eprintln!(
                "   [Primary] openai-compatible: endpoint={} model={} auth={} timeout={}s",
                config.primary_endpoint, model, auth, config.primary_timeout_secs
            );
        }
        GatewayBackend::OllamaNative => {
            let model = config.primary_model.as_deref().unwrap_or("<unset>");
            eprintln!(
                "   [Primary] ollama-native: endpoint={} model={} timeout={}s",
                config.primary_endpoint, model, config.primary_timeout_secs
            );
        }
        GatewayBackend::OpenClaw => {
            eprintln!(
                "   [Primary] openclaw: cmd='{}' agent={} thinking={} local={} timeout={}s",
                config.openclaw_cmd,
                config.openclaw_agent_primary,
                config.openclaw_thinking.as_deref().unwrap_or("default"),
                if config.openclaw_local { "on" } else { "off" },
                config.primary_timeout_secs
            );
        }
        GatewayBackend::CodexDirect => {
            eprintln!(
                "   [Primary] codex-direct: model={} auth_file={} timeout={}s",
                config.codex_model,
                config.codex_auth_file.display(),
                config.primary_timeout_secs
            );
        }
    }

    // Print Worker Setup
    match config.worker_backend {
        GatewayBackend::ClaudeCli => eprintln!("   [Worker] Claude CLI: sesión oficial"),
        GatewayBackend::OpenAiCompatible => {
            let model = config.worker_model.as_deref().unwrap_or("<unset>");
            let auth = if config.worker_api_key.is_some() {
                "api-key:set"
            } else {
                "api-key:unset"
            };
            eprintln!(
                "   [Worker]  openai-compatible: endpoint={} model={} auth={} timeout={}s",
                config.worker_endpoint, model, auth, config.worker_timeout_secs
            );
        }
        GatewayBackend::OllamaNative => {
            let model = config.worker_model.as_deref().unwrap_or("<unset>");
            let think = config
                .worker_ollama_think
                .as_ref()
                .map(Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            let num_ctx = config
                .worker_ollama_num_ctx
                .map(|value| value.to_string())
                .unwrap_or_else(|| "default".to_string());
            eprintln!(
                "   [Worker]  ollama-native: endpoint={} model={} think={} format_json={} num_ctx={} temp={} timeout={}s",
                config.worker_endpoint,
                model,
                think,
                config.worker_ollama_format_json,
                num_ctx,
                config.worker_temperature,
                config.worker_timeout_secs
            );
        }
        GatewayBackend::OpenClaw => {
            eprintln!(
                "   [Worker]  openclaw: cmd='{}' agent={} thinking={} local={} timeout={}s",
                config.openclaw_cmd,
                config.openclaw_agent_worker,
                config.openclaw_thinking.as_deref().unwrap_or("default"),
                if config.openclaw_local { "on" } else { "off" },
                config.worker_timeout_secs
            );
        }
        GatewayBackend::CodexDirect => {
            eprintln!(
                "   [Worker]  codex-direct: model={} auth_file={} timeout={}s",
                config.codex_model,
                config.codex_auth_file.display(),
                config.worker_timeout_secs
            );
        }
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let resp = GatewayResponse {
                    ok: false,
                    tool_calls: None,
                    content: None,
                    error: Some(format!("IO error: {}", e)),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: GatewayRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = GatewayResponse {
                    ok: false,
                    tool_calls: None,
                    content: None,
                    error: Some(format!("JSON parse error: {}", e)),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                continue;
            }
        };

        let response = process_request(request, &config).await;
        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
        let _ = stdout.flush();
    }
}

fn route_timeout_secs(route: Route, config: &GatewayConfig) -> u64 {
    match route {
        Route::Primary => config.primary_timeout_secs,
        Route::Worker => config.worker_timeout_secs,
    }
}

fn route_model(route: Route, config: &GatewayConfig) -> Option<String> {
    match route {
        Route::Primary => config.primary_model.clone(),
        Route::Worker => config.worker_model.clone(),
    }
}

fn route_temperature(route: Route, config: &GatewayConfig) -> f32 {
    match route {
        Route::Primary => 0.7,
        Route::Worker => config.worker_temperature,
    }
}

fn route_openclaw_agent(route: Route, config: &GatewayConfig) -> String {
    match route {
        Route::Primary => config.openclaw_agent_primary.clone(),
        Route::Worker => config.openclaw_agent_worker.clone(),
    }
}

fn derive_token_split(
    total_tokens: Option<u32>,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
) -> TokenUsageSplit {
    let resolved_input = input_tokens.or_else(|| {
        total_tokens
            .zip(output_tokens)
            .map(|(total, output)| total.saturating_sub(output))
    });
    let resolved_output = output_tokens.or_else(|| {
        total_tokens
            .zip(input_tokens)
            .map(|(total, input)| total.saturating_sub(input))
    });

    TokenUsageSplit {
        input_tokens: resolved_input,
        output_tokens: resolved_output,
    }
}

fn llama_usage_split(usage: &LlamaUsage) -> TokenUsageSplit {
    derive_token_split(
        usage.total_tokens,
        usage.prompt_tokens,
        usage.completion_tokens,
    )
}

/// Procesa una request y despacha al backend configurado.
async fn process_request(req: GatewayRequest, config: &GatewayConfig) -> GatewayResponse {
    let route = match parse_route(req.route.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(e),
                input_tokens: None,
                output_tokens: None,
                model: None,
            };
        }
    };

    let backend = match route {
        Route::Primary => config.primary_backend,
        Route::Worker => config.worker_backend,
    };

    let etiqueta = match backend {
        GatewayBackend::ClaudeCli => "claude_cli",
        GatewayBackend::OpenAiCompatible => "openai_compatible",
        GatewayBackend::OllamaNative => "ollama_native",
        GatewayBackend::OpenClaw => "openclaw",
        GatewayBackend::CodexDirect => "codex_direct",
    };
    let respuesta = match backend {
        GatewayBackend::ClaudeCli => process_request_claude_cli(req, route, config).await,
        GatewayBackend::OpenAiCompatible => {
            process_request_openai_compatible(req, route, config).await
        }
        GatewayBackend::OllamaNative => process_request_ollama_native(req, route, config).await,
        GatewayBackend::OpenClaw => process_request_openclaw(req, route, config).await,
        GatewayBackend::CodexDirect => process_request_codex_direct(req, route, config).await,
    };
    // Una línea por llamada, igual para todos los adaptadores: el cerebro la
    // vuelca a su registro y el arnés sabe por ella cuándo terminó un turno.
    eprintln!(
        "[gateway] backend={} model={} ok={} tool_calls={} content_chars={}{}",
        etiqueta,
        respuesta.model.as_deref().unwrap_or("?"),
        respuesta.ok,
        respuesta.tool_calls.as_ref().map_or(0, Vec::len),
        respuesta.content.as_ref().map_or(0, |c| c.chars().count()),
        respuesta
            .error
            .as_ref()
            .map(|e| format!(" error={}", truncate_for_error(e, 160).replace('\n', " ")))
            .unwrap_or_default()
    );
    respuesta
}

/// Backend legacy OpenAI-compatible (`/v1/chat/completions`).
async fn process_request_openai_compatible(
    req: GatewayRequest,
    route: Route,
    config: &GatewayConfig,
) -> GatewayResponse {
    let endpoint = match route {
        Route::Primary => config.primary_endpoint.clone(),
        Route::Worker => config.worker_endpoint.clone(),
    };
    let api_key = match route {
        Route::Primary => config.primary_api_key.as_deref(),
        Route::Worker => config.worker_api_key.as_deref(),
    };
    let timeout_secs = route_timeout_secs(route, config);
    let route_model = route_model(route, config);

    let model = match normalized(req.model).or(route_model) {
        Some(m) => m,
        None => {
            let missing = match route {
                Route::Primary => "QUIRON_LLM_MODEL_PRIMARY",
                Route::Worker => "QUIRON_LLM_MODEL_WORKER",
            };
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!(
                    "model not configured for route; send request.model or set {}",
                    missing
                )),
                input_tokens: None,
                output_tokens: None,
                model: None,
            };
        }
    };

    let mut messages = Vec::new();
    if let Some(system) = req.system {
        messages.push(Message::text("system", system));
    }
    // Con conversación estructurada, `prompt` es su aplanado: se ignora para
    // no duplicar el contexto (misma regla que en Codex y Claude CLI).
    if req.input_items.is_empty() {
        messages.push(Message::text("user", req.prompt));
    } else {
        messages.extend(openai_messages_from_items(&req.input_items));
    }

    let tools_ofrecidas = req.tools;
    let llama_req = LlamaRequest {
        model: model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: route_temperature(route, config),
        stream: false,
        tools: if tools_ofrecidas.is_empty() {
            None
        } else {
            Some(tools_ofrecidas.iter().map(openai_tool_def).collect())
        },
    };
    let url = openai_chat_url(&endpoint);

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("HTTP client error: {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    let mut request_builder = client.post(&url).header("Content-Type", "application/json");
    if let Some(key) = api_key {
        request_builder = request_builder.bearer_auth(key);
    }

    let response = match request_builder.json(&llama_req).send().await {
        Ok(r) => r,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("Request error (¿llama-server corriendo?): {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    let status = response.status();
    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("Response read error: {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    if !status.is_success() {
        return GatewayResponse {
            ok: false,
            tool_calls: None,
            content: None,
            error: Some(format!("API error ({}): {}", status, body)),
            input_tokens: None,
            output_tokens: None,
            model: Some(model),
        };
    }

    let llama_resp: LlamaResponse = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!(
                    "JSON parse error: {} - Body: {}",
                    e,
                    truncate_for_error(&body, 200)
                )),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    if let Some(err) = llama_resp.error {
        return GatewayResponse {
            ok: false,
            tool_calls: None,
            content: None,
            error: Some(format!("LLM error: {}", err.message)),
            input_tokens: None,
            output_tokens: None,
            model: Some(model),
        };
    }

    let (mut text, estructuradas) = llama_resp
        .choices
        .and_then(|c| c.into_iter().next())
        .map(|c| (c.message.content, c.message.tool_calls.unwrap_or_default()))
        .unwrap_or_default();
    let usage = llama_resp
        .usage
        .as_ref()
        .map(llama_usage_split)
        .unwrap_or_default();

    let mut llamadas: Vec<ToolCall> = estructuradas
        .into_iter()
        .enumerate()
        .map(|(i, c)| ToolCall {
            call_id: if c.id.is_empty() { format!("call_{}_{}", now_epoch_secs(), i) } else { c.id },
            name: c.function.name,
            arguments: if c.function.arguments.is_empty() { "{}".to_string() } else { c.function.arguments },
        })
        .collect();
    if llamadas.is_empty() {
        if let Some(llamada) = tool_call_from_text(&text, &tools_ofrecidas) {
            llamadas.push(llamada);
            text.clear();
        }
    }

    GatewayResponse {
        ok: true,
        tool_calls: if llamadas.is_empty() { None } else { Some(llamadas) },
        content: Some(text),
        error: None,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        model: Some(model),
    }
}

/// `endpoint` puede venir con o sin `/v1` (OpenAI, Ollama y llama-server lo
/// sirven bajo `/v1`); no se duplica.
fn openai_chat_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn openai_tool_def(tool: &ToolDef) -> OpenAiToolDef {
    OpenAiToolDef {
        kind: "function",
        function: OpenAiFunctionDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        },
    }
}

/// La conversación del cerebro en mensajes de OpenAI: las llamadas
/// consecutivas del asistente van juntas en un mensaje `assistant` con
/// `tool_calls`, y cada resultado es un mensaje `tool` con su `tool_call_id`.
fn openai_messages_from_items(items: &[GatewayInputItem]) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for item in items {
        match item {
            GatewayInputItem::Message { role, text } => out.push(Message::text(role, text.clone())),
            GatewayInputItem::FunctionCall { call_id, name, arguments } => {
                let llamada = OpenAiToolCall {
                    id: call_id.clone(),
                    kind: "function".to_string(),
                    function: OpenAiFunctionCall { name: name.clone(), arguments: arguments.clone() },
                };
                match out.last_mut() {
                    Some(m) if m.role == "assistant" && m.tool_calls.is_some() => {
                        m.tool_calls.as_mut().expect("comprobado").push(llamada);
                    }
                    _ => out.push(Message {
                        role: "assistant".to_string(),
                        content: String::new(),
                        thinking: None,
                        tool_calls: Some(vec![llamada]),
                        tool_call_id: None,
                    }),
                }
            }
            GatewayInputItem::FunctionCallOutput { call_id, output } => out.push(Message {
                role: "tool".to_string(),
                content: output.clone(),
                thinking: None,
                tool_calls: None,
                tool_call_id: Some(call_id.clone()),
            }),
        }
    }
    out
}

/// Los modelos pequeños servidos en local (p. ej. Qwen2.5-Coder 1.5B en
/// llama-server) no siempre emiten la llamada por el canal estructurado: la
/// escriben como un objeto JSON, a veces dentro de un bloque de código o de
/// etiquetas `<tool_call>`. Si el contenido es solo eso y nombra una
/// herramienta ofrecida, se acepta como llamada; cualquier otra cosa es texto.
fn tool_call_from_text(content: &str, tools: &[ToolDef]) -> Option<ToolCall> {
    if tools.is_empty() {
        return None;
    }
    let mut limpio = content.trim();
    for marca in ["<tool_call>", "</tool_call>", "```json", "```"] {
        limpio = limpio.trim_start_matches(marca).trim_end_matches(marca).trim();
    }
    if !limpio.starts_with('{') || !limpio.ends_with('}') {
        return None;
    }
    let valor: Value = serde_json::from_str(limpio).ok()?;
    let cuerpo = valor.get("function").unwrap_or(&valor);
    let name = cuerpo.get("name").and_then(Value::as_str)?;
    if !tools.iter().any(|t| t.name == name) {
        return None;
    }
    let argumentos = cuerpo
        .get("arguments")
        .or_else(|| cuerpo.get("parameters"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let arguments = match argumentos {
        Value::String(s) => s,
        otro => otro.to_string(),
    };
    Some(ToolCall { call_id: format!("call_{}", now_epoch_secs()), name: name.to_string(), arguments })
}

async fn process_request_ollama_native(
    req: GatewayRequest,
    route: Route,
    config: &GatewayConfig,
) -> GatewayResponse {
    let endpoint = match route {
        Route::Primary => config.primary_endpoint.clone(),
        Route::Worker => config.worker_endpoint.clone(),
    };
    let timeout_secs = route_timeout_secs(route, config);
    let route_model = route_model(route, config);

    let model = match normalized(req.model).or(route_model) {
        Some(m) => m,
        None => {
            let missing = match route {
                Route::Primary => "QUIRON_LLM_MODEL_PRIMARY",
                Route::Worker => "QUIRON_LLM_MODEL_WORKER",
            };
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!(
                    "model not configured for route; send request.model or set {}",
                    missing
                )),
                input_tokens: None,
                output_tokens: None,
                model: None,
            };
        }
    };

    let mut messages = Vec::new();
    if let Some(system) = req.system {
        messages.push(Message::text("system", system));
    }
    messages.push(Message::text("user", req.prompt));

    let options = match route {
        Route::Primary => Some(OllamaOptions {
            temperature: Some(route_temperature(route, config)),
            num_ctx: None,
        }),
        Route::Worker => Some(OllamaOptions {
            temperature: Some(route_temperature(route, config)),
            num_ctx: config.worker_ollama_num_ctx,
        }),
    };

    let ollama_req = OllamaChatRequest {
        model: model.clone(),
        messages,
        format: match route {
            Route::Worker if config.worker_ollama_format_json => {
                Some(Value::String("json".to_string()))
            }
            _ => None,
        },
        options,
        stream: false,
        think: match route {
            Route::Worker => config.worker_ollama_think.clone(),
            Route::Primary => None,
        },
    };

    let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("HTTP client error: {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    let response = match client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&ollama_req)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("Request error (¿ollama corriendo?): {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    let status = response.status();
    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("Response read error: {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    if !status.is_success() {
        return GatewayResponse {
            ok: false,
            tool_calls: None,
            content: None,
            error: Some(format!("API error ({}): {}", status, body)),
            input_tokens: None,
            output_tokens: None,
            model: Some(model),
        };
    }

    let ollama_resp: OllamaChatResponse = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!(
                    "JSON parse error: {} - Body: {}",
                    e,
                    truncate_for_error(&body, 200)
                )),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    if let Some(err) = ollama_resp.error {
        return GatewayResponse {
            ok: false,
            tool_calls: None,
            content: None,
            error: Some(format!("LLM error: {}", err)),
            input_tokens: None,
            output_tokens: None,
            model: Some(model),
        };
    }

    let message = ollama_resp.message.unwrap_or(ChoiceMessage {
        content: String::new(),
        reasoning: None,
        tool_calls: None,
    });
    if message.content.trim().is_empty() && message.reasoning.is_some() {
        return GatewayResponse {
            ok: false,
            tool_calls: None,
            content: None,
            error: Some(
                "Ollama returned reasoning without final content; verify think=false for worker"
                    .to_string(),
            ),
            input_tokens: ollama_resp.prompt_eval_count,
            output_tokens: ollama_resp.eval_count,
            model: Some(ollama_resp.model.unwrap_or(model)),
        };
    }

    GatewayResponse {
        ok: true,
        tool_calls: None,
        content: Some(message.content),
        error: None,
        input_tokens: ollama_resp.prompt_eval_count,
        output_tokens: ollama_resp.eval_count,
        model: Some(ollama_resp.model.unwrap_or(model)),
    }
}

/// Backend OpenClaw CLI (`openclaw agent --json`).
async fn process_request_openclaw(
    req: GatewayRequest,
    route: Route,
    config: &GatewayConfig,
) -> GatewayResponse {
    let route_model = route_model(route, config);
    let requested_model = normalized(req.model).or(route_model);
    let timeout_secs = route_timeout_secs(route, config);
    let openclaw_agent = route_openclaw_agent(route, config);
    let message = compose_openclaw_message(req.prompt, req.system);
    let args = build_openclaw_command_args(
        &openclaw_agent,
        &message,
        timeout_secs,
        config.openclaw_thinking.as_deref(),
        config.openclaw_local,
    );

    let output = match run_command_capture(&config.openclaw_cmd, &args, timeout_secs).await {
        Ok(out) => out,
        Err(err) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("openclaw spawn failed: {}", err)),
                input_tokens: None,
                output_tokens: None,
                model: requested_model,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let stderr_line = first_non_empty_line(&stderr)
            .map(sanitize_openclaw_error)
            .unwrap_or_else(|| "<empty>".to_string());
        let stdout_line = first_non_empty_line(&stdout)
            .map(sanitize_openclaw_error)
            .unwrap_or_else(|| "<empty>".to_string());
        return GatewayResponse {
            ok: false,
            tool_calls: None,
            content: None,
            error: Some(format!(
                "openclaw command failed (status {:?}): stderr={} stdout={}",
                output.status.code(),
                truncate_for_error(&stderr_line, 180),
                truncate_for_error(&stdout_line, 180)
            )),
            input_tokens: None,
            output_tokens: None,
            model: requested_model,
        };
    }

    let envelope = match parse_openclaw_envelope(&stdout) {
        Ok(v) => v,
        Err(err) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!(
                    "openclaw JSON parse error: {} | stdout={}",
                    err,
                    truncate_for_error(&stdout, 280)
                )),
                input_tokens: None,
                output_tokens: None,
                model: requested_model,
            };
        }
    };

    let payloads = openclaw_payloads(&envelope);
    let meta = openclaw_meta(&envelope);
    let text = collect_openclaw_text(payloads);
    let detected_model = meta
        .as_ref()
        .and_then(|m| m.agent_meta.as_ref())
        .and_then(|a| normalized(a.model.clone()));
    let usage = meta
        .as_ref()
        .and_then(|m| m.agent_meta.as_ref())
        .and_then(|a| a.usage.as_ref())
        .map(openclaw_usage_split)
        .unwrap_or_default();

    GatewayResponse {
        ok: true,
        tool_calls: None,
        content: Some(text),
        error: None,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        model: detected_model.or(requested_model),
    }
}

fn compose_openclaw_message(prompt: String, system: Option<String>) -> String {
    match system
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(system_prompt) => format!("[SYSTEM]\n{}\n\n[USER]\n{}", system_prompt, prompt),
        None => prompt,
    }
}

fn build_openclaw_command_args(
    agent_id: &str,
    message: &str,
    timeout_secs: u64,
    thinking: Option<&str>,
    local_mode: bool,
) -> Vec<String> {
    let mut args = vec![
        "agent".to_string(),
        "--agent".to_string(),
        agent_id.to_string(),
        "--message".to_string(),
        message.to_string(),
        "--json".to_string(),
        "--timeout".to_string(),
        timeout_secs.to_string(),
    ];

    if let Some(level) = thinking {
        args.push("--thinking".to_string());
        args.push(level.to_string());
    }
    if local_mode {
        args.push("--local".to_string());
    }
    args
}

async fn run_command_capture(
    cmd: &str,
    args: &[String],
    timeout_secs: u64,
) -> Result<std::process::Output, String> {
    let mut command = Command::new(cmd);
    command
        .kill_on_drop(true)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let timeout = std::time::Duration::from_secs(timeout_secs.saturating_add(5));
    match tokio::time::timeout(timeout, command.output()).await {
        Ok(result) => result.map_err(|e| e.to_string()),
        Err(_) => Err(format!(
            "openclaw command timed out after {}s",
            timeout.as_secs()
        )),
    }
}

fn parse_openclaw_envelope(stdout: &str) -> Result<OpenClawEnvelope, String> {
    let json_slice = extract_json_object_slice(stdout)
        .ok_or_else(|| "no JSON object found in stdout".to_string())?;
    serde_json::from_str(json_slice).map_err(|e| e.to_string())
}

fn extract_json_object_slice(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    Some(&text[start..=end])
}

fn collect_openclaw_text(payloads: &[OpenClawPayload]) -> String {
    let chunks: Vec<&str> = payloads
        .iter()
        .map(|p| p.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    chunks.join("\n\n")
}

fn openclaw_payloads(envelope: &OpenClawEnvelope) -> &[OpenClawPayload] {
    if !envelope.payloads.is_empty() {
        &envelope.payloads
    } else if let Some(result) = envelope.result.as_ref() {
        &result.payloads
    } else {
        &[]
    }
}

fn openclaw_meta(envelope: &OpenClawEnvelope) -> Option<&OpenClawMeta> {
    envelope
        .meta
        .as_ref()
        .or_else(|| envelope.result.as_ref().and_then(|r| r.meta.as_ref()))
}

fn openclaw_usage_split(usage: &OpenClawUsage) -> TokenUsageSplit {
    derive_token_split(usage.total, usage.input, usage.output)
}

fn truncate_for_error(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }
    let mut out = String::new();
    for (idx, ch) in trimmed.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.to_string())
}

fn sanitize_openclaw_error(text: String) -> String {
    text.replace("API key", "auth").replace("api key", "auth")
}

// ============================================================================
// OAUTH TOKEN MANAGEMENT
// ============================================================================

// Solo `Deserialize`: estos structs contienen el token OAuth. No derivan `Debug`
// ni `Serialize`, de modo que un `{:?}` o una serialización accidental no puedan
// filtrar el secreto a un log o a la salida.
#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: CodexAuthTokens,
}

#[derive(Deserialize)]
struct CodexAuthTokens {
    access_token: String,
    #[serde(default)]
    account_id: Option<String>,
}

fn read_codex_auth_file(path: &Path) -> Result<CodexAuthFile, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| {
        format!(
            "cannot stat '{}': {} (sign in with Codex or the VS Code extension)",
            path.display(),
            e
        )
    })?;
    if meta.file_type().is_symlink() {
        return Err("symlink not allowed for token file".into());
    }
    if !meta.is_file() {
        return Err("token path is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode() & 0o777;
        if (mode & 0o077) != 0 {
            return Err(format!(
                "insecure token file mode {:o}; require owner-only (600)",
                mode
            ));
        }
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("invalid Codex auth JSON in '{}': {}", path.display(), e))
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Read the credential maintained by Codex/VS Code without refreshing or rewriting it.
fn get_codex_access_token(config: &GatewayConfig) -> Result<(String, Option<String>), String> {
    let auth = read_codex_auth_file(&config.codex_auth_file)?;
    let tokens = auth.tokens;

    if extract_jwt_expiry(&tokens.access_token)
        .is_some_and(|expiry| now_epoch_secs() + 60 >= expiry)
    {
        return Err(
            "Codex access token expired; sign in or refresh the session in VS Code/Codex"
                .to_string(),
        );
    }

    // Extract account_id from JWT or stored value
    let account_id = tokens.account_id.clone().or_else(|| {
        extract_jwt_claim(
            &tokens.access_token,
            "https://api.openai.com/auth",
            "chatgpt_account_id",
        )
    });

    Ok((tokens.access_token, account_id))
}

fn decode_jwt_payload(jwt: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = parts[1];
    let padded = match payload.len() % 4 {
        2 => format!("{}==", payload),
        3 => format!("{}=", payload),
        _ => payload.to_string(),
    };
    let decoded = padded.replace('-', "+").replace('_', "/");
    let bytes = base64_decode(&decoded)?;
    serde_json::from_slice(&bytes).ok()
}

fn extract_jwt_expiry(jwt: &str) -> Option<u64> {
    decode_jwt_payload(jwt)?.get("exp")?.as_u64()
}

fn extract_jwt_claim(jwt: &str, namespace: &str, key: &str) -> Option<String> {
    let json = decode_jwt_payload(jwt)?;
    // Try namespace.key path
    json.get(namespace)
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            json.get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Simple base64 decoder - no external crate needed for JWT parsing
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'=').collect();
    for chunk in bytes.chunks(4) {
        let vals: Vec<u8> = chunk
            .iter()
            .filter_map(|&b| TABLE.iter().position(|&t| t == b).map(|p| p as u8))
            .collect();
        if vals.len() >= 2 {
            output.push((vals[0] << 2) | (vals[1] >> 4));
        }
        if vals.len() >= 3 {
            output.push((vals[1] << 4) | (vals[2] >> 2));
        }
        if vals.len() >= 4 {
            output.push((vals[2] << 6) | vals[3]);
        }
    }
    Some(output)
}

// ============================================================================
// OPENAI CODEX — DIRECT BACKEND
// ============================================================================

#[derive(Serialize)]
struct CodexRequest {
    model: String,
    store: bool,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<CodexInputItem>,
    text: CodexTextConfig,
    include: Vec<String>,
    /// Herramientas ofrecidas al modelo. Se omite si no hay ninguna, para no
    /// alterar el cuerpo de las peticiones sin herramientas.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<CodexTool>,
}

/// Herramienta en el formato de la Responses API de Codex.
#[derive(Serialize)]
struct CodexTool {
    #[serde(rename = "type")]
    tool_type: String,
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// El endpoint de Codex por cuenta de suscripción RECHAZA `max_output_tokens`
// («Unsupported parameter», 400). Probado el 2026-07-10. Por eso `max_tokens` del
// request no se propaga a Codex: no se puede. Si algún día se soporta, el campo
// va aquí y se rellena en `build_codex_request`.

/// Un item del array `input` de la Responses API.
///
/// `untagged`: la variante `Message` serializa `{role, content}` sin campo
/// `type`, exactamente el cuerpo verificado contra la cuenta antes de la capa
/// 4. Las otras dos variantes llevan su `type` explícito, como los emite y
/// espera el propio endpoint.
#[derive(Serialize)]
#[serde(untagged)]
enum CodexInputItem {
    Message(CodexInputMessage),
    FunctionCall(CodexFunctionCallItem),
    FunctionCallOutput(CodexFunctionCallOutputItem),
}

#[derive(Serialize)]
struct CodexInputMessage {
    role: String,
    content: Vec<CodexContentPart>,
}

/// Eco de una llamada emitida por el modelo en un turno anterior. Se devuelve
/// solo con su `call_id` (sin el `id` interno `fc_...`), para que el servidor
/// no intente correlacionarla con items de razonamiento que no conservamos.
#[derive(Serialize)]
struct CodexFunctionCallItem {
    #[serde(rename = "type")]
    item_type: &'static str,
    call_id: String,
    name: String,
    arguments: String,
}

/// Resultado de una herramienta ejecutada por el editor tras el arnés.
#[derive(Serialize)]
struct CodexFunctionCallOutputItem {
    #[serde(rename = "type")]
    item_type: &'static str,
    call_id: String,
    output: String,
}

#[derive(Serialize)]
struct CodexContentPart {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

/// Traduce los items de la conversación al formato de la Responses API.
///
/// La API distingue quién dijo cada texto: lo del asistente viaja como
/// `output_text`; lo del usuario (y system), como `input_text`. Confundirlos
/// es un 400.
fn codex_input_from_items(items: &[GatewayInputItem]) -> Vec<CodexInputItem> {
    items
        .iter()
        .map(|item| match item {
            GatewayInputItem::Message { role, text } => {
                let content_type = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                CodexInputItem::Message(CodexInputMessage {
                    role: role.clone(),
                    content: vec![CodexContentPart {
                        content_type: content_type.to_string(),
                        text: text.clone(),
                    }],
                })
            }
            GatewayInputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => CodexInputItem::FunctionCall(CodexFunctionCallItem {
                item_type: "function_call",
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            GatewayInputItem::FunctionCallOutput { call_id, output } => {
                CodexInputItem::FunctionCallOutput(CodexFunctionCallOutputItem {
                    item_type: "function_call_output",
                    call_id: call_id.clone(),
                    output: output.clone(),
                })
            }
        })
        .collect()
}

#[derive(Serialize)]
struct CodexTextConfig {
    verbosity: String,
}

fn build_codex_request(
    prompt: &str,
    system: Option<&str>,
    model: &str,
    tools: &[ToolDef],
    input_items: &[GatewayInputItem],
) -> CodexRequest {
    // Con items explícitos (capa 4: conversación con herramientas) se reenvía
    // la conversación entera; `store: false` obliga a que viaje cada vez.
    // Sin ellos, el mensaje único de siempre, byte a byte.
    let input = if input_items.is_empty() {
        vec![CodexInputItem::Message(CodexInputMessage {
            role: "user".to_string(),
            content: vec![CodexContentPart {
                content_type: "input_text".to_string(),
                text: prompt.to_string(),
            }],
        })]
    } else {
        codex_input_from_items(input_items)
    };

    let instructions = system
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let tools = tools
        .iter()
        .map(|tool| CodexTool {
            tool_type: "function".to_string(),
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        })
        .collect();

    CodexRequest {
        model: model.to_string(),
        store: false,
        stream: true,
        instructions,
        input,
        text: CodexTextConfig {
            verbosity: "medium".to_string(),
        },
        include: vec!["reasoning.encrypted_content".to_string()],
        tools,
    }
}

/// Parse SSE stream from Codex Responses API.
/// Resultado de interpretar el flujo SSE de la Responses API.
#[derive(Default)]
struct CodexParsed {
    text: String,
    tool_calls: Vec<ToolCall>,
    usage: TokenUsageSplit,
    model: Option<String>,
}

/// Interpreta el flujo SSE de Codex: acumula el texto de
/// `response.output_text.delta` y recoge las llamadas a herramienta que el
/// modelo emite como items `function_call` dentro de `response.completed`.
fn parse_codex_sse_response(body: &str) -> CodexParsed {
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut token_usage = TokenUsageSplit::default();
    let mut detected_model: Option<String> = None;

    for line in body.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                break;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                let event_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");

                match event_type {
                    "response.output_text.delta" => {
                        if let Some(delta) = val.get("delta").and_then(|d| d.as_str()) {
                            text_parts.push(delta.to_string());
                        }
                    }
                    // La llamada a herramienta llega completa aquí, en su propio
                    // item, no dentro de `response.completed`.
                    "response.output_item.done" => {
                        if let Some(item) = val.get("item") {
                            if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                if let Some(call) = tool_call_from_item(item) {
                                    tool_calls.push(call);
                                }
                            }
                        }
                    }
                    "response.completed" | "response.done" => {
                        if let Some(resp) = val.get("response") {
                            // Extract model
                            if let Some(m) = resp.get("model").and_then(|m| m.as_str()) {
                                detected_model = Some(m.to_string());
                            }
                            // Extract usage
                            if let Some(usage) = resp.get("usage") {
                                let input = usage
                                    .get("input_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                let output = usage
                                    .get("output_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                token_usage = TokenUsageSplit {
                                    input_tokens: Some(input),
                                    output_tokens: Some(output),
                                };
                            }
                            if let Some(outputs) = resp.get("output").and_then(|o| o.as_array()) {
                                for output in outputs {
                                    let item_type =
                                        output.get("type").and_then(|t| t.as_str()).unwrap_or("");

                                    // Una llamada a herramienta emitida por el modelo.
                                    if item_type == "function_call" {
                                        if let Some(call) = tool_call_from_item(output) {
                                            tool_calls.push(call);
                                        }
                                        continue;
                                    }

                                    // Texto de respaldo si no llegó por deltas.
                                    if text_parts.is_empty() {
                                        if let Some(t) =
                                            output.get("text").and_then(|t| t.as_str())
                                        {
                                            text_parts.push(t.to_string());
                                        }
                                        if let Some(content) =
                                            output.get("content").and_then(|c| c.as_array())
                                        {
                                            for item in content {
                                                if let Some(t) =
                                                    item.get("text").and_then(|t| t.as_str())
                                                {
                                                    text_parts.push(t.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "error" => {
                        let msg = val
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown codex error");
                        eprintln!("⚠️ vertex-gateway(codex): SSE error event: {}", msg);
                    }
                    _ => {}
                }
            }
        }
    }

    // Una llamada puede llegar por `output_item.done` y de nuevo en el array
    // `output` de `response.completed`. Se conserva una sola por `call_id`.
    tool_calls.dedup_by(|a, b| a.call_id == b.call_id);
    let mut vistos = std::collections::HashSet::new();
    tool_calls.retain(|call| vistos.insert(call.call_id.clone()));

    CodexParsed {
        text: text_parts.concat(),
        tool_calls,
        usage: token_usage,
        model: detected_model,
    }
}

/// Extrae una llamada a herramienta de un item `function_call` del array
/// `output`. Devuelve `None` si le faltan los campos imprescindibles.
fn tool_call_from_item(item: &serde_json::Value) -> Option<ToolCall> {
    let name = item.get("name").and_then(|n| n.as_str())?.to_string();
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|c| c.as_str())?
        .to_string();
    let arguments = item
        .get("arguments")
        .and_then(|a| a.as_str())
        .unwrap_or("{}")
        .to_string();
    Some(ToolCall {
        call_id,
        name,
        arguments,
    })
}

/// Backend OpenAI Codex Responses API (direct HTTP, no OpenClaw).
async fn process_request_codex_direct(
    req: GatewayRequest,
    route: Route,
    config: &GatewayConfig,
) -> GatewayResponse {
    let timeout_secs = route_timeout_secs(route, config);
    let model = normalized(req.model)
        .or_else(|| route_model(route, config))
        .unwrap_or_else(|| config.codex_model.clone());

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("HTTP client error: {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    let (access_token, account_id) = match get_codex_access_token(config) {
        Ok(t) => t,
        Err(e) => {
            return GatewayResponse {
                ok: false,
                tool_calls: None,
                content: None,
                error: Some(format!("oauth error: {}", e)),
                input_tokens: None,
                output_tokens: None,
                model: Some(model),
            };
        }
    };

    // `req.max_tokens` no se propaga: el endpoint de Codex por suscripción no
    // acepta límite de salida (ver nota en `CodexRequest`).
    let codex_req = build_codex_request(
        &req.prompt,
        req.system.as_deref(),
        &model,
        &req.tools,
        &req.input_items,
    );
    let url = "https://chatgpt.com/backend-api/codex/responses";

    let max_retries = 3usize;
    let mut last_error = String::new();

    for attempt in 0..=max_retries {
        let mut request_builder = client
            .post(url)
            .bearer_auth(&access_token)
            .header("Content-Type", "application/json")
            .header("OpenAI-Beta", "responses=experimental");

        if let Some(ref aid) = account_id {
            request_builder = request_builder.header("chatgpt-account-id", aid);
        }

        let resp = request_builder.json(&codex_req).send().await;

        match resp {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let body = match response.text().await {
                        Ok(b) => b,
                        Err(e) => {
                            return GatewayResponse {
                                ok: false,
                                tool_calls: None,
                                content: None,
                                error: Some(format!("response body read error: {}", e)),
                                input_tokens: None,
                                output_tokens: None,
                                model: Some(model),
                            };
                        }
                    };

                    let parsed = parse_codex_sse_response(&body);

                    // Una respuesta que es solo una llamada a herramienta tiene
                    // texto vacío de forma legítima: no es un error.
                    if parsed.text.is_empty() && parsed.tool_calls.is_empty() {
                        return GatewayResponse {
                            ok: false,
                            tool_calls: None,
                            content: None,
                            error: Some("Codex returned empty response".into()),
                            input_tokens: parsed.usage.input_tokens,
                            output_tokens: parsed.usage.output_tokens,
                            model: parsed.model.or(Some(model)),
                        };
                    }

                    return GatewayResponse {
                        ok: true,
                        tool_calls: (!parsed.tool_calls.is_empty()).then_some(parsed.tool_calls),
                        content: (!parsed.text.is_empty()).then_some(parsed.text),
                        error: None,
                        input_tokens: parsed.usage.input_tokens,
                        output_tokens: parsed.usage.output_tokens,
                        model: parsed.model.or(Some(model)),
                    };
                }

                let error_body = response.text().await.unwrap_or_default();
                let is_retryable = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);

                if is_retryable && attempt < max_retries {
                    let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt as u32));
                    eprintln!(
                        "⚠️ vertex-gateway(codex): {} on attempt {}, retrying in {:?}...",
                        status,
                        attempt + 1,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    last_error =
                        format!("HTTP {}: {}", status, truncate_for_error(&error_body, 200));
                    continue;
                }

                return GatewayResponse {
                    ok: false,
                    tool_calls: None,
                    content: None,
                    error: Some(format!(
                        "Codex API error ({}): {}",
                        status,
                        truncate_for_error(&error_body, 300)
                    )),
                    input_tokens: None,
                    output_tokens: None,
                    model: Some(model),
                };
            }
            Err(e) => {
                if attempt < max_retries {
                    let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt as u32));
                    eprintln!(
                        "⚠️ vertex-gateway(codex): network error on attempt {}: {}, retrying in {:?}",
                        attempt + 1, e, delay
                    );
                    tokio::time::sleep(delay).await;
                    last_error = format!("network: {}", e);
                    continue;
                }

                return GatewayResponse {
                    ok: false,
                    tool_calls: None,
                    content: None,
                    error: Some(format!("Codex request failed after retries: {}", e)),
                    input_tokens: None,
                    output_tokens: None,
                    model: Some(model),
                };
            }
        }
    }

    GatewayResponse {
        ok: false,
        tool_calls: None,
        content: None,
        error: Some(format!(
            "Codex request failed after {} retries: {}",
            max_retries, last_error
        )),
        input_tokens: None,
        output_tokens: None,
        model: Some(model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn herramientas_de_prueba() -> Vec<ToolDef> {
        vec![ToolDef {
            name: "search_text".into(),
            description: "busca".into(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }

    #[test]
    fn los_items_se_vuelven_mensajes_openai_con_llamadas_agrupadas() {
        let items = vec![
            GatewayInputItem::Message { role: "user".into(), text: "hola".into() },
            GatewayInputItem::FunctionCall { call_id: "c1".into(), name: "search_text".into(), arguments: "{\"query\":\"a\"}".into() },
            GatewayInputItem::FunctionCall { call_id: "c2".into(), name: "read_file".into(), arguments: "{}".into() },
            GatewayInputItem::FunctionCallOutput { call_id: "c1".into(), output: "3 líneas".into() },
            GatewayInputItem::FunctionCallOutput { call_id: "c2".into(), output: "fn x".into() },
            GatewayInputItem::Message { role: "assistant".into(), text: "listo".into() },
        ];
        let mensajes = openai_messages_from_items(&items);
        let roles: Vec<&str> = mensajes.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["user", "assistant", "tool", "tool", "assistant"]);
        assert_eq!(mensajes[1].tool_calls.as_ref().unwrap().len(), 2);
        assert_eq!(mensajes[2].tool_call_id.as_deref(), Some("c1"));
        let json = serde_json::to_string(&mensajes[1]).unwrap();
        assert!(json.contains("\"type\":\"function\"") && !json.contains("tool_call_id"), "{json}");
    }

    #[test]
    fn una_llamada_escrita_como_json_se_acepta_si_nombra_una_herramienta_ofrecida() {
        let tools = herramientas_de_prueba();
        let en_bloque = "```json\n{\n  \"name\": \"search_text\",\n  \"arguments\": {\"query\": \"ensure_current\"}\n}\n```";
        let llamada = tool_call_from_text(en_bloque, &tools).expect("bloque JSON");
        assert_eq!(llamada.name, "search_text");
        assert_eq!(llamada.arguments, "{\"query\":\"ensure_current\"}");
        let qwen = "<tool_call>{\"name\":\"search_text\",\"arguments\":{\"query\":\"x\"}}</tool_call>";
        assert!(tool_call_from_text(qwen, &tools).is_some());
        // Texto normal, JSON de otra cosa o herramienta desconocida: es respuesta.
        assert!(tool_call_from_text("La función está en walk.rs.", &tools).is_none());
        assert!(tool_call_from_text("{\"name\":\"borrar_todo\",\"arguments\":{}}", &tools).is_none());
        assert!(tool_call_from_text("Ejemplo: {\"name\":\"search_text\"} y más texto", &tools).is_none());
        assert!(tool_call_from_text("{\"name\":\"search_text\"}", &[]).is_none());
    }

    #[test]
    fn la_url_del_chat_no_duplica_v1_y_el_contenido_nulo_se_lee() {
        assert_eq!(openai_chat_url("https://api.openai.com"), "https://api.openai.com/v1/chat/completions");
        assert_eq!(openai_chat_url("https://api.openai.com/v1/"), "https://api.openai.com/v1/chat/completions");
        assert_eq!(openai_chat_url("http://127.0.0.1:11434"), "http://127.0.0.1:11434/v1/chat/completions");
        let cuerpo = r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"search_text","arguments":"{\"query\":\"a\"}"}}]}}]}"#;
        let resp: LlamaResponse = serde_json::from_str(cuerpo).unwrap();
        let msg = resp.choices.unwrap().remove(0).message;
        assert_eq!(msg.content, "");
        assert_eq!(msg.tool_calls.unwrap()[0].function.name, "search_text");
    }

    #[test]
    fn parse_route_accepts_aliases() {
        assert!(matches!(
            parse_route(Some("primary")).unwrap(),
            Route::Primary
        ));
        assert!(matches!(
            parse_route(Some("planner")).unwrap(),
            Route::Primary
        ));
        assert!(matches!(
            parse_route(Some("worker")).unwrap(),
            Route::Worker
        ));
        assert!(matches!(
            parse_route(Some("local-worker")).unwrap(),
            Route::Worker
        ));
    }

    #[test]
    fn parse_route_rejects_invalid_value() {
        let err = parse_route(Some("bad-route")).unwrap_err();
        assert!(err.contains("invalid route"));
    }

    #[test]
    fn extract_json_object_slice_handles_wrapper_text() {
        let raw = "diagnostic\n{\n  \"payloads\": [{\"text\": \"hola\"}]\n}\n";
        let slice = extract_json_object_slice(raw).unwrap();
        assert!(slice.starts_with('{'));
        assert!(slice.ends_with('}'));
    }

    #[test]
    fn parse_openclaw_envelope_reads_payloads_model_and_tokens() {
        let raw = r#"
{
  "payloads": [{"text": "hola"}, {"text": "mundo"}],
  "meta": {
    "agentMeta": {
      "model": "openai-codex/gpt-5.3-codex",
      "usage": {"input": 10, "output": 5}
    }
  }
}
"#;
        let env = parse_openclaw_envelope(raw).unwrap();
        assert_eq!(
            collect_openclaw_text(openclaw_payloads(&env)),
            "hola\n\nmundo"
        );
        let model = openclaw_meta(&env)
            .as_ref()
            .and_then(|m| m.agent_meta.as_ref())
            .and_then(|a| a.model.clone())
            .unwrap();
        assert_eq!(model, "openai-codex/gpt-5.3-codex");
        let usage = openclaw_meta(&env)
            .as_ref()
            .and_then(|m| m.agent_meta.as_ref())
            .and_then(|a| a.usage.as_ref())
            .unwrap();
        assert_eq!(
            openclaw_usage_split(usage),
            TokenUsageSplit {
                input_tokens: Some(10),
                output_tokens: Some(5),
            }
        );
    }

    #[test]
    fn parse_openclaw_envelope_reads_nested_result_payloads() {
        let raw = r#"
{
  "runId": "abc",
  "status": "ok",
  "result": {
    "payloads": [{"text": "quiron conectado"}],
    "meta": {
      "agentMeta": {
        "model": "openai-codex/gpt-5.3-codex",
        "usage": {"input": 7, "output": 3}
      }
    }
  }
}
"#;
        let env = parse_openclaw_envelope(raw).unwrap();
        assert_eq!(
            collect_openclaw_text(openclaw_payloads(&env)),
            "quiron conectado"
        );
        let usage = openclaw_meta(&env)
            .as_ref()
            .and_then(|m| m.agent_meta.as_ref())
            .and_then(|a| a.usage.as_ref())
            .unwrap();
        assert_eq!(
            openclaw_usage_split(usage),
            TokenUsageSplit {
                input_tokens: Some(7),
                output_tokens: Some(3),
            }
        );
    }

    #[test]
    fn openclaw_usage_split_derives_missing_side_from_total() {
        let usage = OpenClawUsage {
            total: Some(15),
            input: Some(10),
            output: None,
        };
        assert_eq!(
            openclaw_usage_split(&usage),
            TokenUsageSplit {
                input_tokens: Some(10),
                output_tokens: Some(5),
            }
        );
    }

    #[test]
    fn build_openclaw_command_args_includes_expected_flags() {
        let args = build_openclaw_command_args("main", "ping", 30, Some("minimal"), true);
        assert_eq!(args[0], "agent");
        assert!(args.contains(&"--agent".to_string()));
        assert!(args.contains(&"main".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"--timeout".to_string()));
        assert!(args.contains(&"30".to_string()));
        assert!(args.contains(&"--thinking".to_string()));
        assert!(args.contains(&"minimal".to_string()));
        assert!(args.contains(&"--local".to_string()));
    }

    // ====== Direct backend tests ======

    #[test]
    fn parse_codex_sse_extracts_deltas_and_usage() {
        let raw = "\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\
data: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n\
data: [DONE]\n";
        let parsed = parse_codex_sse_response(raw);
        assert_eq!(parsed.text, "Hello world");
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(
            parsed.usage,
            TokenUsageSplit {
                input_tokens: Some(10),
                output_tokens: Some(5),
            }
        );
        assert_eq!(parsed.model.as_deref(), Some("gpt-5.3-codex"));
    }

    #[test]
    fn parse_codex_sse_extracts_function_call() {
        // Formato real observado: la llamada llega en `response.output_item.done`,
        // y de nuevo dentro del array `output` de `response.completed`. Debe
        // recogerse una sola vez.
        let raw = "\
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_abc\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}}\n\
data: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-5.5\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"call_abc\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}],\"usage\":{\"input_tokens\":8,\"output_tokens\":4}}}\n\
data: [DONE]\n";
        let parsed = parse_codex_sse_response(raw);
        assert!(parsed.text.is_empty(), "una tool call no lleva texto");
        assert_eq!(parsed.tool_calls.len(), 1, "no debe duplicarse");
        let call = &parsed.tool_calls[0];
        assert_eq!(call.name, "read_file");
        assert_eq!(call.call_id, "call_abc");
        assert!(call.arguments.contains("README.md"));
    }

    #[test]
    fn parse_codex_sse_fallback_to_output_text() {
        let raw = "\
data: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-5.3-codex\",\"output\":[{\"text\":\"fallback text\"}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\
data: [DONE]\n";
        let parsed = parse_codex_sse_response(raw);
        assert_eq!(parsed.text, "fallback text");
        let usage = parsed.usage;
        assert_eq!(
            usage,
            TokenUsageSplit {
                input_tokens: Some(3),
                output_tokens: Some(2),
            }
        );
    }

    #[test]
    fn build_codex_request_has_correct_shape() {
        let req = build_codex_request("hello", Some("be helpful"), "gpt-5.3-codex", &[], &[]);
        assert_eq!(req.model, "gpt-5.3-codex");
        assert!(!req.store);
        assert!(req.stream);
        assert!(req.tools.is_empty());
        assert_eq!(req.instructions.as_deref(), Some("be helpful"));
        assert_eq!(req.input.len(), 1);
        // El cuerpo de un turno simple debe seguir siendo byte-idéntico al
        // verificado contra la cuenta: {role, content} sin campo `type`.
        let input = serde_json::to_value(&req.input).unwrap();
        assert_eq!(
            input,
            serde_json::json!([{
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }])
        );
    }

    #[test]
    fn build_codex_request_does_not_invent_system_instructions() {
        let req = build_codex_request("hello", None, "gpt-5.4", &[], &[]);
        assert!(req.instructions.is_none());
    }

    #[test]
    fn codex_request_no_envia_limite_de_salida() {
        // El endpoint por suscripción rechaza `max_output_tokens`; el cuerpo no
        // debe contenerlo.
        let req = build_codex_request("hi", None, "gpt-5.5", &[], &[]);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("max_output_tokens"), "{json}");
    }

    // ====== Capa 4: conversación multi-turno con herramientas ======

    #[test]
    fn gateway_request_acepta_input_items() {
        // El formato de cable que envía quiron-brain: items etiquetados.
        let raw = r##"{
            "prompt": "",
            "input_items": [
                { "type": "message", "role": "user", "text": "¿qué dice el README?" },
                { "type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"README.md\"}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "# Quirón" }
            ]
        }"##;
        let req: GatewayRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.input_items.len(), 3);
        assert!(matches!(
            &req.input_items[1],
            GatewayInputItem::FunctionCall { call_id, name, .. }
                if call_id == "call_1" && name == "read_file"
        ));
    }

    #[test]
    fn gateway_request_sin_input_items_sigue_siendo_valido() {
        // Retrocompatibilidad: todos los llamadores anteriores a la capa 4.
        let raw = r#"{ "prompt": "hola" }"#;
        let req: GatewayRequest = serde_json::from_str(raw).unwrap();
        assert!(req.input_items.is_empty());
    }

    #[test]
    fn codex_input_traduce_la_conversacion_completa() {
        let items = vec![
            GatewayInputItem::Message {
                role: "user".into(),
                text: "¿qué dice el README?".into(),
            },
            GatewayInputItem::Message {
                role: "assistant".into(),
                text: "Voy a leerlo.".into(),
            },
            GatewayInputItem::FunctionCall {
                call_id: "call_1".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"README.md\"}".into(),
            },
            GatewayInputItem::FunctionCallOutput {
                call_id: "call_1".into(),
                output: "# Quirón".into(),
            },
        ];
        let input = serde_json::to_value(codex_input_from_items(&items)).unwrap();
        assert_eq!(
            input,
            serde_json::json!([
                { "role": "user", "content": [{ "type": "input_text", "text": "¿qué dice el README?" }] },
                // Lo dicho por el asistente viaja como output_text, no input_text.
                { "role": "assistant", "content": [{ "type": "output_text", "text": "Voy a leerlo." }] },
                // La llamada se devuelve con su call_id, sin `id` interno fc_...
                { "type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"README.md\"}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "# Quirón" }
            ])
        );
    }

    #[test]
    fn build_codex_request_con_items_ignora_el_prompt() {
        let items = vec![GatewayInputItem::Message {
            role: "user".into(),
            text: "pregunta real".into(),
        }];
        let req = build_codex_request("prompt legado", None, "gpt-5.5", &[], &items);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("pregunta real"));
        assert!(!json.contains("prompt legado"), "{json}");
    }

    #[test]
    fn base64_decode_works() {
        // "hello" in base64 is "aGVsbG8="
        let decoded = base64_decode("aGVsbG8=").unwrap();
        assert_eq!(std::str::from_utf8(&decoded).unwrap(), "hello");
    }

    #[test]
    fn gateway_backend_parses_new_variants() {
        assert!(matches!(
            GatewayBackend::from_env(Some("codex_direct".into())).unwrap(),
            GatewayBackend::CodexDirect
        ));
        assert!(matches!(
            GatewayBackend::from_env(Some("codex".into())).unwrap(),
            GatewayBackend::CodexDirect
        ));
    }
}
