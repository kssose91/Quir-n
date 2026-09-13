//! # LLM Gateway — Quirón
//!
//! Binario que conecta quiron-brain a providers LLM.
//! Soporta proveedores compatibles, Ollama y las CLI oficiales de Claude y Codex.
//!
//! ## Uso:
//! ```bash
//! echo '{"prompt": "Hola"}' | QUIRON_GATEWAY_BACKEND=claude_cli vertex-gateway
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
mod claude_cli;
mod codex_cli;
use claude_cli::process_request_claude_cli;

// ============================================================================
// CONFIGURACIÓN
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayBackend {
    OpenAiCompatible,
    OllamaNative,
    CodexCli,
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
            Some("codex_cli") | Some("codex-cli") => Ok(GatewayBackend::CodexCli),
            Some(other) => Err(format!(
                "invalid QUIRON_GATEWAY_BACKEND='{}' (use openai_compatible|ollama_native|codex_cli|claude_cli)",
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
    /// Selección de CLI por conversación; nunca modifica la ruta del worker.
    #[serde(default)]
    provider: Option<String>,
    prompt: String,
    #[serde(default)]
    system: Option<String>,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
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
        GatewayBackend::CodexCli => eprintln!("   [Primary] Codex CLI: sesión oficial"),
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
    }

    // Print Worker Setup
    match config.worker_backend {
        GatewayBackend::CodexCli => eprintln!("   [Worker] Codex CLI: sesión oficial"),
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

fn requested_chat_provider(provider: Option<&str>, route: Route) -> Result<Option<GatewayBackend>, String> {
    let Some(provider) = provider else { return Ok(None); };
    if matches!(route, Route::Worker) {
        return Err("La selección del chat no puede cambiar el proveedor del worker".into());
    }
    match provider {
        "claude_cli" => Ok(Some(GatewayBackend::ClaudeCli)),
        "codex_cli" => Ok(Some(GatewayBackend::CodexCli)),
        _ => Err("Proveedor de chat no admitido; usa claude_cli o codex_cli".into()),
    }
}

/// La elección explícita de CLI pertenece a esta petición, no al servicio.
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

    if req.provider.is_some() && req.model.as_deref().is_none_or(|m|m.trim().is_empty()) {
        return GatewayResponse { error:Some("La elección de proveedor requiere indicar un modelo".into()), ..Default::default() };
    }
    let configured_backend = match route {
        Route::Primary => config.primary_backend,
        Route::Worker => config.worker_backend,
    };
    let backend = match requested_chat_provider(req.provider.as_deref(), route) {
        Ok(override_backend) => override_backend.unwrap_or(configured_backend),
        Err(error) => return GatewayResponse { error: Some(error), ..Default::default() },
    };

    let etiqueta = match backend {
        GatewayBackend::CodexCli => "codex_cli",
        GatewayBackend::ClaudeCli => "claude_cli",
        GatewayBackend::OpenAiCompatible => "openai_compatible",
        GatewayBackend::OllamaNative => "ollama_native",
    };
    let respuesta = match backend {
        GatewayBackend::CodexCli => codex_cli::process(req, route, config).await,
        GatewayBackend::ClaudeCli => process_request_claude_cli(req, route, config).await,
        GatewayBackend::OpenAiCompatible => {
            process_request_openai_compatible(req, route, config).await
        }
        GatewayBackend::OllamaNative => process_request_ollama_native(req, route, config).await,
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


fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_provider_selection_is_explicit_and_cannot_redirect_worker() {
        assert_eq!(requested_chat_provider(Some("claude_cli"),Route::Primary).unwrap(),Some(GatewayBackend::ClaudeCli));
        assert_eq!(requested_chat_provider(Some("codex_cli"),Route::Primary).unwrap(),Some(GatewayBackend::CodexCli));
        assert_eq!(requested_chat_provider(None,Route::Worker).unwrap(),None);
        assert!(requested_chat_provider(Some("claude_cli"),Route::Worker).is_err());
        assert!(requested_chat_provider(Some("https://arbitrary.invalid"),Route::Primary).is_err());
    }

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

}
