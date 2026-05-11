use serde::{Deserialize, Serialize};

use crate::evidence::event::ExecutionContext;

/// Current manifest schema version.
///
/// Evidence schemas become hard to migrate once traces exist on developer
/// machines and CI artifacts. Version from the first commit.
pub const MANIFEST_VERSION: u32 = 1;

/// Immutable trace manifest written once at execution completion.
///
/// This is the evidence contract. Treat it as append-only data, not mutable
/// runtime state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceManifest {
    pub manifest_version: u32,
    pub trace_id: String,

    /// External runtime context, if any.
    ///
    /// The standalone CLI mainly uses `parent_trace_id`; future embedded
    /// integrations can populate thread/turn/item/tool IDs.
    pub context: ExecutionContext,

    pub tool_kind: String,
    pub command: Vec<String>,
    pub cwd: String,

    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u128,

    pub exit_code: Option<i32>,

    pub artifacts: ArtifactPaths,
    pub integrity: ArtifactIntegrity,

    pub git: GitEvidence,
    pub workspace: WorkspaceEvidence,

    /// Allowlisted environment variables only.
    ///
    /// This is a vector rather than a map so serialization order is stable.
    pub env: Vec<EnvVar>,
}

/// Relative artifact paths inside the trace directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactPaths {
    pub stdout: String,
    pub stderr: String,

    /// Optional because normal process execution has separate stdout/stderr.
    /// PTY-backed integrations should write terminal bytes to `pty.log`.
    pub pty: Option<String>,
}

/// Content integrity metadata for written artifacts.
///
/// The manifest hash itself is written as `manifest.sha256` sidecar to avoid
/// recursive self-hashing inside `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactIntegrity {
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub pty_sha256: Option<String>,
}

/// Best-effort git evidence.
///
/// Git data must never be required. Tracebox must work in temp dirs, generated
/// workspaces, extracted archives, and non-git directories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitEvidence {
    pub commit_before: Option<String>,
    pub commit_after: Option<String>,
    pub branch_before: Option<String>,
    pub branch_after: Option<String>,
    pub dirty_before: bool,
    pub dirty_after: bool,
}

/// Workspace evidence derived from before/after snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEvidence {
    /// Dirty files before execution.
    pub dirty_before: Vec<WorkspaceFileState>,

    /// Dirty files after execution.
    pub dirty_after: Vec<WorkspaceFileState>,

    /// Conservative mutation attribution derived from before/after states.
    pub changes: WorkspaceChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFileState {
    pub path: String,
    pub status: String,
}

/// Conservative workspace mutation summary.
///
/// Important limitation:
/// If a file was already dirty before execution and remains dirty with the same
/// coarse status after execution, v0.1 does not claim the tool changed it. That
/// would require content hashing before and after.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceChanges {
    pub modified_files: Vec<String>,
    pub created_files: Vec<String>,
    pub deleted_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}
