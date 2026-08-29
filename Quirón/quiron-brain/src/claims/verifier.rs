//! Verificador de claims

use std::path::PathBuf;

use super::evidence::{BacklinkStatus, Evidence, EvidenceSource};
use super::*;

pub struct ClaimVerifier {
    policy: VerificationPolicy,
    // Nota: En producción, estos serían los retrievers reales
    // retriever: Arc<crate::retrieval::EnsembleRetriever>,
    // ledger: Arc<crate::ledger::Ledger>,
}

/// Contexto para verificación
#[derive(Debug, Clone)]
pub struct VerificationContext {
    pub conversation_id: String,
    pub open_files: Vec<PathBuf>,
    pub current_file: Option<PathBuf>,
    pub available_evidence: Vec<Evidence>,
}

impl ClaimVerifier {
    pub fn new(policy: VerificationPolicy) -> Self {
        Self { policy }
    }

    /// Verificar una afirmación y categorizarla
    pub async fn verify(&self, claim_text: &str, context: &VerificationContext) -> ClaimType {
        // 1. Intentar verificación directa
        if let Some(verified) = self.try_direct_verification(claim_text, context).await {
            return ClaimType::Verified(verified);
        }

        // 2. Buscar en memoria/ledger
        if let Some(verified) = self.try_memory_verification(claim_text, context).await {
            return ClaimType::Verified(verified);
        }

        // 3. Buscar evidencia indirecta para hipótesis
        if let Some(hypothesis) = self.try_build_hypothesis(claim_text, context).await {
            return ClaimType::Hypothesis(hypothesis);
        }

        // 4. Si no hay nada, necesitamos más contexto
        self.build_needs_context(claim_text, context).await
    }

    async fn try_direct_verification(
        &self,
        claim_text: &str,
        context: &VerificationContext,
    ) -> Option<VerifiedClaim> {
        // Buscar evidencia fuerte en el contexto disponible
        let mut evidence = Vec::new();
        let mut verification_methods = Vec::new();

        for ev in &context.available_evidence {
            if self.evidence_supports_claim(ev, claim_text) {
                // Verificar backlink si la política lo requiere
                let mut ev_clone = ev.clone();

                if self.policy.require_valid_backlinks {
                    // Verificar el backlink
                    match ev_clone.verify_backlink().await {
                        Ok(true) => {
                            // Backlink válido, añadir evidencia y método
                            let method = self.derive_verification_method(&ev_clone.source);
                            if !verification_methods.contains(&method) {
                                verification_methods.push(method);
                            }
                            evidence.push(ev_clone);
                        }
                        Ok(false) => {
                            // Backlink inválido, descartar evidencia
                            tracing::debug!(
                                "Backlink inválido descartado: {:?}",
                                ev_clone.backlink.uri
                            );
                            continue;
                        }
                        Err(e) => {
                            // Error verificando, descartar para ser conservador
                            tracing::warn!("Error verificando backlink: {}", e);
                            continue;
                        }
                    }
                } else {
                    // No requiere validación de backlinks
                    let method = self.derive_verification_method(&ev_clone.source);
                    if !verification_methods.contains(&method) {
                        verification_methods.push(method);
                    }
                    evidence.push(ev_clone);
                }
            }
        }

        if evidence.is_empty() || verification_methods.is_empty() {
            return None;
        }

        // Calcular confianza basada en evidencia
        let confidence = self.calculate_confidence(&evidence);

        if confidence >= self.policy.min_confidence_for_verified {
            // Usar el constructor que valida (devuelve Result)
            VerifiedClaim::new(
                claim_text.to_string(),
                evidence,
                confidence,
                verification_methods,
                ClaimOrigin::from_conversation(context.conversation_id.clone()),
            )
            .ok() // Convierte Result a Option, None si verification_methods estaba vacío
        } else {
            None
        }
    }

    async fn try_memory_verification(
        &self,
        _claim_text: &str,
        _context: &VerificationContext,
    ) -> Option<VerifiedClaim> {
        // TODO: Integrar con crate::ledger para buscar eventos relacionados
        // Ejemplo de búsqueda:
        // let query = format!("claim OR fact OR decision: {}", claim_text);
        // let events = self.ledger.search_events(&query, 10).await.ok()?;
        //
        // Por ahora retornamos None - la verificación se hace solo con
        // evidence disponible en el contexto
        None
    }

