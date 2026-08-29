//! Integración bidireccional entre Claims y Ledger
//!
//! Este módulo conecta el sistema de verificación de claims con el ledger,
//! permitiendo:
//! 1. Verificar backlinks a eventos del ledger (¿existe realmente?)
//! 2. Registrar claims verificados/rechazados en el ledger
//! 3. Consultar claims que dependen de un evento

use crate::claims::{ClaimOrigin, VerifiedClaim};
use crate::error::Result;
use crate::ledger::{LedgerReader, LedgerWriter};
use crate::storage::Storage;
use crate::types::{Event, EventId, EventKind};
use std::str::FromStr;

/// Integración Claims ↔ Ledger
pub struct ClaimLedgerIntegration {
    reader: LedgerReader,
    writer: LedgerWriter,
}

impl ClaimLedgerIntegration {
    /// Crear nueva instancia con storage compartido
    pub fn new(storage: Storage) -> Self {
        Self {
            reader: LedgerReader::new(storage.clone()),
            writer: LedgerWriter::new(storage),
        }
    }

    /// Verificar que un evento del ledger existe y es válido
    ///
    /// Retorna:
    /// - Ok(true) si el evento existe
    /// - Ok(false) si el evento no existe
    /// - Err si hay error de I/O
    pub fn verify_ledger_event(&self, event_id: &str) -> Result<bool> {
        // Parsear el event_id como ULID
        let id = match ulid::Ulid::from_str(event_id) {
            Ok(ulid) => EventId(ulid),
            Err(_) => return Ok(false), // ID inválido = no existe
        };

        match self.reader.get(&id)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Obtener un evento del ledger por ID
    pub fn get_event(&self, event_id: &str) -> Result<Option<Event>> {
        let id = match ulid::Ulid::from_str(event_id) {
            Ok(ulid) => EventId(ulid),
            Err(_) => return Ok(None),
        };
        self.reader.get(&id)
    }

    /// Registrar un claim verificado en el ledger
    ///
    /// Crea un evento ClaimVerified con:
    /// - Descripción del claim
    /// - Referencias a las evidencias
    /// - Nivel de confianza
    pub fn record_claim_verified(&self, claim: &VerifiedClaim) -> Result<Event> {
        let evidence_refs: Vec<String> = claim
            .evidence
            .iter()
            .map(|e| e.backlink.uri.clone())
            .collect();

        // Los methods están disponibles en claim.verification_methods si se necesitan

        let mut event = Event::new(
            EventKind::ClaimVerified,
            &format!(
                "Claim verificado (confidence={:.0}%): {}",
                claim.confidence * 100.0,
                truncate_text(&claim.text, 100)
            ),
        );

        // Añadir metadatos
        event.tags = vec![
            "claims".to_string(),
            "verified".to_string(),
            format!("confidence:{:.2}", claim.confidence),
        ];

        // Inputs: las URIs de evidencia
        event.inputs = evidence_refs;

        // Outputs: el ID del claim
        event.outputs = vec![claim.id.clone()];

        // Importancia basada en confidence
        event.importance = claim.confidence;

        // Parent event si existe
        if let Some(ref ledger_id) = claim.origin.ledger_event_id {
            if let Ok(ulid) = ulid::Ulid::from_str(ledger_id) {
                event.parent_event_id = Some(EventId(ulid));
            }
        }

        self.writer.append(event)
    }

    /// Registrar un claim rechazado en el ledger
    pub fn record_claim_rejected(
        &self,
        claim_text: &str,
        reason: &str,
        origin: &ClaimOrigin,
    ) -> Result<Event> {
        let mut event = Event::new(
            EventKind::ClaimRejected,
            &format!(
                "Claim rechazado: {} | Razón: {}",
                truncate_text(claim_text, 80),
                truncate_text(reason, 80)
            ),
        );

        event.tags = vec!["claims".to_string(), "rejected".to_string()];
        event.importance = 0.7; // Rechazos son importantes para aprender

        // Parent event si existe
        if let Some(ref ledger_id) = origin.ledger_event_id {
            if let Ok(ulid) = ulid::Ulid::from_str(ledger_id) {
                event.parent_event_id = Some(EventId(ulid));
            }
        }

        self.writer.append(event)
    }

    /// Registrar que se hizo un claim (antes de verificar)
    pub fn record_claim_made(&self, claim_text: &str, origin: &ClaimOrigin) -> Result<Event> {
        let mut event = Event::new(
            EventKind::ClaimMade,
            &format!("Claim propuesto: {}", truncate_text(claim_text, 120)),
        );

        event.tags = vec!["claims".to_string(), "pending".to_string()];
        event.importance = 0.3; // Baja hasta que se verifique

        if let Some(ref conv_id) = origin.conversation_id {
            event = event.with_project(conv_id);
        }

        self.writer.append(event)
    }

    /// Buscar eventos de claims verificados que referencian un evento dado
    ///
    /// Útil para responder: "¿Qué claims dependen de este evento?"
    pub fn claims_referencing_event(&self, event_id: &str, limit: usize) -> Result<Vec<Event>> {
        // Buscar en todos los eventos de tipo claim (evita heurística limit*10)
        let all_claims = self.reader.all()?;

        let matching: Vec<Event> = all_claims
            .into_iter()
            .filter(|e| {
                matches!(e.kind, EventKind::ClaimVerified | EventKind::ClaimMade)
                    && (e.inputs.iter().any(|input| input.contains(event_id))
                        || e.parent_event_id
                            .as_ref()
                            .map(|p| p.to_string().contains(event_id))
                            .unwrap_or(false))
            })
            .take(limit)
            .collect();

        Ok(matching)
    }

    /// Verificar integridad: ¿todos los claims verificados tienen evidencias válidas?
    pub fn audit_claims_integrity(&self, limit: usize) -> Result<ClaimAuditReport> {
        let verified_events = self
            .reader
            .recent(limit)?
            .into_iter()
            .filter(|e| e.kind == EventKind::ClaimVerified)
            .collect::<Vec<_>>();

        let mut report = ClaimAuditReport {
            total_verified_claims: verified_events.len(),
            claims_with_valid_evidence: 0,
            claims_with_broken_evidence: 0,
            broken_evidence_refs: Vec::new(),
        };

        for event in verified_events {
            let mut has_broken = false;

            for input in &event.inputs {
                // Si es un ledger:// URI, verificar que existe
                if input.starts_with("ledger://") {
                    let event_id = input.trim_start_matches("ledger://");
                    if !self.verify_ledger_event(event_id)? {
                        has_broken = true;
                        report.broken_evidence_refs.push(format!(
                            "Claim {} references non-existent event: {}",
                            event.id, event_id
                        ));
                    }
                }
                // TODO: verificar file:// URIs también
            }

            if has_broken {
                report.claims_with_broken_evidence += 1;
            } else {
                report.claims_with_valid_evidence += 1;
            }
        }

        Ok(report)
    }
}

/// Reporte de auditoría de claims
#[derive(Debug, Clone)]
pub struct ClaimAuditReport {
    pub total_verified_claims: usize,
    pub claims_with_valid_evidence: usize,
    pub claims_with_broken_evidence: usize,
    pub broken_evidence_refs: Vec<String>,
}

impl ClaimAuditReport {
    /// ¿El reporte indica problemas?
    pub fn has_issues(&self) -> bool {
        self.claims_with_broken_evidence > 0
    }

