use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::evidence::env;
use crate::evidence::event::{ToolFinished, ToolStarted};
use crate::evidence::ids::generate_trace_id;
use crate::evidence::integrity::sha256_file;
use crate::evidence::manifest::{
    ArtifactIntegrity, ArtifactPaths, TraceManifest, MANIFEST_VERSION,
};
use crate::evidence::output::OutputStream;
use crate::evidence::store::{FilesystemTraceStore, TracePaths, TraceStoreConfig};
use crate::evidence::workspace::{
    build_git_evidence, build_workspace_evidence, capture_workspace_snapshot, WorkspaceSnapshot,
};

/// Handle returned when a trace starts.
///
/// Callers keep this handle and pass it back for output and finish events. This
/// prevents the runtime from depending on filesystem paths or recorder internals.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolTraceHandle {
    pub trace_id: String,
}

/// Recorder trait used by both the standalone runner and future embedded
/// integrations.
///
/// The trait is synchronous by design for v0.1:
///
/// - no async runtime requirement;
/// - simple call sites;
/// - easy embedding into thread-based process readers;
/// - future buffered/async implementations can live behind the same trait.
pub trait EvidenceRecorder: Send + Sync {
    fn tool_started(&self, event: ToolStarted) -> Result<ToolTraceHandle>;

    fn tool_output(
        &self,
        handle: &ToolTraceHandle,
        stream: OutputStream,
        bytes: &[u8],
    ) -> Result<()>;

    fn tool_finished(&self, handle: ToolTraceHandle, event: ToolFinished) -> Result<TraceManifest>;
}

/// Filesystem-backed evidence recorder.
///
/// Each in-flight execution owns:
///
/// - a trace directory;
/// - open stdout/stderr/pty files;
/// - a before workspace snapshot;
/// - a start timestamp;
/// - immutable start metadata.
pub struct FilesystemEvidenceRecorder {
    store: FilesystemTraceStore,
    state: Mutex<HashMap<String, InProgressTrace>>,
}

struct InProgressTrace {
    event: ToolStarted,
    paths: TracePaths,
    stdout: File,
    stderr: File,
    pty: File,
    saw_pty_output: bool,
    started_at: chrono::DateTime<Utc>,
    before: WorkspaceSnapshot,
    workspace_ignored_path_prefixes: Vec<String>,
}

impl FilesystemEvidenceRecorder {
    pub fn new(config: TraceStoreConfig) -> Self {
        Self {
            store: FilesystemTraceStore::new(config),
            state: Mutex::new(HashMap::new()),
        }
    }

    pub fn shared(config: TraceStoreConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    pub fn store(&self) -> &FilesystemTraceStore {
        &self.store
    }
}

impl EvidenceRecorder for FilesystemEvidenceRecorder {
    fn tool_started(&self, event: ToolStarted) -> Result<ToolTraceHandle> {
        let trace_id = generate_trace_id();

        let workspace_ignored_path_prefixes =
            workspace_ignored_path_prefixes(&event.cwd, self.store.root());

        // Snapshot must happen before trace directory creation. Otherwise `.traces`
        // itself makes a clean git workspace look dirty.
        let before = capture_workspace_snapshot(&event.cwd, &workspace_ignored_path_prefixes);

        let paths = self.store.create_trace_dir(&trace_id)?;

        let stdout = FilesystemTraceStore::create_artifact(&paths.stdout)?;
        let stderr = FilesystemTraceStore::create_artifact(&paths.stderr)?;
        let pty = FilesystemTraceStore::create_artifact(&paths.pty)?;

        let started_at = Utc::now();

        let in_progress = InProgressTrace {
            event,
            paths,
            stdout,
            stderr,
            pty,
            saw_pty_output: false,
            started_at,
            before,
            workspace_ignored_path_prefixes,
        };

        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("failed to acquire evidence recorder lock"))?;

        state.insert(trace_id.clone(), in_progress);

        Ok(ToolTraceHandle { trace_id })
    }

    fn tool_output(
        &self,
        handle: &ToolTraceHandle,
        stream: OutputStream,
        bytes: &[u8],
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("failed to acquire evidence recorder lock"))?;

        let trace = state
            .get_mut(&handle.trace_id)
            .ok_or_else(|| anyhow!("unknown trace handle {}", handle.trace_id))?;

        let file = match stream {
            OutputStream::Stdout => &mut trace.stdout,
            OutputStream::Stderr => &mut trace.stderr,
            OutputStream::Pty => {
                trace.saw_pty_output = true;
                &mut trace.pty
            }
        };

        file.write_all(bytes)
            .with_context(|| format!("failed to write {:?} evidence", stream))
    }

    fn tool_finished(
        &self,
        handle: ToolTraceHandle,
        finished: ToolFinished,
    ) -> Result<TraceManifest> {
        let trace = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("failed to acquire evidence recorder lock"))?;

            state
                .remove(&handle.trace_id)
                .ok_or_else(|| anyhow!("unknown trace handle {}", handle.trace_id))?
        };

        finish_trace(&self.store, handle.trace_id, trace, finished)
    }
}