    async fn try_build_hypothesis(
        &self,
        claim_text: &str,
        context: &VerificationContext,
    ) -> Option<HypothesisClaim> {
        // Buscar evidencia parcial
        let mut partial_evidence = Vec::new();

        for ev in &context.available_evidence {
            if self.evidence_partially_supports(ev, claim_text) {
                // Verificar backlink si la política lo requiere
                let mut ev_clone = ev.clone();

                if self.policy.require_valid_backlinks {
                    match ev_clone.verify_backlink().await {
                        Ok(true) => partial_evidence.push(ev_clone),
                        _ => continue, // Descartar evidencia con backlinks inválidos
                    }
                } else {
                    partial_evidence.push(ev_clone);
                }
            }
        }

        if partial_evidence.is_empty() {
            return None;
        }

        // Calcular confianza real basada en evidencia parcial (penalizada)
        let base_confidence = self.calculate_confidence(&partial_evidence);
        let hypothesis_confidence = (base_confidence * 0.8).min(1.0); // Penalización por ser hipótesis

        // Solo crear hipótesis si supera el umbral mínimo
        if hypothesis_confidence < self.policy.min_confidence_for_hypothesis {
            return None; // Cae a NeedsContext
        }

        Some(HypothesisClaim::new(
            claim_text.to_string(),
            "Basado en evidencia parcial encontrada".to_string(),
            hypothesis_confidence,
        ))
        .map(|mut h| {
            h.partial_evidence = partial_evidence;
            h.needs_verification = vec![VerificationNeed {
                description: "Verificar con más contexto".to_string(),
                how_to_get: "Buscar archivos relacionados".to_string(),
                priority: Priority::Medium,
            }];
            h.risks_if_wrong = vec!["Podría dar información incorrecta".to_string()];
            h
        })
    }

    async fn build_needs_context(
        &self,
        claim_text: &str,
        _context: &VerificationContext,
    ) -> ClaimType {
        ClaimType::NeedsContext(NeedsContextClaim {
            partial_text: claim_text.to_string(),
            missing: vec![MissingInfo {
                description: "Contexto de código relevante".to_string(),
                why_needed: "Para verificar la afirmación".to_string(),
                where_to_look: vec!["Archivos del proyecto".to_string()],
            }],
            suggested_queries: vec![SuggestedQuery {
                query: claim_text.to_string(),
                search_type: SearchType::SemanticSearch,
                expected_result: "Código relacionado con la afirmación".to_string(),
            }],
            can_assert: None,
        })
    }

    /// Derivar el método de verificación del tipo de evidencia
    fn derive_verification_method(&self, source: &EvidenceSource) -> VerificationMethod {
        match source {
            EvidenceSource::CodeFile { .. } => VerificationMethod::CodeSearch,
            EvidenceSource::LedgerEvent { .. } => VerificationMethod::LedgerSearch,
            EvidenceSource::TestResult { .. } => VerificationMethod::TestExecution,
            EvidenceSource::AstAnalysis { .. } => VerificationMethod::AstAnalysis,
            EvidenceSource::UserStatement { .. } => VerificationMethod::UserConfirmation,
            // Documentation y otros: tratamos como búsqueda de código por defecto
            EvidenceSource::Documentation { .. } => VerificationMethod::CodeSearch,
            EvidenceSource::SystemState { .. } => VerificationMethod::CodeSearch,
            EvidenceSource::CommandOutput { .. } => VerificationMethod::TestExecution,
        }
    }

    fn evidence_supports_claim(&self, evidence: &Evidence, claim_text: &str) -> bool {
        // Tokenizar ambos textos para matching exacto (evita "is" → "this")
        let claim_lower = claim_text.to_lowercase();
        let claim_tokens: std::collections::HashSet<String> = claim_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|s| s.to_string())
            .collect();

        let excerpt_lower = evidence.current_excerpt.to_lowercase();
        let excerpt_tokens: std::collections::HashSet<String> = excerpt_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|s| s.to_string())
            .collect();

        // Contar matches exactos de tokens
        let matching = claim_tokens.intersection(&excerpt_tokens).count();

        // Si más del 50% de los tokens del claim están en el excerpt
        // Y el backlink no está marcado como roto
        let ratio = matching as f32 / claim_tokens.len().max(1) as f32;
        ratio > 0.5 && evidence.backlink.status != BacklinkStatus::Broken
    }

    fn evidence_partially_supports(&self, evidence: &Evidence, claim_text: &str) -> bool {
        // Tokenizar para matching exacto
        let claim_lower = claim_text.to_lowercase();
        let claim_tokens: std::collections::HashSet<String> = claim_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|s| s.to_string())
            .collect();

        let excerpt_lower = evidence.current_excerpt.to_lowercase();
        let excerpt_tokens: std::collections::HashSet<String> = excerpt_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|s| s.to_string())
            .collect();

        let matching = claim_tokens.intersection(&excerpt_tokens).count();

        // Entre 25% y 50%, y backlink no está roto
        let ratio = matching as f32 / claim_tokens.len().max(1) as f32;
        ratio > 0.25 && ratio <= 0.5 && evidence.backlink.status != BacklinkStatus::Broken
    }

    fn calculate_confidence(&self, evidence: &[Evidence]) -> f32 {
        if evidence.is_empty() {
            return 0.0;
        }

        // Promedio ponderado por strength y status del backlink
        let total_strength: f32 = evidence
            .iter()
            .map(|e| {
                let base = e.strength.to_confidence_modifier() * e.relevance;
                // Penalizar evidencia con backlinks no verificados o stale
                match e.backlink.status {
                    BacklinkStatus::Valid => base,
                    BacklinkStatus::Unverified => base * 0.8,
                    BacklinkStatus::Stale => base * 0.5,
                    BacklinkStatus::Broken => 0.0, // No debería llegar aquí
                }
            })
            .sum();

        let count = evidence.len() as f32;

        (total_strength / count).min(1.0)
    }
}
