use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

#[derive(Debug, Serialize)]
struct ListedTrace {
    trace_id: String,
    started_at: String,
    exit_code: Option<i32>,
    duration_ms: u128,
    command: String,
}

pub fn execute(trace_root: PathBuf, json: bool) -> Result<()> {
    if !trace_root.exists() {
        if json {
            println!("[]");
        } else {
            println!("No traces found at {}", trace_root.display());
        }

        return Ok(());
    }

    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let mut traces = Vec::new();

    for entry in fs::read_dir(&trace_root)
        .with_context(|| format!("failed to read trace root {}", trace_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let trace_id = entry.file_name().to_string_lossy().to_string();

        let Ok(manifest) = store.load_manifest(&trace_id) else {
            // A partially written or foreign directory should not make listing
            // unusable. Inspection can report detailed errors for a specific ID.
            continue;
        };

        traces.push(ListedTrace {
            trace_id,
            started_at: manifest.started_at,
            exit_code: manifest.exit_code,
            duration_ms: manifest.duration_ms,
            command: manifest.command.join(" "),
        });
    }

    traces.sort_by(|a, b| a.started_at.cmp(&b.started_at));

    if json {
        println!("{}", serde_json::to_string_pretty(&traces)?);
        return Ok(());
    }

    if traces.is_empty() {
        println!("No traces found at {}", trace_root.display());
        return Ok(());
    }

    println!(
        "{:<42} {:<25} {:<8} {:<10} COMMAND",
        "TRACE ID", "STARTED", "EXIT", "DURATION"
    );

    for trace in traces {
        let exit = trace
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<42} {:<25} {:<8} {:<10} {}",
            trace.trace_id,
            trace.started_at,
            exit,
            format!("{}ms", trace.duration_ms),
            trace.command
        );
    }

    Ok(())
}
