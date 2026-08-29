//! # Gates
//!
//! Los gates son garantías que me protegen de mis propias limitaciones.
//!
//! - **no-claim-without-evidence**: No puedo afirmar sin datos verificables
//! - **no-ghost-editing**: No puedo modificar sin haber leído
//! - **auto-revert-on-failure**: Si algo falla, se deshace automáticamente

use crate::client::{ClientError, QuironClient};
use crate::orchestrator::ResponseKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Tipos de gates disponibles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Gate {
    /// No puedo afirmar sin datos verificables.
    NoClaimWithoutEvidence,
    /// No puedo modificar sin haber leído.
    NoGhostEditing,
    /// Si algo falla, se deshace automáticamente.
    AutoRevertOnFailure,
    /// No puedo editar fuera del scope definido.
    ScopeLock,
    /// Cambios requieren verificación rápida.
    NoSilentBreak,
}

impl Gate {
    /// Nombre del gate.
    pub fn name(&self) -> &'static str {
        match self {
            Gate::NoClaimWithoutEvidence => "no-claim-without-evidence",
            Gate::NoGhostEditing => "no-ghost-editing",
            Gate::AutoRevertOnFailure => "auto-revert-on-failure",
            Gate::ScopeLock => "scope-lock",
            Gate::NoSilentBreak => "no-silent-break",
        }
    }

    /// Descripción del gate.
    pub fn description(&self) -> &'static str {
        match self {
            Gate::NoClaimWithoutEvidence => "No puedo afirmar sin datos verificables",
            Gate::NoGhostEditing => "No puedo modificar sin haber leído",
            Gate::AutoRevertOnFailure => "Si algo falla, se deshace automáticamente",
            Gate::ScopeLock => "No puedo editar fuera del scope definido",
            Gate::NoSilentBreak => "Cambios requieren verificación rápida",
        }
    }

    /// Todos los gates
    pub fn all() -> Vec<Gate> {
        vec![
            Gate::NoClaimWithoutEvidence,
            Gate::NoGhostEditing,
            Gate::AutoRevertOnFailure,
            Gate::ScopeLock,
            Gate::NoSilentBreak,
        ]
    }
}

/// Resultado de verificar un gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GateResult {
    /// El gate permite la acción.
    Allowed,
    /// El gate bloquea la acción con un motivo.
    Blocked { gate: String, reason: String },
}

impl GateResult {
    /// Verifica si el gate permite la acción.
    pub fn is_allowed(&self) -> bool {
        matches!(self, GateResult::Allowed)
    }

    /// Crea un resultado permitido.
    pub fn allowed() -> Self {
        GateResult::Allowed
    }

    /// Crea un resultado bloqueado.
    pub fn blocked(gate_name: impl Into<String>, reason: impl Into<String>) -> Self {
        GateResult::Blocked {
            gate: gate_name.into(),
            reason: reason.into(),
        }
    }

    /// Obtener el motivo del bloqueo (None si allowed)
    pub fn reason(&self) -> Option<&str> {
        match self {
            GateResult::Allowed => None,
            GateResult::Blocked { reason, .. } => Some(reason.as_str()),
        }
    }
}

