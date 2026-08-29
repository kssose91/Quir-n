//! # Claims Verification System
//!
//! Sistema de verificación de afirmaciones con evidencia obligatoria.
//!
//! ## Principios
//!
//! 1. NINGUNA afirmación sin evidencia verificable
//! 2. Si no hay evidencia → marcar como hipótesis
//! 3. Si falta contexto → pedir más información
//! 4. Audit trail completo de cada claim

pub mod annotator;
pub mod audit;
pub mod backlink;
pub mod evidence;
pub mod ledger_integration;
pub mod policy;
pub mod verifier;

pub use annotator::ClaimAnnotator;
pub use audit::AuditTrail;
pub use evidence::{Evidence, EvidenceSource, EvidenceStrength};
pub use ledger_integration::ClaimLedgerIntegration;
pub use policy::VerificationPolicy;
pub use verifier::ClaimVerifier;

/// Registro de claim con metadatos comunes para audit trail
///
/// Envuelve cualquier tipo de claim con id, origen y timestamps
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaimRecord {
    /// ID único del registro
    pub id: String,
    /// Origen del claim para trazabilidad
    pub origin: ClaimOrigin,
    /// Timestamp de creación
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// El claim en sí
    pub claim: ClaimType,
}

/// Tipo de afirmación según nivel de evidencia
///
/// Serialización: usa adjacently tagged enum para JSON estable
/// Ejemplo: {"type": "Verified", "data": {...}}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClaimType {
    /// Verificada con evidencia directa
    Verified(VerifiedClaim),

    /// Hipótesis sin verificar
    Hypothesis(HypothesisClaim),

    /// Observación directa del código/estado actual
    DirectObservation(DirectObservationClaim),

    /// Necesita más contexto para verificar
    NeedsContext(NeedsContextClaim),

    /// Citando fuente externa (documentación, usuario)
    Citation(CitationClaim),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifiedClaim {
    /// ID único del claim (para audit trail)
    pub id: String,

    /// Texto de la afirmación
    pub text: String,

    /// Evidencias que la soportan
    pub evidence: Vec<Evidence>,

    /// Nivel de confianza (0.0 - 1.0)
    /// INVARIANTE: debe cumplir 0.0 <= confidence <= 1.0
    /// Ver VerificationPolicy para umbrales por strictness level
    pub confidence: f32,

    /// Método(s) de verificación usado(s)
    pub verification_methods: Vec<VerificationMethod>,

    /// Timestamp de creación del claim
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Timestamp de verificación
    pub verified_at: chrono::DateTime<chrono::Utc>,

    /// Contexto de origen (conversación, evento ledger, etc)
    pub origin: ClaimOrigin,
}

