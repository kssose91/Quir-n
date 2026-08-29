//! Tipos y manejo de evidencia

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Pieza de evidencia que soporta un claim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// ID único
    pub id: String,

    /// Fuente de la evidencia
    pub source: EvidenceSource,

    /// Extracto ORIGINAL que sustentó el claim cuando se creó
    /// INMUTABLE después de la creación (para audit trail)
    pub original_excerpt: String,

    /// Extracto ACTUAL (última verificación exitosa)
    /// Se actualiza si similarity >= threshold
    pub current_excerpt: String,

    /// Relevancia para el claim (0.0 - 1.0)
    /// INVARIANTE: debe cumplir 0.0 <= relevance <= 1.0
    pub relevance: f32,

    /// Fuerza de la evidencia
    pub strength: EvidenceStrength,

    /// Backlink verificable
    pub backlink: Backlink,

    /// Cuándo se verificó por última vez
    pub verified_at: DateTime<Utc>,

    /// Hash del contenido ORIGINAL (para comparación rápida)
    pub original_hash: String,

    /// Hash del contenido ACTUAL
    pub current_hash: String,

    /// Cuánto ha divergido del original (0.0 - 1.0)
    /// 1.0 = idéntico, <0.7 = Stale
    pub drift: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidenceSource {
    /// Código fuente
    /// NOTA: start_line y end_line son 1-indexed (L1 = primera línea)
    CodeFile {
        path: PathBuf,
        start_line: usize,
        end_line: usize,
        language: String,
    },

    /// Evento en el ledger
    LedgerEvent {
        event_id: String,
        event_kind: String,
    },

    /// Documentación
    Documentation {
        path: PathBuf,
        section: Option<String>,
    },

    /// Resultado de test
    TestResult {
        test_name: String,
        test_path: PathBuf,
        passed: bool,
        output: Option<String>,
    },

    /// Afirmación del usuario
    UserStatement {
        conversation_id: String,
        timestamp: DateTime<Utc>,
        message_id: String,
    },

    /// Resultado de análisis AST
    AstAnalysis {
        path: PathBuf,
        node_type: String,
        node_path: String,
    },

    /// Estado del sistema
    SystemState {
        component: String,
        property: String,
        value: String,
        timestamp: DateTime<Utc>,
    },

    /// Output de comando
    CommandOutput {
        command: String,
        output: String,
        exit_code: i32,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceStrength {
    /// Evidencia directa e irrefutable
    Strong,

    /// Evidencia buena pero con margen de duda
    Moderate,

    /// Evidencia indirecta o circunstancial
    Weak,

    /// Solo indicios
    Circumstantial,
}

impl EvidenceStrength {
    pub fn to_confidence_modifier(&self) -> f32 {
        match self {
            EvidenceStrength::Strong => 1.0,
            EvidenceStrength::Moderate => 0.8,
            EvidenceStrength::Weak => 0.5,
            EvidenceStrength::Circumstantial => 0.3,
        }
    }
}

/// Backlink para verificación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backlink {
    /// URI verificable
    pub uri: String,

    /// Tipo de backlink
    pub link_type: BacklinkType,

    /// Estado de verificación
    pub status: BacklinkStatus,

    /// Última verificación
    pub last_verified: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BacklinkType {
    /// Archivo local
    LocalFile { path: PathBuf },

    /// Línea específica en archivo (1-indexed: L1 = primera línea)
    FileLine { path: PathBuf, line: usize },

    /// Rango de líneas (1-indexed, inclusive: L1-L10 = líneas 1 a 10)
    FileRange {
        path: PathBuf,
        start: usize,
        end: usize,
    },

    /// Evento en ledger
    LedgerEvent { event_id: String },

    /// URL externa
    ExternalUrl { url: String },

    /// Nodo en grafo
    GraphNode { node_id: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BacklinkStatus {
    /// Verificado y válido
    Valid,

    /// Contenido cambió desde verificación
    Stale,

    /// Link roto (archivo no existe, etc)
    Broken,

    /// No verificado todavía
    Unverified,
}

impl Evidence {
    /// Crear nueva evidencia desde código
    ///
    /// IMPORTANTE: start_line y end_line son 1-indexed (L1 = primera línea)
    pub fn from_code(
        path: PathBuf,
        start_line: usize,
        end_line: usize,
        excerpt: String,
        language: &str,
    ) -> Self {
        debug_assert!(start_line >= 1, "start_line debe ser >= 1 (1-indexed)");
        debug_assert!(start_line <= end_line, "start_line debe ser <= end_line");

        let content_hash = Self::compute_hash(&excerpt);
        let backlink = Backlink {
            uri: Self::path_to_uri(&path, Some((start_line, end_line))),
            link_type: BacklinkType::FileRange {
                path: path.clone(),
                start: start_line,
                end: end_line,
            },
            // No asumimos Valid - debe verificarse primero
            status: BacklinkStatus::Unverified,
            last_verified: Utc::now(),
        };

        Self {
            id: ulid::Ulid::new().to_string(),
            source: EvidenceSource::CodeFile {
                path,
                start_line,
                end_line,
                language: language.to_string(),
            },
            original_excerpt: excerpt.clone(),
            current_excerpt: excerpt,
            relevance: 1.0,
            strength: EvidenceStrength::Strong,
            backlink,
            verified_at: Utc::now(),
            original_hash: content_hash.clone(),
            current_hash: content_hash,
            drift: 1.0, // Recién creado, sin divergencia
        }
    }

    /// Crear evidencia desde evento del ledger
    pub fn from_ledger_event(event_id: &str, event_kind: &str, excerpt: String) -> Self {
        let content_hash = Self::compute_hash(&excerpt);
        let backlink = Backlink {
            uri: format!("ledger://{}", event_id),
            link_type: BacklinkType::LedgerEvent {
                event_id: event_id.to_string(),
            },
            // Ledger events son inmutables, así que Valid es correcto
            status: BacklinkStatus::Valid,
            last_verified: Utc::now(),
        };

        Self {
            id: ulid::Ulid::new().to_string(),
            source: EvidenceSource::LedgerEvent {
                event_id: event_id.to_string(),
                event_kind: event_kind.to_string(),
            },
            original_excerpt: excerpt.clone(),
            current_excerpt: excerpt,
            relevance: 1.0,
            strength: EvidenceStrength::Strong,
            backlink,
            verified_at: Utc::now(),
            original_hash: content_hash.clone(),
            current_hash: content_hash,
            drift: 1.0, // Ledger es inmutable, drift siempre 1.0
        }
    }

    /// Verificar si el backlink sigue siendo válido
    pub async fn verify_backlink(&mut self) -> Result<bool, std::io::Error> {
        let now = Utc::now();

        match &self.backlink.link_type {
            BacklinkType::LocalFile { path } => {
                // Usar async para no bloquear
                match tokio::fs::metadata(path).await {
                    Ok(_) => {
                        self.backlink.status = BacklinkStatus::Valid;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(true)
                    }
                    Err(_) => {
                        self.backlink.status = BacklinkStatus::Broken;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(false)
                    }
                }
            }
            BacklinkType::FileLine { path, line } => {
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let line_count = content.lines().count();
                        // line es 1-indexed: válido si 1 <= line <= line_count
                        if *line >= 1 && *line <= line_count {
                            self.backlink.status = BacklinkStatus::Valid;
                            self.backlink.last_verified = now;
                            self.verified_at = now;
                            Ok(true)
                        } else {
                            self.backlink.status = BacklinkStatus::Stale;
                            self.backlink.last_verified = now;
                            self.verified_at = now;
                            Ok(false)
                        }
                    }
                    Err(_) => {
                        self.backlink.status = BacklinkStatus::Broken;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(false)
                    }
                }
            }
            BacklinkType::FileRange { path, start, end } => {
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let line_count = lines.len();

                        // start/end son 1-indexed: válido si 1 <= start <= end <= line_count
                        if *start < 1 || *start > *end || *end > line_count {
                            self.backlink.status = BacklinkStatus::Stale;
                            self.backlink.last_verified = now;
                            self.verified_at = now;
                            self.drift = 0.0; // Rango inválido = máxima divergencia
                            return Ok(false);
                        }

                        // Obtener excerpt actual (convertir 1-indexed a 0-indexed para slice)
                        let current_excerpt_new: String = lines
                            .get((*start - 1)..*end)
                            .map(|slice| slice.join("\n"))
                            .unwrap_or_default();

                        let current_hash_new = Self::compute_hash(&current_excerpt_new);

                        // SIEMPRE actualizar current_excerpt y current_hash
                        self.current_excerpt = current_excerpt_new.clone();
                        self.current_hash = current_hash_new;

                        // Calcular drift respecto al ORIGINAL (no al current anterior)
                        self.drift = Self::similarity(&self.original_excerpt, &current_excerpt_new);

                        if self.drift < 0.7 {
                            // Demasiada divergencia del original
                            self.backlink.status = BacklinkStatus::Stale;
                            self.backlink.last_verified = now;
                            self.verified_at = now;
                            return Ok(false);
                        }

                        // Drift >= 0.7: contenido suficientemente similar al original
                        self.backlink.status = BacklinkStatus::Valid;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(true)
                    }
                    Err(_) => {
                        self.backlink.status = BacklinkStatus::Broken;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(false)
                    }
                }
            }
            BacklinkType::LedgerEvent { .. } => {
                // Ledger events son inmutables
                self.backlink.status = BacklinkStatus::Valid;
                self.backlink.last_verified = now;
                self.verified_at = now;
                Ok(true)
            }
            BacklinkType::ExternalUrl { url } => {
                // Cliente con timeout para evitar bloqueos
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                match client.get(url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        self.backlink.status = BacklinkStatus::Valid;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(true)
                    }
                    Ok(resp) if resp.status().as_u16() == 404 => {
                        self.backlink.status = BacklinkStatus::Broken;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(false)
                    }
                    Ok(_) => {
                        self.backlink.status = BacklinkStatus::Stale;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Ok(false)
                    }
                    Err(e) => {
                        // Error de red: no podemos concluir Broken/Stale con certeza
                        self.backlink.status = BacklinkStatus::Unverified;
                        self.backlink.last_verified = now;
                        self.verified_at = now;
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Network error verifying URL: {}", e),
                        ))
                    }
                }
            }
            BacklinkType::GraphNode { .. } => {
                // TODO: Verificar en grafo real
                self.backlink.status = BacklinkStatus::Valid;
                self.backlink.last_verified = now;
                self.verified_at = now;
                Ok(true)
            }
        }
    }

    /// Calcular hash del contenido usando blake3 (determinista y rápido)
    fn compute_hash(content: &str) -> String {
        blake3::hash(content.as_bytes()).to_hex().to_string()
    }

    /// Calcular similaridad entre dos textos (Jaccard por palabras)
    fn similarity(a: &str, b: &str) -> f32 {
        let a_words: std::collections::HashSet<&str> = a.split_whitespace().collect();
        let b_words: std::collections::HashSet<&str> = b.split_whitespace().collect();

        if a_words.is_empty() && b_words.is_empty() {
            return 1.0;
        }

        let intersection = a_words.intersection(&b_words).count();
        let union = a_words.union(&b_words).count();

        intersection as f32 / union as f32
    }

    /// Validar que relevance está en rango correcto (limpia NaN/Inf)
    pub fn with_relevance(mut self, relevance: f32) -> Self {
        self.relevance = if relevance.is_finite() {
            relevance.clamp(0.0, 1.0)
        } else {
            0.0 // NaN/Inf -> 0.0 como fallback seguro
        };
        self
    }

    /// Convertir path a URI con percent-encoding básico
    fn path_to_uri(path: &std::path::Path, line_range: Option<(usize, usize)>) -> String {
        // Percent-encoding manual para caracteres problemáticos
        let path_str = path.display().to_string();
        let encoded: String = path_str
            .chars()
            .map(|c| match c {
                ' ' => "%20".to_string(),
                '#' => "%23".to_string(),
                '?' => "%3F".to_string(),
                '%' => "%25".to_string(),
                _ => c.to_string(),
            })
            .collect();

        // Evitar "file:////" en paths absolutos Unix (queremos "file:///")
        let base = if encoded.starts_with('/') {
            format!("file://{}", encoded) // encoded ya tiene el / inicial
        } else {
            format!("file://{}", encoded)
        };

        match line_range {
            Some((start, end)) => format!("{}#L{}-L{}", base, start, end),
            None => base,
        }
    }
}
