//! Políticas de verificación

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Política de verificación de claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPolicy {
    /// Confianza mínima para marcar como "Verified"
    pub min_confidence_for_verified: f32,

    /// Confianza mínima para marcar como "Hypothesis" (vs NeedsContext)
    pub min_confidence_for_hypothesis: f32,

    /// Requerir backlinks válidos
    pub require_valid_backlinks: bool,

    /// Máximo de claims sin verificar permitidos en una respuesta
    pub max_unverified_claims: usize,

    /// Forzar re-verificación después de este tiempo
    pub reverify_after: std::time::Duration,

    /// Nivel de strictness
    pub strictness: StrictnessLevel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StrictnessLevel {
    /// Relajado: permite muchas hipótesis
    Relaxed,

    /// Normal: balance
    Normal,

    /// Estricto: casi todo debe ser verificado
    Strict,

    /// Paranoico: nada sin evidencia fuerte
    Paranoid,
}

/// Error de validación de política
#[derive(Debug)]
pub enum PolicyConfigError {
    /// Confidence fuera de rango [0.0, 1.0]
    ConfidenceOutOfRange { field: &'static str, value: f32 },
    /// min_verified debe ser >= min_hypothesis
    VerifiedLessThanHypothesis,
    /// reverify_after debe ser > 0
    ZeroReverifyDuration,
    /// Paranoid debe tener max_unverified = 0
    ParanoidWithUnverifiedAllowed,
}

impl std::fmt::Display for PolicyConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyConfigError::ConfidenceOutOfRange { field, value } => {
                write!(f, "{} = {} is not in [0.0, 1.0]", field, value)
            }
            PolicyConfigError::VerifiedLessThanHypothesis => {
                write!(
                    f,
                    "min_confidence_for_verified must be >= min_confidence_for_hypothesis"
                )
            }
            PolicyConfigError::ZeroReverifyDuration => {
                write!(f, "reverify_after must be > 0")
            }
            PolicyConfigError::ParanoidWithUnverifiedAllowed => {
                write!(f, "Paranoid strictness requires max_unverified_claims = 0")
            }
        }
    }
}

impl std::error::Error for PolicyConfigError {}

impl Default for VerificationPolicy {
    fn default() -> Self {
        Self {
            min_confidence_for_verified: 0.7,
            min_confidence_for_hypothesis: 0.4,
            require_valid_backlinks: true,
            max_unverified_claims: 3,
            reverify_after: std::time::Duration::from_secs(3600), // 1 hora
            strictness: StrictnessLevel::Normal,
        }
    }
}

impl VerificationPolicy {
    /// Política estricta para código crítico
    pub fn strict() -> Self {
        Self {
            min_confidence_for_verified: 0.85,
            min_confidence_for_hypothesis: 0.6,
            require_valid_backlinks: true,
            max_unverified_claims: 1,
            reverify_after: std::time::Duration::from_secs(300), // 5 minutos
            strictness: StrictnessLevel::Strict,
        }
    }

    /// Política paranoica para código de seguridad
    pub fn paranoid() -> Self {
        Self {
            min_confidence_for_verified: 0.95,
            min_confidence_for_hypothesis: 0.8,
            require_valid_backlinks: true,
            max_unverified_claims: 0,
            reverify_after: std::time::Duration::from_secs(60), // 1 minuto
            strictness: StrictnessLevel::Paranoid,
        }
    }

    /// Política relajada para exploración
    pub fn relaxed() -> Self {
        Self {
            min_confidence_for_verified: 0.5,
            min_confidence_for_hypothesis: 0.2,
            require_valid_backlinks: false,
            max_unverified_claims: 10,
            reverify_after: std::time::Duration::from_secs(86400), // 1 día
            strictness: StrictnessLevel::Relaxed,
        }
    }

    /// Validar que la política tiene configuración coherente
    pub fn validate(&self) -> Result<(), PolicyConfigError> {
        // Validar rangos de confidence
        fn valid_conf(x: f32) -> bool {
            x.is_finite() && (0.0..=1.0).contains(&x)
        }

        if !valid_conf(self.min_confidence_for_verified) {
            return Err(PolicyConfigError::ConfidenceOutOfRange {
                field: "min_confidence_for_verified",
                value: self.min_confidence_for_verified,
            });
        }

        if !valid_conf(self.min_confidence_for_hypothesis) {
            return Err(PolicyConfigError::ConfidenceOutOfRange {
                field: "min_confidence_for_hypothesis",
                value: self.min_confidence_for_hypothesis,
            });
        }

        // min_verified >= min_hypothesis
        if self.min_confidence_for_verified < self.min_confidence_for_hypothesis {
            return Err(PolicyConfigError::VerifiedLessThanHypothesis);
        }

        // reverify_after > 0
        if self.reverify_after.is_zero() {
            return Err(PolicyConfigError::ZeroReverifyDuration);
        }

        // Paranoid debe tener max_unverified = 0
        if self.strictness == StrictnessLevel::Paranoid && self.max_unverified_claims > 0 {
            return Err(PolicyConfigError::ParanoidWithUnverifiedAllowed);
        }

        Ok(())
    }