    /// Porcentaje de integridad
    pub fn integrity_percentage(&self) -> f32 {
        if self.total_verified_claims == 0 {
            return 100.0;
        }
        (self.claims_with_valid_evidence as f32 / self.total_verified_claims as f32) * 100.0
    }
}

/// Truncar texto para logs/descripciones
fn truncate_text(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    let char_count = text.chars().count();
    if char_count <= max_len {
        text.to_string()
    } else {
        let trunc_len = max_len.saturating_sub(3);
        if trunc_len == 0 {
            return text.chars().take(max_len).collect();
        }
        let prefix: String = text.chars().take(trunc_len).collect();
        format!("{}...", prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_verify_nonexistent_event() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let integration = ClaimLedgerIntegration::new(storage);

        // Evento que no existe
        let result = integration
            .verify_ledger_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_record_claim_made() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let integration = ClaimLedgerIntegration::new(storage);

        let origin = ClaimOrigin::from_conversation("test-conv".to_string());
        let event = integration
            .record_claim_made("El sistema usa blake3 para hashing", &origin)
            .unwrap();

        assert_eq!(event.kind, EventKind::ClaimMade);
        assert!(event.tags.contains(&"claims".to_string()));
    }

    #[test]
    fn test_audit_empty_ledger() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let integration = ClaimLedgerIntegration::new(storage);

        let report = integration.audit_claims_integrity(100).unwrap();
        assert_eq!(report.total_verified_claims, 0);
        assert!(!report.has_issues());
        assert_eq!(report.integrity_percentage(), 100.0);
    }

    #[test]
    fn test_truncate_text_utf8_safe() {
        let text = "áéíóú";
        assert_eq!(truncate_text(text, 4), "á...");
    }
}
