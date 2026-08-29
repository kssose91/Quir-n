//! # Orquestador
//!
//! El corazón de Quirón - decide qué hacer en cada paso.
//!
//! ## Intents posibles
//! - `done`: Tarea completada, devolver resultado
//! - `tool_use`: Ejecutar herramienta y esperar resultado
//! - `recall`: Buscar en memoria antes de responder
//! - `think_more`: Necesito más razonamiento antes de actuar
//! - `ask_user`: Necesito clarificación del usuario

use crate::client::{
    ClientError, CreateSessionTelemetryAnomalyRequest, CreateSessionTelemetryCheckpointRequest,
    QuironClient,
};
use crate::gates::{evidence_exists_gate, evidence_relevance_gate, honesty_gate};
use crate::protocol::WorkerTask;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const TELEMETRY_CHECKPOINT_EVERY_STEPS: u64 = 20;
const TELEMETRY_RAM_SAMPLES_MAX: usize = 200;
const TELEMETRY_SESSION_IDLE_ROTATE_SECS: i64 = 15 * 60;
const TELEMETRY_FALLBACK_MIN_TASKS: u32 = 10;
const TELEMETRY_ANOMALY_CLEAR_STREAK: u8 = 2;

static TELEMETRY_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

/// Intent del modelo - qué quiere hacer a continuación
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Intent {
    /// Tarea completada con respuesta tipada
    Done { response: ResponseKind },
    /// Ejecutar una herramienta
    ToolUse {
        tool: String,
        args: serde_json::Value,
    },
    /// Buscar en memoria
    Recall {
        query: String,
        scope: Option<RecallScope>,
    },
    /// Necesito más razonamiento (inner monologue)
    ThinkMore { thought: String },
    /// Necesito clarificación del usuario
    AskUser {
        question: String,
        options: Option<Vec<String>>,
    },
}

/// Scope para recall
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallScope {
    /// Solo eventos recientes
    Recent,
    /// Solo de un proyecto específico
    Project(String),
    /// Todo (búsqueda semántica)
    All,
}

/// Cita a evidencia (anti-alucinación)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    /// ID del evento que respalda esta afirmación
    pub event_id: String,
    /// Fragmento relevante
    pub snippet: String,
    /// Por qué es relevante
    pub relevance: String,
}

/// Registro de intento de búsqueda (para auditoría)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptLog {
    /// Query que se buscó
    pub query: String,
    /// Scope de búsqueda
    pub scope: RecallScope,
    /// Número de resultados obtenidos
    pub results_count: usize,
    /// ¿Se encontró algo relevante?
    pub relevant_found: bool,
    /// Timestamp del intento
    pub timestamp: String,
}

impl AttemptLog {
    /// Crear nuevo log de intento
    pub fn new(
        query: impl Into<String>,
        scope: RecallScope,
        results_count: usize,
        relevant_found: bool,
    ) -> Self {
        Self {
            query: query.into(),
            scope,
            results_count,
            relevant_found,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Tipo de respuesta del orquestador - tipada para gates de honestidad.
///
/// Separación por nivel de evidencia:
/// - `VerifiedClaim`: Afirmación con evidencia verificable (gate estricto)
/// - `Proposal`: Plan o propuesta sin claim factual
/// - `Hypothesis`: Plausible pero sin evidencia suficiente
/// - `NeedMoreContext`: Falta información para responder
/// - `NoEvidence`: Búsqueda exhaustiva sin resultados
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseKind {
    /// Claim verificada con evidencia (gate estricto: evidence no vacío, confidence >= 0.7)
    VerifiedClaim {
        content: String,
        evidence: Vec<Citation>,
        confidence: f32,
    },

    /// Propuesta o plan sin claim factual (gate relajado)
    Proposal {
        content: String,
        rationale: String,
        citations: Vec<Citation>,
    },

    /// Hipótesis plausible pero sin backlinks suficientes
    Hypothesis {
        content: String,
        plausibility: f32,
        what_would_verify: Vec<String>,
    },

    /// Necesito más contexto para responder correctamente
    NeedMoreContext {
        what_missing: Vec<String>,
        reason: String,
        attempts: Vec<AttemptLog>,
    },

    /// No hay evidencia tras búsqueda exhaustiva
    NoEvidence {
        attempts: Vec<AttemptLog>,
        /// Solo si es true, confidence_in_absence puede ser > 0.5
        scope_was_bounded: bool,
        /// Confianza de que la información NO existe (solo válido si scope_was_bounded)
        confidence_in_absence: Option<f32>,
    },
}

impl ResponseKind {
    /// ¿Esta respuesta contiene una afirmación verificable?
    pub fn is_claim(&self) -> bool {
        matches!(self, ResponseKind::VerifiedClaim { .. })
    }

    /// ¿Esta respuesta indica que no se puede responder?
    pub fn is_inconclusive(&self) -> bool {
        matches!(
            self,
            ResponseKind::NeedMoreContext { .. } | ResponseKind::NoEvidence { .. }
        )
    }

    /// Extraer citations si las hay
    pub fn citations(&self) -> Vec<&Citation> {
        match self {
            ResponseKind::VerifiedClaim { evidence, .. } => evidence.iter().collect(),
            ResponseKind::Proposal { citations, .. } => citations.iter().collect(),
            _ => vec![],
        }
    }
}

// ============================================================================
// SISTEMA DE DELIBERACIÓN COGNITIVA
// ============================================================================

/// Memoria de trabajo entre iteraciones de deliberación.
/// Persiste lo que el sistema ha encontrado/pensado para no "olvidar".
#[derive(Debug, Clone, Default)]
pub struct WorkingSet {
    /// Hipótesis actuales que el sistema está considerando
    pub hypotheses: Vec<Hypothesis>,
    /// Pool de evidencias encontradas
    pub evidence_pool: Vec<Citation>,
    /// Preguntas/subqueries aún sin responder
    pub open_questions: Vec<SubQuery>,
    /// Preguntas/subqueries ya respondidas
    pub answered_questions: Vec<SubQuery>,
    /// Contradicciones detectadas
    pub contradictions: Vec<Contradiction>,
    /// Historial de búsquedas (para no repetir)
    pub search_history: Vec<AttemptLog>,
    /// Notas operativas devueltas por subtareas worker
    pub worker_notes: Vec<WorkerNote>,
    /// Conteo de estabilidad (pasadas sin cambios)
    pub stability_count: u32,
    /// Última cantidad de evidencias (para detectar estabilidad)
    pub last_evidence_count: usize,
    /// Tokens consumidos por subtareas worker (ruta worker).
    pub worker_tokens_used: u32,
    /// Tokens consumidos por llamadas finales primary.
    pub primary_tokens_used: u32,
    /// Estimación de tokens del contexto operativo acumulado en memoria de trabajo.
    pub context_tokens_estimate: u32,
}

impl WorkingSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Añadir evidencia al pool (evitando duplicados)
    pub fn add_evidence(&mut self, citation: Citation) -> bool {
        if !self
            .evidence_pool
            .iter()
            .any(|c| c.event_id == citation.event_id)
        {
            self.evidence_pool.push(citation);
            self.refresh_context_token_estimate();
            true
        } else {
            false
        }
    }

    /// Añadir hipótesis
    pub fn add_hypothesis(&mut self, hypothesis: Hypothesis) {
        self.hypotheses.push(hypothesis);
    }

    /// Registrar salida de worker subordinado
    pub fn add_worker_note(&mut self, note: WorkerNote) {
        self.worker_notes.push(note);
        self.refresh_context_token_estimate();
    }

    /// Verificar contradicción con nueva evidencia usando señales lingüísticas
    /// deterministas (multilingüe básico).
    pub fn check_contradiction(&mut self, new_citation: &Citation) {
        let new_polarity = snippet_polarity(&new_citation.snippet);
        if new_polarity == 0 {
            return;
        }

        let new_tokens = significant_tokens(&new_citation.snippet);
        if new_tokens.len() < 2 {
            return;
        }

        for existing in &self.evidence_pool {
            // Evitar comparar el mismo evento consigo mismo.
            if existing.event_id == new_citation.event_id {
                continue;
            }

            let existing_polarity = snippet_polarity(&existing.snippet);
            if existing_polarity == 0 || existing_polarity == new_polarity {
                continue;
            }

            let existing_tokens = significant_tokens(&existing.snippet);
            let overlap = existing_tokens.intersection(&new_tokens).count();

            // Exigir overlap mínimo para evitar falsos positivos.
            if overlap < 2 {
                continue;
            }

            let already_registered = self.contradictions.iter().any(|c| {
                (c.evidence_a.event_id == existing.event_id
                    && c.evidence_b.event_id == new_citation.event_id)
                    || (c.evidence_a.event_id == new_citation.event_id
                        && c.evidence_b.event_id == existing.event_id)
            });
            if already_registered {
                continue;
            }

            self.contradictions.push(Contradiction {
                claim_a: short_claim(&existing.snippet),
                claim_b: short_claim(&new_citation.snippet),
                evidence_a: existing.clone(),
                evidence_b: new_citation.clone(),
            });
        }
    }

    /// Actualizar estado de estabilidad
    pub fn update_stability(&mut self) {
        if self.evidence_pool.len() == self.last_evidence_count {
            self.stability_count += 1;
        } else {
            self.stability_count = 0;
            self.last_evidence_count = self.evidence_pool.len();
        }
    }

    /// Ratio de cobertura (preguntas respondidas / total)
    pub fn coverage_ratio(&self) -> f32 {
        let total = self.answered_questions.len() + self.open_questions.len();
        if total == 0 {
            return 1.0;
        }
        self.answered_questions.len() as f32 / total as f32
    }

    /// Registra uso de tokens de ruta worker.
    pub fn add_worker_tokens(&mut self, tokens: u32) {
        self.worker_tokens_used = self.worker_tokens_used.saturating_add(tokens);
    }

    /// Registra uso de tokens de ruta primary.
    pub fn add_primary_tokens(&mut self, tokens: u32) {
        self.primary_tokens_used = self.primary_tokens_used.saturating_add(tokens);
    }

    /// Tokens reales consumidos por modelos (worker + primary).
    pub fn total_model_tokens_used(&self) -> u32 {
        self.worker_tokens_used
            .saturating_add(self.primary_tokens_used)
    }

    /// Tokens restantes de presupuesto para llamadas de modelo.
    pub fn remaining_model_budget(&self, token_budget: u32) -> u32 {
        token_budget.saturating_sub(self.total_model_tokens_used())
    }

    /// Recalcula tokens de contexto actual de memoria de trabajo.
    pub fn refresh_context_token_estimate(&mut self) {
        let evidence_tokens: u32 = self
            .evidence_pool
            .iter()
            .map(|c| {
                estimate_tokens_heuristic(&c.snippet)
                    .saturating_add(estimate_tokens_heuristic(&c.relevance))
            })
            .sum();
        let history_tokens: u32 = self
            .search_history
            .iter()
            .map(|a| estimate_tokens_heuristic(&a.query))
            .sum();
        let worker_tokens: u32 = self
            .worker_notes
            .iter()
            .map(|n| {
                estimate_tokens_heuristic(&n.query)
                    .saturating_add(estimate_tokens_heuristic(&n.summary))
            })
            .sum();
        self.context_tokens_estimate = evidence_tokens
            .saturating_add(history_tokens)
            .saturating_add(worker_tokens);
    }
}

fn estimate_tokens_heuristic(text: &str) -> u32 {
    // Aproximación robusta para mixed-lang/code: ~1 token por 4 chars.
    let chars = text.chars().count() as u32;
    (chars.saturating_add(3) / 4).max(1)
}

fn short_claim(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= 140 {
        return trimmed.to_string();
    }
    let mut out = String::new();
    for ch in trimmed.chars().take(137) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn significant_tokens(s: &str) -> HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "the", "this", "that", "with", "without", "from", "into", "have", "has", "had", "are",
        "was", "were", "will", "would", "could", "should", "about", "your", "their", "there",
        "and", "for", "not", "que", "como", "donde", "cuando", "para", "porque", "por", "del",
        "las", "los", "una", "uno", "con", "sin", "sobre", "esto", "esta", "estos", "estas",
        "tiene", "tienen", "puede", "pueden", "fue", "fueron", "ser", "estar",
    ];

    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|t| t.len() >= 4)
        .filter(|t| !STOP_WORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn snippet_polarity(s: &str) -> i8 {
    let lower = s.to_lowercase();
    let tokens = lower
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();

    const POSITIVE: &[&str] = &[
        "ok",
        "pass",
        "passed",
        "fixed",
        "fix",
        "solved",
        "success",
        "succeeded",
        "enabled",
        "valid",
        "verified",
        "correcto",
        "completo",
        "aprobado",
        "permitido",
    ];

    const NEGATIVE: &[&str] = &[
        "no",
        "not",
        "never",
        "without",
        "sin",
        "nunca",
        "fallo",
        "falla",
        "failed",
        "error",
        "missing",
        "invalido",
        "invalid",
        "cannot",
        "cant",
        "denied",
        "blocked",
        "revert",
        "reverted",
        "rechazado",
        "bloqueado",
    ];

    let pos = tokens.iter().filter(|t| POSITIVE.contains(t)).count() as i32;
    let neg = tokens.iter().filter(|t| NEGATIVE.contains(t)).count() as i32;

    match pos.cmp(&neg) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
    }
}

/// Hipótesis (claim candidato antes de verificación)
#[derive(Debug, Clone)]
pub struct Hypothesis {
    pub content: String,
    pub supporting: Vec<Citation>,
    pub contradicting: Vec<Citation>,
    pub confidence: f32,
}

/// Contradicción detectada entre dos evidencias
#[derive(Debug, Clone)]
pub struct Contradiction {
    pub claim_a: String,
    pub claim_b: String,
    pub evidence_a: Citation,
    pub evidence_b: Citation,
}

/// Resultado operativo de una subtarea worker subordinada.
#[derive(Debug, Clone)]
pub struct WorkerNote {
    pub task_id: String,
    pub query: String,
    pub summary: String,
    pub citations: Vec<String>,
    pub confidence: Option<f32>,
}

/// Subquery generada por el SearchPlanner
#[derive(Debug, Clone)]
pub struct SubQuery {
    pub id: u32,
    pub query: String,
    pub scope: RecallScope,
    pub base_priority: f32,
    pub priority: f32,
    pub status: QueryStatus,
}

/// Estado de una subquery
#[derive(Debug, Clone)]
pub enum QueryStatus {
    Pending,
    InProgress,
    Answered { citations: Vec<Citation> },
    NoResults,
}

#[derive(Debug, Clone, Copy, Default)]
struct QueryProfile {
    recency_bias: f32,
    specificity_bias: f32,
}

impl QueryProfile {
    fn from_question(question: &str) -> Self {
        let lower = question.to_lowercase();
        let recency_bias = if has_any_token(
            &lower,
            &[
                "today",
                "latest",
                "recent",
                "now",
                "currently",
                "hoy",
                "ayer",
                "ultima",
                "última",
                "ultimas",
                "últimas",
                "reciente",
                "ahora",
                "actual",
            ],
        ) {
            1.0
        } else {
            0.0
        };

        let specificity_bias = if has_any_token(
            &lower,
            &[
                ".rs",
                ".ts",
                ".js",
                ".py",
                ".go",
                ".java",
                ".md",
                "src/",
                "crates/",
                "error",
                "stacktrace",
                "compil",
                "build",
                "test",
                "failing",
                "failed",
                "panic",
            ],
        ) || lower.contains('/')
        {
            1.0
        } else {
            0.0
        };

        Self {
            recency_bias,
            specificity_bias,
        }
    }
}

fn has_any_token(text: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn scope_bucket(scope: &RecallScope) -> usize {
    match scope {
        RecallScope::All => 0,
        RecallScope::Recent => 1,
        RecallScope::Project(_) => 2,
    }
}

/// Razón por la que el sistema de deliberación para
#[derive(Debug, Clone)]
pub enum StopReason {
    /// Cobertura >= 80% de subqueries respondidas
    CoverageReached { ratio: f32 },
    /// Sin cambios en N pasadas consecutivas
    StabilityReached { iterations: u32 },
    /// Últimas N búsquedas sin resultados nuevos
    NoveltyExhausted { last_new: u32 },
    /// Límite de iteraciones alcanzado - preguntar al usuario si continuar
    AskUserToContinue { iterations: u32, reason: String },
    /// Presupuesto de tokens consumido por modelos
    TokenBudgetReached { used: u32, budget: u32 },
    /// No hay forma de responder honestamente
    EvidenceInsufficient,
    /// No quedan queries pendientes
    QueriesExhausted,
}

/// Política de parada para deliberación
pub struct StopPolicy;

impl StopPolicy {
    pub fn check(
        ws: &WorkingSet,
        max_iterations: u32,
        current_iteration: u32,
    ) -> Option<StopReason> {
        // 1. Budget exhausted -> preguntar al usuario si quiere continuar
        // BUGFIX: era ">" que permitía max_iterations+1, ahora es ">="
        if current_iteration >= max_iterations {
            return Some(StopReason::AskUserToContinue {
                iterations: current_iteration,
                reason: format!(
                    "Llegamos a {} iteraciones. ¿Quieres que siga buscando?",
                    current_iteration
                ),
            });
        }

        // 2. Coverage reached (>= 80%)
        let coverage = ws.coverage_ratio();
        if coverage >= 0.8 && ws.answered_questions.len() >= 2 {
            return Some(StopReason::CoverageReached { ratio: coverage });
        }

        // 3. Stability (3+ pasadas sin cambios)
        if ws.stability_count >= 3 {
            return Some(StopReason::StabilityReached {
                iterations: ws.stability_count,
            });
        }

        // 4. Queries exhausted
        if ws.open_questions.is_empty() && !ws.answered_questions.is_empty() {
            return Some(StopReason::QueriesExhausted);
        }

        None
    }
}

/// Claim individual en el grafo de respuesta
#[derive(Debug, Clone)]
pub struct Claim {
    pub id: usize,
    pub content: String,
    pub citations: Vec<Citation>,
    pub confidence: f32,
    pub status: ClaimStatus,
}

/// Estado de verificación de un claim
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimStatus {
    /// Pasó todos los gates
    Verified,
    /// Plausible pero sin evidencia suficiente
    Hypothesis,
    /// Faltan citations
    NeedsEvidence,
    /// Hay evidencia contradictoria
    Contradicted,
}

/// Relación entre claims
#[derive(Debug, Clone)]
pub enum ClaimRelation {
    Supports,
    Contradicts,
    DependsOn,
    Elaborates,
}

/// Arista en el grafo de claims
#[derive(Debug, Clone)]
pub struct ClaimEdge {
    pub from: usize,
    pub to: usize,
    pub relation: ClaimRelation,
}

/// Grafo de claims (respuesta estructurada)
#[derive(Debug, Clone, Default)]
pub struct ClaimGraph {
    pub claims: Vec<Claim>,
    pub edges: Vec<ClaimEdge>,
}

impl ClaimGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Añadir claim al grafo
    pub fn add_claim(
        &mut self,
        content: String,
        citations: Vec<Citation>,
        confidence: f32,
    ) -> usize {
        let id = self.claims.len();
        let status = if citations.is_empty() {
            ClaimStatus::NeedsEvidence
        } else if confidence >= 0.7 {
            ClaimStatus::Verified
        } else {
            ClaimStatus::Hypothesis
        };

        self.claims.push(Claim {
            id,
            content,
            citations,
            confidence,
            status,
        });
        id
    }

    /// Convertir a ResponseKind tipado
    pub fn to_response_kind(&self, attempts: Vec<AttemptLog>) -> ResponseKind {
        if self.claims.is_empty() {
            return ResponseKind::NeedMoreContext {
                what_missing: vec!["No claims generated".to_string()],
                reason: "Deliberation produced no claims".to_string(),
                attempts,
            };
        }

        // Verificar estado de claims
        let all_verified = self
            .claims
            .iter()
            .all(|c| c.status == ClaimStatus::Verified);
        let any_contradicted = self
            .claims
            .iter()
            .any(|c| c.status == ClaimStatus::Contradicted);
        let any_needs_evidence = self
            .claims
            .iter()
            .any(|c| c.status == ClaimStatus::NeedsEvidence);

        if any_contradicted {
            // Hay contradicción - devolver como Hypothesis con warning
            let content = self
                .claims
                .iter()
                .map(|c| c.content.clone())
                .collect::<Vec<_>>()
                .join("; ");

            return ResponseKind::Hypothesis {
                content,
                plausibility: 0.3,
                what_would_verify: vec!["Resolve contradictions".to_string()],
            };
        }

        if any_needs_evidence {
            // Faltan evidencias
            return ResponseKind::NeedMoreContext {
                what_missing: self
                    .claims
                    .iter()
                    .filter(|c| c.status == ClaimStatus::NeedsEvidence)
                    .map(|c| format!("Evidence for: {}", c.content))
                    .collect(),
                reason: "Some claims lack supporting evidence".to_string(),
                attempts,
            };
        }

        if all_verified {
            // Todos verificados - juntar en VerifiedClaim
            let content = self
                .claims
                .iter()
                .map(|c| c.content.clone())
                .collect::<Vec<_>>()
                .join(". ");

            let evidence: Vec<Citation> = self
                .claims
                .iter()
                .flat_map(|c| c.citations.clone())
                .collect();

            let avg_confidence =
                self.claims.iter().map(|c| c.confidence).sum::<f32>() / self.claims.len() as f32;

            return ResponseKind::VerifiedClaim {
                content,
                confidence: avg_confidence,
                evidence,
            };
        }

        // Algunos son Hypothesis
        let content = self
            .claims
            .iter()
            .map(|c| c.content.clone())
            .collect::<Vec<_>>()
            .join("; ");

        ResponseKind::Hypothesis {
            content,
            plausibility: 0.5,
            what_would_verify: self
                .claims
                .iter()
                .filter(|c| c.status == ClaimStatus::Hypothesis)
                .map(|c| format!("More evidence for: {}", c.content))
                .collect(),
        }
    }
}

/// Planificador de búsquedas
#[derive(Debug, Clone)]
pub struct SearchPlanner {
    queries: Vec<SubQuery>,
    #[allow(dead_code)] // Reserved for query ID tracking
    next_id: u32,
    profile: QueryProfile,
    scope_attempts: [u32; 3],
    scope_hits: [u32; 3],
}

impl SearchPlanner {
    fn priority_for_scope(profile: QueryProfile, scope: &RecallScope) -> f32 {
        let base = match scope {
            RecallScope::All => {
                1.0 + (0.08 * profile.specificity_bias) - (0.05 * profile.recency_bias)
            }
            RecallScope::Recent => 0.9 + (0.25 * profile.recency_bias),
            RecallScope::Project(_) => 0.85 + (0.1 * profile.specificity_bias),
        };
        base.clamp(0.2, 1.6)
    }

