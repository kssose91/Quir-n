//! Unidades del índice de código: Archivo, Lógica y Cambio.
//!
//! Son la materia del índice descrito en la memoria del TFM (§4.2.2). Cada
//! unidad tiene identificador estable, hash y proyecto, y no atraviesa estados
//! de promoción: se escribe y se retira según el hash (§4.2.1). El código
//! completo no se copia; la unidad conserva ruta, rango y hash para recuperar el
//! original cuando haga falta.
//!
//! Las unidades son deterministas: las produce el indexador a partir del árbol
//! sintáctico. Los campos que exigen juicio —la responsabilidad de un archivo,
//! el texto semántico de una lógica— quedan vacíos aquí; los redacta la red
//! obrera, que propone y nunca ejecuta.

use crate::types::ids::NodeId;
use crate::types::node::{Node, NodeKind};
use crate::types::EventId;
use serde::{Deserialize, Serialize};

/// Identificador estable de una unidad, derivado de su identidad lógica
/// —proyecto, ruta y símbolo— y no de su contenido. Mover una función dentro
/// del archivo no cambia su identidad; renombrarla, sí.
///
/// Reutiliza `NodeId::from_content` (blake3) para que la unidad y su nodo en el
/// grafo compartan el mismo identificador, de modo que un resultado vectorial
/// pueda expandirse por el grafo y viceversa.
pub fn stable_id(project_id: &str, path: &str, symbol: &str) -> NodeId {
    let mut key = Vec::with_capacity(project_id.len() + path.len() + symbol.len() + 2);
    key.extend_from_slice(project_id.as_bytes());
    key.push(0);
    key.extend_from_slice(path.as_bytes());
    key.push(0);
    key.extend_from_slice(symbol.as_bytes());
    NodeId::from_content(&key)
}

/// Hash de contenido en hexadecimal (blake3).
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Clase de una unidad Lógica, según el nodo del árbol sintáctico que la origina.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
}

impl LogicKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogicKind::Function => "function",
            LogicKind::Method => "method",
            LogicKind::Struct => "struct",
            LogicKind::Enum => "enum",
            LogicKind::Trait => "trait",
            LogicKind::Impl => "impl",
        }
    }
}

/// Unidad **Archivo**: un fichero fuente del proyecto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUnit {
    /// Identificador estable (proyecto + ruta).
    pub id: NodeId,
    /// Proyecto al que pertenece. Ninguna unidad sin proyecto.
    pub project_id: String,
    /// Ruta relativa a la raíz del proyecto.
    pub path: String,
    /// Lenguaje detectado.
    pub language: String,
    /// Hash del contenido completo del archivo.
    pub content_hash: String,
    /// Símbolos principales de nivel superior.
    pub symbols: Vec<String>,
    /// Líneas de código.
    pub loc: usize,
    /// Responsabilidad principal (≤120 palabras). La redacta la red obrera.
    pub responsibility: Option<String>,
}

impl FileUnit {
    pub fn new(project_id: &str, path: &str, language: &str, source: &[u8]) -> Self {
        Self {
            id: stable_id(project_id, path, ""),
            project_id: project_id.to_string(),
            path: path.to_string(),
            language: language.to_string(),
            content_hash: content_hash(source),
            symbols: Vec::new(),
            loc: source.iter().filter(|&&b| b == b'\n').count() + 1,
            responsibility: None,
        }
    }

    /// Proyecta la unidad a un nodo del grafo interno. El nodo comparte el
    /// identificador estable de la unidad, de modo que reindexar sustituye en
    /// lugar de duplicar.
    pub fn to_node(&self, event_id: EventId) -> Node {
        let mut node = Node::new(NodeKind::FileUnit, self.path.clone(), event_id)
            .with_project(self.project_id.clone())
            .with_properties(serde_json::json!({
                "path": self.path,
                "language": self.language,
                "content_hash": self.content_hash,
                "symbols": self.symbols,
                "loc": self.loc,
                "responsibility": self.responsibility,
            }));
        node.id = self.id;
        if let Some(resp) = &self.responsibility {
            node.description = Some(resp.clone());
        }
        node
    }

    /// Texto que representa el archivo para la búsqueda semántica. Mientras la
    /// red obrera no redacte la responsabilidad, se usa ruta y símbolos.
    pub fn semantic_text(&self) -> String {
        match &self.responsibility {
            Some(r) => r.clone(),
            None => format!("{} — {}", self.path, self.symbols.join(", ")),
        }
    }
}

/// Unidad **Lógica**: una función, método, clase o bloque equivalente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicUnit {
    /// Identificador estable (proyecto + ruta + símbolo).
    pub id: NodeId,
    /// Proyecto al que pertenece. Ninguna unidad sin proyecto.
    pub project_id: String,
    /// Ruta relativa del archivo que la contiene.
    pub path: String,
    /// Símbolo: nombre de la función, método (`Tipo::método`), struct, etc.
    pub symbol: String,
    /// Clase de la lógica.
    pub kind: LogicKind,
    /// Firma (declaración, sin cuerpo).
    pub signature: String,
    /// Primera línea del rango (1-indexada).
    pub start_line: usize,
    /// Última línea del rango (1-indexada).
    pub end_line: usize,
    /// Hash normalizado del cuerpo, para detectar implementaciones equivalentes.
    pub normalized_hash: String,
    /// Texto semántico (≤180 palabras): propósito, entradas, salidas, efectos.
    /// Lo redacta la red obrera.
    pub semantic_text: Option<String>,
    /// Nombres a los que llama el cuerpo, tal como aparecen: `foo` (función
    /// libre o de módulo), `Tipo::metodo` (ruta con tipo) o `.metodo` (método
    /// sin receptor resuelto). Se resuelven contra las unidades del proyecto al
    /// proyectar el grafo; lo ambiguo no se enlaza.
    #[serde(default)]
    pub calls: Vec<String>,
}

