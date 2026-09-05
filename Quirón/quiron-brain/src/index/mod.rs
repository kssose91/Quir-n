//! El índice de código: una de las dos proyecciones del registro (memoria §4.2).
//!
//! Tres tipos de unidad —Archivo, Lógica y Cambio— con identificador estable,
//! hash y proyecto. El indexador es determinista e incremental: detecta el
//! cambio por hash, extrae las unidades afectadas del árbol sintáctico y
//! sustituye las fichas anteriores. La red obrera es la última pieza; este
//! módulo funciona sin una sola red entrenada.

pub mod extract;
pub mod unit;
pub mod walk;

#[cfg(feature = "semantic")]
pub mod vectorize;

#[cfg(feature = "semantic")]
pub mod worker;

pub use unit::{ChangeReason, ChangeUnit, FileUnit, LogicKind, LogicUnit};

use crate::graph::GraphBuilder;
use crate::storage::Storage;
use crate::types::edge::{Edge, EdgeKind};
use crate::types::EventId;
use crate::Result;
use std::path::Path;

/// Cuentas de un barrido, para verificar y para dejar traza.
#[derive(Debug, Default, Clone, Copy)]
pub struct IndexStats {
    /// Ficheros convertidos en unidad Archivo.
    pub files: usize,
    /// Unidades Lógica extraídas.
    pub logic_units: usize,
    /// Ficheros recorridos pero no analizables (lenguaje no soportado).
    pub skipped: usize,
}

/// Las unidades de un barrido, antes de persistir. La extracción es
/// determinista; la persistencia al grafo y la vectorización son pasos
/// separados que consumen esto.
#[derive(Debug, Default)]
pub struct CollectedUnits {
    pub files: Vec<FileUnit>,
    pub logic: Vec<LogicUnit>,
    pub skipped: usize,
}

impl CollectedUnits {
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            files: self.files.len(),
            logic_units: self.logic.len(),
            skipped: self.skipped,
        }
    }
}

/// El indexador: proyecta el árbol sintáctico del proyecto al grafo interno.
///
/// Escribe unidades Archivo y Lógica como nodos con identificador estable, y
/// una arista `PartOf` de cada Lógica a su Archivo. Es idempotente: reindexar
/// sustituye las fichas anteriores en lugar de duplicarlas. Determinista: no
/// interviene ningún modelo.
pub struct Indexer {
    graph: GraphBuilder,
}

impl Indexer {
    pub fn new(storage: Storage) -> Self {
        Self { graph: GraphBuilder::new(storage) }
    }

    /// Recoge las unidades del proyecto sin persistir. Paso determinista puro:
    /// walk + extract. No toca ningún almacén.
    pub fn collect_units(
        &self,
        root: &Path,
        project_id: &str,
        max_bytes: u64,
    ) -> Result<CollectedUnits> {
        let files = walk::walk(root, max_bytes)?;
        let mut out = CollectedUnits::default();
        for wf in files {
            match extract::extract(project_id, &wf.rel_path, &wf.content) {
                Some((file_unit, mut logic_units)) => {
                    out.files.push(file_unit);
                    out.logic.append(&mut logic_units);
                }
                None => out.skipped += 1,
            }
        }
        Ok(out)
    }

    /// Persiste las unidades al grafo interno: cada unidad es un nodo con
    /// identificador estable, y cada Lógica cuelga de su Archivo por `PartOf`.
    /// Idempotente: reindexar sustituye en lugar de duplicar.
    pub fn persist_to_graph(&self, units: &CollectedUnits) -> Result<()> {
        // Un único evento identifica este barrido como origen de las unidades.
        let event_id = EventId::new();
        for f in &units.files {
            self.graph.add_node(&f.to_node(event_id))?;
        }
        for l in &units.logic {
            self.graph.add_node(&l.to_node(event_id))?;
            // La lógica es parte de su archivo (definida en él). El id del
            // archivo se deriva de su identidad, sin necesidad de emparejar.
            let file_id = unit::stable_id(&l.project_id, &l.path, "");
            let edge = Edge::deterministic(EdgeKind::PartOf, l.id, file_id, event_id);
            self.graph.add_edge(&edge)?;
        }
        Ok(())
    }

    /// Barre el proyecto `root` bajo el identificador `project_id` y lo persiste
    /// al grafo. `max_bytes` descarta ficheros anómalamente grandes. Todas las
    /// unidades llevan el proyecto; ninguna queda sin mundo.
    pub fn index_repo(
        &self,
        root: &Path,
        project_id: &str,
        max_bytes: u64,
    ) -> Result<IndexStats> {
        let units = self.collect_units(root, project_id, max_bytes)?;
        self.persist_to_graph(&units)?;
        Ok(units.stats())
    }
}
