//! Anotador de respuestas con claims

use super::*;

/// Anotador que marca respuestas con claims tipados
pub struct ClaimAnnotator {
    verifier: ClaimVerifier,
}

/// Respuesta anotada con claims
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnnotatedResponse {
    /// Texto original de la respuesta
    pub original_text: String,

    /// Claims extraídos y verificados
    pub claims: Vec<AnnotatedClaim>,

    /// Confianza general de la respuesta
    pub overall_confidence: f32,

    /// Warnings si hay claims sin verificar
    pub warnings: Vec<String>,
}

/// Claim individual anotado
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnnotatedClaim {
    /// Posición en el texto original (start, end)
    pub span: (usize, usize),

    /// Texto del claim
    pub text: String,

    /// Tipo de claim con evidencia
    pub claim_type: ClaimType,

    /// Citación inline para mostrar al usuario
    pub inline_citation: Option<String>,
}

impl ClaimAnnotator {
    /// Factor de penalización para hipótesis no verificadas (0.0 = ignorar, 1.0 = sin penalización)
    /// TODO: Mover a VerificationPolicy cuando se refactorice la configuración
    const HYPOTHESIS_PENALTY: f32 = 0.5;

    pub fn new(verifier: ClaimVerifier) -> Self {
        Self { verifier }
    }

    /// Anotar una respuesta con claims verificados
    pub async fn annotate(
        &self,
        response: &str,
        context: &verifier::VerificationContext,
    ) -> AnnotatedResponse {
        // 1. Extraer claims del texto
        let raw_claims = self.extract_claims(response);

        // 2. Verificar cada claim
        let mut annotated_claims = Vec::new();
        let mut warnings = Vec::new();

        for (span, text) in raw_claims {
            let claim_type = self.verifier.verify(&text, context).await;

            // Generar citación inline si hay evidencia
            let inline_citation = self.generate_inline_citation(&claim_type);

            // Añadir warning si es hipótesis o necesita contexto
            match &claim_type {
                ClaimType::Hypothesis(_) => {
                    warnings.push(format!(
                        "⚠️ Hipótesis sin verificar: '{}'",
                        text.chars().take(50).collect::<String>()
                    ));
                }
                ClaimType::NeedsContext(_) => {
                    warnings.push(format!(
                        "❓ Necesita más contexto: '{}'",
                        text.chars().take(50).collect::<String>()
                    ));
                }
                _ => {}
            }

            annotated_claims.push(AnnotatedClaim {
                span,
                text,
                claim_type,
                inline_citation,
            });
        }

        // 3. Calcular confianza general
        let overall_confidence = self.calculate_overall_confidence(&annotated_claims);

        AnnotatedResponse {
            original_text: response.to_string(),
            claims: annotated_claims,
            overall_confidence,
            warnings,
        }
    }

    /// Extraer claims del texto (simplificado)
    /// Nota: Los spans son byte offsets, no char offsets (compatible con UTF-8)
    fn extract_claims(&self, text: &str) -> Vec<((usize, usize), String)> {
        let mut claims = Vec::new();

        let delimiters = ['.', '!', '?'];
        let mut current_start: usize = 0;

        for (idx, ch) in text.char_indices() {
            if delimiters.contains(&ch) {
                let sentence_end = idx + ch.len_utf8();
                // Incluimos el delimitador para saber si era pregunta
                let sentence = &text[current_start..sentence_end];
                let trimmed_sentence = sentence.trim();

                // Extraemos el texto claim sin el delimitador final
                let claim_text = trimmed_sentence
                    .trim_end_matches(|c: char| c == '.' || c == '!' || c == '?')
                    .trim();

                if !claim_text.is_empty() && self.looks_like_claim(claim_text, Some(ch)) {
                    // Calcular byte offsets del claim_text
                    let rel = match sentence.find(claim_text) {
                        Some(r) => r,
                        None => {
                            tracing::warn!(
                                "extract_claims: find() retornó None para '{}'",
                                &claim_text[..claim_text.len().min(50)]
                            );
                            0
                        }
                    };
                    let claim_start = current_start + rel;
                    let claim_end = claim_start + claim_text.len();
                    claims.push(((claim_start, claim_end), claim_text.to_string()));
                }

                current_start = sentence_end;
            }
        }

        // Procesar último segmento si no termina en delimitador
        if current_start < text.len() {
            let sentence = &text[current_start..];
            let claim_text = sentence.trim();
            if !claim_text.is_empty() && self.looks_like_claim(claim_text, None) {
                let rel = match sentence.find(claim_text) {
                    Some(r) => r,
                    None => {
                        tracing::warn!("extract_claims: find() retornó None (last segment)");
                        0
                    }
                };
                let claim_start = current_start + rel;
                let claim_end = claim_start + claim_text.len();
                claims.push(((claim_start, claim_end), claim_text.to_string()));
            }
        }

        claims
    }