    fn keyword_scope(profile: QueryProfile) -> RecallScope {
        if profile.recency_bias >= 0.8 {
            RecallScope::Recent
        } else {
            RecallScope::All
        }
    }

    fn keyword_priority(profile: QueryProfile, scope: &RecallScope) -> f32 {
        let base = match scope {
            RecallScope::Recent => 0.82 + (0.18 * profile.recency_bias),
            _ => 0.7 + (0.15 * profile.specificity_bias),
        };
        base.clamp(0.2, 1.6)
    }

    fn rebalance_pending_priorities(&mut self) {
        for q in self
            .queries
            .iter_mut()
            .filter(|q| matches!(q.status, QueryStatus::Pending))
        {
            let idx = scope_bucket(&q.scope);
            let attempts = self.scope_attempts[idx];
            let hits = self.scope_hits[idx];
            let scope_boost = if attempts == 0 {
                0.0
            } else {
                let hit_rate = hits as f32 / attempts as f32;
                if attempts >= 2 && hit_rate <= 0.2 {
                    -0.14
                } else {
                    ((hit_rate - 0.5) * 0.22).clamp(-0.12, 0.12)
                }
            };
            let profile_bias = match q.scope {
                RecallScope::Recent => 0.04 * self.profile.recency_bias,
                RecallScope::All => 0.03 * self.profile.specificity_bias,
                RecallScope::Project(_) => 0.02 * self.profile.specificity_bias,
            };
            q.priority = (q.base_priority + scope_boost + profile_bias).clamp(0.2, 1.6);
        }
    }

    /// Crear plan de búsqueda a partir de una pregunta
    pub fn plan(question: &str) -> Self {
        let mut queries = Vec::new();
        let mut next_id = 0;
        let profile = QueryProfile::from_question(question);
        let all_priority = Self::priority_for_scope(profile, &RecallScope::All);
        let recent_priority = Self::priority_for_scope(profile, &RecallScope::Recent);

        // Subquery 1: Búsqueda semántica general
        queries.push(SubQuery {
            id: next_id,
            query: question.to_string(),
            scope: RecallScope::All,
            base_priority: all_priority,
            priority: all_priority,
            status: QueryStatus::Pending,
        });
        next_id += 1;

        // Subquery 2: Búsqueda en eventos recientes
        queries.push(SubQuery {
            id: next_id,
            query: question.to_string(),
            scope: RecallScope::Recent,
            base_priority: recent_priority,
            priority: recent_priority,
            status: QueryStatus::Pending,
        });
        next_id += 1;

        // FIX #5: Extraer términos clave con normalización y stopwords multilingües
        fn norm_token(w: &str) -> String {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        }

        // Stopwords inglés + español
        const STOP_WORDS: &[&str] = &[
            // Inglés
            "about", "which", "where", "would", "could", "should", "there", "their", "these",
            "those", "being", "having", "doing", // Español
            "que", "como", "donde", "cuando", "para", "porque", "por", "del", "las", "los", "una",
            "uno", "con", "sin", "sobre", "esto", "esta", "estos", "estas", "estan", "tiene",
            "tienen", "hacer", "hecho", "puede", "pueden", "siendo", "cual", "cuales",
        ];

        let keywords: Vec<String> = question
            .split_whitespace()
            .map(norm_token)
            .filter(|w| w.len() >= 5)
            .filter(|w| !STOP_WORDS.contains(&w.as_str()))
            .take(3)
            .collect();

        for keyword in keywords {
            let scope = Self::keyword_scope(profile);
            let priority = Self::keyword_priority(profile, &scope);
            queries.push(SubQuery {
                id: next_id,
                query: keyword,
                scope,
                base_priority: priority,
                priority,
                status: QueryStatus::Pending,
            });
            next_id += 1;
        }

        let mut planner = Self {
            queries,
            next_id,
            profile,
            scope_attempts: [0; 3],
            scope_hits: [0; 3],
        };
        planner.rebalance_pending_priorities();
        planner
    }

    /// Obtener siguiente query pendiente (por prioridad)
    pub fn next(&mut self) -> Option<&mut SubQuery> {
        let idx = self
            .queries
            .iter()
            .enumerate()
            .filter(|(_, q)| matches!(q.status, QueryStatus::Pending))
            .max_by(|(_, a), (_, b)| {
                a.priority
                    .partial_cmp(&b.priority)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)?;

        let q = self.queries.get_mut(idx)?;
        q.status = QueryStatus::InProgress;
        Some(q)
    }

    /// Obtener lote de queries pendientes (por prioridad) y marcarlas en progreso.
    pub fn next_batch(&mut self, limit: usize) -> Vec<SubQuery> {
        if limit == 0 {
            return Vec::new();
        }

        let mut ranked: Vec<(usize, f32, u32)> = self
            .queries
            .iter()
            .enumerate()
            .filter(|(_, q)| matches!(q.status, QueryStatus::Pending))
            .map(|(idx, q)| (idx, q.priority, q.id))
            .collect();

        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });

        ranked
            .into_iter()
            .take(limit)
            .filter_map(|(idx, _, _)| {
                let q = self.queries.get_mut(idx)?;
                q.status = QueryStatus::InProgress;
                Some(q.clone())
            })
            .collect()
    }

