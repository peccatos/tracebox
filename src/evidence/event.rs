use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Optional references to an external runtime's execution entities.
///
/// In the standalone CLI, most of these fields are empty. They exist because
/// Tracebox is meant to be embeddable later into runtimes such as agent systems,
/// CI executors, or `codex-rs`.
///
/// Ownership rule:
///
/// - the host runtime owns session/thread/tool IDs;
/// - Tracebox owns evidence IDs;
/// - the manifest links both worlds without making either storage model depend
///   on the other.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub parent_trace_id: Option<String>,
}

/// Tool/process execution start event.
///
/// This is the boundary where evidence capture starts. For correctness, callers
/// should emit this before spawning the process or invoking the side-effecting
/// tool so the workspace `before` snapshot is honest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStarted {
    pub tool_kind: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub context: ExecutionContext,
}

/// Tool/process execution finish event.
///
/// `exit_code = None` is allowed for spawn failures, signal termination, or
/// non-process tools that do not naturally produce a POSIX exit status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFinished {
    pub exit_code: Option<i32>,
}
