//! # Ledger
//!
//! Registro inmutable de todos los eventos del sistema.
//! Cada acción deja trazabilidad.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Tipos de eventos en el ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    /// Petición recibida del editor.
    Request,
    /// Respuesta enviada al editor.
    Response,
    /// Búsqueda en memoria.
    Recall,
    /// Llamada a modelo externo.
    ModelCall,
    /// Edición de archivo.
    FileEdit,
    /// Lectura de archivo.
    FileRead,
    /// Gate verificado.
    GateCheck,
    /// Error ocurrido.
    Error,
}

/// Un evento en el ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Timestamp del evento (Unix epoch ms).
    pub timestamp: u64,
    /// Tipo de evento.
    pub kind: EventKind,
    /// Contenido/descripción del evento.
    pub content: String,
    /// Metadatos adicionales (JSON).
    pub metadata: Option<String>,
}

impl Event {
    /// Crear un nuevo evento.
    pub fn new(kind: EventKind, content: impl Into<String>) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            kind,
            content: content.into(),
            metadata: None,
        }
    }

    /// Añadir metadatos al evento.
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }
}

/// El ledger de eventos.
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    /// Crear un nuevo ledger.
    pub fn new(agent_path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let path = agent_path.join("ledger.jsonl");

        // Crear directorio si no existe
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(Self { path })
    }

    /// Registrar un evento.
    pub fn log(&self, event: Event) {
        if let Ok(json) = serde_json::to_string(&event) {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = writeln!(file, "{}", json);
            }
        }
    }

    /// Leer los últimos N eventos.
    pub fn recent(&self, n: usize) -> Vec<Event> {
        if let Ok(content) = fs::read_to_string(&self.path) {
            content
                .lines()
                .rev()
                .take(n)
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = Event::new(EventKind::Request, "Test request");
        assert!(event.timestamp > 0);
        assert_eq!(event.content, "Test request");
    }
}
