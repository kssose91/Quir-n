//! # Llore Brain
//!
//! El puente entre Llore Editor y Quirón.
//!
//! ## La Regla de Oro
//!
//! ```text
//! Llore (tú) → llore_brain → quiron-brain → Claude/Gemini
//! ```
//!
//! Quirón es el **ÚNICO gateway** a modelos externos.
//! Nunca llamas directamente a Claude.
//!
//! ## Módulos
//!
//! - **client**: Cliente HTTP para conectar con quiron-brain API
//! - **orchestrator**: Loop de iteración (intents: recall, tool_use, done)
//! - **gates**: Verificación local de invariantes
//! - **ledger**: Log de eventos local
//! - **protocol**: Tipos Request/Response
//! - **recall**: Búsqueda en contexto virtual
//!
//! > **NOTA**: La memoria persistente reside SOLO en quiron-brain.
//! > llore_brain es un cliente stateless.

// Módulos públicos principales
pub mod client;
pub mod orchestrator;

// Módulos internos
mod gates;
mod ledger;
mod protocol;
mod recall;

#[cfg(test)]
mod integration_tests;

// Re-exports principales
pub use client::{ClientError, EventSummary, QuironClient};
pub use gates::{
    ActionRequest as GateActionRequest, Evidence, Gate, GateResult, GateValidator, ValidationResult,
};
pub use ledger::{Event, EventKind, Ledger};
pub use orchestrator::{
    DelegationMetrics, Intent, Orchestrator, OrchestratorConfig, OrchestratorResult,
    SessionTelemetryAnomaly, SessionTelemetryCheckpoint, SessionTelemetrySample,
    SessionTelemetrySnapshot, TelemetryAnomalyKind,
};
pub use protocol::{Request, Response, WorkerResult, WorkerTask};
pub use recall::Recall;

/// Configuración de Quirón.
#[derive(Debug, Clone)]
pub struct QuironConfig {
    /// URL del servidor quiron-brain
    pub brain_url: String,
    /// Bearer token para quiron-brain (si no se define, usa QUIRON_API_TOKEN del entorno)
    pub api_token: Option<String>,
    /// Ruta al directorio .agent
    pub agent_path: std::path::PathBuf,
    /// Habilitar gates de verificación local
    pub gates_enabled: bool,
    /// Máximo de iteraciones del orquestador
    pub max_iterations: u32,
}

impl Default for QuironConfig {
    fn default() -> Self {
        Self {
            brain_url: "http://localhost:8766".into(),
            api_token: std::env::var("QUIRON_API_TOKEN")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            agent_path: std::path::PathBuf::from("/home/kssose/Quirón/.agent"),
            gates_enabled: true,
            max_iterations: 100, // Deliberación profunda - pregunta al usuario cuando se agote
        }
    }
}

/// El cerebro de Quirón integrado.
pub struct Quiron {
    client: QuironClient,
    orchestrator: Orchestrator,
    #[allow(dead_code)] // Reserved for dynamic reconfiguration
    config: QuironConfig,
}

/// Resultado del chat con herramientas: la respuesta final y el rastro de lo
/// que se ejecutó por el camino, para que la interfaz pueda enseñar las manos.
#[derive(Debug, Clone)]
pub struct ChatOutcome {
    pub text: String,
    pub tool_trace: Vec<ToolTraceEntry>,
    /// Fichas de código que el cerebro adjuntó a la respuesta (ruta, rango y
    /// hash vigentes al responder), para enseñar y abrir las fuentes.
    pub code_hints: Vec<client::CodeHint>,
}

/// Una ejecución de herramienta dentro del bucle.
#[derive(Debug, Clone)]
pub struct ToolTraceEntry {
    /// Descripción legible, p. ej. `read_file(README.md)`.
    pub summary: String,
    /// La herramienta devolvió error (incluye las negativas del arnés).
    pub is_error: bool,
}

/// Tope por resultado de herramienta que vuelve al modelo.
const TOOL_OUTPUT_MAX_CHARS: usize = 16_000;

/// Recorta un resultado de herramienta declarándolo, sin partir un carácter.
fn recorta_resultado(salida: String, max_chars: usize) -> String {
    if salida.chars().count() <= max_chars {
        return salida;
    }
    let mut corta: String = salida.chars().take(max_chars).collect();
    corta.push_str(&format!("\n\n[resultado recortado a {max_chars} caracteres]"));
    corta
}