fn finish_trace(
    store: &FilesystemTraceStore,
    trace_id: String,
    mut trace: InProgressTrace,
    finished: ToolFinished,
) -> Result<TraceManifest> {
    trace
        .stdout
        .flush()
        .with_context(|| format!("failed to flush {}", trace.paths.stdout.display()))?;

    trace
        .stderr
        .flush()
        .with_context(|| format!("failed to flush {}", trace.paths.stderr.display()))?;

    trace
        .pty
        .flush()
        .with_context(|| format!("failed to flush {}", trace.paths.pty.display()))?;

    let saw_pty_output = trace.saw_pty_output;

    // Close files before hashing them.
    drop(trace.stdout);
    drop(trace.stderr);
    drop(trace.pty);

    let finished_at = Utc::now();
    let after =
        capture_workspace_snapshot(&trace.event.cwd, &trace.workspace_ignored_path_prefixes);

    let stdout_sha256 = sha256_file(&trace.paths.stdout)?;
    let stderr_sha256 = sha256_file(&trace.paths.stderr)?;
    let pty_sha256 = if saw_pty_output {
        Some(sha256_file(&trace.paths.pty)?)
    } else {
        None
    };

    let duration_ms = finished_at
        .signed_duration_since(trace.started_at)
        .num_milliseconds()
        .max(0) as u128;

    let manifest = TraceManifest {
        manifest_version: MANIFEST_VERSION,
        trace_id,
        context: trace.event.context,
        tool_kind: trace.event.tool_kind,
        command: trace.event.command,
        cwd: trace.event.cwd.display().to_string(),
        started_at: trace.started_at.to_rfc3339(),
        finished_at: finished_at.to_rfc3339(),
        duration_ms,
        exit_code: finished.exit_code,
        artifacts: ArtifactPaths {
            stdout: "stdout.log".to_string(),
            stderr: "stderr.log".to_string(),
            pty: saw_pty_output.then(|| "pty.log".to_string()),
        },
        integrity: ArtifactIntegrity {
            stdout_sha256,
            stderr_sha256,
            pty_sha256,
        },
        git: build_git_evidence(&trace.before, &after),
        workspace: build_workspace_evidence(&trace.before, &after),
        env: env::collect_env(),
    };

    store.write_manifest_and_hash(&trace.paths, &manifest)?;

    Ok(manifest)
}

fn workspace_ignored_path_prefixes(
    cwd: &std::path::Path,
    trace_root: &std::path::Path,
) -> Vec<String> {
    let absolute_trace_root = if trace_root.is_absolute() {
        trace_root.to_path_buf()
    } else {
        cwd.join(trace_root)
    };

    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    let trace_root = absolute_trace_root
        .canonicalize()
        .unwrap_or(absolute_trace_root);

    let Ok(relative) = trace_root.strip_prefix(&cwd) else {
        return Vec::new();
    };

    let relative = relative.to_string_lossy().replace('\\', "/");

    if relative.is_empty() {
        Vec::new()
    } else {
        vec![relative]
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::evidence::event::{ExecutionContext, ToolFinished, ToolStarted};
    use crate::evidence::output::OutputStream;
    use crate::evidence::recorder::{EvidenceRecorder, FilesystemEvidenceRecorder};
    use crate::evidence::store::TraceStoreConfig;

    #[test]
    fn records_stdout_stderr_and_manifest() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        let traces_root = temp.path().join(".traces");
        let recorder = FilesystemEvidenceRecorder::new(TraceStoreConfig::new(&traces_root));

        let handle = recorder.tool_started(ToolStarted {
            tool_kind: "process".to_string(),
            command: vec!["echo".to_string(), "hello".to_string()],
            cwd: temp.path().to_path_buf(),
            context: ExecutionContext::default(),
        })?;

        recorder.tool_output(&handle, OutputStream::Stdout, b"hello\n")?;
        recorder.tool_output(&handle, OutputStream::Stderr, b"warning\n")?;

        let manifest =
            recorder.tool_finished(handle.clone(), ToolFinished { exit_code: Some(0) })?;

        let trace_root = traces_root.join(&handle.trace_id);
        let stdout = fs::read_to_string(trace_root.join("stdout.log"))?;
        let stderr = fs::read_to_string(trace_root.join("stderr.log"))?;
        let manifest_json = fs::read_to_string(trace_root.join("manifest.json"))?;

        assert_eq!(stdout, "hello\n");
        assert_eq!(stderr, "warning\n");
        assert!(manifest_json.contains("manifest_version"));
        assert_eq!(manifest.exit_code, Some(0));
        assert!(trace_root.join("manifest.sha256").exists());

        Ok(())
    }
}