impl LogicUnit {
    pub fn to_node(&self, event_id: EventId) -> Node {
        let mut node = Node::new(NodeKind::LogicUnit, self.symbol.clone(), event_id)
            .with_project(self.project_id.clone())
            .with_properties(serde_json::json!({
                "path": self.path,
                "symbol": self.symbol,
                "kind": self.kind.as_str(),
                "signature": self.signature,
                "start_line": self.start_line,
                "end_line": self.end_line,
                "normalized_hash": self.normalized_hash,
                "semantic_text": self.semantic_text,
            }));
        node.id = self.id;
        node.description = Some(self.semantic_text.clone().unwrap_or_else(|| self.signature.clone()));
        node
    }

    /// Texto para la búsqueda semántica. Sin ficha de la red obrera, la firma.
    pub fn semantic_text(&self) -> String {
        match &self.semantic_text {
            Some(t) => t.clone(),
            None => format!("{} {}\n{}", self.kind.as_str(), self.symbol, self.signature),
        }
    }
}

/// Motivo de un cambio, con fuente explícita. Nunca se deduce sin evidencia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source", content = "detail")]
pub enum ChangeReason {
    /// Tarea de usuario.
    UserTask(String),
    /// Commit.
    Commit(String),
    /// Sin evidencia disponible.
    Unknown,
}

/// Unidad **Cambio**: la transición de hash de un archivo o símbolo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeUnit {
    /// Identificador estable (proyecto + objetivo + hash nuevo).
    pub id: NodeId,
    /// Proyecto al que pertenece.
    pub project_id: String,
    /// Objetivo afectado: ruta, o `ruta#símbolo`.
    pub target: String,
    /// Hash anterior (ausente si la unidad es nueva).
    pub prev_hash: Option<String>,
    /// Hash posterior.
    pub new_hash: String,
    /// Motivo, con fuente explícita o `unknown`.
    pub reason: ChangeReason,
}

impl ChangeUnit {
    pub fn new(
        project_id: &str,
        target: &str,
        prev_hash: Option<String>,
        new_hash: String,
        reason: ChangeReason,
    ) -> Self {
        // El identificador incluye el hash nuevo: cada transición es una unidad
        // distinta, y reproyectar el mismo cambio es idempotente.
        let id = stable_id(project_id, target, &new_hash);
        Self {
            id,
            project_id: project_id.to_string(),
            target: target.to_string(),
            prev_hash,
            new_hash,
            reason,
        }
    }

    pub fn to_node(&self, event_id: EventId) -> Node {
        let mut node = Node::new(NodeKind::ChangeUnit, self.target.clone(), event_id)
            .with_project(self.project_id.clone())
            .with_properties(serde_json::json!({
                "target": self.target,
                "prev_hash": self.prev_hash,
                "new_hash": self.new_hash,
                "reason": self.reason,
            }));
        node.id = self.id;
        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identidad_estable_no_depende_del_contenido() {
        // La misma identidad lógica produce el mismo id, aunque cambie el hash.
        let a = stable_id("proj", "src/main.rs", "fn:run");
        let b = stable_id("proj", "src/main.rs", "fn:run");
        assert_eq!(a, b);
    }

    #[test]
    fn identidad_distingue_simbolo_ruta_y_proyecto() {
        let base = stable_id("proj", "src/main.rs", "fn:run");
        assert_ne!(base, stable_id("proj", "src/main.rs", "fn:stop"));
        assert_ne!(base, stable_id("proj", "src/lib.rs", "fn:run"));
        assert_ne!(base, stable_id("otro", "src/main.rs", "fn:run"));
    }

    #[test]
    fn file_unit_cuenta_lineas_y_hashea() {
        let f = FileUnit::new("proj", "src/a.rs", "rust", b"fn a() {}\nfn b() {}\n");
        assert_eq!(f.project_id, "proj");
        assert_eq!(f.loc, 3); // dos saltos + 1
        assert!(!f.content_hash.is_empty());
        assert_eq!(f.id, stable_id("proj", "src/a.rs", ""));
    }

    #[test]
    fn change_unit_es_idempotente_por_hash_nuevo() {
        let c1 = ChangeUnit::new("proj", "src/a.rs", None, "h2".into(), ChangeReason::Unknown);
        let c2 = ChangeUnit::new("proj", "src/a.rs", Some("h1".into()), "h2".into(), ChangeReason::Unknown);
        // Mismo objetivo y mismo hash nuevo -> misma unidad, aunque difiera el previo.
        assert_eq!(c1.id, c2.id);
    }
}