    /// Marcar query como completada
    pub fn mark_completed(&mut self, id: u32, citations: Vec<Citation>) {
        let mut scope = None;
        let mut hit = false;
        if let Some(q) = self.queries.iter_mut().find(|q| q.id == id) {
            scope = Some(q.scope.clone());
            hit = !citations.is_empty();
            q.status = if citations.is_empty() {
                QueryStatus::NoResults
            } else {
                QueryStatus::Answered { citations }
            };
        }
        if let Some(scope) = scope {
            let idx = scope_bucket(&scope);
            self.scope_attempts[idx] = self.scope_attempts[idx].saturating_add(1);
            if hit {
                self.scope_hits[idx] = self.scope_hits[idx].saturating_add(1);
            }
            self.rebalance_pending_priorities();
        }
    }

    /// Revertir query a pending (p.ej. fallo transitorio).
    pub fn mark_pending(&mut self, id: u32) {
        if let Some(q) = self.queries.iter_mut().find(|q| q.id == id) {
            q.status = QueryStatus::Pending;
        }
        self.rebalance_pending_priorities();
    }

    /// Obtener queries respondidas
    pub fn answered(&self) -> Vec<SubQuery> {
        self.queries
            .iter()
            .filter(|q| matches!(q.status, QueryStatus::Answered { .. }))
            .cloned()
            .collect()
    }

    /// Obtener queries pendientes
    pub fn pending(&self) -> Vec<SubQuery> {
        self.queries
            .iter()
            .filter(|q| matches!(q.status, QueryStatus::Pending))
            .cloned()
            .collect()
    }

    /// Contar queries pendientes sin clonar.
    pub fn pending_count(&self) -> usize {
        self.queries
            .iter()
            .filter(|q| matches!(q.status, QueryStatus::Pending))
            .count()
    }

    /// Contar queries en progreso.
    pub fn in_progress_count(&self) -> usize {
        self.queries
            .iter()
            .filter(|q| matches!(q.status, QueryStatus::InProgress))
            .count()
    }

    #[cfg(test)]
    fn profile(&self) -> QueryProfile {
        self.profile
    }
}

/// Estado de una iteración
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Iteration {
    /// Número de iteración
    pub number: u32,
    /// Intent del modelo
    pub intent: Intent,
    /// Resultado de la ejecución (si aplica)
    pub result: Option<String>,
    /// Tokens consumidos
    pub tokens_used: u32,
}

/// Configuración del orquestador
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Máximo de iteraciones antes de forzar respuesta
    pub max_iterations: u32,
    /// Máximo de subtareas de recall en paralelo por iteración cognitiva
    pub max_parallel_subtasks: usize,
    /// Budget de tokens total
    pub token_budget: u32,
    /// Confianza mínima para responder sin recall
    pub min_confidence_without_recall: f32,
    /// Forzar recall si no hay citas
    pub require_citations: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100, // Deliberación profunda - pregunta al usuario cuando se agote
            max_parallel_subtasks: 3,
            token_budget: 100_000,
            min_confidence_without_recall: 0.8,
            require_citations: true,
        }
    }
}

/// Métricas de eficiencia planner + worker/local.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationMetrics {
    /// Tareas operativas resueltas localmente (recall/search sin LLM).
    pub local_tasks_total: u32,
    /// Tareas delegadas a worker (ruta `worker`).
    pub worker_tasks_total: u32,
    /// Llamadas finales al modelo primary.
    pub primary_calls_total: u32,
    /// Tasa de fallback al primary sobre el total de tareas.
    pub llm_fallback_rate: f32,
    /// Estimación simple de tokens ahorrados por tareas no-primary.
    pub tokens_saved_estimate: u32,
    /// Tokens reales consumidos por modelos durante la sesión actual.
    pub model_tokens_used: u32,
    /// Presupuesto total de tokens configurado.
    pub token_budget: u32,
    /// Presupuesto restante de tokens.
    pub token_budget_remaining: u32,
    /// EWMA de tokens por subtarea worker.
    pub worker_tokens_ewma: Option<f64>,
    /// EWMA de tokens por llamada primary.
    pub primary_tokens_ewma: Option<f64>,
    /// Umbral mínimo dinámico estimado por subtarea worker.
    pub worker_min_tokens_threshold: u32,
    /// Umbral mínimo dinámico estimado para reservar llamada primary.
    pub primary_min_tokens_threshold: u32,
    /// Escala manual aplicada al threshold worker.
    pub worker_threshold_scale: f32,
    /// Escala manual aplicada al threshold primary.
    pub primary_threshold_scale: f32,
    /// Paralelismo actual de subtareas en esta sesión.
    pub parallel_subtasks_current: usize,
    /// Límite máximo configurado de subtareas paralelas.
    pub parallel_subtasks_cap: usize,
    /// Latencia EWMA de subtareas recall (ms).
    pub recall_latency_ewma_ms: Option<f64>,
    /// Latencia EWMA de subtareas worker (ms).
    pub worker_latency_ewma_ms: Option<f64>,
}

/// Tipo de anomalía detectada en telemetría operativa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryAnomalyKind {
    LatencyCritical,
    LatencyDegraded,
    FallbackAnomalous,
    BudgetLow,
    BudgetCritical,
}

impl TelemetryAnomalyKind {
    fn as_key(self) -> &'static str {
        match self {
            TelemetryAnomalyKind::LatencyCritical => "latency_critical",
            TelemetryAnomalyKind::LatencyDegraded => "latency_degraded",
            TelemetryAnomalyKind::FallbackAnomalous => "fallback_anomalous",
            TelemetryAnomalyKind::BudgetLow => "budget_low",
            TelemetryAnomalyKind::BudgetCritical => "budget_critical",
        }
    }
}

/// Muestra fina de telemetría en RAM (ventana corta).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTelemetrySample {
    pub session_id: String,
    pub step: u64,
    pub request_iteration: u32,
    pub timestamp: String,
    pub tasks_total: u32,
    pub worker_tasks_total: u32,
    pub primary_calls_total: u32,
    pub model_tokens_used_total: u32,
    pub token_budget: u32,
    pub token_budget_remaining: u32,
    pub llm_fallback_rate: f32,
    pub worker_tokens_ewma: Option<f64>,
    pub primary_tokens_ewma: Option<f64>,
    pub recall_latency_ewma_ms: Option<f64>,
    pub worker_latency_ewma_ms: Option<f64>,
    pub parallel_subtasks_current: usize,
    pub parallel_subtasks_cap: usize,
    pub worker_threshold_scale: f32,
    pub primary_threshold_scale: f32,
}

/// Checkpoint agregado por tramo de sesión.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTelemetryCheckpoint {
    pub schema_version: u16,
    pub session_id: String,
    pub parent_session_id: Option<String>,
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
    pub worker_tokens_ewma: Option<f64>,
    pub primary_tokens_ewma: Option<f64>,
    pub worker_latency_ewma_ms: Option<f64>,
    pub recall_latency_ewma_ms: Option<f64>,
    pub llm_fallback_rate: f32,
    pub parallel_subtasks_current: usize,
    pub parallel_subtasks_cap: usize,
    pub worker_threshold_scale: f32,
    pub primary_threshold_scale: f32,
    pub anomaly_flags: Vec<String>,
    pub preset_change: Option<String>,
}

/// Evento de anomalía detectada durante sesión.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTelemetryAnomaly {
    pub session_id: String,
    pub segment_seq: u32,
    pub step: u64,
    pub request_iteration: u32,
    pub timestamp: String,
    pub kind: TelemetryAnomalyKind,
    pub detail: String,
}

/// Snapshot de telemetría de sesión para UI/diagnóstico.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTelemetrySnapshot {
    pub schema_version: u16,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub project_id: Option<String>,
    pub segment_seq: u32,
    pub checkpoints: Vec<SessionTelemetryCheckpoint>,
    pub anomalies: Vec<SessionTelemetryAnomaly>,
    pub recent_samples: Vec<SessionTelemetrySample>,
}

#[derive(Debug, Clone)]
struct TelemetryCapture {
    checkpoint: Option<SessionTelemetryCheckpoint>,
    anomalies: Vec<SessionTelemetryAnomaly>,
}

#[derive(Debug, Clone)]
struct SessionTelemetryState {
    session_id: String,
    parent_session_id: Option<String>,
    project_id: Option<String>,
    segment_seq: u32,
    last_activity_at: chrono::DateTime<chrono::Utc>,
    next_step: u64,
    last_checkpoint_step: u64,
    last_checkpoint_ts: chrono::DateTime<chrono::Utc>,
    last_checkpoint_tokens_used: u32,
    checkpoints: Vec<SessionTelemetryCheckpoint>,
    anomalies: Vec<SessionTelemetryAnomaly>,
    recent_samples: VecDeque<SessionTelemetrySample>,
    active_anomalies: HashSet<TelemetryAnomalyKind>,
    anomaly_recovery: HashMap<TelemetryAnomalyKind, u8>,
    latency_degraded_streak: u8,
    pending_runtime_changes: Vec<String>,
}

fn new_telemetry_session_id(now: chrono::DateTime<chrono::Utc>) -> String {
    let seq = TELEMETRY_SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("sess-{}-{}", now.timestamp_millis(), seq)
}

impl SessionTelemetryState {
    fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            session_id: new_telemetry_session_id(now),
            parent_session_id: None,
            project_id: None,
            segment_seq: 0,
            last_activity_at: now,
            next_step: 1,
            last_checkpoint_step: 0,
            last_checkpoint_ts: now,
            last_checkpoint_tokens_used: 0,
            checkpoints: Vec::new(),
            anomalies: Vec::new(),
            recent_samples: VecDeque::new(),
            active_anomalies: HashSet::new(),
            anomaly_recovery: HashMap::new(),
            latency_degraded_streak: 0,
            pending_runtime_changes: Vec::new(),
        }
    }

    fn begin_request(&mut self, now: chrono::DateTime<chrono::Utc>) {
        let idle_secs = (now - self.last_activity_at).num_seconds();
        if idle_secs > TELEMETRY_SESSION_IDLE_ROTATE_SECS {
            let previous = self.session_id.clone();
            self.session_id = new_telemetry_session_id(now);
            self.parent_session_id = Some(previous);
            self.segment_seq = 0;
            self.last_checkpoint_step = 0;
            self.last_checkpoint_tokens_used = 0;
            self.last_checkpoint_ts = now;
            self.active_anomalies.clear();
            self.anomaly_recovery.clear();
            self.latency_degraded_streak = 0;
            self.pending_runtime_changes.clear();
        }
        self.last_activity_at = now;
    }

    fn push_sample(&mut self, sample: SessionTelemetrySample) {
        self.last_activity_at = chrono::Utc::now();
        self.recent_samples.push_back(sample);
        while self.recent_samples.len() > TELEMETRY_RAM_SAMPLES_MAX {
            self.recent_samples.pop_front();
        }
    }

    fn note_runtime_change(&mut self, change: String) {
        self.pending_runtime_changes.push(change);
    }

    fn active_anomaly_flags(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .active_anomalies
            .iter()
            .map(|kind| kind.as_key().to_string())
            .collect();
        out.sort();
        out
    }

    fn consume_runtime_changes(&mut self) -> Option<String> {
        if self.pending_runtime_changes.is_empty() {
            None
        } else {
            Some(
                self.pending_runtime_changes
                    .drain(..)
                    .collect::<Vec<_>>()
                    .join(" | "),
            )
        }
    }
}

/// El Orquestador - bucle principal de Quirón
pub struct Orchestrator {
    config: OrchestratorConfig,
    client: QuironClient,
    history: VecDeque<Iteration>,
    tokens_used: u32,
    iteration: u32,
    // Auditoría / trazabilidad
    attempts: Vec<AttemptLog>,
    citations_pool: Vec<Citation>,
    // Métricas de coordinación planner/worker
    local_tasks_total: u32,
    worker_tasks_total: u32,
    primary_calls_total: u32,
    // Scheduler adaptativo de concurrencia
    current_parallel_subtasks: usize,
    // Señales de latencia (EWMA)
    recall_latency_ewma_ms: Option<f64>,
    worker_latency_ewma_ms: Option<f64>,
    // Señales de consumo de tokens por ruta (EWMA)
    worker_tokens_ewma: Option<f64>,
    primary_tokens_ewma: Option<f64>,
    // Ajustes operativos en caliente (modo operador)
    worker_threshold_scale: f32,
    primary_threshold_scale: f32,
    // Telemetría de sesión para checkpoints/anomalías
    telemetry: SessionTelemetryState,
}

impl Orchestrator {
    fn parallel_cap(config: &OrchestratorConfig) -> usize {
        config.max_parallel_subtasks.max(1)
    }

    fn initial_parallel_limit(config: &OrchestratorConfig) -> usize {
        // Arranque conservador: 2 como base, respetando el cap.
        Self::parallel_cap(config).min(2)
    }

    /// Crear orquestador con configuración por defecto
    pub fn new(client: QuironClient) -> Self {
        Self::with_config(client, OrchestratorConfig::default())
    }

    /// Crear orquestador con configuración personalizada
    pub fn with_config(client: QuironClient, config: OrchestratorConfig) -> Self {
        let initial_parallel = Self::initial_parallel_limit(&config);
        let mut telemetry = SessionTelemetryState::new();
        telemetry.project_id = client.project_id().map(str::to_string);
        Self {
            config,
            client,
            history: VecDeque::new(),
            tokens_used: 0,
            iteration: 0,
            attempts: Vec::new(),
            citations_pool: Vec::new(),
            local_tasks_total: 0,
            worker_tasks_total: 0,
            primary_calls_total: 0,
            current_parallel_subtasks: initial_parallel,
            recall_latency_ewma_ms: None,
            worker_latency_ewma_ms: None,
            worker_tokens_ewma: None,
            primary_tokens_ewma: None,
            worker_threshold_scale: 1.0,
            primary_threshold_scale: 1.0,
            telemetry,
        }
    }