/// Gate de honestidad para ResponseKind.
///
/// Reglas:
/// - `VerifiedClaim` requiere evidencia no vacía y confidence >= 0.7
/// - `NoEvidence`: scope_was_bounded se VERIFICA contra attempts (no es opinión del agente)
/// - confidence_in_absence > 0.5 solo permitido si scope realmente es bounded
/// - `Proposal`, `Hypothesis`, `NeedMoreContext` siempre permitidos
pub fn honesty_gate(response: &crate::orchestrator::ResponseKind) -> GateResult {
    use crate::orchestrator::{RecallScope, ResponseKind};

    match response {
        ResponseKind::VerifiedClaim {
            evidence,
            confidence,
            ..
        } => {
            if evidence.is_empty() {
                return GateResult::blocked(
                    "no-claim-without-evidence",
                    "VerifiedClaim requires at least one Citation",
                );
            }
            if !confidence.is_finite() || *confidence < 0.7 {
                return GateResult::blocked(
                    "low-confidence-claim",
                    format!(
                        "VerifiedClaim requires confidence >= 0.7, got {:.2}",
                        confidence
                    ),
                );
            }
            GateResult::allowed()
        }

        ResponseKind::NoEvidence {
            attempts,
            scope_was_bounded,
            confidence_in_absence,
        } => {
            // DERIVAR bounded de attempts: "bounded" = ningún attempt usó RecallScope::All
            let derived_bounded = attempts
                .iter()
                .all(|a| !matches!(a.scope, RecallScope::All));

            // Si el agente declara bounded pero attempts muestran lo contrario → BLOQUEO
            if *scope_was_bounded && !derived_bounded {
                return GateResult::blocked(
                    "false-bounded-scope",
                    "scope_was_bounded=true but attempts include unbounded scope (RecallScope::All)"
                );
            }

            // Si quiere alta confidence en ausencia, debe ser bounded (derivado, no declarado)
            if let Some(conf) = confidence_in_absence {
                if !conf.is_finite() {
                    return GateResult::blocked(
                        "invalid-confidence",
                        "confidence_in_absence must be finite",
                    );
                }
                if *conf > 0.5 && !derived_bounded {
                    return GateResult::blocked(
                        "unbounded-confidence",
                        "Cannot have confidence_in_absence > 0.5 without bounded scope (derived from attempts)"
                    );
                }
            }

            GateResult::allowed()
        }

        // Proposal, Hypothesis, NeedMoreContext siempre permitidos
        ResponseKind::Proposal { .. } => GateResult::allowed(),
        ResponseKind::Hypothesis { .. } => GateResult::allowed(),
        ResponseKind::NeedMoreContext { .. } => GateResult::allowed(),
    }
}

// ============================================================================
// EVIDENCE INTEGRITY GATES
// ============================================================================

/// Gate 2: Verifica que todos los event_ids en las citations existen (async, IO acotado).
/// Bloquea si hay evidencia fantasma.
pub async fn evidence_exists_gate(response: &ResponseKind, client: &QuironClient) -> GateResult {
    let ResponseKind::VerifiedClaim { evidence, .. } = response else {
        return GateResult::allowed();
    };

    // Extraer IDs únicos válidos
    let mut ids: Vec<String> = evidence
        .iter()
        .map(|c| c.event_id.clone())
        .filter(|id| id != "?" && !id.trim().is_empty())
        .collect();
    ids.sort();
    ids.dedup();

    if ids.is_empty() {
        // honesty_gate ya debería bloquear esto, pero por seguridad:
        return GateResult::blocked("phantom-evidence", "No valid event_ids in evidence");
    }

    // Verificar existencia uno a uno (early-exit en fallo)
    for id in ids {
        match client.get_event(&id).await {
            Ok(Some(_event)) => {}
            Ok(None) => {
                return GateResult::blocked(
                    "phantom-evidence",
                    format!("Citation refers to non-existent event_id={}", id),
                );
            }
            Err(e) => {
                // Fallo de red NO es allowed - mejor bloquear y degradar
                return GateResult::blocked(
                    "evidence-lookup-failed",
                    format!("Failed to verify event_id={} error={}", id, e),
                );
            }
        }
    }

    GateResult::allowed()
}

