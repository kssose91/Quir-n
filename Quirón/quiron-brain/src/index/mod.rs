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

    /// Barre el proyecto `root` bajo el identificador `project_id`.
    ///
    /// `max_bytes` descarta ficheros anómalamente grandes. Todas las unidades
    /// llevan el proyecto; ninguna queda sin mundo.
    pub fn index_repo(
        &self,
        root: &Path,
        project_id: &str,
        max_bytes: u64,
    ) -> Result<IndexStats> {
        let files = walk::walk(root, max_bytes)?;
        // Un único evento identifica este barrido como origen de las unidades.
        let event_id = EventId::new();
        let mut stats = IndexStats::default();

        for wf in files {
            let Some((file_unit, logic_units)) =
                extract::extract(project_id, &wf.rel_path, &wf.content)
            else {
                stats.skipped += 1;
                continue;
            };

            self.graph.add_node(&file_unit.to_node(event_id))?;
            stats.files += 1;

            for lu in &logic_units {
                self.graph.add_node(&lu.to_node(event_id))?;
                // La lógica es parte de su archivo (definida en él).
                let edge = Edge::deterministic(EdgeKind::PartOf, lu.id, file_unit.id, event_id);
                self.graph.add_edge(&edge)?;
                stats.logic_units += 1;
            }
        }

        Ok(stats)
    }
}
