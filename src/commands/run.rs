use std::path::PathBuf;

use anyhow::Result;

use crate::runtime::runner::{self, RunConfig};

pub fn execute(
    trace_root: PathBuf,
    parent_trace_id: Option<String>,
    tool_kind: String,
    command: Vec<String>,
) -> Result<i32> {
    let manifest = runner::run(RunConfig {
        trace_root: trace_root.clone(),
        parent_trace_id,
        tool_kind,
        command,
    })?;

    println!("Trace created: {}", manifest.trace_id);
    println!(
        "Trace path: {}",
        trace_root.join(&manifest.trace_id).display()
    );

    // If the child was terminated by signal or failed to spawn, we use `1`.
    // This keeps wrapper behavior predictable while preserving details in the
    // trace manifest and stderr artifact.
    Ok(manifest.exit_code.unwrap_or(1))
}
