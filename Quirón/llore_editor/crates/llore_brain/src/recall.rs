//! # Recall
//!
//! Sistema de búsqueda en contexto virtual.
//! "Si no sé, busco" - Virtual Context Tools.
//!
//! > **NOTA**: La búsqueda de memoria se hace via quiron-brain API,
//! > no con una memoria local.

use std::path::PathBuf;

/// Sistema de recall (Virtual Context Tools).
pub struct Recall {
    /// Ruta base del agente.
    agent_path: PathBuf,
}

impl Recall {
    /// Crear nuevo sistema de recall.
    pub fn new(agent_path: PathBuf) -> Self {
        Self { agent_path }
    }

    /// Buscar en archivos del proyecto.
    pub fn search_files(&self, query: &str, project_path: &PathBuf) -> Vec<SearchResult> {
        let mut results = Vec::new();

        // Búsqueda simple por contenido
        if let Ok(entries) = std::fs::read_dir(project_path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if content.contains(query) {
                            results.push(SearchResult {
                                path: path.to_string_lossy().to_string(),
                                snippet: extract_snippet(&content, query),
                                score: 1.0,
                            });
                        }
                    }
                }
            }
        }

        results
    }

    /// Buscar en el knowledge base de Quirón.
    pub fn search_knowledge(&self, query: &str) -> Vec<SearchResult> {
        let knowledge_path = self
            .agent_path
            .parent()
            .map(|p| p.join("antigravity").join("knowledge"))
            .unwrap_or_default();

        self.search_files(query, &knowledge_path)
    }

    // NOTA: search_memory se ha eliminado.
    // La memoria se consulta vía client.recall() → quiron-brain:8766/recall
    // (con fallback legado a /search cuando el backend no soporta /recall).
}

/// Resultado de búsqueda.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Ruta del archivo.
    pub path: String,
    /// Fragmento relevante.
    pub snippet: String,
    /// Score de relevancia (0.0 - 1.0).
    pub score: f32,
}

/// Extraer un fragmento del contenido que contiene la query.
fn extract_snippet(content: &str, query: &str) -> String {
    if let Some(pos) = content.find(query) {
        let start = pos.saturating_sub(50);
        let end = (pos + query.len() + 50).min(content.len());

        // Encontrar límites de línea
        let snippet = &content[start..end];
        format!("...{}...", snippet.trim())
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_snippet() {
        let content = "Esta es una línea de prueba con algún contenido importante aquí.";
        let snippet = extract_snippet(content, "contenido");
        assert!(snippet.contains("contenido"));
    }
}
