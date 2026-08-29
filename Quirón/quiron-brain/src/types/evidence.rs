//! Evidence system for Senior Supervisor Protocol.
//!
//! Every claim must be backed by verifiable evidence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Reference to verifiable evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// Type of evidence.
    pub kind: EvidenceKind,
    /// Blake3 hash of the evidence content.
    pub hash: [u8; 32],
    /// Pointer to the actual evidence data.
    pub pointer: EvidencePointer,
    /// When the evidence was captured.
    pub ts: DateTime<Utc>,
}

/// Type of evidence that backs a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EvidenceKind {
    /// File content was read (path + lines + hash).
    FileSpan = 0,
    /// Command was executed (cmd + args + exit + stdout/stderr).
    ToolRun = 1,
    /// Artifact was generated (path + hash).
    Artifact = 2,
    /// Repository snapshot (commit + tree hash).
    RepoSnapshot = 3,
    /// Test/build/lint was executed with result.
    Verification = 4,
    /// Patch/diff was created.
    Patch = 5,
}

/// Pointer to the actual evidence data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidencePointer {
    /// File content that was read.
    FileSpan {
        path: String,
        start_line: u32,
        end_line: u32,
        content_hash: [u8; 32],
    },

    /// Command that was executed.
    ToolRun {
        cmd: String,
        args: Vec<String>,
        cwd: String,
        exit_code: i32,
        stdout_hash: [u8; 32],
        stderr_hash: [u8; 32],
        duration_ms: u64,
    },

    /// Artifact that was created.
    Artifact {
        path: String,
        size: u64,
        content_hash: [u8; 32],
    },

    /// Repository snapshot.
    RepoSnapshot {
        /// Git commit hash (if available).
        commit: Option<String>,
        /// Hash of file tree.
        tree_hash: String,
        /// Number of files in repo.
        file_count: u32,
    },

    /// Verification result (test/build/lint).
    Verification {
        /// Suite that was run (e.g., "cargo test", "pytest").
        suite: String,
        /// Whether all tests passed.
        passed: bool,
        /// Number of tests run.
        tests_run: u32,
        /// Number of tests failed.
        tests_failed: u32,
        /// Duration in milliseconds.
        duration_ms: u64,
        /// Output hash for reproducibility.
        output_hash: [u8; 32],
    },

    /// Patch/diff that was created or applied.
    Patch {
        /// Unified diff content hash.
        diff_hash: [u8; 32],
        /// Files affected.
        files_affected: Vec<String>,
        /// Lines added.
        lines_added: u32,
        /// Lines removed.
        lines_removed: u32,
    },
}

impl EvidenceRef {
    /// Create a new file span evidence.
    pub fn file_span(
        path: impl Into<String>,
        start_line: u32,
        end_line: u32,
        content_hash: [u8; 32],
    ) -> Self {
        let path = path.into();
        let pointer = EvidencePointer::FileSpan {
            path: path.clone(),
            start_line,
            end_line,
            content_hash,
        };
        Self {
            kind: EvidenceKind::FileSpan,
            hash: content_hash,
            pointer,
            ts: Utc::now(),
        }
    }

    /// Create a new tool run evidence.
    pub fn tool_run(
        cmd: impl Into<String>,
        args: Vec<String>,
        cwd: impl Into<String>,
        exit_code: i32,
        stdout: &[u8],
        stderr: &[u8],
        duration_ms: u64,
    ) -> Self {
        let stdout_hash: [u8; 32] = *blake3::hash(stdout).as_bytes();
        let stderr_hash: [u8; 32] = *blake3::hash(stderr).as_bytes();

        // Combined hash of the run
        let mut hasher = blake3::Hasher::new();
        hasher.update(&stdout_hash);
        hasher.update(&stderr_hash);
        hasher.update(&exit_code.to_le_bytes());
        let hash: [u8; 32] = *hasher.finalize().as_bytes();

        Self {
            kind: EvidenceKind::ToolRun,
            hash,
            pointer: EvidencePointer::ToolRun {
                cmd: cmd.into(),
                args,
                cwd: cwd.into(),
                exit_code,
                stdout_hash,
                stderr_hash,
                duration_ms,
            },
            ts: Utc::now(),
        }
    }