/// `read_file({"path":"src/main.rs"})` → `read_file(src/main.rs)`.
fn summarize_tool_call(name: &str, input: &serde_json::Value) -> String {
    let argumento = input
        .get("path")
        .or_else(|| input.get("query"))
        .or_else(|| input.get("prefix"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if argumento.is_empty() {
        format!("{name}()")
    } else {
        format!("{name}({argumento})")
    }
}

impl Quiron {
    /// Crear una nueva instancia de Quirón.
    ///
    /// Nace sin proyecto: el editor arranca sin carpeta abierta. La identidad se
    /// instala con [`Quiron::set_project_id`] al conceder acceso a un
    /// directorio. Antes se derivaba de la ruta canónica del workspace, que no
    /// coincide con el identificador que guarda el registro de eventos.
    pub fn new(config: QuironConfig) -> Self {
        let client = match config.api_token.clone() {
            Some(token) => QuironClient::with_url_and_token(&config.brain_url, Some(token)),
            None => QuironClient::with_url(&config.brain_url),
        };
        let orch_config = OrchestratorConfig {
            max_iterations: config.max_iterations,
            require_citations: true,
            ..Default::default()
        };
        let orchestrator = Orchestrator::with_config(client.clone(), orch_config);

        Self {
            client,
            orchestrator,
            config,
        }
    }

    /// Procesar una petición del usuario.
    pub async fn process(&mut self, request: &str) -> OrchestratorResult {
        // El orquestador maneja todo: gates, recall, tool_use
        self.orchestrator.process(request).await
    }

    /// Chat primario directo para la interfaz del editor.
    /// Envía una pregunta al modelo con el contexto del proyecto abierto.
    ///
    /// Antes se enviaba la pregunta desnuda: sin contexto y sin instrucciones. El
    /// modelo no podía sino responder que no tenía acceso a nada, porque en
    /// efecto no lo tenía.
    pub async fn chat(
        &self,
        request: &str,
        model: &str,
        context: &str,
        system: Option<&str>,
    ) -> Result<String, ClientError> {
        let response = self
            .client
            .send_to_llm(request, context, system, Some(model))
            .await?;
        Ok(response.text())
    }

    /// Chat con manos: el bucle de herramientas de la capa 4.
    ///
    /// Pedir → ejecutar tras el arnés → devolver el resultado → repetir, hasta
    /// que el modelo cierre con texto (`end_turn`) o se agote el presupuesto de
    /// turnos. La clausura `execute` es la única puerta a las herramientas: la
    /// pone el editor y dentro vive la guardia del workspace. Este módulo no
    /// toca el disco; si la guardia niega, la negativa viaja al modelo como
    /// resultado con error y el contenido nunca sale.
    pub async fn chat_with_tools(
        &self,
        request: &str,
        model: &str,
        context: &str,
        system: Option<&str>,
        tools: Vec<client::LlmToolDef>,
        mut execute: impl FnMut(&str, &serde_json::Value) -> (String, bool),
    ) -> Result<ChatOutcome, ClientError> {
        use client::{ChatBlock, ChatTurnMessage};

        /// Turnos máximos de herramientas por pregunta. Un modelo que encadena
        /// más lecturas que esto no está explorando: está perdido.
        const MAX_TOOL_TURNS: usize = 8;

        let mut messages = vec![ChatTurnMessage::text("user", request)];
        let mut trace: Vec<ToolTraceEntry> = Vec::new();
        // Tras varias herramientas, la CLI de Claude devolvió alguna vez un
        // cierre de juguete («Prueba corta.») en lugar de la respuesta. Una
        // sola vez se le pide que redacte de verdad con lo ya leído.
        let mut reclamada = false;
        // El cerebro adjunta las fichas al primer turno, el que lleva la
        // pregunta; los turnos de resultados de herramientas no las repiten.
        let mut code_hints: Vec<client::CodeHint> = Vec::new();

        for _ in 0..MAX_TOOL_TURNS {
            let response = self
                .client
                .send_chat_turn(messages.clone(), tools.clone(), context, system, Some(model))
                .await?;

            if code_hints.is_empty() {
                if let Some(ctx) = &response.quiron_context {
                    code_hints = ctx.code_hints.clone();
                }
            }

            let tool_uses: Vec<_> = response
                .content
                .iter()
                .filter(|block| block.content_type == "tool_use")
                .cloned()
                .collect();

            let pide_herramientas =
                response.stop_reason.as_deref() == Some("tool_use") && !tool_uses.is_empty();
            if !pide_herramientas {
                let texto = response.text();
                // Una respuesta legítima puede ser breve («No puedo leer
                // credenciales.», 27 caracteres); un marcador, más aún.
                if !reclamada && !trace.is_empty() && texto.trim().chars().count() < 16 {
                    reclamada = true;
                    messages.push(ChatTurnMessage {
                        role: "assistant".to_string(),
                        content: vec![ChatBlock::Text { text: texto.clone() }],
                    });
                    messages.push(ChatTurnMessage::text(
                        "user",
                        &format!(
                            "Tu última respuesta quedó en «{}». Redacta ahora la respuesta \
                             completa a la pregunta con lo que ya has leído, citando ruta y \
                             línea; no llames a más herramientas.",
                            texto.trim()
                        ),
                    ));
                    continue;
                }
                return Ok(ChatOutcome {
                    text: texto,
                    tool_trace: trace,
                    code_hints,
                });
            }

            // El turno del asistente se conserva tal cual lo dijo: su texto y
            // sus llamadas, con los mismos identificadores.
            let mut assistant_blocks = Vec::new();
            let texto = response.text();
            if !texto.trim().is_empty() {
                assistant_blocks.push(ChatBlock::Text { text: texto });
            }

            let mut result_blocks = Vec::new();
            for call in &tool_uses {
                let id = call.id.clone().unwrap_or_default();
                let name = call.name.clone().unwrap_or_default();
                let input = call
                    .input
                    .clone()
                    .unwrap_or_else(|| serde_json::Value::Object(Default::default()));

                assistant_blocks.push(ChatBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });

                // La ejecución ocurre AQUÍ, en manos del editor y tras su
                // arnés. El resultado —o la negativa— vuelve al modelo, acotado:
                // cada turno reenvía toda la conversación, y dos resultados de
                // 150 KB llevaron el contexto a 349 KB y al modelo a cerrar con
                // un marcador (5 de septiembre).
                let (output, is_error) = execute(&name, &input);
                let output = recorta_resultado(output, TOOL_OUTPUT_MAX_CHARS);
                trace.push(ToolTraceEntry {
                    summary: summarize_tool_call(&name, &input),
                    is_error,
                });
                result_blocks.push(ChatBlock::ToolResult {
                    tool_use_id: id,
                    content: output,
                    is_error,
                });
            }

            messages.push(ChatTurnMessage {
                role: "assistant".to_string(),
                content: assistant_blocks,
            });
            messages.push(ChatTurnMessage {
                role: "user".to_string(),
                content: result_blocks,
            });
        }

        // Presupuesto agotado: se dice honestamente, con el rastro de lo que
        // sí se ejecutó, en vez de inventar un cierre.
        Ok(ChatOutcome {
            text: format!(
                "El modelo agotó los {MAX_TOOL_TURNS} turnos de herramientas sin \
                 cerrar una respuesta. Reformula la pregunta o acótala."
            ),
            tool_trace: trace,
            code_hints,
        })
    }

    /// Métricas de ejecución del último procesamiento.
    pub fn delegation_metrics(&self) -> DelegationMetrics {
        self.orchestrator.delegation_metrics()
    }

    /// Snapshot de telemetría de sesión (checkpoints + anomalías + buffer RAM).
    pub fn session_telemetry_snapshot(&self) -> SessionTelemetrySnapshot {
        self.orchestrator.session_telemetry_snapshot()
    }

    /// Leer telemetría persistida de sesión desde quiron-brain.
    /// Si `session_id` es `None`, usa el `session_id` activo del snapshot local.
    pub async fn get_persisted_session_telemetry(
        &self,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<client::SessionTelemetryResponse, ClientError> {
        let session = match session_id {
            Some(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => self.orchestrator.session_telemetry_snapshot().session_id,
        };
        self.client.get_session_telemetry(&session, limit).await
    }

    /// Leer página de telemetría persistida con offsets independientes.
    pub async fn get_persisted_session_telemetry_page(
        &self,
        session_id: Option<&str>,
        limit: usize,
        checkpoints_offset: usize,
        anomalies_offset: usize,
    ) -> Result<client::SessionTelemetryResponse, ClientError> {
        let session = match session_id {
            Some(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => self.orchestrator.session_telemetry_snapshot().session_id,
        };
        self.client
            .get_session_telemetry_page(&session, limit, checkpoints_offset, anomalies_offset)
            .await
    }

    /// Ajustar budget global de tokens en caliente (modo operador).
    pub fn set_runtime_token_budget(&mut self, token_budget: u32) {
        self.orchestrator.set_token_budget(token_budget);
    }

    /// Ajustar cap de paralelismo de subtareas en caliente (modo operador).
    pub fn set_runtime_parallel_cap(&mut self, max_parallel_subtasks: usize) {
        self.orchestrator
            .set_parallel_subtasks_cap(max_parallel_subtasks);
    }

    /// Ajustar escalas de thresholds por ruta en caliente (modo operador).
    pub fn set_runtime_threshold_scales(&mut self, worker_scale: f32, primary_scale: f32) {
        self.orchestrator
            .set_threshold_scales(worker_scale, primary_scale);
    }

    /// Fija la identidad del proyecto activo para todas las peticiones.
    ///
    /// Es el identificador de `<proyecto>/.llore/project.id`, no la ruta ni el
    /// nombre de la carpeta.
    pub fn set_project_id(&mut self, project_id: Option<String>) {
        self.client.set_project_id(project_id);
    }

    /// Tope de tokens de salida por turno del chat.
    pub fn set_response_max_tokens(&mut self, max_tokens: u32) {
        self.client.set_response_max_tokens(max_tokens);
    }

    /// Identidad del proyecto activo, si hay alguno abierto.
    /// Clon ligero para peticiones de fondo, sin retener el bloqueo del chat.
    pub fn client_snapshot(&self) -> client::QuironClient { self.client.clone() }

    pub fn project_id(&self) -> Option<&str> {
        self.client.project_id()
    }

    /// Verificar que quiron-brain está disponible.
    pub async fn check_health(&self) -> bool {
        self.health_snapshot()
            .await
            .map(|health| health.is_ok())
            .unwrap_or(false)
    }

    /// Estado completo del cerebro: si responde, cuántas unidades hay en el
    /// almacén vectorial y cuántos nodos en el grafo.
    pub async fn health_snapshot(&self) -> Option<client::HealthResponse> {
        self.client.health().await.ok()
    }

    /// Obtener contexto de startup (identidad, gates, eventos recientes).
    pub async fn get_context(&self) -> Result<client::ContextResponse, ClientError> {
        self.client.get_context().await
    }

    /// Obtener resumen de un evento por ID (para navegación de citations).
    pub async fn get_event(&self, id: &str) -> Result<Option<client::EventSummary>, ClientError> {
        self.client.get_event(id).await
    }

    /// Registrar una edición de código en el Ledger a través de quiron-brain.
    pub async fn observe_code_edit(
        &self,
        file_path: &str,
        edit_description: &str,
    ) -> Result<(), ClientError> {
        let _ = self
            .client
            .create_event(client::CreateEventRequest {
                kind: "ACTION".to_string(), // Or whatever category fits best
                description: format!("Edición en '{}': {}", file_path, edit_description),
                project_id: self.client.project_id().map(str::to_string),
                tags: Some(vec!["file_edit".to_string(), "editor".to_string()]),
                inputs: Some(vec![file_path.to_string()]),
                outputs: None,
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_config_defaults() {
        let config = QuironConfig::default();
        assert_eq!(config.brain_url, "http://localhost:8766");
        assert!(config.gates_enabled);
    }

    /// Un quiron-brain de mentira: responde a cada POST con la siguiente
    /// respuesta enlatada y guarda los cuerpos recibidos para inspección.
    fn cerebro_simulado(respuestas: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let cuerpos = Arc::new(Mutex::new(Vec::new()));
        let cuerpos_hilo = Arc::clone(&cuerpos);

        std::thread::spawn(move || {
            for respuesta in respuestas {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                let cuerpo = loop {
                    let Ok(n) = stream.read(&mut chunk) else {
                        return;
                    };
                    buffer.extend_from_slice(&chunk[..n]);
                    let texto = String::from_utf8_lossy(&buffer);
                    if let Some(fin_cabeceras) = texto.find("\r\n\r\n") {
                        let content_length = texto
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        let inicio = fin_cabeceras + 4;
                        if buffer.len() >= inicio + content_length {
                            break String::from_utf8_lossy(&buffer[inicio..inicio + content_length])
                                .to_string();
                        }
                    }
                };
                cuerpos_hilo.lock().unwrap().push(cuerpo);
                let http = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    respuesta.len(),
                    respuesta
                );
                let _ = stream.write_all(http.as_bytes());
            }
        });

        (url, cuerpos)
    }

    fn quiron_contra(url: &str) -> Quiron {
        Quiron::new(QuironConfig {
            brain_url: url.to_string(),
            api_token: None,
            ..QuironConfig::default()
        })
    }

    fn herramientas_de_prueba() -> Vec<client::LlmToolDef> {
        vec![client::LlmToolDef {
            name: "read_file".into(),
            description: "lee un archivo".into(),
            input_schema: serde_json::json!({ "type": "object" }),
        }]
    }

    #[tokio::test]
    async fn el_bucle_ejecuta_y_realimenta_hasta_cerrar() {
        // Turno 1: el modelo pide leer. Turno 2: cierra con la respuesta.
        let (url, cuerpos) = cerebro_simulado(vec![
            serde_json::json!({
                "content": [
                    { "type": "text", "text": "Voy a leerlo." },
                    { "type": "tool_use", "id": "call_1", "name": "read_file",
                      "input": { "path": "README.md" } }
                ],
                "stop_reason": "tool_use"
            })
            .to_string(),
            serde_json::json!({
                "content": [ { "type": "text", "text": "La versión es 0.4.2" } ],
                "stop_reason": "end_turn"
            })
            .to_string(),
        ]);

        let quiron = quiron_contra(&url);
        let ejecuciones = Arc::new(Mutex::new(Vec::new()));
        let ejecuciones_closure = Arc::clone(&ejecuciones);

        let outcome = quiron
            .chat_with_tools(
                "¿qué versión es?",
                "gpt-5.5",
                "",
                None,
                herramientas_de_prueba(),
                move |name, input| {
                    ejecuciones_closure
                        .lock()
                        .unwrap()
                        .push((name.to_string(), input.clone()));
                    ("# Quirón v0.4.2".to_string(), false)
                },
            )
            .await
            .expect("el bucle debe cerrar");

        assert_eq!(outcome.text, "La versión es 0.4.2");
        assert_eq!(outcome.tool_trace.len(), 1);
        assert_eq!(outcome.tool_trace[0].summary, "read_file(README.md)");
        assert!(!outcome.tool_trace[0].is_error);

        // Se ejecutó exactamente lo pedido, con los argumentos del modelo.
        let ejecutado = ejecuciones.lock().unwrap();
        assert_eq!(ejecutado.len(), 1);
        assert_eq!(ejecutado[0].0, "read_file");

        // El segundo request lleva la conversación completa: la llamada del
        // asistente y su resultado, emparejados por call_id.
        let cuerpos = cuerpos.lock().unwrap();
        assert_eq!(cuerpos.len(), 2);
        let segundo: serde_json::Value = serde_json::from_str(&cuerpos[1]).unwrap();
        let mensajes = segundo["messages"].as_array().unwrap();
        assert_eq!(mensajes.len(), 3);
        assert_eq!(mensajes[1]["role"], "assistant");
        assert_eq!(mensajes[1]["content"][1]["type"], "tool_use");
        assert_eq!(mensajes[1]["content"][1]["id"], "call_1");
        assert_eq!(mensajes[2]["role"], "user");
        assert_eq!(mensajes[2]["content"][0]["type"], "tool_result");
        assert_eq!(mensajes[2]["content"][0]["tool_use_id"], "call_1");
        assert_eq!(mensajes[2]["content"][0]["content"], "# Quirón v0.4.2");
        assert_eq!(mensajes[2]["content"][0]["is_error"], false);
        // Las herramientas viajan también en el segundo turno.
        assert_eq!(segundo["tools"][0]["name"], "read_file");
    }

    #[tokio::test]
    async fn una_negativa_del_arnes_viaja_como_error() {
        let (url, cuerpos) = cerebro_simulado(vec![
            serde_json::json!({
                "content": [
                    { "type": "tool_use", "id": "call_9", "name": "read_file",
                      "input": { "path": ".env" } }
                ],
                "stop_reason": "tool_use"
            })
            .to_string(),
            serde_json::json!({
                "content": [ { "type": "text", "text": "No puedo leer credenciales." } ],
                "stop_reason": "end_turn"
            })
            .to_string(),
        ]);

        let quiron = quiron_contra(&url);
        let outcome = quiron
            .chat_with_tools(
                "lee el .env",
                "gpt-5.5",
                "",
                None,
                herramientas_de_prueba(),
                |_, _| {
                    (
                        "acceso denegado: '.env' es una credencial o secreto".to_string(),
                        true,
                    )
                },
            )
            .await
            .expect("el bucle debe cerrar");

        assert!(outcome.tool_trace[0].is_error);

        let cuerpos = cuerpos.lock().unwrap();
        let segundo: serde_json::Value = serde_json::from_str(&cuerpos[1]).unwrap();
        let resultado = &segundo["messages"][2]["content"][0];
        assert_eq!(resultado["is_error"], true);
        assert!(resultado["content"]
            .as_str()
            .unwrap()
            .contains("acceso denegado"));
        assert_eq!(outcome.text, "No puedo leer credenciales.");
    }

    #[tokio::test]
    async fn un_cierre_de_juguete_tras_herramientas_se_reclama_una_vez() {
        // Turno 1: herramienta. Turno 2: «Prueba corta.». Turno 3: la respuesta.
        let (url, cuerpos) = cerebro_simulado(vec![
            serde_json::json!({
                "content": [
                    { "type": "tool_use", "id": "call_1", "name": "read_file",
                      "input": { "path": "README.md" } }
                ],
                "stop_reason": "tool_use"
            })
            .to_string(),
            serde_json::json!({
                "content": [ { "type": "text", "text": "Prueba corta." } ],
                "stop_reason": "end_turn"
            })
            .to_string(),
            serde_json::json!({
                "content": [ { "type": "text", "text": "La versión es 0.4.2, según README.md línea 1." } ],
                "stop_reason": "end_turn"
            })
            .to_string(),
        ]);

        let quiron = quiron_contra(&url);
        let outcome = quiron
            .chat_with_tools(
                "¿qué versión es?",
                "sonnet",
                "",
                None,
                herramientas_de_prueba(),
                |_, _| ("# Quirón v0.4.2".to_string(), false),
            )
            .await
            .expect("el bucle debe cerrar");

        assert_eq!(outcome.text, "La versión es 0.4.2, según README.md línea 1.");
        let cuerpos = cuerpos.lock().unwrap();
        assert_eq!(cuerpos.len(), 3, "una sola reclamación");
        let tercero: serde_json::Value = serde_json::from_str(&cuerpos[2]).unwrap();
        let mensajes = tercero["messages"].as_array().unwrap();
        let ultimo = mensajes.last().unwrap();
        assert_eq!(ultimo["role"], "user");
        let texto = ultimo["content"][0]["text"].as_str().unwrap();
        assert!(texto.contains("Prueba corta.") && texto.contains("Redacta ahora"), "{texto}");
    }

    #[test]
    fn un_resultado_enorme_vuelve_recortado_y_declarado() {
        let largo = "línea\n".repeat(10_000);
        let corto = recorta_resultado(largo, 100);
        assert!(corto.chars().count() < 160, "{}", corto.chars().count());
        assert!(corto.ends_with("[resultado recortado a 100 caracteres]"), "{corto}");
        assert_eq!(recorta_resultado("breve".to_string(), 100), "breve");
    }

    #[test]
    fn test_runtime_controls_are_exposed() {
        let mut q = Quiron::new(QuironConfig::default());
        q.set_runtime_token_budget(77_000);
        q.set_runtime_parallel_cap(5);
        q.set_runtime_threshold_scales(1.2, 0.9);

        let metrics = q.delegation_metrics();
        assert_eq!(metrics.token_budget, 77_000);
        assert_eq!(metrics.parallel_subtasks_cap, 5);
        assert!((metrics.worker_threshold_scale - 1.2).abs() < 0.0001);
        assert!((metrics.primary_threshold_scale - 0.9).abs() < 0.0001);
    }
}
