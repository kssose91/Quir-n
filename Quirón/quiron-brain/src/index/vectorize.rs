//! Vectorización del índice de código a Qdrant.
//!
//! Cada unidad se vuelve un punto en la colección `quiron_code`, **separada** de
//! `quiron_events` (la memoria de eventos): un mundo contaminado no es un mundo,
//! y el índice de código no comparte colección con la memoria personal.
//!
//! El texto que se vectoriza es el `semantic_text` de la unidad. Mientras la red
//! obrera no redacte las fichas, se usa la firma y la ruta; cuando las redacte,
//! el mismo punto se reescribe con su identidad estable. El punto lleva ruta,
//! símbolo, rango y hash en la carga útil, de modo que todo resultado sea
//! verificable contra el código de origen.

use super::unit::{FileUnit, LogicUnit};
use crate::semantic::{SemanticClient, SemanticConfig};
use crate::types::ids::NodeId;
use crate::Result;
use qdrant_client::qdrant::Value;
use std::collections::HashMap;

/// Colección por defecto del índice de código.
pub const DEFAULT_CODE_COLLECTION: &str = "quiron_code";

/// Vectoriza unidades de código sobre su propia colección de Qdrant.
pub struct CodeVectorizer {
    client: SemanticClient,
}

impl CodeVectorizer {
    /// Conecta usando la configuración semántica del entorno, pero forzando la
    /// colección del índice de código (`QDRANT_CODE_COLLECTION`, por defecto
    /// `quiron_code`). No toca la colección de la memoria.
    pub async fn connect() -> Result<Self> {
        let mut cfg = SemanticConfig::from_env();
        cfg.collection = std::env::var("QDRANT_CODE_COLLECTION")
            .unwrap_or_else(|_| DEFAULT_CODE_COLLECTION.to_string());
        let client = SemanticClient::connect(cfg)
            .await
            .map_err(|e| crate::BrainError::Internal(e))?;
        Ok(Self { client })
    }

    /// Sube archivos y lógicas. Devuelve cuántos puntos se escribieron.
    pub async fn vectorize(
        &self,
        files: &[FileUnit],
        logic: &[LogicUnit],
    ) -> Result<u32> {
        let mut n = 0u32;
        for f in files {
            self.client
                .upsert_text_point(point_id(&f.id), &f.semantic_text(), file_payload(f))
                .await
                .map_err(|e| crate::BrainError::Internal(e))?;
            n += 1;
        }
        for l in logic {
            self.client
                .upsert_text_point(point_id(&l.id), &l.semantic_text(), logic_payload(l))
                .await
                .map_err(|e| crate::BrainError::Internal(e))?;
            n += 1;
        }
        Ok(n)
    }

    /// ¿Está el servicio semántico y Qdrant disponibles?
    pub async fn health(&self) -> bool {
        self.client.health_check().await
    }
}

/// El identificador del punto es el ULID de la unidad como UUID de 128 bits.
/// Conserva la identidad completa (a diferencia de truncar a u64) y hace el
/// upsert idempotente: reindexar la misma unidad reescribe su punto.
fn point_id(id: &NodeId) -> qdrant_client::qdrant::PointId {
    uuid_string(id.to_bytes()).into()
}

fn uuid_string(b: [u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn file_payload(f: &FileUnit) -> HashMap<String, Value> {
    let mut p: HashMap<String, Value> = HashMap::new();
    p.insert("project".into(), f.project_id.clone().into());
    p.insert("unit".into(), "file".into());
    p.insert("path".into(), f.path.clone().into());
    p.insert("language".into(), f.language.clone().into());
    p.insert("content_hash".into(), f.content_hash.clone().into());
    p.insert("loc".into(), (f.loc as i64).into());
    p
}

fn logic_payload(l: &LogicUnit) -> HashMap<String, Value> {
    let mut p: HashMap<String, Value> = HashMap::new();
    p.insert("project".into(), l.project_id.clone().into());
    p.insert("unit".into(), "logic".into());
    p.insert("path".into(), l.path.clone().into());
    p.insert("symbol".into(), l.symbol.clone().into());
    p.insert("kind".into(), l.kind.as_str().into());
    p.insert("signature".into(), l.signature.clone().into());
    p.insert("start_line".into(), (l.start_line as i64).into());
    p.insert("end_line".into(), (l.end_line as i64).into());
    p.insert("normalized_hash".into(), l.normalized_hash.clone().into());
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_de_16_bytes_tiene_forma_canonica() {
        let s = uuid_string([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ]);
        assert_eq!(s, "01234567-89ab-cdef-0123-456789abcdef");
    }

    #[test]
    fn el_payload_de_logica_lleva_ruta_simbolo_y_rango() {
        let l = LogicUnit {
            id: NodeId::new(),
            project_id: "proj".into(),
            path: "src/a.rs".into(),
            symbol: "Foo::bar".into(),
            kind: super::super::unit::LogicKind::Method,
            signature: "fn bar(&self)".into(),
            start_line: 10,
            end_line: 20,
            normalized_hash: "abc".into(),
            semantic_text: None,
            calls: Vec::new(),
        };
        let p = logic_payload(&l);
        assert!(p.contains_key("path"));
        assert!(p.contains_key("symbol"));
        assert!(p.contains_key("start_line"));
        assert!(p.contains_key("project"));
    }
}
