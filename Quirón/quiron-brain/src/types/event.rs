//! Event types for the ledger system.

use crate::types::ids::EventId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An event in the ledger - the fundamental unit of the brain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event ID (ULID - time-ordered).
    pub id: EventId,

    /// Timestamp when event occurred.
    pub ts: DateTime<Utc>,

    /// Agent that created the event.
    pub agent_id: String,

    /// Project this event belongs to (optional).
    pub project_id: Option<String>,

    /// Type of event.
    pub kind: EventKind,

    /// Human-readable description.
    pub description: String,

    /// Files/artifacts read during this event.
    pub inputs: Vec<String>,

    /// Files/artifacts written during this event.
    pub outputs: Vec<String>,

    /// Artifacts with hashes.
    pub artifacts: Vec<ArtifactRef>,

    /// Performance metrics (optional).
    pub metrics: Option<Metrics>,

    /// Tags for categorization.
    pub tags: Vec<String>,

    /// Parent event for tracing.
    pub parent_event_id: Option<EventId>,

    /// Hash chain: previous event hash (for audit).
    pub prev_hash: Option<[u8; 32]>,

    /// Hash chain: this event's hash.
    pub this_hash: Option<[u8; 32]>,

    /// Importance: 0.0 (trivial) - 1.0 (critical)
    /// Calculated automatically based on event type and content.
    #[serde(default = "default_importance")]
    pub importance: f32,
}

fn default_importance() -> f32 {
    0.5
}

/// Type of event in the ledger.
///
/// IMPORTANT: Uses serde_repr for stable binary encoding.
/// The u8 discriminant values are part of the storage format.
/// DO NOT change discriminant values or reorder variants - it will break stored data!
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr,
)]
#[repr(u8)]
pub enum EventKind {
    // === Core Events (0-9) ===
    /// A decision was made.
    Decision = 0,
    /// An action was performed.
    Action = 1,
    /// An observation was recorded.
    Observation = 2,
    /// A test/build run was executed.
    Run = 3,
    /// An artifact was created/modified.
    Artifact = 4,
    /// An alert was triggered.
    Alert = 5,
    /// An invariant was created/modified.
    Invariant = 6,
    /// A query was executed.
    Query = 7,

    // === Senior Supervisor Protocol (20-31) ===
    /// Estado del repo capturado (commit + tree + hashes).
    RepoSnapshotCreated = 20,
    /// Archivo leído con hash de contenido.
    FileRead = 21,
    /// Diff propuesto (sin aplicar aún).
    PatchProposed = 22,
    /// Diff aplicado al código.
    PatchApplied = 23,
    /// Diff revertido.
    PatchReverted = 24,
    /// Comando ejecutado con resultado (exit_code, stdout, stderr).
    ToolRunRecorded = 25,
    /// Test/build/lint ejecutado con resultado.
    VerificationRecorded = 26,
    /// Afirmación del agente (requiere pruebas para validar).
    ClaimMade = 27,
    /// Afirmación verificada con evidencia.
    ClaimVerified = 28,
    /// Afirmación rechazada por falta de evidencia.
    ClaimRejected = 29,
    /// Scope de trabajo definido (archivos/funciones permitidos).
    ScopeDefined = 30,
    /// Intento de editar fuera del scope acordado.
    ScopeViolation = 31,

    // === Conversaciones Humanas (40-59) ===
    /// Conversación general.
    Conversation = 40,
    /// Expresión emocional (alegría, tristeza, preocupación).
    Emotion = 41,
    /// Humor, risa, broma.
    Humor = 42,
    /// Reflexión filosófica o existencial.
    Philosophy = 43,
    /// Metáfora o analogía explicativa.
    Metaphor = 44,
    /// Sueño, objetivo, aspiración.
    Dream = 45,
    /// Recuerdo compartido.
    Memory = 46,
    /// Enseñanza o aprendizaje.
    Teaching = 47,
    /// Gratitud expresada.
    Gratitude = 48,
    /// Frustración expresada.
    Frustration = 49,
    /// Celebración de logro.
    Celebration = 50,
    /// Reflexión personal.
    Reflection = 51,
    /// Pregunta importante.
    Question = 52,
    /// Corrección de error o malentendido.
    Correction = 53,
    /// Ánimo o apoyo moral.
    Encouragement = 54,
    /// Disculpa.
    Apology = 55,
    /// Insight o revelación.
    Insight = 56,
    /// Confusión expresada.
    Confusion = 57,
    /// Acuerdo.
    Agreement = 58,
    /// Desacuerdo respetuoso.
    Disagreement = 59,

