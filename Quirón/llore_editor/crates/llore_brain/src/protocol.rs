//! # Protocol
//!
//! Protocolo de comunicación entre Llore y Quirón.

use serde::{Deserialize, Serialize};

/// Una petición desde el editor hacia Quirón.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// ID único de la petición.
    pub id: String,
    /// Contenido de la petición.
    pub content: String,
    /// Tipo de petición.
    pub kind: RequestKind,
    /// ¿Requiere evidencia para responder?
    pub requires_evidence: bool,
    /// Evidencia adjunta (rutas de archivos, citas, etc.).
    pub evidence: Vec<String>,
    /// ¿Es una petición de edición?
    pub is_edit: bool,
    /// ¿Se leyó el archivo antes de editarlo?
    pub file_was_read: bool,
}

impl Request {
    /// Crear una petición simple de texto.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            id: uuid(),
            content: content.into(),
            kind: RequestKind::Query,
            requires_evidence: false,
            evidence: Vec::new(),
            is_edit: false,
            file_was_read: false,
        }
    }

    /// Crear una petición de edición.
    pub fn edit(file_path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: uuid(),
            content: content.into(),
            kind: RequestKind::Edit,
            requires_evidence: true,
            evidence: vec![file_path.into()],
            is_edit: true,
            file_was_read: false,
        }
    }

    /// Marcar que el archivo fue leído.
    pub fn with_file_read(mut self) -> Self {
        self.file_was_read = true;
        self
    }
}

/// Tipos de peticiones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestKind {
    /// Consulta de información.
    Query,
    /// Edición de archivo.
    Edit,
    /// Búsqueda en memoria.
    Recall,
    /// Ejecución de comando.
    Execute,
}

/// Respuesta de Quirón al editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// ID de la petición original.
    pub request_id: String,
    /// Estado de la respuesta.
    pub status: ResponseStatus,
    /// Contenido de la respuesta.
    pub content: String,
    /// Citas/evidencia utilizada.
    pub citations: Vec<Citation>,
}

impl Response {
    /// Crear una respuesta exitosa.
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            request_id: String::new(),
            status: ResponseStatus::Success,
            content: content.into(),
            citations: Vec::new(),
        }
    }

    /// Crear una respuesta de error.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            request_id: String::new(),
            status: ResponseStatus::Error,
            content: message.into(),
            citations: Vec::new(),
        }
    }

    /// Crear una respuesta pendiente.
    pub fn pending(message: impl Into<String>) -> Self {
        Self {
            request_id: String::new(),
            status: ResponseStatus::Pending,
            content: message.into(),
            citations: Vec::new(),
        }
    }
}

/// Estado de la respuesta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseStatus {
    /// Respuesta exitosa.
    Success,
    /// Error procesando la petición.
    Error,
    /// Procesando (llamando a modelo, etc.).
    Pending,
    /// Gate bloqueó la petición.
    Blocked,
}

/// Una cita/referencia utilizada en la respuesta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    /// Tipo de cita.
    pub kind: CitationKind,
    /// Contenido de la cita.
    pub content: String,
    /// Fuente (ruta de archivo, URL, etc.).
    pub source: String,
}

/// Tipos de citas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CitationKind {
    /// Cita de un archivo.
    File,
    /// Cita de memoria.
    Memory,
    /// Cita de búsqueda web.
    Web,
    /// Cita de documentación.
    Doc,
}

/// Tarea delegada al worker local (subordinado al planner/primary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerTask {
    /// ID correlativo de tarea.
    pub task_id: String,
    /// Tipo de tarea (`recall`, `summarize`, `classify`, etc.).
    pub kind: String,
    /// Objetivo explícito.
    pub objective: String,
    /// Restricciones operativas (sin claims finales, formato, etc.).
    #[serde(default)]
    pub constraints: Vec<String>,
}

/// Resultado del worker local.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    /// Debe coincidir con el `task_id` de entrada.
    pub task_id: String,
    /// Resumen operativo para el planner.
    pub summary: String,
    /// Referencias (event IDs, rutas, etc.).
    #[serde(default)]
    pub citations: Vec<String>,
    /// Claims factuales detectados en la salida (debe ser vacío).
    #[serde(default)]
    pub claims: Vec<String>,
    /// Confianza del worker (0..1).
    #[serde(default)]
    pub confidence: Option<f32>,
}

/// Generar un UUID simple.
fn uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_creation() {
        let req = Request::text("¿Cómo funciona esto?");
        assert!(!req.id.is_empty());
        assert_eq!(req.content, "¿Cómo funciona esto?");
    }

    #[test]
    fn test_response_creation() {
        let resp = Response::success("Funciona así...");
        assert!(matches!(resp.status, ResponseStatus::Success));
    }

    #[test]
    fn test_worker_contract_roundtrip() {
        let task = WorkerTask {
            task_id: "w-1".to_string(),
            kind: "recall".to_string(),
            objective: "buscar eventos relevantes".to_string(),
            constraints: vec!["no_final_claims".to_string()],
        };
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("task_id"));

        let result = WorkerResult {
            task_id: "w-1".to_string(),
            summary: "2 eventos encontrados".to_string(),
            citations: vec!["evt-1".to_string()],
            claims: vec![],
            confidence: Some(0.7),
        };
        let encoded = serde_json::to_string(&result).unwrap();
        let decoded: WorkerResult = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.task_id, "w-1");
        assert!(decoded.claims.is_empty());
    }
}