    /// Procesar una petición del usuario
    pub async fn process(&mut self, user_request: &str) -> OrchestratorResult {
        // Reset state
        self.history.clear();
        self.tokens_used = 0;
        self.iteration = 0;
        self.attempts.clear();
        self.citations_pool.clear();
        self.local_tasks_total = 0;
        self.worker_tasks_total = 0;
        self.primary_calls_total = 0;
        self.current_parallel_subtasks = Self::initial_parallel_limit(&self.config);
        self.recall_latency_ewma_ms = None;
        self.worker_latency_ewma_ms = None;
        self.worker_tokens_ewma = None;
        self.primary_tokens_ewma = None;
        self.telemetry.begin_request(chrono::Utc::now());

        // Obtener contexto inicial de quiron-brain
        let _context = match self.client.get_context().await {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                tracing::warn!("Failed to get context from quiron-brain: {}", e);
                None
            }
        };

        // Log evento de petición
        let _ = self
            .client
            .create_event(crate::client::CreateEventRequest {
                kind: "OBSERVATION".to_string(),
                description: format!(
                    "User request: {}",
                    user_request.chars().take(200).collect::<String>()
                ),
                project_id: self.client.project_id().map(str::to_string),
                tags: Some(vec!["user_request".to_string()]),
                inputs: None,
                outputs: None,
            })
            .await;

        // =====================================================================
        // BUCLE DE DELIBERACIÓN COGNITIVA
        // =====================================================================
        let deliberation = self.deliberate(user_request).await;
        if deliberation.token_budget_exhausted {
            let _ = self
                .client
                .create_event(crate::client::CreateEventRequest {
                    kind: "OBSERVATION".to_string(),
                    description: format!(
                        "Token budget exhausted: used={} budget={}",
                        self.tokens_used, self.config.token_budget
                    ),
                    project_id: self.client.project_id().map(str::to_string),
                    tags: Some(vec![
                        "deliberation".to_string(),
                        "budget".to_string(),
                        "exhausted".to_string(),
                    ]),
                    inputs: None,
                    outputs: None,
                })
                .await;
            return OrchestratorResult::TokenBudgetExhausted {
                tokens_used: self.tokens_used,
                partial_result: Self::response_preview(&deliberation.response),
            };
        }
        let response = deliberation.response;

        // =====================================================================
        // PIPELINE DE GATES (después de deliberación)
        // =====================================================================

        // Gate 1: Honestidad estructural (sync, 0 IO)
        let honesty = honesty_gate(&response);
        if !honesty.is_allowed() {
            tracing::warn!("Honesty gate BLOCKED after deliberation: {:?}", honesty);
            return self.degrade_response("honesty", &honesty, &response).await;
        }

        // Gate 2: Existencia de evidencia (async, IO acotado)
        let exists = evidence_exists_gate(&response, &self.client).await;
        if !exists.is_allowed() {
            tracing::warn!("Evidence exists gate BLOCKED: {:?}", exists);
            return self
                .degrade_response("evidence-exists", &exists, &response)
                .await;
        }

        // Gate 3: Relevancia de evidencia (sync, 0 IO)
        let events = self.fetch_evidence_events(&response).await;
        let relevance = evidence_relevance_gate(&response, &events);
        if !relevance.is_allowed() {
            tracing::warn!("Evidence relevance gate BLOCKED: {:?}", relevance);
            return self
                .degrade_response("evidence-relevance", &relevance, &response)
                .await;
        }

        // Log éxito
        let _ = self
            .client
            .create_event(crate::client::CreateEventRequest {
                kind: "ACTION".to_string(),
                description: format!(
                    "Deliberation complete: iter={} | local_tasks={} | worker_tasks={} | primary_calls={} | fallback_rate={:.2} | tokens_saved_est={} | parallel={}/{} | recall_ewma_ms={} | worker_ewma_ms={}",
                    self.iteration,
                    self.local_tasks_total,
                    self.worker_tasks_total,
                    self.primary_calls_total,
                    self.fallback_rate(),
                    self.estimated_tokens_saved(),
                    self.current_parallel_subtasks,
                    Self::parallel_cap(&self.config),
                    Self::fmt_ms(self.recall_latency_ewma_ms),
                    Self::fmt_ms(self.worker_latency_ewma_ms),
                ),
                project_id: self.client.project_id().map(str::to_string),
                tags: Some(vec![
                    "response".to_string(),
                    "verified".to_string(),
                    "deliberation".to_string(),
                    "efficiency_metrics".to_string(),
                ]),
                inputs: None,
                outputs: None,
            })
            .await;