/// Origen del claim para trazabilidad
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaimOrigin {
    /// ID de la conversación donde se creó
    pub conversation_id: Option<String>,
    /// ID del evento en el ledger (si aplica)
    pub ledger_event_id: Option<String>,
    /// ID del context packet usado
    pub context_packet_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HypothesisClaim {
    /// Texto de la hipótesis
    pub text: String,

    /// Razonamiento que lleva a esta hipótesis
    pub reasoning: String,

    /// Evidencia indirecta o parcial
    pub partial_evidence: Vec<Evidence>,

    /// Qué se necesitaría para verificar
    pub needs_verification: Vec<VerificationNeed>,

    /// Nivel de confianza en la hipótesis
    pub confidence: f32,

    /// Riesgos si la hipótesis es incorrecta
    pub risks_if_wrong: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DirectObservationClaim {
    /// Texto de la observación
    pub text: String,

    /// Fuente exacta de la observación
    pub source: EvidenceSource,

    /// Extracto literal
    pub excerpt: String,

    /// Timestamp de cuándo se observó
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NeedsContextClaim {
    /// Afirmación parcial
    pub partial_text: String,

    /// Qué información falta
    pub missing: Vec<MissingInfo>,

    /// Queries sugeridos para obtener lo que falta
    pub suggested_queries: Vec<SuggestedQuery>,

    /// Lo que se puede afirmar con la información actual
    pub can_assert: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CitationClaim {
    /// Texto citado
    pub text: String,

    /// Fuente de la cita
    pub source: CitationSource,

    /// Contexto adicional
    pub context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VerificationMethod {
    /// Búsqueda directa en código
    CodeSearch,
    /// Búsqueda en ledger/memoria
    LedgerSearch,
    /// Análisis AST
    AstAnalysis,
    /// Ejecución de test
    TestExecution,
    /// Confirmación del usuario
    UserConfirmation,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerificationNeed {
    /// Descripción de lo que se necesita
    pub description: String,
    /// Cómo obtenerlo
    pub how_to_get: String,
    /// Prioridad
    pub priority: Priority,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Priority {
    /// Baja prioridad (ord = 0)
    Low,
    /// Prioridad media (ord = 1)
    Medium,
    /// Alta prioridad (ord = 2)
    High,
    /// Prioridad crítica (ord = 3)
    Critical,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MissingInfo {
    /// Descripción de la información faltante
    pub description: String,
    /// Por qué es necesaria
    pub why_needed: String,
    /// Dónde buscarla
    pub where_to_look: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SuggestedQuery {
    /// Query para ejecutar
    pub query: String,
    /// Tipo de búsqueda
    pub search_type: SearchType,
    /// Qué esperar del resultado
    pub expected_result: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SearchType {
    CodeGrep,
    SemanticSearch,
    LedgerQuery,
    FileRead,
    AskUser,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CitationSource {
    UserStatement {
        conversation_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
        message_id: String,
    },
    Documentation {
        path: std::path::PathBuf,
    },
    Comment {
        path: std::path::PathBuf,
        /// Línea del comentario (1-indexed: L1 = primera línea)
        line: usize,
    },
    ExternalUrl {
        url: String,
    },
}

// === Implementaciones con validación de invariantes ===

// === Funciones de validación ===

/// Valida que un valor de confidence sea finito y esté en rango
pub fn valid_confidence(x: f32) -> bool {
    x.is_finite() && (0.0..=1.0).contains(&x)
}

/// Normaliza confidence: NaN/Inf -> 0.0, fuera de rango -> clamped
fn normalize_confidence(x: f32) -> f32 {
    if !x.is_finite() {
        0.0 // NaN o Infinity -> 0.0 (más conservador)
    } else {
        x.clamp(0.0, 1.0)
    }
}

// === Implementaciones ===

impl ClaimRecord {
    /// Crear nuevo registro de claim
    pub fn new(claim: ClaimType, origin: ClaimOrigin) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            origin,
            created_at: chrono::Utc::now(),
            claim,
        }
    }
}

impl VerifiedClaim {
    /// Crear nuevo VerifiedClaim con confidence validada
    ///
    /// NOTA: verification_methods debe tener al menos un elemento
    pub fn new(
        text: String,
        evidence: Vec<Evidence>,
        confidence: f32,
        verification_methods: Vec<VerificationMethod>,
        origin: ClaimOrigin,
    ) -> Result<Self, &'static str> {
        if verification_methods.is_empty() {
            return Err("verification_methods cannot be empty for VerifiedClaim");
        }

        let now = chrono::Utc::now();
        Ok(Self {
            id: ulid::Ulid::new().to_string(),
            text,
            evidence,
            confidence: normalize_confidence(confidence),
            verification_methods,
            created_at: now,
            verified_at: now,
            origin,
        })
    }

    /// Actualizar confidence con validación (NaN -> 0.0)
    pub fn set_confidence(&mut self, confidence: f32) {
        self.confidence = normalize_confidence(confidence);
    }
}

impl HypothesisClaim {
    /// Crear nueva hipótesis con confidence validada (NaN -> 0.0)
    pub fn new(text: String, reasoning: String, confidence: f32) -> Self {
        Self {
            text,
            reasoning,
            partial_evidence: Vec::new(),
            needs_verification: Vec::new(),
            confidence: normalize_confidence(confidence),
            risks_if_wrong: Vec::new(),
        }
    }

    /// Actualizar confidence con validación (NaN -> 0.0)
    pub fn set_confidence(&mut self, confidence: f32) {
        self.confidence = normalize_confidence(confidence);
    }
}

impl ClaimOrigin {
    /// Origen vacío (para casos donde no hay contexto disponible)
    pub fn empty() -> Self {
        Self {
            conversation_id: None,
            ledger_event_id: None,
            context_packet_id: None,
        }
    }

    /// Desde conversación
    pub fn from_conversation(id: String) -> Self {
        Self {
            conversation_id: Some(id),
            ledger_event_id: None,
            context_packet_id: None,
        }
    }
}