    /// Determinar si un texto parece una afirmación verificable
    fn looks_like_claim(&self, text: &str, delimiter: Option<char>) -> bool {
        // Si el delimitador era '?', es una pregunta -> no es claim
        if delimiter == Some('?') {
            return false;
        }

        let lower = text.to_lowercase();

        // Ignorar comandos/sugerencias (inglés y español)
        let ignore_prefixes = [
            "try ",
            "you should",
            "please ",
            "let's ",
            "prueba ",
            "deberías",
            "debe ",
            "haz ",
            "intenta ",
            "por favor ",
            "vamos a ",
        ];
        if ignore_prefixes.iter().any(|p| lower.starts_with(p)) {
            return false;
        }

        // Tokenizar en palabras para evitar falsos positivos por substring
        let words: std::collections::HashSet<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();

        // Indicadores de afirmación (inglés + español)
        let claim_indicators_en = [
            "is",
            "are",
            "was",
            "were",
            "has",
            "have",
            "does",
            "do",
            "uses",
            "calls",
            "returns",
            "takes",
            "implements",
            "contains",
            "includes",
            "requires",
            "depends",
        ];
        let claim_indicators_es = [
            "es",
            "son",
            "era",
            "eran",
            "tiene",
            "tienen",
            "hace",
            "hacen",
            "usa",
            "llama",
            "retorna",
            "devuelve",
            "toma",
            "implementa",
            "contiene",
            "incluye",
            "requiere",
            "depende",
        ];

        // Verificar que al menos un indicador esté como palabra completa
        claim_indicators_en.iter().any(|ind| words.contains(ind))
            || claim_indicators_es.iter().any(|ind| words.contains(ind))
    }

    fn generate_inline_citation(&self, claim: &ClaimType) -> Option<String> {
        match claim {
            ClaimType::Verified(v) => {
                if let Some(first_evidence) = v.evidence.first() {
                    Some(format!("[📎 {}]", first_evidence.backlink.uri))
                } else {
                    None
                }
            }
            ClaimType::DirectObservation(obs) => Some(format!(
                "[👁️ {}]",
                match &obs.source {
                    evidence::EvidenceSource::CodeFile { path, .. } => path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    _ => "observación directa".to_string(),
                }
            )),
            ClaimType::Hypothesis(_) => Some("[⚠️ hipótesis]".to_string()),
            ClaimType::NeedsContext(_) => Some("[❓ sin contexto]".to_string()),
            ClaimType::Citation(c) => Some(format!("[📖 {:?}]", c.source)),
        }
    }

    fn calculate_overall_confidence(&self, claims: &[AnnotatedClaim]) -> f32 {
        if claims.is_empty() {
            return 1.0; // No claims = no risk
        }

        let mut total_confidence = 0.0;

        for claim in claims {
            let confidence = match &claim.claim_type {
                ClaimType::Verified(v) => v.confidence,
                ClaimType::DirectObservation(_) => 1.0,
                ClaimType::Hypothesis(h) => h.confidence * Self::HYPOTHESIS_PENALTY,
                ClaimType::NeedsContext(_) => 0.3,
                ClaimType::Citation(_) => 0.9,
            };
            total_confidence += confidence;
        }

        total_confidence / claims.len() as f32
    }
}