    /// Verificar si una respuesta cumple la política
    pub fn validate_response(
        &self,
        response: &super::annotator::AnnotatedResponse,
        now: DateTime<Utc>,
    ) -> PolicyValidation {
        let mut violations = Vec::new();

        // Contar claims sin verificar
        let unverified_count = response.claims_needing_attention().len();
        if unverified_count > self.max_unverified_claims {
            violations.push(PolicyViolation {
                kind: ViolationKind::TooManyUnverifiedClaims,
                description: format!(
                    "Respuesta tiene {} claims sin verificar (máx: {})",
                    unverified_count, self.max_unverified_claims
                ),
                severity: match self.strictness {
                    StrictnessLevel::Paranoid => ViolationSeverity::Blocking,
                    StrictnessLevel::Strict => ViolationSeverity::Error,
                    _ => ViolationSeverity::Warning,
                },
            });
        }

        // Verificar confianza general (manejar NaN)
        let confidence = response.overall_confidence;
        if !confidence.is_finite() || confidence < self.min_confidence_for_hypothesis {
            violations.push(PolicyViolation {
                kind: ViolationKind::LowConfidence,
                description: format!(
                    "Confianza general {:.2} es menor que el mínimo {:.2}",
                    confidence, self.min_confidence_for_hypothesis
                ),
                severity: ViolationSeverity::Error,
            });
        }

        // Verificar backlinks si se requiere
        if self.require_valid_backlinks {
            let broken = response.broken_backlinks_count();
            let stale = response.stale_backlinks_count();

            // Reportar backlinks rotos (más grave)
            if broken > 0 {
                violations.push(PolicyViolation {
                    kind: ViolationKind::BrokenBacklink,
                    description: format!("{} backlinks rotos (archivo/URL no existe)", broken),
                    severity: match self.strictness {
                        StrictnessLevel::Paranoid => ViolationSeverity::Blocking,
                        StrictnessLevel::Strict => ViolationSeverity::Error,
                        _ => ViolationSeverity::Warning,
                    },
                });
            }

            // Reportar backlinks stale por separado (menos grave que broken)
            if stale > 0 {
                violations.push(PolicyViolation {
                    kind: ViolationKind::StaleEvidence,
                    description: format!("{} backlinks stale (contenido cambió)", stale),
                    severity: match self.strictness {
                        StrictnessLevel::Paranoid => ViolationSeverity::Error,
                        StrictnessLevel::Strict => ViolationSeverity::Warning,
                        _ => ViolationSeverity::Warning,
                    },
                });
            }
        }

        // Verificar evidencia que necesita re-verificación por tiempo
        let stale_by_time = response.stale_evidence_count(self.reverify_after, now);
        if stale_by_time > 0 {
            violations.push(PolicyViolation {
                kind: ViolationKind::StaleEvidence,
                description: format!(
                    "{} evidencias superan el límite de re-verificación ({:?})",
                    stale_by_time, self.reverify_after
                ),
                severity: match self.strictness {
                    StrictnessLevel::Paranoid => ViolationSeverity::Blocking,
                    StrictnessLevel::Strict => ViolationSeverity::Error,
                    _ => ViolationSeverity::Warning,
                },
            });
        }

        // Determinar validez según strictness
        let is_valid = match self.strictness {
            // Paranoid/Strict: Error también invalida
            StrictnessLevel::Paranoid | StrictnessLevel::Strict => violations
                .iter()
                .all(|v| v.severity == ViolationSeverity::Warning),
            // Normal/Relaxed: solo Blocking invalida
            _ => violations
                .iter()
                .all(|v| v.severity != ViolationSeverity::Blocking),
        };

        PolicyValidation {
            is_valid,
            violations,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolicyValidation {
    pub is_valid: bool,
    pub violations: Vec<PolicyViolation>,
}

#[derive(Debug, Clone)]
pub struct PolicyViolation {
    pub kind: ViolationKind,
    pub description: String,
    pub severity: ViolationSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    TooManyUnverifiedClaims,
    LowConfidence,
    BrokenBacklink,
    StaleEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationSeverity {
    Warning,
    Error,
    Blocking,
}
