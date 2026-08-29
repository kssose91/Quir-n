//! Gestión de backlinks

use chrono::{DateTime, Utc};
use std::path::PathBuf;

pub use super::evidence::{Backlink, BacklinkStatus, BacklinkType};

/// Gestor de backlinks
pub struct BacklinkManager {
    /// Cache de verificaciones
    cache: std::collections::HashMap<String, CachedVerification>,

    /// TTL del cache
    cache_ttl: std::time::Duration,

    /// Cliente HTTP reutilizable con timeout
    http: reqwest::Client,
}

struct CachedVerification {
    status: BacklinkStatus,
    /// Para calcular TTL del cache
    verified_at_instant: std::time::Instant,
    /// Para actualizar last_verified en el backlink
    verified_at_utc: DateTime<Utc>,
}

impl BacklinkManager {
    pub fn new() -> Self {
        Self {
            cache: std::collections::HashMap::new(),
            cache_ttl: std::time::Duration::from_secs(300), // 5 min
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Crear con TTL personalizado
    pub fn with_ttl(cache_ttl: std::time::Duration) -> Self {
        Self {
            cache: std::collections::HashMap::new(),
            cache_ttl,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Verificar un backlink y ACTUALIZAR su estado
    ///
    /// IMPORTANTE: Esta función muta el backlink para actualizar:
    /// - backlink.status
    /// - backlink.last_verified
    pub async fn verify(&mut self, backlink: &mut Backlink) -> BacklinkStatus {
        // Check cache
        if let Some(cached) = self.cache.get(&backlink.uri) {
            if cached.verified_at_instant.elapsed() < self.cache_ttl {
                // Actualizar backlink con valor cacheado (incluyendo last_verified!)
                backlink.status = cached.status;
                backlink.last_verified = cached.verified_at_utc;
                return cached.status;
            }
        }

        let status = self.do_verify(backlink).await;
        let now_utc = Utc::now();

        // Actualizar el backlink
        backlink.status = status;
        backlink.last_verified = now_utc;

        // Cache result
        self.cache.insert(
            backlink.uri.clone(),
            CachedVerification {
                status,
                verified_at_instant: std::time::Instant::now(),
                verified_at_utc: now_utc,
            },
        );

        status
    }

    async fn do_verify(&self, backlink: &Backlink) -> BacklinkStatus {
        match &backlink.link_type {
            BacklinkType::LocalFile { path } => {
                // Usar tokio::fs para async
                match tokio::fs::metadata(path).await {
                    Ok(_) => BacklinkStatus::Valid,
                    Err(_) => BacklinkStatus::Broken,
                }
            }
            BacklinkType::FileLine { path, line } => {
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let line_count = content.lines().count();
                        // CONVENCIÓN: line es 1-indexed (L1 = primera línea)
                        // Válido si: 1 <= line <= line_count
                        if *line >= 1 && *line <= line_count {
                            BacklinkStatus::Valid
                        } else {
                            BacklinkStatus::Stale
                        }
                    }
                    Err(_) => BacklinkStatus::Broken,
                }
            }
            BacklinkType::FileRange { path, start, end } => {
                // Validar rango primero
                if *start < 1 || start > end {
                    return BacklinkStatus::Broken; // Rango inválido
                }

                match tokio::fs::read_to_string(path).await {
                    Ok(content) => {
                        let line_count = content.lines().count();
                        // CONVENCIÓN: start/end son 1-indexed (L1 = primera línea), end inclusive
                        // Válido si: 1 <= start <= end <= line_count
                        if *start >= 1 && *start <= *end && *end <= line_count {
                            BacklinkStatus::Valid
                        } else {
                            BacklinkStatus::Stale
                        }
                    }
                    Err(_) => BacklinkStatus::Broken,
                }
            }
            BacklinkType::LedgerEvent { .. } => {
                // TODO: Verificar en el ledger real
                BacklinkStatus::Valid
            }
            BacklinkType::ExternalUrl { url } => {
                // Usa cliente con timeout
                match self.http.get(url).send().await {
                    Ok(resp) if resp.status().is_success() => BacklinkStatus::Valid,
                    Ok(resp) if resp.status().as_u16() == 404 => BacklinkStatus::Broken,
                    Ok(_) => BacklinkStatus::Stale, // Otros errores HTTP
                    Err(_) => BacklinkStatus::Unverified, // Error de red
                }
            }
            BacklinkType::GraphNode { .. } => {
                // TODO: Verificar en el grafo real
                BacklinkStatus::Valid
            }
        }
    }

    /// Verificar múltiples backlinks (secuencial por simplicidad)
    pub async fn verify_batch(&mut self, backlinks: &mut [Backlink]) -> Vec<BacklinkStatus> {
        let mut results = Vec::with_capacity(backlinks.len());

        for backlink in backlinks.iter_mut() {
            results.push(self.verify(backlink).await);
        }

        results
    }

    /// Limpiar cache expirado
    pub fn cleanup_cache(&mut self) {
        self.cache
            .retain(|_, v| v.verified_at_instant.elapsed() < self.cache_ttl);
    }
}

impl Default for BacklinkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Crear backlink desde path
pub fn backlink_from_path(path: PathBuf) -> Backlink {
    Backlink {
        uri: format!("file://{}", path.display()),
        link_type: BacklinkType::LocalFile { path },
        status: BacklinkStatus::Unverified,
        last_verified: Utc::now(),
    }
}

/// Crear backlink desde path + líneas (1-indexed: L1 = primera línea, end inclusive)
///
/// Ejemplo: `backlink_from_range(path, 10, 15)` cubre líneas 10, 11, 12, 13, 14, 15
pub fn backlink_from_range(
    path: PathBuf,
    start: usize,
    end: usize,
) -> Result<Backlink, &'static str> {
    if start < 1 {
        return Err("start debe ser >= 1 (1-indexed)");
    }
    if start > end {
        return Err("start debe ser <= end");
    }

    Ok(Backlink {
        uri: format!("file://{}#L{}-L{}", path.display(), start, end),
        link_type: BacklinkType::FileRange { path, start, end },
        status: BacklinkStatus::Unverified,
        last_verified: Utc::now(),
    })
}

/// Crear backlink desde path + línea (1-indexed: L1 = primera línea)
pub fn backlink_from_line(path: PathBuf, line: usize) -> Result<Backlink, &'static str> {
    if line < 1 {
        return Err("line debe ser >= 1 (1-indexed)");
    }

    Ok(Backlink {
        uri: format!("file://{}#L{}", path.display(), line),
        link_type: BacklinkType::FileLine { path, line },
        status: BacklinkStatus::Unverified,
        last_verified: Utc::now(),
    })
}
