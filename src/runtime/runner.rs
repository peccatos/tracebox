use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::evidence::{
    EvidenceRecorder, ExecutionContext, FilesystemEvidenceRecorder, ToolStarted, TraceManifest,
    TraceStoreConfig,
};
use crate::runtime::process;

/// Run configuration for standalone Tracebox CLI execution.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub trace_root: PathBuf,
    pub parent_trace_id: Option<String>,
    pub tool_kind: String,
    pub command: Vec<String>,
}

/// Execute a command and return the completed manifest.
///
/// The runner owns CLI-level policy:
///
/// - current working directory;
/// - parent trace linkage;
/// - trace root selection;
/// - standalone tool kind.
pub fn run(config: RunConfig) -> Result<TraceManifest> {
    let cwd = std::env::current_dir()?;
    let recorder: Arc<dyn EvidenceRecorder> =
        FilesystemEvidenceRecorder::shared(TraceStoreConfig::new(config.trace_root));

    let event = ToolStarted {
        tool_kind: config.tool_kind,
        command: config.command,
        cwd,
        context: ExecutionContext {
            parent_trace_id: config.parent_trace_id,
            ..ExecutionContext::default()
        },
    };

    process::execute_command(recorder, event)
}