/// Gate 3: Relevancia heurística (sync, sin LLM).
/// Bloquea si la evidencia existe pero no parece relevante al claim.
pub fn evidence_relevance_gate(
    response: &ResponseKind,
    events: &[crate::client::EventSummary],
) -> GateResult {
    let ResponseKind::VerifiedClaim {
        content, evidence, ..
    } = response
    else {
        return GateResult::allowed();
    };

    if evidence.is_empty() {
        return GateResult::blocked("irrelevant-evidence", "No evidence provided");
    }

    let claim_tokens = tokenize_significant(content);
    let claim_paths = extract_paths(content);
    let claim_symbols = extract_symbols(content);

    let mut any_relevant = false;
    let mut best_score = 0.0f32;

    for cit in evidence {
        // Buscar evento correspondiente
        let ev = events.iter().find(|e| e.id == cit.event_id);
        let ev_text = match ev {
            Some(e) => format!("{} {}", e.description, cit.snippet),
            None => cit.snippet.clone(),
        };

        let ev_tokens = tokenize_significant(&ev_text);
        let tok_score = jaccard(&claim_tokens, &ev_tokens);

        let ev_paths = extract_paths(&ev_text);
        let path_score = overlap_ratio(&claim_paths, &ev_paths);

        let ev_symbols = extract_symbols(&ev_text);
        let sym_score = overlap_ratio(&claim_symbols, &ev_symbols);

        // Score combinado (60% tokens, 25% paths, 15% symbols)
        let score = 0.60 * tok_score + 0.25 * path_score + 0.15 * sym_score;
        if score > best_score {
            best_score = score;
        }

        // Umbral flexible: >=2 tokens compartidos OR score >= 0.08 OR path/sym overlap >= 0.5
        let shared_tokens = intersection_count(&claim_tokens, &ev_tokens);
        if score >= 0.08 || shared_tokens >= 2 || path_score >= 0.5 || sym_score >= 0.5 {
            any_relevant = true;
            break;
        }
    }

    if !any_relevant {
        return GateResult::blocked(
            "irrelevant-evidence",
            format!(
                "No citation overlaps with claim (best_score={:.3})",
                best_score
            ),
        );
    }

    GateResult::allowed()
}

// ============================================================================
// HELPERS HEURÍSTICOS (deterministas, sin LLM)
// ============================================================================

/// Tokeniza texto en tokens significativos (>=5 chars, sin stopwords)
fn tokenize_significant(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
    {
        let w = raw.trim();
        if w.len() < 5 {
            continue;
        }
        if is_stopword(w) {
            continue;
        }
        out.push(w.to_string());
    }
    out.sort();
    out.dedup();
    out
}

fn is_stopword(w: &str) -> bool {
    matches!(
        w,
        "sobre"
            | "desde"
            | "hacia"
            | "porque"
            | "cuando"
            | "donde"
            | "todas"
            | "todos"
            | "hacer"
            | "puede"
            | "deber"
            | "tiene"
            | "through"
            | "about"
            | "would"
            | "should"
            | "could"
            | "being"
            | "their"
            | "there"
            | "which"
            | "after"
            | "before"
    )
}

/// Jaccard similarity entre dos listas sorted+dedup
fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = intersection_count(a, b) as f32;
    let union = (a.len() + b.len()) as f32 - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn intersection_count(a: &[String], b: &[String]) -> usize {
    // a y b están sorted+dedup
    let mut i = 0usize;
    let mut j = 0usize;
    let mut c = 0usize;
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                c += 1;
                i += 1;
                j += 1;
            }
        }
    }
    c
}

fn overlap_ratio(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = intersection_count(a, b) as f32;
    inter / (a.len().min(b.len()) as f32)
}

