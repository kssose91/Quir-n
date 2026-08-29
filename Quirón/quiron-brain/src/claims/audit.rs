//! Audit trail de claims

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Trail de auditoría para claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTrail {
    /// ID del trail
    pub id: String,

    /// Eventos de auditoría
    pub events: Vec<AuditEvent>,

    /// Creación del trail
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Timestamp del evento
    pub timestamp: DateTime<Utc>,

    /// Tipo de evento
    pub event_type: AuditEventType,

    /// ID del claim afectado (obligatorio para eventos de claim)
    pub claim_id: Option<String>,

    /// Detalles estructurados (JSON para consistencia)
    pub details: serde_json::Value,

    /// Actor que causó el evento
    pub actor: AuditActor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    // === Eventos que REQUIEREN claim_id ===
    /// Claim creado
    ClaimCreated,
    /// Claim verificado
    ClaimVerified,
    /// Claim degradado a hipótesis
    ClaimDowngraded,
    /// Evidencia añadida
    EvidenceAdded,
    /// Evidencia invalidada
    EvidenceInvalidated,
    /// Backlink verificado
    BacklinkVerified,
    /// Backlink roto
    BacklinkBroken,

    // === Eventos de sistema (claim_id opcional) ===
    /// Política violada
    PolicyViolated,
    /// Evento de sistema/info
    SystemInfo,
}

impl AuditEventType {
    /// Indica si este tipo de evento requiere claim_id
    pub fn requires_claim_id(&self) -> bool {
        matches!(
            self,
            AuditEventType::ClaimCreated
                | AuditEventType::ClaimVerified
                | AuditEventType::ClaimDowngraded
                | AuditEventType::EvidenceAdded
                | AuditEventType::EvidenceInvalidated
                | AuditEventType::BacklinkVerified
                | AuditEventType::BacklinkBroken
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditActor {
    /// El sistema automáticamente
    System,
    /// El verificador de claims
    Verifier,
    /// El auditor de backlinks
    BacklinkAuditor,
    /// El usuario
    User { id: String },
}

/// Error de auditoría
#[derive(Debug)]
pub enum AuditError {
    /// Falta claim_id para un evento que lo requiere
    MissingClaimId { event_type: AuditEventType },
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::MissingClaimId { event_type } => {
                write!(
                    f,
                    "Event {:?} requires claim_id but none provided",
                    event_type
                )
            }
        }
    }
}

impl std::error::Error for AuditError {}

/// Trait para inyección de reloj (determinismo en tests/replay)
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Reloj real que usa Utc::now()
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

impl AuditTrail {
    /// Crear nuevo trail con reloj del sistema
    pub fn new() -> Self {
        Self::with_clock(&SystemClock)
    }

    /// Crear nuevo trail con reloj inyectable (para tests/replay)
    pub fn with_clock(clock: &dyn Clock) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            events: Vec::new(),
            created_at: clock.now(),
        }
    }

    /// Registrar un evento (valida claim_id si es requerido)
    pub fn record(
        &mut self,
        event_type: AuditEventType,
        claim_id: Option<&str>,
        details: serde_json::Value,
        actor: AuditActor,
    ) -> Result<(), AuditError> {
        // Validar que eventos de claim tengan claim_id
        if event_type.requires_claim_id() && claim_id.is_none() {
            return Err(AuditError::MissingClaimId {
                event_type: event_type.clone(),
            });
        }

        self.events.push(AuditEvent {
            timestamp: Utc::now(),
            event_type,
            claim_id: claim_id.map(String::from),
            details,
            actor,
        });

        Ok(())
    }

    /// Registrar evento con reloj inyectable
    pub fn record_with_clock(
        &mut self,
        event_type: AuditEventType,
        claim_id: Option<&str>,
        details: serde_json::Value,
        actor: AuditActor,
        clock: &dyn Clock,
    ) -> Result<(), AuditError> {
        if event_type.requires_claim_id() && claim_id.is_none() {
            return Err(AuditError::MissingClaimId {
                event_type: event_type.clone(),
            });
        }

        self.events.push(AuditEvent {
            timestamp: clock.now(),
            event_type,
            claim_id: claim_id.map(String::from),
            details,
            actor,
        });

        Ok(())
    }

    /// Obtener eventos recientes
    pub fn recent_events(&self, limit: usize) -> &[AuditEvent] {
        let start = self.events.len().saturating_sub(limit);
        &self.events[start..]
    }

    /// Obtener eventos por claim
    pub fn events_for_claim(&self, claim_id: &str) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| {
                e.claim_id
                    .as_ref()
                    .map(|id| id == claim_id)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Buscar violaciones de política
    pub fn policy_violations(&self) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e.event_type, AuditEventType::PolicyViolated))
            .collect()
    }
}

impl Default for AuditTrail {
    fn default() -> Self {
        Self::new()
    }
}
