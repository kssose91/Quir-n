//! Artifact types for file tracking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata about an artifact (file).
///
/// # Identity
/// The `id` field provides stable identity across renames and content changes.
/// - `id`: Stable ULID assigned when first seen (identity)
/// - `hash`: Content hash (integrity)
/// - `path`: Current location (may change on rename)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    /// Stable identity (survives renames and content changes)
    pub id: ulid::Ulid,

    /// Blake3 hash of the content (64 hex chars).
    /// Changes when content changes.
    pub hash: String,

    /// Normalized canonical path to the artifact.
    /// Should be absolute and cleaned of `.` and `..`.
    pub path: String,

    /// Size in bytes.
    pub size: u64,

    /// When first seen by Quirón.
    pub first_seen: DateTime<Utc>,

    /// When last modified on disk (from fs mtime, not "now").
    pub last_modified: DateTime<Utc>,

    /// MIME type (optional).
    pub mime_type: Option<String>,

    /// Number of times accessed.
    pub access_count: u32,
}

impl ArtifactMeta {
    /// Create new artifact metadata (for newly created files).
    /// Use `from_fs` for existing files on disk.
    pub fn new(hash: impl Into<String>, path: impl Into<String>, size: u64) -> Self {
        let now = Utc::now();
        Self {
            id: ulid::Ulid::new(),
            hash: hash.into(),
            path: normalize_path(&path.into()),
            size,
            first_seen: now,
            last_modified: now,
            mime_type: None,
            access_count: 0,
        }
    }

    /// Create artifact metadata from existing file on disk.
    /// Uses actual fs mtime for last_modified instead of "now".
    pub fn from_fs(
        hash: impl Into<String>,
        path: impl Into<String>,
        size: u64,
        last_modified: DateTime<Utc>,
    ) -> Self {
        Self {
            id: ulid::Ulid::new(),
            hash: hash.into(),
            path: normalize_path(&path.into()),
            size,
            first_seen: Utc::now(),
            last_modified,
            mime_type: None,
            access_count: 0,
        }
    }

    /// Create from path, auto-detecting hash, size, and mtime.
    pub fn from_path(path: &std::path::Path) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        let size = metadata.len();
        let last_modified = metadata
            .modified()
            .map(|t| DateTime::<Utc>::from(t))
            .unwrap_or_else(|_| Utc::now());
        let hash = hash_file(path)?;
        let path_str = path.to_string_lossy().to_string();

        Ok(Self::from_fs(hash, path_str, size, last_modified))
    }

    /// Validate hash format (must be 64 hex chars for blake3).
    pub fn is_valid_hash(&self) -> bool {
        self.hash.len() == 64 && self.hash.chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// Normalize a path: convert to absolute if possible, clean separators.
pub fn normalize_path(path: &str) -> String {
    // Normalize Windows backslashes
    let cleaned = path.replace('\\', "/");

    // Try to canonicalize (makes absolute + resolves symlinks)
    if let Ok(canonical) = std::fs::canonicalize(&cleaned) {
        return canonical.to_string_lossy().to_string();
    }

    // Fallback: just clean the path
    cleaned
}

/// Compute Blake3 hash of content.
pub fn hash_content(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

/// Compute Blake3 hash of a file (streaming, memory efficient).
/// Uses 64KB buffer to handle large files without loading into RAM.
pub fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024]; // 64KB buffer

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_hash_file_streaming() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let hash = hash_file(file.path()).unwrap();
        assert_eq!(hash.len(), 64); // blake3 hex is 64 chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_from_path() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"test content").unwrap();
        file.flush().unwrap();

        let artifact = ArtifactMeta::from_path(file.path()).unwrap();
        assert!(artifact.is_valid_hash());
        assert_eq!(artifact.size, 12);
        assert!(artifact.id.to_string().len() > 0);
    }

    #[test]
    fn test_normalize_path() {
        // Test backslash normalization (path that doesn't exist won't canonicalize)
        let fake_path = "nonexistent\\dir\\file.rs";
        let normalized = normalize_path(fake_path);
        assert_eq!(normalized, "nonexistent/dir/file.rs");
        assert!(!normalized.contains('\\'));
    }
}
