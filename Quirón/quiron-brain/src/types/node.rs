//! Node types for the cognitive graph.

use crate::types::ids::{EventId, NodeId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A node in the cognitive graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique node ID.
    pub id: NodeId,

    /// Type of node.
    pub kind: NodeKind,

    /// Name/title of the node.
    pub name: String,

    /// Optional description.
    pub description: Option<String>,

    /// Project this node belongs to.
    pub project_id: Option<String>,

    /// When the node was created.
    pub created_at: DateTime<Utc>,

    /// Last modification time.
    pub updated_at: DateTime<Utc>,

    /// Event that created this node.
    pub created_by_event: EventId,

    /// Additional properties (JSON string).
    pub properties: String,

    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
}

/// Types of nodes in the cognitive graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    // === Structure ===
    /// A project.
    Project,
    /// A goal/objective.
    Goal,
    /// A task to complete.
    Task,
    /// A milestone to reach.
    Milestone,

    // === Actions ===
    /// A decision that was made.
    Decision,
    /// An action that was performed.
    Action,
    /// An observation that was recorded.
    Observation,

    // === Knowledge ===
    /// A hypothesis to test.
    Hypothesis,
    /// A claim that can be validated.
    Claim,
    /// A concept/topic.
    Concept,

    // === Artifacts ===
    /// A file/artifact.
    Artifact,
    /// A text chunk for semantic search.
    Chunk,

    // === Índice de código (unidades derivadas del árbol sintáctico) ===
    /// Unidad Archivo: un fichero fuente del proyecto.
    FileUnit,
    /// Unidad Lógica: una función, método, struct, enum, trait o impl.
    LogicUnit,
    /// Unidad Cambio: la transición de hash de un archivo o símbolo.
    ChangeUnit,

    // === Verification ===
    /// A test/build run.
    Run,
    /// Evidence supporting/refuting a claim.
    Evidence,

    // === Problems ===
    /// A failure that occurred.
    Failure,
    /// A fix that was applied.
    Fix,

    // === System ===
    /// An invariant rule.
    Invariant,
    /// An alert that was triggered.
    Alert,
    /// A protocol/procedure.
    Protocol,

    // === Human ===
    /// A user preference.
    Preference,
    /// A constraint/limitation.
    Constraint,

    // === Conversaciones Humanas ===
    /// Conversación general.
    Conversation,
    /// Expresión emocional.
    Emotion,
    /// Humor, broma.
    Humor,
    /// Reflexión filosófica.
    Philosophy,
    /// Metáfora.
    Metaphor,
    /// Sueño u objetivo.
    Dream,
    /// Recuerdo compartido.
    Memory,
    /// Enseñanza.
    Teaching,
    /// Pregunta importante.
    Question,

    // === Relaciones ===
    /// Historia personal.
    PersonalStory,
    /// Promesa hecha.
    Promise,
    /// Empatía expresada.
    Empathy,

    // === Creatividad ===
    /// Idea creativa.
    Creative,
    /// Experimento mental.
    ThoughtExperiment,

    // === Evolución del Cerebro ===
    /// Patrón aprendido.
    Pattern,
    /// Fecha recordada.
    DateMemory,
    /// Nuevo tipo descubierto (meta-evolución).
    Emergent,
}

impl Node {
    /// Create a new node.
    pub fn new(kind: NodeKind, name: impl Into<String>, event_id: EventId) -> Self {
        let now = Utc::now();
        Self {
            id: NodeId::new(),
            kind,
            name: name.into(),
            description: None,
            project_id: None,
            created_at: now,
            updated_at: now,
            created_by_event: event_id,
            properties: "{}".to_string(),
            confidence: 1.0,
        }
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set project.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project_id = Some(project.into());
        self
    }

    pub fn with_properties(mut self, props: serde_json::Value) -> Self {
        self.properties = props.to_string();
        self
    }

    /// Set confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}