    /// Create a verification evidence.
    pub fn verification(
        suite: impl Into<String>,
        passed: bool,
        tests_run: u32,
        tests_failed: u32,
        duration_ms: u64,
        output: &[u8],
    ) -> Self {
        let output_hash: [u8; 32] = *blake3::hash(output).as_bytes();

        let mut hasher = blake3::Hasher::new();
        hasher.update(&[passed as u8]);
        hasher.update(&tests_run.to_le_bytes());
        hasher.update(&tests_failed.to_le_bytes());
        hasher.update(&output_hash);
        let hash: [u8; 32] = *hasher.finalize().as_bytes();

        Self {
            kind: EvidenceKind::Verification,
            hash,
            pointer: EvidencePointer::Verification {
                suite: suite.into(),
                passed,
                tests_run,
                tests_failed,
                duration_ms,
                output_hash,
            },
            ts: Utc::now(),
        }
    }

    /// Create a patch evidence.
    pub fn patch(
        diff_content: &[u8],
        files_affected: Vec<String>,
        lines_added: u32,
        lines_removed: u32,
    ) -> Self {
        let diff_hash: [u8; 32] = *blake3::hash(diff_content).as_bytes();

        Self {
            kind: EvidenceKind::Patch,
            hash: diff_hash,
            pointer: EvidencePointer::Patch {
                diff_hash,
                files_affected,
                lines_added,
                lines_removed,
            },
            ts: Utc::now(),
        }
    }

    /// Create a repo snapshot evidence.
    pub fn repo_snapshot(
        commit: Option<String>,
        tree_hash: impl Into<String>,
        file_count: u32,
    ) -> Self {
        let tree_hash_str = tree_hash.into();
        let hash: [u8; 32] = *blake3::hash(tree_hash_str.as_bytes()).as_bytes();

        Self {
            kind: EvidenceKind::RepoSnapshot,
            hash,
            pointer: EvidencePointer::RepoSnapshot {
                commit,
                tree_hash: tree_hash_str,
                file_count,
            },
            ts: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_span_evidence() {
        let content = b"fn main() { }";
        let hash: [u8; 32] = *blake3::hash(content).as_bytes();
        let evidence = EvidenceRef::file_span("src/main.rs", 1, 10, hash);

        assert_eq!(evidence.kind, EvidenceKind::FileSpan);
        assert_eq!(evidence.hash, hash);
    }

    #[test]
    fn test_tool_run_evidence() {
        let stdout = b"test result: ok. 12 passed";
        let stderr = b"";
        let evidence =
            EvidenceRef::tool_run("cargo", vec!["test".into()], ".", 0, stdout, stderr, 1500);

        assert_eq!(evidence.kind, EvidenceKind::ToolRun);
        if let EvidencePointer::ToolRun { exit_code, .. } = evidence.pointer {
            assert_eq!(exit_code, 0);
        } else {
            panic!("Wrong pointer type");
        }
    }

    #[test]
    fn test_verification_evidence() {
        let output = b"Running 12 tests\nAll passed";
        let evidence = EvidenceRef::verification("cargo test", true, 12, 0, 2000, output);

        assert_eq!(evidence.kind, EvidenceKind::Verification);
        if let EvidencePointer::Verification {
            passed, tests_run, ..
        } = evidence.pointer
        {
            assert!(passed);
            assert_eq!(tests_run, 12);
        } else {
            panic!("Wrong pointer type");
        }
    }

    #[test]
    fn test_patch_evidence() {
        let diff = b"--- a/file.rs\n+++ b/file.rs\n@@ -1 +1,2 @@\n old\n+new";
        let evidence = EvidenceRef::patch(diff, vec!["file.rs".into()], 1, 0);

        assert_eq!(evidence.kind, EvidenceKind::Patch);
        if let EvidencePointer::Patch { lines_added, .. } = evidence.pointer {
            assert_eq!(lines_added, 1);
        } else {
            panic!("Wrong pointer type");
        }
    }
}
