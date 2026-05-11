use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "tracebox")]
#[command(version)]
#[command(about = "Immutable execution evidence runtime")]
pub struct Cli {
    /// Directory where trace bundles are stored.
    ///
    /// The default is intentionally local to the current workspace. For CI or
    /// future agent-runtime integration, pass an explicit artifact directory.
    #[arg(long, global = true, default_value = ".traces")]
    pub trace_root: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Execute a command and capture immutable evidence.
    Run {
        /// Optional parent trace ID for retry/lineage graphs.
        #[arg(long)]
        parent: Option<String>,

        /// Logical tool kind. For standalone CLI runs, `process` is the default.
        ///
        /// Future embedded integrations can use values like:
        /// - shell
        /// - sandboxed_shell
        /// - apply_patch
        /// - mcp
        #[arg(long, default_value = "process")]
        tool_kind: String,

        /// Command argv to execute.
        ///
        /// Use `--` before the command:
        ///
        /// `tracebox run -- cargo test`
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// Inspect a trace manifest and optionally print log tails.
    Inspect {
        /// Trace ID, for example `trc_019...`.
        trace_id: String,

        /// Print stdout tail.
        #[arg(long)]
        stdout: bool,

        /// Print stderr tail.
        #[arg(long)]
        stderr: bool,

        /// Number of log lines to print when showing stdout/stderr.
        #[arg(long, default_value_t = 40)]
        tail: usize,

        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Verify trace artifact integrity.
    Verify {
        /// Trace ID, for example `trc_019...`.
        trace_id: String,

        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// List available traces.
    List {
        /// Emit JSON instead of a human table.
        #[arg(long)]
        json: bool,
    },

    /// Compare two traces.
    Diff {
        /// Left/base trace ID.
        left: String,

        /// Right/target trace ID.
        right: String,

        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}