impl AnnotatedResponse {
    /// Formatear respuesta con citaciones inline
    pub fn format_with_citations(&self) -> String {
        let mut result = self.original_text.clone();

        // Insertar citaciones de atrás hacia adelante para no alterar posiciones
        let mut claims_sorted: Vec<_> = self.claims.iter().collect();
        claims_sorted.sort_by(|a, b| b.span.1.cmp(&a.span.1));

        for claim in claims_sorted {
            if let Some(ref citation) = claim.inline_citation {
                if claim.span.1 <= result.len() {
                    result.insert_str(claim.span.1, &format!(" {}", citation));
                }
            }
        }

        result
    }

    /// Obtener solo claims verificados
    pub fn verified_claims(&self) -> Vec<&AnnotatedClaim> {
        self.claims
            .iter()
            .filter(|c| matches!(c.claim_type, ClaimType::Verified(_)))
            .collect()
    }

    /// Obtener claims que necesitan atención
    pub fn claims_needing_attention(&self) -> Vec<&AnnotatedClaim> {
        self.claims
            .iter()
            .filter(|c| {
                matches!(
                    c.claim_type,
                    ClaimType::Hypothesis(_) | ClaimType::NeedsContext(_)
                )
            })
            .collect()
    }

    /// Contar backlinks según predicado (helper para evitar duplicación)
    fn count_backlinks_by<F>(&self, predicate: F) -> usize
    where
        F: Fn(&evidence::BacklinkStatus) -> bool,
    {
        self.claims
            .iter()
            .filter_map(|c| match &c.claim_type {
                ClaimType::Verified(v) => Some(&v.evidence),
                ClaimType::DirectObservation(_) => None,
                ClaimType::Hypothesis(h) => Some(&h.partial_evidence),
                ClaimType::NeedsContext(_) | ClaimType::Citation(_) => None,
            })
            .flat_map(|evidence_list| evidence_list.iter())
            .filter(|e| predicate(&e.backlink.status))
            .count()
    }

    /// Contar backlinks inválidos (Broken + Stale + Unverified)
    pub fn invalid_backlinks_count(&self) -> usize {
        self.count_backlinks_by(|status| {
            matches!(
                status,
                evidence::BacklinkStatus::Broken
                    | evidence::BacklinkStatus::Stale
                    | evidence::BacklinkStatus::Unverified
            )
        })
    }

    /// Contar solo backlinks rotos (archivo no existe, URL 404, etc.)
    pub fn broken_backlinks_count(&self) -> usize {
        self.count_backlinks_by(|status| matches!(status, evidence::BacklinkStatus::Broken))
    }

    /// Contar solo backlinks stale (verificados pero antiguos)
    pub fn stale_backlinks_count(&self) -> usize {
        self.count_backlinks_by(|status| matches!(status, evidence::BacklinkStatus::Stale))
    }

    /// Contar evidencia que necesita re-verificación
    pub fn stale_evidence_count(
        &self,
        max_age: std::time::Duration,
        now: chrono::DateTime<chrono::Utc>,
    ) -> usize {
        self.claims
            .iter()
            .filter_map(|c| match &c.claim_type {
                ClaimType::Verified(v) => Some(&v.evidence),
                ClaimType::DirectObservation(_) => None,
                ClaimType::Hypothesis(h) => Some(&h.partial_evidence),
                ClaimType::NeedsContext(_) | ClaimType::Citation(_) => None,
            })
            .flat_map(|evidence_list| evidence_list.iter())
            .filter(|e| {
                let age = now - e.backlink.last_verified;
                age > chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::MAX)
            })
            .count()
    }
}