    // === Relaciones Interpersonales (60-69) ===
    /// Historia personal compartida.
    PersonalStory = 60,
    /// Consejo solicitado.
    AdviceSought = 61,
    /// Consejo dado.
    AdviceGiven = 62,
    /// Promesa hecha.
    PromiseMade = 63,
    /// Promesa cumplida.
    PromiseKept = 64,
    /// Preocupación por el otro.
    Concern = 65,
    /// Empatía expresada.
    Empathy = 66,

    // === Creatividad (70-79) ===
    /// Idea creativa.
    CreativeIdea = 70,
    /// Nombre o concepto inventado.
    NameCreated = 71,
    /// Diseño o arquitectura propuesta.
    DesignProposed = 72,
    /// Experimento mental.
    ThoughtExperiment = 73,

    // === Evolución y Aprendizaje (80-99) ===
    /// Nueva categoría descubierta en conversación.
    CategoryDiscovered = 80,
    /// Patrón aprendido de interacciones.
    PatternLearned = 81,
    /// Conceptos conectados en el grafo.
    ConceptLinked = 82,
    /// Fecha importante recordada.
    DateRemembered = 83,
    /// Preferencia del usuario aprendida.
    PreferenceLearned = 84,
    /// Corrección de comportamiento aplicada.
    BehaviorCorrected = 85,
    /// Nuevo tipo de evento emergente (meta-evolución).
    NewKindDiscovered = 86,
}

/// Reference to an artifact with its hash.
///
/// Hash semantics by operation:
/// - Read/Write/Create: hash = Some(blake3_hex) - content hash
/// - Delete: hash = None - no content to hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Path to the artifact.
    pub path: String,
    /// Blake3 hash of the content (None for Delete operations).
    pub hash: Option<String>,
    /// Operation performed.
    pub op: ArtifactOp,
}

/// Operation performed on an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactOp {
    Read,
    Write,
    Delete,
    Create,
}

/// Performance metrics for an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    /// Latency in milliseconds.
    pub latency_ms: Option<u64>,
    /// Tokens consumed (input).
    pub tokens_in: Option<u32>,
    /// Tokens generated (output).
    pub tokens_out: Option<u32>,
    /// CPU usage percentage.
    pub cpu_percent: Option<f32>,
    /// GPU usage percentage.
    pub gpu_percent: Option<f32>,
}

impl Event {
    /// Create a new event with the given kind and description.
    pub fn new(kind: EventKind, description: impl Into<String>) -> Self {
        let mut event = Self {
            id: EventId::new(),
            ts: Utc::now(),
            agent_id: "quiron".to_string(),
            project_id: None,
            kind,
            description: description.into(),
            inputs: vec![],
            outputs: vec![],
            artifacts: vec![],
            metrics: None,
            tags: vec![],
            parent_event_id: None,
            prev_hash: None,
            this_hash: None,
            importance: 0.5,
        };
        event.calculate_importance();
        event
    }

    /// Calculate importance based on event type and content.
    pub fn calculate_importance(&mut self) {
        self.importance = match self.kind {
            // === CRÍTICO (0.9-1.0) ===
            EventKind::Alert => 0.95,
            EventKind::ClaimRejected | EventKind::ScopeViolation => 0.9,

            // === ALTO (0.7-0.9) ===
            EventKind::Decision => 0.85,
            EventKind::PatchApplied
            | EventKind::ClaimVerified
            | EventKind::VerificationRecorded => 0.8,
            EventKind::Invariant => 0.75,
            // Humanos importantes
            EventKind::Dream => 0.8,             // Objetivos del usuario
            EventKind::PromiseMade => 0.75,      // Compromisos
            EventKind::Insight => 0.75,          // Revelaciones importantes
            EventKind::PreferenceLearned => 0.7, // Aprendizaje del usuario

            // === MEDIO (0.5-0.7) ===
            EventKind::Run | EventKind::Action => 0.6,
            EventKind::Observation => 0.5,
            EventKind::PatchProposed => 0.55,
            // Humanos medio-alto
            EventKind::Teaching | EventKind::Correction => 0.6,
            EventKind::Philosophy | EventKind::Metaphor => 0.55,
            EventKind::CreativeIdea | EventKind::DesignProposed => 0.6,
            EventKind::DateRemembered => 0.65, // Fechas importantes
            EventKind::PromiseKept => 0.65,
            EventKind::PersonalStory => 0.5,
            EventKind::PatternLearned | EventKind::CategoryDiscovered => 0.6,
            EventKind::NewKindDiscovered => 0.7, // Meta-evolución

            // === BAJO (0.3-0.5) ===
            EventKind::FileRead => 0.4,
            EventKind::Query => 0.35,
            // Humanos bajo (ruido conversacional)
            EventKind::Conversation => 0.3,
            EventKind::Emotion | EventKind::Gratitude | EventKind::Frustration => 0.35,
            EventKind::Humor | EventKind::Celebration => 0.3,
            EventKind::Question | EventKind::Confusion => 0.35,
            EventKind::Agreement | EventKind::Disagreement => 0.3,
            EventKind::Apology => 0.35,
            EventKind::Encouragement | EventKind::Empathy | EventKind::Concern => 0.35,
            EventKind::Reflection => 0.4,
            EventKind::AdviceSought | EventKind::AdviceGiven => 0.45,
            EventKind::ThoughtExperiment => 0.45,
            EventKind::NameCreated => 0.4,
            EventKind::ConceptLinked => 0.5,
            EventKind::BehaviorCorrected => 0.5,

            // Default para cualquier tipo no listado
            _ => 0.5,
        };

        // Boost si tiene muchos inputs/outputs (cambio significativo)
        let scope = self.inputs.len() + self.outputs.len();
        if scope > 5 {
            self.importance = (self.importance + 0.1).min(1.0);
        }
    }