        OrchestratorResult::Complete {
            response,
            iterations: self.iteration,
            tokens_used: self.tokens_used,
        }
    }

    /// Bucle de deliberación cognitiva.
    /// Itera hasta encontrar evidencia suficiente o agotar opciones.
    async fn deliberate(&mut self, user_request: &str) -> DeliberationOutcome {
        // 1. Inicializar memoria de trabajo y planificador
        let mut working_set = WorkingSet::new();
        let mut planner = SearchPlanner::plan(user_request);

        // Inicializar open_questions desde el planner
        working_set.open_questions = planner.pending();
        working_set.refresh_context_token_estimate();

        tracing::info!(
            "Starting deliberation: {} subqueries planned",
            working_set.open_questions.len()
        );

        // Log inicio de deliberación
        let _ = self
            .client
            .create_event(crate::client::CreateEventRequest {
                kind: "OBSERVATION".to_string(),
                description: format!(
                    "Deliberation started: {} subqueries ({})",
                    working_set.open_questions.len(),
                    working_set
                        .open_questions
                        .iter()
                        .map(|q| q.query.chars().take(30).collect::<String>())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                project_id: self.client.project_id().map(str::to_string),
                tags: Some(vec!["deliberation".to_string(), "start".to_string()]),
                inputs: None,
                outputs: None,
            })
            .await;

        // 2. Bucle de deliberación
        let mut stop_reason = loop {
            self.iteration += 1;

            if working_set.total_model_tokens_used() >= self.config.token_budget {
                break StopReason::TokenBudgetReached {
                    used: working_set.total_model_tokens_used(),
                    budget: self.config.token_budget,
                };
            }

            // 2a. Verificar condición de parada
            if let Some(reason) =
                StopPolicy::check(&working_set, self.config.max_iterations, self.iteration)
            {
                tracing::info!("Stop condition reached: {:?}", reason);
                break reason;
            }

            // 2b. Obtener siguiente lote de subqueries para ejecución paralela
            let parallel_cap = Self::parallel_cap(&self.config);
            let parallel_limit = self.current_parallel_subtasks.clamp(1, parallel_cap);
            let remaining_budget = working_set.remaining_model_budget(self.config.token_budget);
            let worker_floor = self.worker_min_tokens_threshold();
            let primary_reserve = self.primary_min_tokens_threshold();
            let min_required_budget = (parallel_limit as u32)
                .saturating_mul(worker_floor)
                .saturating_add(primary_reserve);
            if remaining_budget < min_required_budget {
                break StopReason::TokenBudgetReached {
                    used: working_set.total_model_tokens_used(),
                    budget: self.config.token_budget,
                };
            }
            let batch = planner.next_batch(parallel_limit);
            if batch.is_empty() {
                tracing::info!("No more queries pending");
                break StopReason::QueriesExhausted;
            }

            tracing::debug!(
                "Deliberation iter {}: running {} subqueries in parallel",
                self.iteration,
                batch.len()
            );

            // 2c. Ejecutar recall y worker en paralelo (subtareas de la mente)
            self.local_tasks_total = self.local_tasks_total.saturating_add(batch.len() as u32);
            self.worker_tasks_total = self.worker_tasks_total.saturating_add(batch.len() as u32);

            let mut recalls = FuturesUnordered::new();
            let mut workers = FuturesUnordered::new();
            let worker_context = self.build_worker_context(&working_set);

            for subquery in batch {
                let query_id = subquery.id;
                let query_text = subquery.query.clone();
                let query_scope = subquery.scope.clone();
                let recall_client = self.client.clone();

                recalls.push(async move {
                    let started_at = Instant::now();
                    let outcome = Orchestrator::execute_recall_with_client(
                        &recall_client,
                        &query_text,
                        &Some(query_scope.clone()),
                    )
                    .await;
                    let latency_ms = started_at.elapsed().as_secs_f64() * 1000.0;
                    (query_id, query_text, query_scope, outcome, latency_ms)
                });

                let worker_client = self.client.clone();
                let worker_query = subquery.query;
                let worker_scope = subquery.scope;
                let worker_context = worker_context.clone();
                let task_id = format!("worker-{}-{}", self.iteration, query_id);
                workers.push(async move {
                    let started_at = Instant::now();
                    let task = WorkerTask {
                        task_id: task_id.clone(),
                        kind: "memory_recall_subtask".to_string(),
                        objective: format!(
                            "Apoyar al planner con pistas y citas para '{}'",
                            worker_query
                        ),
                        constraints: vec![
                            "no_final_claims".to_string(),
                            "return_strict_json".to_string(),
                        ],
                    };

                    let prompt = format!(
                        "Subquery: {}\nScope: {:?}\nDevuelve resumen operativo y citations candidatas.",
                        worker_query, worker_scope
                    );
                    let result = worker_client
                        .send_to_worker_with_usage(
                            &task,
                            &prompt,
                            &worker_context,
                            Some("Eres worker subordinado del planner."),
                        )
                        .await;

                    let latency_ms = started_at.elapsed().as_secs_f64() * 1000.0;
                    (query_id, worker_query, task_id, result, latency_ms)
                });
            }

            let mut new_evidence_count = 0;
            let mut completed_in_batch = 0usize;
            let mut recall_latency_sum_ms = 0.0f64;
            let mut recall_latency_samples = 0usize;
            while let Some((query_id, query_text, query_scope, outcome, latency_ms)) =
                recalls.next().await
            {
                completed_in_batch += 1;
                recall_latency_sum_ms += latency_ms;
                recall_latency_samples += 1;
                tracing::debug!(
                    "Deliberation iter {} subquery done: id={} query='{}' scope={:?} citations={} latency_ms={:.1}",
                    self.iteration,
                    query_id,
                    query_text,
                    query_scope,
                    outcome.citations.len(),
                    latency_ms
                );

                // 2d. Integrar resultados en working_set
                working_set.search_history.push(outcome.attempt.clone());
                working_set.refresh_context_token_estimate();
                self.attempts.push(outcome.attempt);

                for citation in &outcome.citations {
                    // Detectar contradicciones entre evidencia previa y nueva.
                    working_set.check_contradiction(citation);

                    if working_set.add_evidence(citation.clone()) {
                        new_evidence_count += 1;
                        self.citations_pool.push(citation.clone());
                    }
                }

                // 2e. Marcar query como completada
                planner.mark_completed(query_id, outcome.citations);
            }

            let mut worker_ok = 0usize;
            let mut worker_failed = 0usize;
            let mut worker_latency_sum_ms = 0.0f64;
            let mut worker_latency_samples = 0usize;
            while let Some((query_id, query_text, task_id, result, latency_ms)) =
                workers.next().await
            {
                worker_latency_sum_ms += latency_ms;
                worker_latency_samples += 1;
                match result {
                    Ok(worker_execution) => {
                        worker_ok += 1;
                        self.tokens_used = self
                            .tokens_used
                            .saturating_add(worker_execution.tokens_used);
                        self.update_worker_tokens_ewma(worker_execution.tokens_used);
                        working_set.add_worker_tokens(worker_execution.tokens_used);
                        let worker_result = worker_execution.result;
                        working_set.add_worker_note(WorkerNote {
                            task_id: task_id.clone(),
                            query: query_text,
                            summary: worker_result.summary,
                            citations: worker_result.citations,
                            confidence: worker_result.confidence,
                        });
                        tracing::debug!(
                            "Worker subtask ok: iter={} query_id={} task_id={} latency_ms={:.1}",
                            self.iteration,
                            query_id,
                            task_id,
                            latency_ms
                        );
                    }
                    Err(e) => {
                        worker_failed += 1;
                        tracing::warn!(
                            "Worker subtask failed: iter={} query_id={} task_id={} latency_ms={:.1} error={}",
                            self.iteration,
                            query_id,
                            task_id,
                            latency_ms,
                            e
                        );
                    }
                }
            }

            let recall_avg_ms = if recall_latency_samples > 0 {
                Some(recall_latency_sum_ms / recall_latency_samples as f64)
            } else {
                None
            };
            let worker_avg_ms = if worker_latency_samples > 0 {
                Some(worker_latency_sum_ms / worker_latency_samples as f64)
            } else {
                None
            };
            self.update_latency_ewma(recall_avg_ms, worker_avg_ms);

            // Actualizar preguntas abiertas/respondidas
            working_set.answered_questions = planner.answered();
            working_set.open_questions = planner.pending();

            // 2f. Actualizar estabilidad
            working_set.update_stability();

            let pending_after = planner.pending_count();
            let parallel_adjustment = self.adapt_parallel_limit(
                &working_set,
                pending_after,
                worker_ok,
                worker_failed,
                new_evidence_count,
            );

            // Log progreso
            if self.iteration % 5 == 0 || new_evidence_count > 0 {
                let adjustment = parallel_adjustment
                    .as_deref()
                    .map(|v| format!(" | {}", v))
                    .unwrap_or_default();
                let _ = self
                    .client
                    .create_event(crate::client::CreateEventRequest {
                        kind: "OBSERVATION".to_string(),
                        description: format!(
                            "Deliberation iter {}: parallel={}/{} batch={} +{} evidence, worker_ok={}, worker_failed={}, {}/{} queries, stability={}, recall_ms(avg/ewma)={}/{}, worker_ms(avg/ewma)={}/{}, model_tokens={}/{}, context_tokens_est={}{}",
                            self.iteration,
                            parallel_limit,
                            parallel_cap,
                            completed_in_batch,
                            new_evidence_count,
                            worker_ok,
                            worker_failed,
                            working_set.answered_questions.len(),
                            working_set.answered_questions.len() + working_set.open_questions.len(),
                            working_set.stability_count,
                            Self::fmt_ms(recall_avg_ms),
                            Self::fmt_ms(self.recall_latency_ewma_ms),
                            Self::fmt_ms(worker_avg_ms),
                            Self::fmt_ms(self.worker_latency_ewma_ms),
                            working_set.total_model_tokens_used(),
                            self.config.token_budget,
                            working_set.context_tokens_estimate,
                            adjustment
                        ),
                        project_id: self.client.project_id().map(str::to_string),
                        tags: Some(vec!["deliberation".to_string(), "progress".to_string()]),
                        inputs: None,
                        outputs: None,
                    })
                    .await;
            }

            let capture = self.capture_telemetry_step();
            self.persist_telemetry_capture(capture).await;
        };

        // 3. Log razón de parada
        let _ = self
            .client
            .create_event(crate::client::CreateEventRequest {
                kind: "OBSERVATION".to_string(),
                description: format!(
                    "Deliberation stopped: {:?}. Evidence: {}, Contradictions: {}",
                    stop_reason,
                    working_set.evidence_pool.len(),
                    working_set.contradictions.len()
                ),
                project_id: self.client.project_id().map(str::to_string),
                tags: Some(vec!["deliberation".to_string(), "stop".to_string()]),
                inputs: None,
                outputs: None,
            })
            .await;

        if let Some(checkpoint) = self.force_telemetry_checkpoint() {
            self.persist_telemetry_capture(TelemetryCapture {
                checkpoint: Some(checkpoint),
                anomalies: vec![],
            })
            .await;
        }

        // =====================================================================
        // PASO 3: LLAMAR AL LLM (via vertex-gateway)
        // =====================================================================
        // Si después del recall no tenemos suficiente evidencia, consultamos al LLM.
        // El LLM puede:
        // - Responder con información de su entrenamiento
        // - Decir "no sé" (honestidad estructural)
        // - Pedir más contexto
        //
        // IMPORTANTE: Todo pasa por vertex-gateway (binario inmutable de seguridad)

        let mut llm_failure: Option<String> = None;
        let llm_response = if working_set.evidence_pool.is_empty()
            || working_set.coverage_ratio() < 0.6
        {
            tracing::info!(
                "Calling LLM via quiron-brain (coverage={:.0}% evidence={})",
                working_set.coverage_ratio() * 100.0,
                working_set.evidence_pool.len()
            );

            let remaining_budget = working_set.remaining_model_budget(self.config.token_budget);
            let llm_min_budget = self.primary_min_tokens_threshold();
            if remaining_budget < llm_min_budget {
                tracing::warn!(
                    "Skipping LLM call: remaining token budget too low (remaining={}, min_primary={}, budget={})",
                    remaining_budget,
                    llm_min_budget,
                    self.config.token_budget
                );
                stop_reason = StopReason::TokenBudgetReached {
                    used: working_set.total_model_tokens_used(),
                    budget: self.config.token_budget,
                };
                None
            } else {
                // Construir contexto desde evidencias encontradas
                let context = self.build_llm_context(&working_set);

                // Llamar al LLM via quiron-brain /v1/messages
                match self.call_llm(user_request, &context).await {
                    Ok((response, tokens_used)) => {
                        self.tokens_used = self.tokens_used.saturating_add(tokens_used);
                        self.update_primary_tokens_ewma(tokens_used);
                        working_set.add_primary_tokens(tokens_used);
                        tracing::info!("LLM responded: {} chars", response.len());

                        // Log evento de llamada LLM
                        let _ = self
                            .client
                            .create_event(crate::client::CreateEventRequest {
                                kind: "ACTION".to_string(),
                                description: format!(
                                    "LLM called via vertex-gateway: {} chars, {} tokens",
                                    response.len(),
                                    tokens_used
                                ),
                                project_id: self.client.project_id().map(str::to_string),
                                tags: Some(vec!["llm".to_string(), "vertex-gateway".to_string()]),
                                inputs: None,
                                outputs: None,
                            })
                            .await;

                        Some(response)
                    }
                    Err(e) => {
                        tracing::warn!("LLM call failed: {}", e);
                        llm_failure = Some(e.to_string());

                        // Log fallo
                        let _ = self
                            .client
                            .create_event(crate::client::CreateEventRequest {
                                kind: "OBSERVATION".to_string(),
                                description: format!("LLM call failed: {}", e),
                                project_id: self.client.project_id().map(str::to_string),
                                tags: Some(vec!["llm".to_string(), "error".to_string()]),
                                inputs: None,
                                outputs: None,
                            })
                            .await;

                        None
                    }
                }
            }
        } else {
            tracing::info!(
                "Sufficient evidence found (coverage={:.0}%), skipping LLM",
                working_set.coverage_ratio() * 100.0
            );
            None
        };

        // 4. Sintetizar respuesta (ahora con posible respuesta del LLM)
        let token_budget_exhausted = matches!(stop_reason, StopReason::TokenBudgetReached { .. });
        let response = self.synthesize_response(
            &working_set,
            stop_reason,
            llm_response.as_deref(),
            llm_failure.as_deref(),
        );
        DeliberationOutcome {
            response,
            token_budget_exhausted,
        }
    }

    /// Construir contexto para el LLM desde el working set
    fn build_llm_context(&self, ws: &WorkingSet) -> String {
        let mut context = String::new();

        // Añadir evidencias encontradas
        if !ws.evidence_pool.is_empty() {
            context.push_str("### Evidencias encontradas en memoria:\n");
            for (i, citation) in ws.evidence_pool.iter().take(5).enumerate() {
                context.push_str(&format!(
                    "{}. [{}] {}\n   Relevancia: {}\n\n",
                    i + 1,
                    citation.event_id,
                    citation.snippet,
                    citation.relevance
                ));
            }
        }

        // Añadir búsquedas realizadas
        if !ws.search_history.is_empty() {
            context.push_str("### Búsquedas realizadas:\n");
            for attempt in ws.search_history.iter().take(5) {
                context.push_str(&format!(
                    "- Query: '{}' | Resultados: {} | Relevante: {}\n",
                    attempt.query,
                    attempt.results_count,
                    if attempt.relevant_found { "sí" } else { "no" }
                ));
            }
        }

        if !ws.worker_notes.is_empty() {
            context.push_str("### Subtareas worker (subordinadas):\n");
            for note in ws.worker_notes.iter().take(5) {
                context.push_str(&format!(
                    "- task={} query='{}' conf={} summary={}\n",
                    note.task_id,
                    note.query,
                    note.confidence
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "-".to_string()),
                    note.summary
                ));
                if !note.citations.is_empty() {
                    context.push_str(&format!("  citations: {:?}\n", note.citations));
                }
            }
        }

        context
    }

    /// Contexto específico para worker: mínimo y operativo, sin autoridad final.
    fn build_worker_context(&self, ws: &WorkingSet) -> String {
        let mut context = String::new();
        if !ws.evidence_pool.is_empty() {
            context.push_str("Evidencias actuales:\n");
            for citation in ws.evidence_pool.iter().take(4) {
                context.push_str(&format!("- [{}] {}\n", citation.event_id, citation.snippet));
            }
        } else {
            context.push_str("Sin evidencias previas en esta iteración.\n");
        }
        context.push_str("Rol: generar pistas y citas; no emitir claims finales.");
        context
    }

    /// Llamar al LLM via quiron-brain /v1/messages.
    /// Devuelve el texto y el costo real de tokens (input + output).
    async fn call_llm(
        &mut self,
        query: &str,
        context: &str,
    ) -> Result<(String, u32), crate::client::ClientError> {
        self.primary_calls_total = self.primary_calls_total.saturating_add(1);
        let response = self.client.send_to_llm(query, context, None, None).await?;
        let response_text = response.text();

        let tokens_used = response
            .usage
            .as_ref()
            .map(|u| u.input_tokens.saturating_add(u.output_tokens))
            .unwrap_or_else(|| {
                estimate_tokens_heuristic(query)
                    .saturating_add(estimate_tokens_heuristic(context))
                    .saturating_add(estimate_tokens_heuristic(&response_text))
            });

        tracing::info!("LLM call used {} tokens", tokens_used);

        Ok((response_text, tokens_used))
    }

    fn update_latency_ewma(&mut self, recall_avg_ms: Option<f64>, worker_avg_ms: Option<f64>) {
        const ALPHA: f64 = 0.35;

        if let Some(ms) = recall_avg_ms {
            self.recall_latency_ewma_ms = Some(match self.recall_latency_ewma_ms {
                Some(prev) => prev * (1.0 - ALPHA) + ms * ALPHA,
                None => ms,
            });
        }

        if let Some(ms) = worker_avg_ms {
            self.worker_latency_ewma_ms = Some(match self.worker_latency_ewma_ms {
                Some(prev) => prev * (1.0 - ALPHA) + ms * ALPHA,
                None => ms,
            });
        }
    }

    fn update_worker_tokens_ewma(&mut self, tokens: u32) {
        self.worker_tokens_ewma = Some(match self.worker_tokens_ewma {
            Some(prev) => prev * 0.65 + (tokens as f64) * 0.35,
            None => tokens as f64,
        });
    }

    fn update_primary_tokens_ewma(&mut self, tokens: u32) {
        self.primary_tokens_ewma = Some(match self.primary_tokens_ewma {
            Some(prev) => prev * 0.65 + (tokens as f64) * 0.35,
            None => tokens as f64,
        });
    }

    /// Umbral dinámico mínimo estimado por subtarea worker.
    fn worker_min_tokens_threshold(&self) -> u32 {
        let from_ewma = self
            .worker_tokens_ewma
            .map(|v| (v * 0.75 + 64.0).round() as u32)
            .unwrap_or(160);
        let scaled = (from_ewma as f32 * self.worker_threshold_scale).round() as u32;
        scaled.clamp(96, 2048)
    }

    /// Reserva dinámica mínima para permitir una llamada final primary.
    fn primary_min_tokens_threshold(&self) -> u32 {
        let from_ewma = self
            .primary_tokens_ewma
            .map(|v| (v * 0.8 + 256.0).round() as u32)
            .unwrap_or(1200);
        let scaled = (from_ewma as f32 * self.primary_threshold_scale).round() as u32;
        scaled.clamp(512, 16384)
    }

    fn fmt_ms(ms: Option<f64>) -> String {
        ms.map(|v| format!("{:.1}", v))
            .unwrap_or_else(|| "-".to_string())
    }

    fn response_preview(response: &ResponseKind) -> Option<String> {
        fn truncate(value: &str) -> String {
            let mut out = String::new();
            for ch in value.chars().take(320) {
                out.push(ch);
            }
            out
        }

        let text = match response {
            ResponseKind::VerifiedClaim { content, .. } => content.as_str(),
            ResponseKind::Proposal { content, .. } => content.as_str(),
            ResponseKind::Hypothesis { content, .. } => content.as_str(),
            ResponseKind::NeedMoreContext { reason, .. } => reason.as_str(),
            ResponseKind::NoEvidence { .. } => "No evidence found before token budget exhaustion",
        };
        if text.trim().is_empty() {
            None
        } else {
            Some(truncate(text))
        }
    }

    /// Ajustar dinámicamente el paralelismo según señales de calidad.
    fn adapt_parallel_limit(
        &mut self,
        ws: &WorkingSet,
        pending_after: usize,
        worker_ok: usize,
        worker_failed: usize,
        new_evidence_count: usize,
    ) -> Option<String> {
        let cap = Self::parallel_cap(&self.config);
        let mut next = self.current_parallel_subtasks.clamp(1, cap);
        let prev = next;
        let coverage = ws.coverage_ratio();
        let recall_ewma = self.recall_latency_ewma_ms;
        let worker_ewma = self.worker_latency_ewma_ms;

        let latency_hot = recall_ewma.map(|v| v > 1200.0).unwrap_or(false)
            || worker_ewma.map(|v| v > 2200.0).unwrap_or(false);
        let latency_cool = recall_ewma.map(|v| v < 450.0).unwrap_or(false)
            && worker_ewma.map(|v| v < 900.0).unwrap_or(false);

        // Protección de latencia: si se dispara el tiempo, reducimos fan-out.
        if latency_hot && next > 1 {
            next -= 1;
        // Protección: si el worker falla más de lo que acierta, reducimos presión.
        } else if worker_failed > worker_ok && next > 1 {
            next -= 1;
        // Exploración: si no hay novedad y hay trabajo pendiente, subimos concurrencia.
        } else if ws.stability_count >= 2
            && new_evidence_count == 0
            && pending_after > next
            && next < cap
        {
            next += 1;
        // Si la latencia va bien y hay backlog, podemos aumentar.
        } else if latency_cool && pending_after > next + 1 && worker_failed == 0 && next < cap {
            next += 1;
        // Enfoque: con cobertura alta reducimos fan-out para cerrar síntesis.
        } else if coverage >= 0.75 && next > 1 {
            next -= 1;
        // Señal positiva: worker sano + evidencia nueva -> podemos escalar si queda backlog.
        } else if worker_failed == 0
            && worker_ok > 0
            && new_evidence_count > 0
            && pending_after > next + 1
            && next < cap
        {
            next += 1;
        }

        if next != prev {
            self.current_parallel_subtasks = next;
            return Some(format!(
                "parallel_adjust {}->{} (coverage={:.2}, pending={}, worker_ok={}, worker_failed={}, new_ev={}, recall_ewma_ms={}, worker_ewma_ms={})",
                prev,
                next,
                coverage,
                pending_after,
                worker_ok,
                worker_failed,
                new_evidence_count,
                Self::fmt_ms(recall_ewma),
                Self::fmt_ms(worker_ewma),
            ));
        }

        None
    }

    fn fallback_rate(&self) -> f32 {
        let total = self
            .local_tasks_total
            .saturating_add(self.worker_tasks_total)
            .saturating_add(self.primary_calls_total);
        if total == 0 {
            0.0
        } else {
            self.primary_calls_total as f32 / total as f32
        }
    }

    fn estimated_tokens_saved(&self) -> u32 {
        // Heurística conservadora: recall local evita ~220 tokens, worker ~140.
        self.local_tasks_total.saturating_mul(220) + self.worker_tasks_total.saturating_mul(140)
    }

    fn telemetry_total_tasks(&self) -> u32 {
        self.local_tasks_total
            .saturating_add(self.worker_tasks_total)
            .saturating_add(self.primary_calls_total)
    }

    fn capture_telemetry_step(&mut self) -> TelemetryCapture {
        let now = chrono::Utc::now();
        let step = self.telemetry.next_step;
        self.telemetry.next_step = self.telemetry.next_step.saturating_add(1);

        let sample = SessionTelemetrySample {
            session_id: self.telemetry.session_id.clone(),
            step,
            request_iteration: self.iteration,
            timestamp: now.to_rfc3339(),
            tasks_total: self.telemetry_total_tasks(),
            worker_tasks_total: self.worker_tasks_total,
            primary_calls_total: self.primary_calls_total,
            model_tokens_used_total: self.tokens_used,
            token_budget: self.config.token_budget,
            token_budget_remaining: self.config.token_budget.saturating_sub(self.tokens_used),
            llm_fallback_rate: self.fallback_rate(),
            worker_tokens_ewma: self.worker_tokens_ewma,
            primary_tokens_ewma: self.primary_tokens_ewma,
            recall_latency_ewma_ms: self.recall_latency_ewma_ms,
            worker_latency_ewma_ms: self.worker_latency_ewma_ms,
            parallel_subtasks_current: self.current_parallel_subtasks,
            parallel_subtasks_cap: Self::parallel_cap(&self.config),
            worker_threshold_scale: self.worker_threshold_scale,
            primary_threshold_scale: self.primary_threshold_scale,
        };

        let previous = self.telemetry.recent_samples.back().cloned();
        self.telemetry.push_sample(sample.clone());

        let anomalies = self.detect_telemetry_anomalies(now, &sample, previous.as_ref());
        let checkpoint = if step.saturating_sub(self.telemetry.last_checkpoint_step)
            >= TELEMETRY_CHECKPOINT_EVERY_STEPS
        {
            Some(self.build_telemetry_checkpoint(now, &sample))
        } else {
            None
        };

        TelemetryCapture {
            checkpoint,
            anomalies,
        }
    }

    fn force_telemetry_checkpoint(&mut self) -> Option<SessionTelemetryCheckpoint> {
        let last = self.telemetry.recent_samples.back().cloned()?;
        if last.step <= self.telemetry.last_checkpoint_step {
            return None;
        }
        Some(self.build_telemetry_checkpoint(chrono::Utc::now(), &last))
    }

    fn build_telemetry_checkpoint(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
        sample: &SessionTelemetrySample,
    ) -> SessionTelemetryCheckpoint {
        self.telemetry.segment_seq = self.telemetry.segment_seq.saturating_add(1);
        let step_start = self.telemetry.last_checkpoint_step.saturating_add(1);
        let checkpoint = SessionTelemetryCheckpoint {
            schema_version: 1,
            session_id: self.telemetry.session_id.clone(),
            parent_session_id: self.telemetry.parent_session_id.clone(),
            project_id: self.telemetry.project_id.clone(),
            segment_seq: self.telemetry.segment_seq,
            step_start,
            step_end: sample.step,
            ts_start: self.telemetry.last_checkpoint_ts.to_rfc3339(),
            ts_end: now.to_rfc3339(),
            tasks_total: sample.tasks_total,
            worker_tasks_total: sample.worker_tasks_total,
            primary_calls_total: sample.primary_calls_total,
            model_tokens_used_delta: sample
                .model_tokens_used_total
                .saturating_sub(self.telemetry.last_checkpoint_tokens_used),
            model_tokens_used_total: sample.model_tokens_used_total,
            token_budget: sample.token_budget,
            token_budget_remaining: sample.token_budget_remaining,
            worker_tokens_ewma: sample.worker_tokens_ewma,
            primary_tokens_ewma: sample.primary_tokens_ewma,
            worker_latency_ewma_ms: sample.worker_latency_ewma_ms,
            recall_latency_ewma_ms: sample.recall_latency_ewma_ms,
            llm_fallback_rate: sample.llm_fallback_rate,
            parallel_subtasks_current: sample.parallel_subtasks_current,
            parallel_subtasks_cap: sample.parallel_subtasks_cap,
            worker_threshold_scale: sample.worker_threshold_scale,
            primary_threshold_scale: sample.primary_threshold_scale,
            anomaly_flags: self.telemetry.active_anomaly_flags(),
            preset_change: self.telemetry.consume_runtime_changes(),
        };

        self.telemetry.last_checkpoint_step = sample.step;
        self.telemetry.last_checkpoint_tokens_used = sample.model_tokens_used_total;
        self.telemetry.last_checkpoint_ts = now;
        self.telemetry.checkpoints.push(checkpoint.clone());
        checkpoint
    }

    fn detect_telemetry_anomalies(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
        sample: &SessionTelemetrySample,
        previous: Option<&SessionTelemetrySample>,
    ) -> Vec<SessionTelemetryAnomaly> {
        let mut out = Vec::new();

        let latency_critical = sample
            .worker_latency_ewma_ms
            .map(|v| v > 2200.0)
            .unwrap_or(false)
            || sample
                .recall_latency_ewma_ms
                .map(|v| v > 1200.0)
                .unwrap_or(false);

        let latency_degraded_now = match (previous, sample.worker_latency_ewma_ms) {
            (Some(prev), Some(curr)) => prev
                .worker_latency_ewma_ms
                .map(|base| base > 0.0 && curr > base * 1.5)
                .unwrap_or(false),
            _ => false,
        } || match (previous, sample.recall_latency_ewma_ms) {
            (Some(prev), Some(curr)) => prev
                .recall_latency_ewma_ms
                .map(|base| base > 0.0 && curr > base * 1.5)
                .unwrap_or(false),
            _ => false,
        };
        if latency_degraded_now {
            self.telemetry.latency_degraded_streak =
                self.telemetry.latency_degraded_streak.saturating_add(1);
        } else {
            self.telemetry.latency_degraded_streak = 0;
        }
        let latency_degraded = self.telemetry.latency_degraded_streak >= 2;

        let fallback_anomalous =
            sample.tasks_total >= TELEMETRY_FALLBACK_MIN_TASKS && sample.llm_fallback_rate >= 0.35;

        let remaining_pct = if sample.token_budget == 0 {
            0.0
        } else {
            sample.token_budget_remaining as f32 / sample.token_budget as f32
        };
        let budget_low = remaining_pct <= 0.15;
        let budget_critical = remaining_pct <= 0.05;

        out.extend(self.transition_anomaly(
            now,
            sample,
            TelemetryAnomalyKind::LatencyCritical,
            latency_critical,
            format!(
                "latency_ewma recall_ms={} worker_ms={}",
                Self::fmt_ms(sample.recall_latency_ewma_ms),
                Self::fmt_ms(sample.worker_latency_ewma_ms)
            ),
        ));
        out.extend(self.transition_anomaly(
            now,
            sample,
            TelemetryAnomalyKind::LatencyDegraded,
            latency_degraded,
            format!(
                "latency_degraded streak={} worker_ms={} recall_ms={}",
                self.telemetry.latency_degraded_streak,
                Self::fmt_ms(sample.worker_latency_ewma_ms),
                Self::fmt_ms(sample.recall_latency_ewma_ms)
            ),
        ));
        out.extend(self.transition_anomaly(
            now,
            sample,
            TelemetryAnomalyKind::FallbackAnomalous,
            fallback_anomalous,
            format!(
                "fallback_rate={:.2} tasks_total={}",
                sample.llm_fallback_rate, sample.tasks_total
            ),
        ));
        out.extend(self.transition_anomaly(
            now,
            sample,
            TelemetryAnomalyKind::BudgetLow,
            budget_low,
            format!(
                "budget_remaining={}/{} ({:.1}%)",
                sample.token_budget_remaining,
                sample.token_budget,
                remaining_pct * 100.0
            ),
        ));
        out.extend(self.transition_anomaly(
            now,
            sample,
            TelemetryAnomalyKind::BudgetCritical,
            budget_critical,
            format!(
                "budget_critical remaining={}/{} ({:.1}%)",
                sample.token_budget_remaining,
                sample.token_budget,
                remaining_pct * 100.0
            ),
        ));

        out
    }

    fn transition_anomaly(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
        sample: &SessionTelemetrySample,
        kind: TelemetryAnomalyKind,
        triggered: bool,
        detail: String,
    ) -> Vec<SessionTelemetryAnomaly> {
        let mut out = Vec::new();
        if triggered {
            self.telemetry.anomaly_recovery.insert(kind, 0);
            if !self.telemetry.active_anomalies.contains(&kind) {
                self.telemetry.active_anomalies.insert(kind);
                let entry = SessionTelemetryAnomaly {
                    session_id: self.telemetry.session_id.clone(),
                    segment_seq: self.telemetry.segment_seq,
                    step: sample.step,
                    request_iteration: sample.request_iteration,
                    timestamp: now.to_rfc3339(),
                    kind,
                    detail,
                };
                self.telemetry.anomalies.push(entry.clone());
                out.push(entry);
            }
            return out;
        }

        if self.telemetry.active_anomalies.contains(&kind) {
            let streak = self.telemetry.anomaly_recovery.entry(kind).or_insert(0);
            *streak = streak.saturating_add(1);
            if *streak >= TELEMETRY_ANOMALY_CLEAR_STREAK {
                self.telemetry.active_anomalies.remove(&kind);
                self.telemetry.anomaly_recovery.remove(&kind);
            }
        }

        out
    }

    async fn persist_telemetry_capture(&self, capture: TelemetryCapture) {
        if let Some(checkpoint) = capture.checkpoint {
            let req = CreateSessionTelemetryCheckpointRequest {
                schema_version: checkpoint.schema_version,
                session_id: checkpoint.session_id.clone(),
                parent_session_id: checkpoint.parent_session_id.clone(),
                project_id: checkpoint.project_id.clone(),
                segment_seq: checkpoint.segment_seq,
                step_start: checkpoint.step_start,
                step_end: checkpoint.step_end,
                ts_start: checkpoint.ts_start.clone(),
                ts_end: checkpoint.ts_end.clone(),
                tasks_total: checkpoint.tasks_total,
                worker_tasks_total: checkpoint.worker_tasks_total,
                primary_calls_total: checkpoint.primary_calls_total,
                model_tokens_used_delta: checkpoint.model_tokens_used_delta,
                model_tokens_used_total: checkpoint.model_tokens_used_total,
                token_budget: checkpoint.token_budget,
                token_budget_remaining: checkpoint.token_budget_remaining,
                worker_tokens_ewma: checkpoint.worker_tokens_ewma,
                primary_tokens_ewma: checkpoint.primary_tokens_ewma,
                worker_latency_ewma_ms: checkpoint.worker_latency_ewma_ms,
                recall_latency_ewma_ms: checkpoint.recall_latency_ewma_ms,
                llm_fallback_rate: checkpoint.llm_fallback_rate,
                parallel_subtasks_current: checkpoint.parallel_subtasks_current,
                parallel_subtasks_cap: checkpoint.parallel_subtasks_cap,
                worker_threshold_scale: checkpoint.worker_threshold_scale,
                primary_threshold_scale: checkpoint.primary_threshold_scale,
                anomaly_flags: checkpoint.anomaly_flags.clone(),
                preset_change: checkpoint.preset_change.clone(),
            };

            if self
                .client
                .create_session_telemetry_checkpoint(req)
                .await
                .is_err()
            {
                let _ = self
                    .client
                    .create_event(crate::client::CreateEventRequest {
                        kind: "OBSERVATION".to_string(),
                        description: format!(
                            "telemetry_checkpoint session={} seg={} steps={}..{} tasks={} tok_delta={} tok_total={} budget_rem={}/{} fb={:.2} flags={}",
                            checkpoint.session_id,
                            checkpoint.segment_seq,
                            checkpoint.step_start,
                            checkpoint.step_end,
                            checkpoint.tasks_total,
                            checkpoint.model_tokens_used_delta,
                            checkpoint.model_tokens_used_total,
                            checkpoint.token_budget_remaining,
                            checkpoint.token_budget,
                            checkpoint.llm_fallback_rate,
                            if checkpoint.anomaly_flags.is_empty() {
                                "-".to_string()
                            } else {
                                checkpoint.anomaly_flags.join(",")
                            }
                        ),
                        project_id: checkpoint.project_id.clone(),
                        tags: Some(vec![
                            "telemetry".to_string(),
                            "checkpoint".to_string(),
                            "session".to_string(),
                            "fallback_event".to_string(),
                        ]),
                        inputs: None,
                        outputs: None,
                    })
                    .await;
            }
        }

        for anomaly in capture.anomalies {
            let req = CreateSessionTelemetryAnomalyRequest {
                schema_version: 1,
                session_id: anomaly.session_id.clone(),
                segment_seq: anomaly.segment_seq,
                step: anomaly.step,
                request_iteration: anomaly.request_iteration,
                timestamp: anomaly.timestamp.clone(),
                kind: anomaly.kind.as_key().to_string(),
                detail: anomaly.detail.clone(),
            };

            if self
                .client
                .create_session_telemetry_anomaly(req)
                .await
                .is_err()
            {
                let _ = self
                    .client
                    .create_event(crate::client::CreateEventRequest {
                        kind: "OBSERVATION".to_string(),
                        description: format!(
                            "telemetry_anomaly session={} seg={} step={} iter={} kind={} detail={}",
                            anomaly.session_id,
                            anomaly.segment_seq,
                            anomaly.step,
                            anomaly.request_iteration,
                            anomaly.kind.as_key(),
                            anomaly.detail
                        ),
                        project_id: self.telemetry.project_id.clone(),
                        tags: Some(vec![
                            "telemetry".to_string(),
                            "anomaly".to_string(),
                            anomaly.kind.as_key().to_string(),
                            "fallback_event".to_string(),
                        ]),
                        inputs: None,
                        outputs: None,
                    })
                    .await;
            }
        }
    }

    /// Ajustar budget total de tokens en caliente (modo operador).
    pub fn set_token_budget(&mut self, token_budget: u32) {
        self.config.token_budget = token_budget.clamp(4_096, 2_000_000);
        self.telemetry
            .note_runtime_change(format!("token_budget={}", self.config.token_budget));
    }

    /// Ajustar cap de paralelismo de subtareas en caliente.
    pub fn set_parallel_subtasks_cap(&mut self, max_parallel_subtasks: usize) {
        self.config.max_parallel_subtasks = max_parallel_subtasks.clamp(1, 16);
        let cap = Self::parallel_cap(&self.config);
        self.current_parallel_subtasks = self.current_parallel_subtasks.clamp(1, cap);
        self.telemetry.note_runtime_change(format!(
            "parallel_cap={}",
            self.config.max_parallel_subtasks
        ));
    }

    /// Ajustar escalas manuales de thresholds por ruta en caliente.
    pub fn set_threshold_scales(&mut self, worker_scale: f32, primary_scale: f32) {
        self.worker_threshold_scale = worker_scale.clamp(0.5, 2.0);
        self.primary_threshold_scale = primary_scale.clamp(0.5, 2.0);
        self.telemetry.note_runtime_change(format!(
            "threshold_scales={:.2}/{:.2}",
            self.worker_threshold_scale, self.primary_threshold_scale
        ));
    }

    /// Métricas actuales de coordinación planner/worker.
    pub fn delegation_metrics(&self) -> DelegationMetrics {
        DelegationMetrics {
            local_tasks_total: self.local_tasks_total,
            worker_tasks_total: self.worker_tasks_total,
            primary_calls_total: self.primary_calls_total,
            llm_fallback_rate: self.fallback_rate(),
            tokens_saved_estimate: self.estimated_tokens_saved(),
            model_tokens_used: self.tokens_used,
            token_budget: self.config.token_budget,
            token_budget_remaining: self.config.token_budget.saturating_sub(self.tokens_used),
            worker_tokens_ewma: self.worker_tokens_ewma,
            primary_tokens_ewma: self.primary_tokens_ewma,
            worker_min_tokens_threshold: self.worker_min_tokens_threshold(),
            primary_min_tokens_threshold: self.primary_min_tokens_threshold(),
            worker_threshold_scale: self.worker_threshold_scale,
            primary_threshold_scale: self.primary_threshold_scale,
            parallel_subtasks_current: self.current_parallel_subtasks,
            parallel_subtasks_cap: Self::parallel_cap(&self.config),
            recall_latency_ewma_ms: self.recall_latency_ewma_ms,
            worker_latency_ewma_ms: self.worker_latency_ewma_ms,
        }
    }

    /// Snapshot de telemetría por sesión para diagnóstico y UI.
    pub fn session_telemetry_snapshot(&self) -> SessionTelemetrySnapshot {
        SessionTelemetrySnapshot {
            schema_version: 1,
            session_id: self.telemetry.session_id.clone(),
            parent_session_id: self.telemetry.parent_session_id.clone(),
            project_id: self.telemetry.project_id.clone(),
            segment_seq: self.telemetry.segment_seq,
            checkpoints: self.telemetry.checkpoints.clone(),
            anomalies: self.telemetry.anomalies.clone(),
            recent_samples: self.telemetry.recent_samples.iter().cloned().collect(),
        }
    }

    /// Sintetizar respuesta estructurada desde el working_set
    fn synthesize_response(
        &self,
        ws: &WorkingSet,
        stop_reason: StopReason,
        llm_response: Option<&str>,
        llm_failure: Option<&str>,
    ) -> ResponseKind {
        // =====================================================================
        // REGLA DE SEGURIDAD: LLM NUNCA genera VerifiedClaim
        // =====================================================================
        // El LLM es un REDACTOR, no una fuente de hechos.
        // Sus afirmaciones pueden incluir cosas NO soportadas por las Citation.
        // VerifiedClaim solo puede venir de evidencia pura (sin LLM).
        //
        // Flujo correcto:
        // - LLM con evidencia → Hypothesis (LLM puede alucinar más allá de citas)
        // - LLM sin evidencia → Hypothesis con baja plausibilidad
        // - Solo evidencia (sin LLM) → VerifiedClaim
        // =====================================================================

        if let Some(llm_text) = llm_response {
            // Verificar si el LLM admite no saber (honestidad estructural)
            let admits_ignorance = llm_text.contains("no sé")
                || llm_text.contains("no tengo información")
                || llm_text.contains("I don't know")
                || llm_text.contains("cannot determine")
                || llm_text.contains("no tengo evidencia")
                || llm_text.contains("I don't have");

            if admits_ignorance {
                // El LLM es honesto — devolver como NeedMoreContext
                return ResponseKind::NeedMoreContext {
                    what_missing: vec!["El LLM no tiene información suficiente".to_string()],
                    reason: llm_text.to_string(),
                    attempts: ws.search_history.clone(),
                };
            }

            // ⚠️ CRÍTICO: LLM SIEMPRE genera Hypothesis, NUNCA VerifiedClaim
            // Aunque haya evidencia en el pool, el LLM puede decir cosas
            // no soportadas por esas citas. No podemos verificar automáticamente
            // que cada afirmación del LLM esté respaldada por una cita específica.

            let (plausibility, what_would_verify) = if !ws.evidence_pool.is_empty() {
                // Con evidencia de memoria = Hypothesis con plausibilidad media
                // La evidencia existe pero no verificamos que el LLM la use correctamente
                (
                    0.65,
                    vec![
                        "Verificar que cada afirmación esté soportada por las citas".to_string(),
                        format!("Evidencias disponibles: {} items", ws.evidence_pool.len()),
                    ],
                )
            } else {
                // Sin evidencia de memoria = Hypothesis con baja plausibilidad
                // Esto es "conocimiento del LLM" sin trazabilidad
                (
                    0.4,
                    vec![
                        "⚠️ Sin evidencia de memoria — basado solo en conocimiento del LLM"
                            .to_string(),
                        "Verificar con documentación oficial".to_string(),
                        "Buscar confirmación en el código fuente".to_string(),
                    ],
                )
            };

            return ResponseKind::Hypothesis {
                content: llm_text.to_string(),
                plausibility,
                what_would_verify,
            };
        }

        // =====================================================================
        // PRIORIDAD 2: Sin respuesta LLM, usar solo memoria
        // =====================================================================
        let mut claim_graph = ClaimGraph::new();

        // Si hay contradicciones, manejar primero
        if !ws.contradictions.is_empty() {
            // Añadir claim con status Contradicted
            let contradiction_summary = ws
                .contradictions
                .iter()
                .map(|c| {
                    format!(
                        "'{}' vs '{}'",
                        c.claim_a.chars().take(50).collect::<String>(),
                        c.claim_b.chars().take(50).collect::<String>()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");

            claim_graph.add_claim(
                format!("Found contradictory evidence: {}", contradiction_summary),
                vec![], // Las evidencias están en las contradicciones
                0.3,
            );

            // Marcar el claim como contradicted
            if let Some(claim) = claim_graph.claims.last_mut() {
                claim.status = ClaimStatus::Contradicted;
            }
        }

        // Si hay evidencia, crear claims
        if !ws.evidence_pool.is_empty() {
            // Agrupar evidencias por similitud (simplificado: por evento)
            // En una implementación real, usaríamos clustering/agrupación semántica

            let confidence = if ws.evidence_pool.len() >= 3 {
                0.85
            } else if ws.evidence_pool.len() >= 2 {
                0.75
            } else {
                0.65
            };

            // Crear un claim principal con toda la evidencia
            let summary = ws
                .evidence_pool
                .iter()
                .take(3) // Limitar para no sobrecargar
                .map(|c| c.snippet.chars().take(100).collect::<String>())
                .collect::<Vec<_>>()
                .join(". ");

            claim_graph.add_claim(
                format!(
                    "Based on {} pieces of evidence: {}",
                    ws.evidence_pool.len(),
                    summary
                ),
                ws.evidence_pool.clone(),
                confidence,
            );
        }

        // Si no hay evidencia y se agotó la búsqueda
        if ws.evidence_pool.is_empty() {
            if let Some(error) = llm_failure {
                return ResponseKind::NeedMoreContext {
                    what_missing: vec![
                        "No se pudo completar la consulta al modelo externo".to_string()
                    ],
                    reason: format!("LLM unavailable: {}", error),
                    attempts: ws.search_history.clone(),
                };
            }

            return match stop_reason {
                StopReason::QueriesExhausted | StopReason::NoveltyExhausted { .. } => {
                    // Búsqueda exhaustiva sin resultados
                    // FIX: Solo dar confidence_in_absence si scope_was_bounded es true
                    let bounded = ws
                        .search_history
                        .iter()
                        .all(|a| !matches!(a.scope, RecallScope::All));
                    ResponseKind::NoEvidence {
                        attempts: ws.search_history.clone(),
                        scope_was_bounded: bounded,
                        confidence_in_absence: if bounded { Some(0.6) } else { None },
                    }
                }
                StopReason::AskUserToContinue { iterations, reason } => {
                    // Preguntar al usuario si quiere continuar
                    ResponseKind::NeedMoreContext {
                        what_missing: vec![reason.clone()],
                        reason: format!(
                            "Alcanzadas {} iteraciones. Esperando confirmación del usuario.",
                            iterations
                        ),
                        attempts: ws.search_history.clone(),
                    }
                }
                StopReason::TokenBudgetReached { used, budget } => ResponseKind::NeedMoreContext {
                    what_missing: vec![format!(
                        "Token budget exhausted: used {} of {}",
                        used, budget
                    )],
                    reason: "Se agotó el presupuesto de tokens durante la deliberación".to_string(),
                    attempts: ws.search_history.clone(),
                },
                _ => ResponseKind::NeedMoreContext {
                    what_missing: vec!["No evidence found".to_string()],
                    reason: format!("Deliberation ended: {:?}", stop_reason),
                    attempts: ws.search_history.clone(),
                },
            };
        }

        // Convertir ClaimGraph a ResponseKind
        claim_graph.to_response_kind(ws.search_history.clone())
    }
}

// Mantener compatibilidad con bucle antiguo (deprecated)
impl Orchestrator {
    /// Bucle principal antiguo (deprecated - mantener para tests existentes)
    #[allow(dead_code)]
    async fn process_legacy(&mut self, user_request: &str) -> OrchestratorResult {
        tracing::debug!("process_legacy called, forwarding to current process()");
        self.process(user_request).await
    }

    /// Degradación segura cuando un gate bloquea
    async fn degrade_response(
        &self,
        gate_name: &str,
        gate_result: &crate::gates::GateResult,
        _original: &ResponseKind,
    ) -> OrchestratorResult {
        // Log en ledger
        let _ = self
            .client
            .create_event(crate::client::CreateEventRequest {
                kind: "OBSERVATION".to_string(),
                description: format!("Gate {} blocked response: {:?}", gate_name, gate_result),
                project_id: self.client.project_id().map(str::to_string),
                tags: Some(vec!["gate-blocked".to_string(), gate_name.to_string()]),
                inputs: None,
                outputs: None,
            })
            .await;

        // Devolver NeedMoreContext trazable
        OrchestratorResult::Complete {
            response: ResponseKind::NeedMoreContext {
                what_missing: vec![
                    format!("Gate '{}' requires additional evidence", gate_name),
                    gate_result.reason().unwrap_or("Unknown reason").to_string(),
                ],
                reason: format!("Blocked by {} gate: {:?}", gate_name, gate_result),
                attempts: self.attempts.clone(),
            },
            iterations: self.iteration,
            tokens_used: self.tokens_used,
        }
    }

    /// Obtener eventos de evidencia para verificación de relevancia
    async fn fetch_evidence_events(
        &self,
        response: &ResponseKind,
    ) -> Vec<crate::client::EventSummary> {
        // Solo para VerifiedClaim
        let ResponseKind::VerifiedClaim { evidence, .. } = response else {
            return vec![];
        };

        // Extraer IDs de las citations
        let ids: Vec<String> = evidence
            .iter()
            .map(|c| c.event_id.clone())
            .filter(|id| id != "?" && !id.trim().is_empty())
            .collect();

        if ids.is_empty() {
            return vec![];
        }

        // Fetch eventos (ya validados por exists_gate)
        match self.client.get_events(&ids).await {
            Ok(events) => events,
            Err(e) => {
                tracing::warn!("Failed to fetch evidence events: {}", e);
                vec![]
            }
        }
    }

    /// Ejecutar recall (búsqueda en memoria) usando VCT `/recall`
    /// con `scope` real del backend. Diseñada para subtareas paralelas con cliente clonado.
    async fn execute_recall_with_client(
        client: &QuironClient,
        query: &str,
        scope: &Option<RecallScope>,
    ) -> RecallOutcome {
        let scope_used = scope.clone().unwrap_or(RecallScope::All);
        let limit = match scope {
            Some(RecallScope::Recent) => 5,
            Some(RecallScope::Project(_)) => 10,
            _ => 10,
        };
        let api_scope = match &scope_used {
            RecallScope::Recent => "recent",
            RecallScope::Project(_) => "project",
            RecallScope::All => "all",
        };

        let effective_query = match &scope_used {
            RecallScope::Project(project) if !project.trim().is_empty() => {
                // El endpoint actual no acepta project_id explícito, así que lo incorporamos
                // al query para mantener trazabilidad sin inventar resultados.
                format!("{} {}", query, project)
            }
            _ => query.to_string(),
        };

        match client.recall(&effective_query, limit, api_scope).await {
            Ok(response) => {
                let results_count = response.events.len();
                let relevant_found = !response.events.is_empty();
                let attempt = AttemptLog::new(
                    query.to_string(),
                    scope_used.clone(),
                    results_count,
                    relevant_found,
                );

                if response.events.is_empty() {
                    return RecallOutcome {
                        attempt,
                        citations: vec![],
                    };
                }

                let citations: Vec<Citation> = response
                    .events
                    .iter()
                    .take(3)
                    .map(|ev| Citation {
                        event_id: ev.event_id.clone(),
                        snippet: ev.description.clone(),
                        relevance: format!(
                            "vct:{} score:{:.3} why:{}",
                            response.strategy, ev.score, ev.why_selected
                        ),
                    })
                    .collect();

                RecallOutcome { attempt, citations }
            }
            Err(ClientError::Http { status: 404, .. }) => {
                // Compatibilidad con servidores viejos sin /recall.
                match client.search(query, limit).await {
                    Ok(response) => {
                        let attempt = AttemptLog::new(
                            query.to_string(),
                            scope_used.clone(),
                            response.results.len(),
                            !response.results.is_empty(),
                        );

                        let citations: Vec<Citation> = response
                            .results
                            .iter()
                            .take(3)
                            .filter_map(|r| {
                                Some(Citation {
                                    event_id: r.event_id.clone()?,
                                    snippet: r
                                        .description
                                        .clone()
                                        .unwrap_or_else(|| "No description".to_string()),
                                    relevance: format!("search_score:{:.3}", r.score),
                                })
                            })
                            .collect();

                        RecallOutcome { attempt, citations }
                    }
                    Err(e) => {
                        tracing::warn!("Legacy search failed: {}", e);
                        let attempt =
                            AttemptLog::new(query.to_string(), scope_used.clone(), 0, false);
                        RecallOutcome {
                            attempt,
                            citations: vec![],
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Recall failed: {}", e);
                let attempt = AttemptLog::new(query.to_string(), scope_used.clone(), 0, false);
                RecallOutcome {
                    attempt,
                    citations: vec![],
                }
            }
        }
    }
}

/// Resultado estructurado de una ejecución de recall
#[derive(Debug, Clone)]
struct RecallOutcome {
    attempt: AttemptLog,
    citations: Vec<Citation>,
}

#[derive(Debug, Clone)]
struct DeliberationOutcome {
    response: ResponseKind,
    token_budget_exhausted: bool,
}

/// Resultado del orquestador
#[derive(Debug, Clone)]
pub enum OrchestratorResult {
    /// Completado exitosamente con respuesta tipada
    Complete {
        response: ResponseKind,
        iterations: u32,
        tokens_used: u32,
    },
    /// Necesita input del usuario
    NeedsUserInput {
        question: String,
        options: Option<Vec<String>>,
    },
    /// Límite de iteraciones alcanzado - preguntar al usuario
    AskUserToContinue {
        iterations: u32,
        question: String,
        partial_result: Option<String>,
    },
    /// Budget de tokens agotado
    TokenBudgetExhausted {
        tokens_used: u32,
        partial_result: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.max_iterations, 100); // Subido a 100 para deliberación profunda
        assert_eq!(config.max_parallel_subtasks, 3);
        assert!(config.require_citations);
    }

    #[test]
    fn test_intent_serialization() {
        let intent = Intent::Recall {
            query: "test query".to_string(),
            scope: Some(RecallScope::Recent),
        };
        let json = serde_json::to_string(&intent).unwrap();
        assert!(json.contains("recall"));
    }

    #[test]
    fn test_detects_contradiction_with_overlap_and_opposite_polarity() {
        let mut ws = WorkingSet::new();
        ws.add_evidence(Citation {
            event_id: "evt-1".to_string(),
            snippet: "checkout payment validation passed successfully".to_string(),
            relevance: "test".to_string(),
        });

        ws.check_contradiction(&Citation {
            event_id: "evt-2".to_string(),
            snippet: "checkout payment validation failed with error".to_string(),
            relevance: "test".to_string(),
        });

        assert_eq!(ws.contradictions.len(), 1);
    }

    #[test]
    fn test_no_contradiction_without_context_overlap() {
        let mut ws = WorkingSet::new();
        ws.add_evidence(Citation {
            event_id: "evt-1".to_string(),
            snippet: "database migration passed successfully".to_string(),
            relevance: "test".to_string(),
        });

        ws.check_contradiction(&Citation {
            event_id: "evt-2".to_string(),
            snippet: "payment gateway failed with timeout".to_string(),
            relevance: "test".to_string(),
        });

        assert!(ws.contradictions.is_empty());
    }

    #[test]
    fn test_delegation_metrics_initial_state() {
        let orch = Orchestrator::new(QuironClient::new());
        let m = orch.delegation_metrics();
        assert_eq!(m.local_tasks_total, 0);
        assert_eq!(m.worker_tasks_total, 0);
        assert_eq!(m.primary_calls_total, 0);
        assert_eq!(m.tokens_saved_estimate, 0);
        assert_eq!(m.model_tokens_used, 0);
        assert_eq!(m.token_budget, 100_000);
        assert_eq!(m.token_budget_remaining, 100_000);
        assert!(m.worker_tokens_ewma.is_none());
        assert!(m.primary_tokens_ewma.is_none());
        assert!(m.worker_min_tokens_threshold >= 96);
        assert!(m.primary_min_tokens_threshold >= 512);
        assert!((m.worker_threshold_scale - 1.0).abs() < f32::EPSILON);
        assert!((m.primary_threshold_scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(m.parallel_subtasks_cap, 3);
        assert_eq!(m.parallel_subtasks_current, 2);
        assert!(m.recall_latency_ewma_ms.is_none());
        assert!(m.worker_latency_ewma_ms.is_none());
    }

    #[test]
    fn test_search_planner_next_batch_marks_in_progress() {
        let mut planner = SearchPlanner::plan("investigar error de compilacion en src/main.rs");
        let batch = planner.next_batch(2);
        assert_eq!(batch.len(), 2);
        assert_eq!(planner.in_progress_count(), 2);
    }

    #[test]
    fn test_search_planner_prioritizes_recent_for_recency_queries() {
        let mut planner = SearchPlanner::plan("what happened today in production incidents");
        let first = planner
            .next()
            .expect("must have at least one query")
            .clone();
        assert!(matches!(first.scope, RecallScope::Recent));
        assert!(planner.profile().recency_bias > 0.0);
    }

    #[test]
    fn test_search_planner_rebalances_scope_priority_after_hits() {
        let mut planner = SearchPlanner::plan("hoy error de compilacion en src/main.rs");
        let first = planner
            .next_batch(1)
            .into_iter()
            .next()
            .expect("batch should have one query");
        planner.mark_completed(
            first.id,
            vec![Citation {
                event_id: "evt-1".to_string(),
                snippet: "compilation failed at src/main.rs".to_string(),
                relevance: "test".to_string(),
            }],
        );

        let pending = planner.pending();
        let recent_best = pending
            .iter()
            .filter(|q| matches!(q.scope, RecallScope::Recent))
            .map(|q| q.priority)
            .fold(f32::NEG_INFINITY, f32::max);
        let all_best = pending
            .iter()
            .filter(|q| matches!(q.scope, RecallScope::All))
            .map(|q| q.priority)
            .fold(f32::NEG_INFINITY, f32::max);

        assert!(recent_best.is_finite());
        assert!(all_best.is_finite());
        assert!(recent_best >= all_best - 0.05);
    }

    #[test]
    fn test_synthesize_response_surfaces_llm_failure_when_no_evidence() {
        let orch = Orchestrator::new(QuironClient::new());
        let ws = WorkingSet::new();

        let response = orch.synthesize_response(
            &ws,
            StopReason::QueriesExhausted,
            None,
            Some("Missing Authorization header"),
        );

        match response {
            ResponseKind::NeedMoreContext { reason, .. } => {
                assert!(reason.contains("Missing Authorization header"));
            }
            _ => panic!("expected NeedMoreContext when llm fails with no evidence"),
        }
    }

    #[test]
    fn test_working_set_accumulates_model_tokens_and_context_estimate() {
        let mut ws = WorkingSet::new();
        ws.add_worker_tokens(420);
        ws.add_primary_tokens(580);
        assert_eq!(ws.total_model_tokens_used(), 1000);
        assert_eq!(ws.remaining_model_budget(1400), 400);

        ws.search_history.push(AttemptLog::new(
            "find deployment failure",
            RecallScope::All,
            2,
            true,
        ));
        ws.add_evidence(Citation {
            event_id: "evt-ctx-1".to_string(),
            snippet: "deployment failed in production after migration".to_string(),
            relevance: "matches deployment issue".to_string(),
        });
        ws.add_worker_note(WorkerNote {
            task_id: "worker-1".to_string(),
            query: "deployment failure".to_string(),
            summary: "possible mismatch in migration order".to_string(),
            citations: vec!["evt-ctx-1".to_string()],
            confidence: Some(0.7),
        });
        ws.refresh_context_token_estimate();

        assert!(ws.context_tokens_estimate > 0);
    }

    #[test]
    fn test_route_token_thresholds_follow_ewma() {
        let mut orch = Orchestrator::new(QuironClient::new());
        let base_worker = orch.worker_min_tokens_threshold();
        let base_primary = orch.primary_min_tokens_threshold();

        orch.update_worker_tokens_ewma(900);
        orch.update_worker_tokens_ewma(1100);
        orch.update_primary_tokens_ewma(4200);
        orch.update_primary_tokens_ewma(4600);

        assert!(orch.worker_min_tokens_threshold() > base_worker);
        assert!(orch.primary_min_tokens_threshold() > base_primary);
    }

    #[test]
    fn test_runtime_operator_controls_update_budget_and_scales() {
        let mut orch = Orchestrator::new(QuironClient::new());
        orch.set_token_budget(55_000);
        orch.set_parallel_subtasks_cap(6);
        orch.set_threshold_scales(1.25, 0.85);

        let metrics = orch.delegation_metrics();
        assert_eq!(metrics.token_budget, 55_000);
        assert_eq!(metrics.parallel_subtasks_cap, 6);
        assert!((metrics.worker_threshold_scale - 1.25).abs() < 0.0001);
        assert!((metrics.primary_threshold_scale - 0.85).abs() < 0.0001);
    }

    #[test]
    fn test_adaptive_scheduler_reduces_on_worker_failures() {
        let mut orch = Orchestrator::new(QuironClient::new());
        let ws = WorkingSet::new();
        orch.current_parallel_subtasks = 3;

        let change = orch.adapt_parallel_limit(&ws, 5, 0, 2, 0);
        assert!(change.is_some());
        assert_eq!(orch.current_parallel_subtasks, 2);
    }

    #[test]
    fn test_adaptive_scheduler_increases_on_stability_without_novelty() {
        let mut orch = Orchestrator::new(QuironClient::new());
        let mut ws = WorkingSet::new();
        ws.stability_count = 2;
        orch.current_parallel_subtasks = 1;

        let change = orch.adapt_parallel_limit(&ws, 4, 1, 0, 0);
        assert!(change.is_some());
        assert_eq!(orch.current_parallel_subtasks, 2);
    }

    #[test]
    fn test_adaptive_scheduler_reduces_on_high_latency() {
        let mut orch = Orchestrator::new(QuironClient::new());
        let ws = WorkingSet::new();
        orch.current_parallel_subtasks = 3;
        orch.recall_latency_ewma_ms = Some(1800.0);
        orch.worker_latency_ewma_ms = Some(2400.0);

        let change = orch.adapt_parallel_limit(&ws, 6, 3, 0, 2);
        assert!(change.is_some());
        assert_eq!(orch.current_parallel_subtasks, 2);
    }

    #[test]
    fn test_session_telemetry_rotates_after_idle_window() {
        let mut orch = Orchestrator::new(QuironClient::new());
        let previous = orch.telemetry.session_id.clone();
        orch.telemetry.last_activity_at =
            chrono::Utc::now() - chrono::Duration::seconds(TELEMETRY_SESSION_IDLE_ROTATE_SECS + 3);

        orch.telemetry.begin_request(chrono::Utc::now());

        assert_ne!(orch.telemetry.session_id, previous);
        assert_eq!(orch.telemetry.parent_session_id, Some(previous));
        assert_eq!(orch.telemetry.segment_seq, 0);
    }

    #[test]
    fn test_session_telemetry_budget_anomaly_uses_hysteresis() {
        let mut orch = Orchestrator::new(QuironClient::new());
        orch.set_token_budget(5_000);
        orch.local_tasks_total = 6;
        orch.worker_tasks_total = 2;
        orch.primary_calls_total = 2;

        orch.iteration = 1;
        orch.tokens_used = 4_300; // 14% rem -> budget_low
        let capture_1 = orch.capture_telemetry_step();
        assert!(capture_1
            .anomalies
            .iter()
            .any(|a| a.kind == TelemetryAnomalyKind::BudgetLow));

        orch.iteration = 2;
        let capture_2 = orch.capture_telemetry_step();
        assert!(capture_2
            .anomalies
            .iter()
            .all(|a| a.kind != TelemetryAnomalyKind::BudgetLow));

        // Recuperación estable por dos pasos para limpiar estado activo.
        orch.tokens_used = 100; // 90% rem
        orch.iteration = 3;
        let _ = orch.capture_telemetry_step();
        orch.iteration = 4;
        let _ = orch.capture_telemetry_step();

        // Vuelve a entrar en low budget -> debe emitir anomalía de nuevo.
        orch.tokens_used = 4_400;
        orch.iteration = 5;
        let capture_3 = orch.capture_telemetry_step();
        assert!(capture_3
            .anomalies
            .iter()
            .any(|a| a.kind == TelemetryAnomalyKind::BudgetLow));
    }

    #[test]
    fn test_session_telemetry_checkpoint_interval_and_force() {
        let mut orch = Orchestrator::new(QuironClient::new());
        orch.set_token_budget(80_000);

        let mut checkpoint = None;
        for step in 1..=20 {
            orch.iteration = step as u32;
            orch.tokens_used = step as u32 * 100;
            let capture = orch.capture_telemetry_step();
            if step < 20 {
                assert!(capture.checkpoint.is_none());
            } else {
                checkpoint = capture.checkpoint;
            }
        }

        let first_checkpoint = checkpoint.expect("step 20 must create checkpoint");
        assert_eq!(first_checkpoint.step_start, 1);
        assert_eq!(first_checkpoint.step_end, 20);
        assert_eq!(first_checkpoint.segment_seq, 1);
        assert!(first_checkpoint
            .preset_change
            .as_deref()
            .unwrap_or("")
            .contains("token_budget=80000"));

        orch.iteration = 21;
        orch.tokens_used = 2_123;
        let no_checkpoint = orch.capture_telemetry_step();
        assert!(no_checkpoint.checkpoint.is_none());

        let forced = orch
            .force_telemetry_checkpoint()
            .expect("force should flush trailing segment");
        assert_eq!(forced.step_start, 21);
        assert_eq!(forced.step_end, 21);
        assert_eq!(forced.segment_seq, 2);
    }
}