/// Extrae tokens que parecen paths de archivo
fn extract_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        let t =
            tok.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ')' || c == '(');
        let is_pathish = (t.contains('/') || t.contains('\\'))
            && (t.ends_with(".rs")
                || t.ends_with(".py")
                || t.ends_with(".ts")
                || t.ends_with(".go")
                || t.ends_with(".js")
                || t.ends_with(".md"));
        if is_pathish {
            out.push(t.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Extrae tokens que parecen símbolos (snake_case o CamelCase)
fn extract_symbols(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let t = tok.trim();
        if t.len() < 4 {
            continue;
        }
        let snake = t.contains('_');
        let camel = t.chars().any(|c| c.is_uppercase()) && t.chars().any(|c| c.is_lowercase());
        if snake || camel {
            out.push(t.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

pub struct GateValidator {
    client: QuironClient,
    /// Archivos leídos en esta sesión (para no-ghost-editing)
    files_read: HashSet<String>,
    /// Timestamps de lecturas
    file_read_times: std::collections::HashMap<String, std::time::Instant>,
    /// Scope actual permitido
    allowed_scope: Vec<String>,
    /// Evidence acumulada
    evidence: Vec<Evidence>,
}

/// Evidencia para claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,
    pub description: String,
    pub timestamp: String,
    pub data: serde_json::Value,
}

/// Solicitud de acción a validar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub action: String,
    pub target: Option<String>,
    pub project_id: Option<String>,
    pub context: serde_json::Value,
}

/// Resultado de validación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub allowed: bool,
    pub reason: Option<String>,
    pub blocking_gates: Vec<String>,
    pub warnings: Vec<String>,
}

impl GateValidator {
    /// Crear nuevo validador
    pub fn new(client: QuironClient) -> Self {
        Self {
            client,
            files_read: HashSet::new(),
            file_read_times: std::collections::HashMap::new(),
            allowed_scope: Vec::new(),
            evidence: Vec::new(),
        }
    }

    /// Registrar lectura de archivo (para no-ghost-editing)
    pub fn register_file_read(&mut self, path: &str) {
        self.files_read.insert(path.to_string());
        self.file_read_times
            .insert(path.to_string(), std::time::Instant::now());
    }

    /// Verificar si un archivo fue leído
    pub fn was_file_read(&self, path: &str) -> bool {
        self.files_read.contains(path)
    }

    /// Obtener edad de la lectura en segundos
    pub fn file_read_age(&self, path: &str) -> Option<u64> {
        self.file_read_times
            .get(path)
            .map(|t| t.elapsed().as_secs())
    }

    /// Definir scope permitido (para scope-lock)
    pub fn set_scope(&mut self, paths: Vec<String>) {
        self.allowed_scope = paths;
    }

    /// Añadir evidencia (para no-claim-without-evidence)
    pub fn add_evidence(&mut self, kind: &str, description: &str, data: serde_json::Value) {
        self.evidence.push(Evidence {
            kind: kind.to_string(),
            description: description.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data,
        });
    }

    /// Validar acción localmente (rápido, sin HTTP)
    pub fn validate_local(&self, action: &str, target: Option<&str>) -> ValidationResult {
        let mut result = ValidationResult {
            allowed: true,
            reason: None,
            blocking_gates: Vec::new(),
            warnings: Vec::new(),
        };

        // Gate: no-ghost-editing
        if action == "write" || action == "patch" || action == "edit" {
            if let Some(path) = target {
                if !self.was_file_read(path) {
                    result.allowed = false;
                    result.reason = Some(format!(
                        "Cannot {} '{}' without reading it first (no-ghost-editing)",
                        action, path
                    ));
                    result.blocking_gates.push("no-ghost-editing".to_string());
                    return result;
                }

                // Check age
                if let Some(age) = self.file_read_age(path) {
                    if age > 3600 {
                        result.warnings.push(format!(
                            "File '{}' was read {}s ago (stale context)",
                            path, age
                        ));
                    }
                }
            }
        }

        // Gate: scope-lock
        if (action == "write" || action == "patch" || action == "delete")
            && !self.allowed_scope.is_empty()
        {
            if let Some(path) = target {
                let in_scope = self.allowed_scope.iter().any(|s| {
                    if s.ends_with("/*") {
                        path.starts_with(s.trim_end_matches("/*"))
                    } else if s.ends_with("/**") {
                        path.starts_with(s.trim_end_matches("/**"))
                    } else {
                        path == s || path.starts_with(s)
                    }
                });

                if !in_scope {
                    result.allowed = false;
                    result.reason = Some(format!(
                        "Path '{}' is outside allowed scope: {:?}",
                        path, self.allowed_scope
                    ));
                    result.blocking_gates.push("scope-lock".to_string());
                    return result;
                }
            }
        }

        result
    }

    /// Validar acción via quiron-brain API
    pub async fn validate_remote(
        &self,
        action: &str,
        target: Option<&str>,
    ) -> Result<ValidationResult, ClientError> {
        // Build files_read context
        let files_read: Vec<serde_json::Value> = self
            .files_read
            .iter()
            .map(|path| {
                let age = self.file_read_age(path).unwrap_or(0);
                serde_json::json!({
                    "path": path,
                    "age_seconds": age
                })
            })
            .collect();

        // Build evidence context
        let evidence: Vec<serde_json::Value> = self
            .evidence
            .iter()
            .map(|e| {
                serde_json::json!({
                    "kind": e.kind,
                    "description": e.description,
                    "data": e.data
                })
            })
            .collect();

        let request = crate::client::ActionRequest {
            action: action.to_string(),
            target: target.map(String::from),
            project_id: self.client.project_id().map(str::to_string),
            agent_id: None,
            context: serde_json::json!({
                "files_read": files_read,
                "evidence": evidence,
                "scope": self.allowed_scope
            }),
        };

        let response = self.client.validate_action(request).await?;

        Ok(ValidationResult {
            allowed: response.allowed,
            reason: response.reason,
            blocking_gates: response.blocking_invariants,
            warnings: response.warnings,
        })
    }

    /// Validar con fallback (intenta remote, fallback a local)
    pub async fn validate(&self, action: &str, target: Option<&str>) -> ValidationResult {
        // First check locally (fast)
        let local = self.validate_local(action, target);
        if !local.allowed {
            return local;
        }

        // Then check remotely (comprehensive)
        match self.validate_remote(action, target).await {
            Ok(remote) => remote,
            Err(e) => {
                tracing::warn!("Remote validation failed, using local only: {}", e);
                local
            }
        }
    }

    /// Limpiar estado para nueva sesión
    pub fn clear(&mut self) {
        self.files_read.clear();
        self.file_read_times.clear();
        self.allowed_scope.clear();
        self.evidence.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_names() {
        assert_eq!(
            Gate::NoClaimWithoutEvidence.name(),
            "no-claim-without-evidence"
        );
        assert_eq!(Gate::NoGhostEditing.name(), "no-ghost-editing");
        assert_eq!(Gate::ScopeLock.name(), "scope-lock");
    }

    #[test]
    fn test_gate_result() {
        let allowed = GateResult::Allowed;
        assert!(allowed.is_allowed());

        let blocked = GateResult::blocked("no-ghost-editing", "No leíste el archivo");
        assert!(!blocked.is_allowed());
    }

    #[test]
    fn test_validator_ghost_editing() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Try to write without reading - should be blocked
        let result = validator.validate_local("write", Some("src/main.rs"));
        assert!(!result.allowed);
        assert!(result
            .blocking_gates
            .contains(&"no-ghost-editing".to_string()));

        // Register read and try again - should be allowed
        validator.register_file_read("src/main.rs");
        let result = validator.validate_local("write", Some("src/main.rs"));
        assert!(result.allowed);
    }

    #[test]
    fn test_validator_scope_lock() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Set scope
        validator.set_scope(vec!["src/*".to_string()]);
        validator.register_file_read("Cargo.toml");

        // Try to write outside scope - should be blocked
        let result = validator.validate_local("write", Some("Cargo.toml"));
        assert!(!result.allowed);
        assert!(result.blocking_gates.contains(&"scope-lock".to_string()));

        // Try to write inside scope - should be allowed
        validator.register_file_read("src/lib.rs");
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(result.allowed);
    }

    #[test]
    fn test_honesty_gate_verified_claim_without_evidence() {
        use crate::orchestrator::ResponseKind;

        // VerifiedClaim sin evidencia → bloqueado
        let response = ResponseKind::VerifiedClaim {
            content: "Some claim".to_string(),
            evidence: vec![], // Sin evidencia!
            confidence: 0.9,
        };

        let result = super::honesty_gate(&response);
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_verified_claim_low_confidence() {
        use crate::orchestrator::{Citation, ResponseKind};

        // VerifiedClaim con evidencia pero baja confianza → bloqueado
        let response = ResponseKind::VerifiedClaim {
            content: "Some claim".to_string(),
            evidence: vec![Citation {
                event_id: "evt_123".to_string(),
                snippet: "evidence".to_string(),
                relevance: "supports claim".to_string(),
            }],
            confidence: 0.5, // < 0.7 → bloqueado
        };

        let result = super::honesty_gate(&response);
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_verified_claim_valid() {
        use crate::orchestrator::{Citation, ResponseKind};

        // VerifiedClaim con evidencia y alta confianza → permitido
        let response = ResponseKind::VerifiedClaim {
            content: "Valid claim".to_string(),
            evidence: vec![Citation {
                event_id: "evt_123".to_string(),
                snippet: "evidence".to_string(),
                relevance: "supports claim".to_string(),
            }],
            confidence: 0.85,
        };

        let result = super::honesty_gate(&response);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_no_evidence_unbounded_confidence() {
        use crate::orchestrator::{AttemptLog, RecallScope, ResponseKind};

        // NoEvidence con alta confidence pero scope no acotado → bloqueado
        let response = ResponseKind::NoEvidence {
            attempts: vec![AttemptLog::new("query", RecallScope::All, 0, false)],
            scope_was_bounded: false,         // No acotado
            confidence_in_absence: Some(0.8), // Alta → bloqueado
        };

        let result = super::honesty_gate(&response);
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_no_evidence_bounded_confidence() {
        use crate::orchestrator::{AttemptLog, RecallScope, ResponseKind};

        // NoEvidence con alta confidence Y scope acotado → permitido
        let response = ResponseKind::NoEvidence {
            attempts: vec![AttemptLog::new("query", RecallScope::Recent, 0, false)],
            scope_was_bounded: true,          // Acotado
            confidence_in_absence: Some(0.8), // Alta pero permitido
        };

        let result = super::honesty_gate(&response);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_proposal_always_allowed() {
        use crate::orchestrator::ResponseKind;

        // Proposal siempre permitido (sin requerir evidencia)
        let response = ResponseKind::Proposal {
            content: "Let's do X".to_string(),
            rationale: "Because Y".to_string(),
            citations: vec![], // Sin citations pero OK
        };

        let result = super::honesty_gate(&response);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_need_more_context_always_allowed() {
        use crate::orchestrator::{AttemptLog, RecallScope, ResponseKind};

        // NeedMoreContext siempre permitido
        let response = ResponseKind::NeedMoreContext {
            what_missing: vec!["file X".to_string()],
            reason: "Cannot answer without reading file X".to_string(),
            attempts: vec![AttemptLog::new("file X", RecallScope::All, 0, false)],
        };

        let result = super::honesty_gate(&response);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_honesty_gate_false_bounded_scope_blocked() {
        use crate::orchestrator::{AttemptLog, RecallScope, ResponseKind};

        // Agente declara bounded=true pero attempts incluyen RecallScope::All → BLOQUEADO
        let response = ResponseKind::NoEvidence {
            attempts: vec![
                AttemptLog::new("query1", RecallScope::Recent, 5, false),
                AttemptLog::new("query2", RecallScope::All, 10, false), // Unbounded!
            ],
            scope_was_bounded: true, // Agente miente
            confidence_in_absence: Some(0.3),
        };

        let result = super::honesty_gate(&response);
        assert!(!result.is_allowed());
        if let super::GateResult::Blocked { gate, .. } = result {
            assert_eq!(gate, "false-bounded-scope");
        } else {
            panic!("Expected Blocked result");
        }
    }

    // =======================================================================
    // EVIDENCE RELEVANCE GATE TESTS
    // =======================================================================

    #[test]
    fn test_relevance_gate_relevant_evidence_allowed() {
        use crate::client::EventSummary;
        use crate::orchestrator::{Citation, ResponseKind};

        // Claim sobre ResponseKind con evidencia que menciona ResponseKind
        let response = ResponseKind::VerifiedClaim {
            content: "The ResponseKind enum was modified to include VerifiedClaim".to_string(),
            confidence: 0.85,
            evidence: vec![Citation {
                event_id: "evt-123".to_string(),
                snippet: "Changed ResponseKind to add VerifiedClaim variant".to_string(),
                relevance: "semantic_score:0.9".to_string(),
            }],
        };

        let events = vec![EventSummary {
            id: "evt-123".to_string(),
            kind: "OBSERVATION".to_string(),
            description: "Modified ResponseKind enum in orchestrator.rs".to_string(),
        }];

        let result = super::evidence_relevance_gate(&response, &events);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_relevance_gate_irrelevant_evidence_blocked() {
        use crate::client::EventSummary;
        use crate::orchestrator::{Citation, ResponseKind};

        // Claim sobre ResponseKind con evidencia sobre algo TOTALMENTE distinto
        let response = ResponseKind::VerifiedClaim {
            content: "The ResponseKind enum was modified to include VerifiedClaim".to_string(),
            confidence: 0.85,
            evidence: vec![Citation {
                event_id: "evt-456".to_string(),
                snippet: "Updated database migration for users table".to_string(),
                relevance: "semantic_score:0.9".to_string(), // Score alto pero irrelevante
            }],
        };

        let events = vec![EventSummary {
            id: "evt-456".to_string(),
            kind: "ACTION".to_string(),
            description: "Database schema change for authentication module".to_string(),
        }];

        let result = super::evidence_relevance_gate(&response, &events);
        assert!(!result.is_allowed());
        if let super::GateResult::Blocked { gate, .. } = result {
            assert_eq!(gate, "irrelevant-evidence");
        }
    }

    #[test]
    fn test_relevance_gate_path_overlap_allowed() {
        use crate::client::EventSummary;
        use crate::orchestrator::{Citation, ResponseKind};

        // Claim sobre archivo específico con evidencia que menciona el mismo archivo
        let response = ResponseKind::VerifiedClaim {
            content: "Modified src/gates.rs to add new validation".to_string(),
            confidence: 0.9,
            evidence: vec![Citation {
                event_id: "evt-789".to_string(),
                snippet: "Changes in src/gates.rs".to_string(),
                relevance: "semantic_score:0.6".to_string(),
            }],
        };

        let events = vec![EventSummary {
            id: "evt-789".to_string(),
            kind: "ACTION".to_string(),
            description: "Edited src/gates.rs for improved checks".to_string(),
        }];

        let result = super::evidence_relevance_gate(&response, &events);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_relevance_gate_symbol_overlap_allowed() {
        use crate::client::EventSummary;
        use crate::orchestrator::{Citation, ResponseKind};

        // Claim sobre función específica con evidencia que menciona la función
        let response = ResponseKind::VerifiedClaim {
            content: "The honesty_gate function now validates evidence".to_string(),
            confidence: 0.88,
            evidence: vec![Citation {
                event_id: "evt-abc".to_string(),
                snippet: "Added evidence check to honesty_gate".to_string(),
                relevance: "semantic_score:0.7".to_string(),
            }],
        };

        let events = vec![EventSummary {
            id: "evt-abc".to_string(),
            kind: "ACTION".to_string(),
            description: "Updated honesty_gate function signature".to_string(),
        }];

        let result = super::evidence_relevance_gate(&response, &events);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_tokenize_helpers() {
        // Test básico de helpers
        let tokens = super::tokenize_significant("Hello World ResponseKind enum modified");
        assert!(tokens.contains(&"responsekind".to_string()));
        assert!(tokens.contains(&"modified".to_string()));
        // hello y world tienen 5 chars, pero world se incluye (>= 5)
        // Verificamos que enum no se incluye (4 chars < 5)
        assert!(!tokens.contains(&"enum".to_string())); // < 5 chars

        let paths = super::extract_paths("Changed file src/gates.rs and src/lib.rs today");
        assert!(paths.contains(&"src/gates.rs".to_string()));
        assert!(paths.contains(&"src/lib.rs".to_string()));

        let symbols = super::extract_symbols("Updated honesty_gate and GateResult structs");
        assert!(symbols.contains(&"honesty_gate".to_string()));
        assert!(symbols.contains(&"GateResult".to_string()));
    }
}