    /// Set the project for this event.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project_id = Some(project.into());
        self
    }

    /// Set tags for this event.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Set artifacts for this event.
    pub fn with_artifacts(mut self, artifacts: Vec<ArtifactRef>) -> Self {
        self.artifacts = artifacts;
        self
    }

    /// Set parent event for tracing.
    pub fn with_parent(mut self, parent: EventId) -> Self {
        self.parent_event_id = Some(parent);
        self
    }

    /// Set inputs (files read).
    pub fn with_inputs(mut self, inputs: Vec<String>) -> Self {
        self.inputs = inputs;
        self.calculate_importance(); // Recalcular para aplicar boost por scope
        self
    }

    /// Set outputs (files written).
    pub fn with_outputs(mut self, outputs: Vec<String>) -> Self {
        self.outputs = outputs;
        self.calculate_importance(); // Recalcular para aplicar boost por scope
        self
    }

    /// Set metrics.
    pub fn with_metrics(mut self, metrics: Metrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Set agent ID.
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent_id = agent.into();
        self
    }
}

impl Default for Event {
    fn default() -> Self {
        Self::new(EventKind::Action, "")
    }
}

// ============================================================================
// LEGACY SUPPORT: EventV1 (without importance field)
// ============================================================================
// Events stored before the importance field was added use this format.
// The deserialize_event function tries the new format first, then falls back.

use crate::types::ids::EventId as LegacyEventId;

/// Old Event struct WITHOUT importance field (for bincode compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventV1 {
    pub id: LegacyEventId,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub agent_id: String,
    pub project_id: Option<String>,
    pub kind: EventKind,
    pub description: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub artifacts: Vec<ArtifactRef>,
    pub metrics: Option<Metrics>,
    pub tags: Vec<String>,
    pub parent_event_id: Option<LegacyEventId>,
    pub prev_hash: Option<[u8; 32]>,
    pub this_hash: Option<[u8; 32]>,
}

impl EventV1 {
    /// Convert to current Event format with calculated importance
    pub fn into_event(self) -> Event {
        let mut event = Event {
            id: self.id,
            ts: self.ts,
            agent_id: self.agent_id,
            project_id: self.project_id,
            kind: self.kind,
            description: self.description,
            inputs: self.inputs,
            outputs: self.outputs,
            artifacts: self.artifacts,
            metrics: self.metrics,
            tags: self.tags,
            parent_event_id: self.parent_event_id,
            prev_hash: self.prev_hash,
            this_hash: self.this_hash,
            importance: 0.5, // default, will be calculated
        };
        event.calculate_importance();
        event
    }
}

/// Deserialize an event from bytes, handling both old (V1) and new formats.
/// This provides backward compatibility for events stored before the importance field.
///
/// Fallback chain:
/// 1. Try Event (current format with importance + serde_repr EventKind)
/// 2. Try EventV1 (without importance field)
/// 3. Log diagnosis and return error
pub fn deserialize_event(bytes: &[u8]) -> Result<Event, bincode::Error> {
    // Try new format first (with importance)
    match bincode::deserialize::<Event>(bytes) {
        Ok(event) => return Ok(event),
        Err(e) => {
            // Log para diagnóstico (no es error aún, intentaremos fallback)
            tracing::debug!("Event decode attempt 1 failed (trying V1 fallback): {}", e);
        }
    }

    // Fall back to old format (without importance)
    match bincode::deserialize::<EventV1>(bytes) {
        Ok(old_event) => {
            tracing::debug!("Event decoded via V1 fallback");
            Ok(old_event.into_event())
        }
        Err(e) => {
            // Ambos fallaron - log detallado para diagnóstico
            tracing::warn!(
                "Event decode failed (both V1 and current): {}, bytes_len={}",
                e,
                bytes.len()
            );
            Err(e)
        }
    }
}
