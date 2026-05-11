use serde::{Deserialize, Serialize};

/// Output stream identifier for process/tool output.
///
/// PTY is modeled explicitly even though the standalone v0.1 runner does not
/// yet allocate a PTY. This prevents a future integration from incorrectly
/// mixing terminal byte streams into stdout/stderr artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
    Pty,
}
