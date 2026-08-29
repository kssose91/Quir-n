#![allow(dead_code)]
//! Experimental project map kept outside the canonical runtime path.
//! Project Map - Real project memory with file structure and symbols.
//!
//! This module provides deterministic project scanning for the Senior Supervisor Protocol.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A complete snapshot of a project's structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMap {
    /// Root path of the project.
    pub root: PathBuf,
    /// When the map was created.
    pub created_at: DateTime<Utc>,
    /// Blake3 hash of the tree structure.
    pub tree_hash: [u8; 32],
    /// All files in the project.
    pub files: HashMap<PathBuf, FileInfo>,
    /// Total file count.
    pub file_count: u32,
    /// Total size in bytes.
    pub total_size: u64,
}

/// Information about a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    /// Relative path from project root.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Blake3 hash of content.
    pub content_hash: [u8; 32],
    /// Last modification time.
    pub modified: DateTime<Utc>,
    /// Detected language/type.
    pub language: Option<String>,
    /// Symbols extracted from the file.
    pub symbols: Vec<Symbol>,
}

/// A symbol (function, class, struct, etc.) in a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Symbol name.
    pub name: String,
    /// Symbol kind.
    pub kind: SymbolKind,
    /// Line number where it starts.
    pub line_start: u32,
    /// Line number where it ends.
    pub line_end: u32,
    /// Parent symbol (for nested items).
    pub parent: Option<String>,
}

/// Type of symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum SymbolKind {
    Function = 0,
    Method = 1,
    Class = 2,
    Struct = 3,
    Enum = 4,
    Trait = 5,
    Interface = 6,
    Module = 7,
    Constant = 8,
    Variable = 9,
    Type = 10,
    Macro = 11,
}

/// Scope definition for modifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectScope {
    /// Files that can be modified.
    pub allowed_files: Vec<PathBuf>,
    /// Directories that can be modified (recursive).
    pub allowed_dirs: Vec<PathBuf>,
    /// Specific symbols that can be modified.
    pub allowed_symbols: Vec<String>,
    /// Files explicitly excluded.
    pub excluded_files: Vec<PathBuf>,
}

impl ProjectMap {
    /// Create a new project map by scanning a directory.
    pub fn scan(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut files = HashMap::new();
        let mut total_size = 0u64;
        let mut hasher = blake3::Hasher::new();

        Self::scan_dir(&root, &root, &mut files, &mut total_size, &mut hasher)?;

        let tree_hash: [u8; 32] = *hasher.finalize().as_bytes();

        Ok(Self {
            root,
            created_at: Utc::now(),
            tree_hash,
            file_count: files.len() as u32,
            files,
            total_size,
        })
    }

    fn scan_dir(
        root: &Path,
        dir: &Path,
        files: &mut HashMap<PathBuf, FileInfo>,
        total_size: &mut u64,
        hasher: &mut blake3::Hasher,
    ) -> std::io::Result<()> {
        // Skip hidden directories and common ignore patterns (but not root)
        if dir != root {
            let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('.') || dir_name == "target" || dir_name == "node_modules" {
                return Ok(());
            }
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()), // Skip unreadable directories
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                Self::scan_dir(root, &path, files, total_size, hasher)?;
            } else if path.is_file() {
                if let Ok(info) = Self::scan_file(root, &path) {
                    hasher.update(&info.content_hash);
                    *total_size += info.size;
                    files.insert(info.path.clone(), info);
                }
            }
        }

        Ok(())
    }

    fn scan_file(root: &Path, path: &Path) -> std::io::Result<FileInfo> {
        let content = std::fs::read(path)?;
        let metadata = std::fs::metadata(path)?;

        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let content_hash: [u8; 32] = *blake3::hash(&content).as_bytes();

        // Detect language from extension
        let language = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| match ext {
                "rs" => "rust",
                "py" => "python",
                "js" | "jsx" => "javascript",
                "ts" | "tsx" => "typescript",
                "go" => "go",
                "java" => "java",
                "c" | "h" => "c",
                "cpp" | "hpp" | "cc" => "cpp",
                "md" => "markdown",
                "toml" => "toml",
                "yaml" | "yml" => "yaml",
                "json" => "json",
                _ => ext,
            })
            .map(String::from);

        // Basic symbol extraction for Rust files
        let symbols = if language.as_deref() == Some("rust") {
            Self::extract_rust_symbols(&content)
        } else {
            Vec::new()
        };

        Ok(FileInfo {
            path: relative,
            size: metadata.len(),
            content_hash,
            modified: metadata
                .modified()
                .map(|t| DateTime::from(t))
                .unwrap_or_else(|_| Utc::now()),
            language,
            symbols,
        })
    }

    /// Basic Rust symbol extraction (functions, structs, enums, traits).
    fn extract_rust_symbols(content: &[u8]) -> Vec<Symbol> {
        let text = match std::str::from_utf8(content) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        let mut symbols = Vec::new();
        let mut current_block: Option<(String, SymbolKind, u32)> = None;
        let mut brace_depth = 0;

        for (line_num, line) in text.lines().enumerate() {
            let line_num = (line_num + 1) as u32;
            let trimmed = line.trim();

            // Track brace depth
            brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
            brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;

            // End current block
            if let Some((name, kind, start)) = current_block.take() {
                if brace_depth == 0 {
                    symbols.push(Symbol {
                        name,
                        kind,
                        line_start: start,
                        line_end: line_num,
                        parent: None,
                    });
                } else {
                    current_block = Some((name, kind, start));
                }
            }

            // Detect new symbols
            if let Some(name) = Self::extract_symbol_name(trimmed, "fn ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Function, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "pub fn ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Function, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "struct ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Struct, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "pub struct ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Struct, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "enum ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Enum, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "pub enum ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Enum, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "trait ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Trait, line_num));
                }
            } else if let Some(name) = Self::extract_symbol_name(trimmed, "pub trait ") {
                if current_block.is_none() {
                    current_block = Some((name, SymbolKind::Trait, line_num));
                }
            }
        }

        // Don't forget the last symbol if still open
        if let Some((name, kind, start)) = current_block {
            let last_line = text.lines().count() as u32;
            symbols.push(Symbol {
                name,
                kind,
                line_start: start,
                line_end: last_line,
                parent: None,
            });
        }

        symbols
    }

    fn extract_symbol_name(line: &str, prefix: &str) -> Option<String> {
        if line.starts_with(prefix) {
            let rest = &line[prefix.len()..];
            let name_end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            if name_end > 0 {
                return Some(rest[..name_end].to_string());
            }
        }
        None
    }

    /// Check if a path is within scope.
    pub fn is_in_scope(&self, path: &Path, scope: &ProjectScope) -> bool {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);

        // Check exclusions first
        for excluded in &scope.excluded_files {
            if relative == excluded {
                return false;
            }
        }

        // Check allowed files
        for allowed in &scope.allowed_files {
            if relative == allowed {
                return true;
            }
        }

        // Check allowed directories
        for allowed_dir in &scope.allowed_dirs {
            if relative.starts_with(allowed_dir) {
                return true;
            }
        }

        // If no explicit allows, deny
        scope.allowed_files.is_empty() && scope.allowed_dirs.is_empty()
    }
}

impl ProjectScope {
    /// Create a new scope allowing a single directory.
    pub fn directory(dir: impl AsRef<Path>) -> Self {
        Self {
            allowed_files: Vec::new(),
            allowed_dirs: vec![dir.as_ref().to_path_buf()],
            allowed_symbols: Vec::new(),
            excluded_files: Vec::new(),
        }
    }

    /// Create a scope for specific files.
    pub fn files(files: Vec<PathBuf>) -> Self {
        Self {
            allowed_files: files,
            allowed_dirs: Vec::new(),
            allowed_symbols: Vec::new(),
            excluded_files: Vec::new(),
        }
    }

    /// Add an exclusion.
    pub fn exclude(mut self, path: impl AsRef<Path>) -> Self {
        self.excluded_files.push(path.as_ref().to_path_buf());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_scan_project() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create some files
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() { }").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let map = ProjectMap::scan(root).unwrap();

        assert_eq!(map.file_count, 2);
        assert!(map.files.contains_key(&PathBuf::from("src/main.rs")));
        assert!(map.files.contains_key(&PathBuf::from("Cargo.toml")));
    }

    #[test]
    fn test_scope_check() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(root.join("README.md"), "").unwrap();

        let map = ProjectMap::scan(root).unwrap();
        let scope = ProjectScope::directory("src");

        assert!(map.is_in_scope(&PathBuf::from("src/lib.rs"), &scope));
        assert!(!map.is_in_scope(&PathBuf::from("README.md"), &scope));
    }

    #[test]
    fn test_rust_symbol_extraction() {
        let code = b"fn foo() { }\npub struct Bar { }\nenum Baz { A, B }";
        let symbols = ProjectMap::extract_rust_symbols(code);

        assert!(symbols.iter().any(|s| s.name == "foo"));
        assert!(symbols.iter().any(|s| s.name == "Bar"));
        assert!(symbols.iter().any(|s| s.name == "Baz"));
    }
}
