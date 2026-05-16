use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::evidence::manifest::TraceManifest;
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

#[derive(Debug, Serialize)]
struct ListedTrace {
    trace_id: String,
    archived: bool,
    started_at: String,
    exit_code: Option<i32>,
    duration_ms: u128,
    command: String,
}

pub fn execute(trace_root: PathBuf, json: bool, archived: bool, all: bool) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let mut traces = Vec::new();

    let include_active = !archived || all;
    let include_archived = archived || all;

    if include_active {
        collect_traces(&trace_root, false, &mut traces)?;
    }

    if include_archived {
        let archive_root = store.archive_root();
        collect_traces(&archive_root, true, &mut traces)?;
    }

    traces.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then(a.archived.cmp(&b.archived))
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&traces)?);
        return Ok(());
    }

    if traces.is_empty() {
        println!(
            "{}",
            empty_message(&store, include_active, include_archived)
        );
        return Ok(());
    }

    println!(
        "{:<42} {:<10} {:<25} {:<8} {:<10} COMMAND",
        "TRACE ID", "STATUS", "STARTED", "EXIT", "DURATION"
    );

    for trace in traces {
        let exit = trace
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string());
        let status = if trace.archived { "ARCHIVED" } else { "ACTIVE" };

        println!(
            "{:<42} {:<10} {:<25} {:<8} {:<10} {}",
            trace.trace_id,
            status,
            trace.started_at,
            exit,
            format!("{}ms", trace.duration_ms),
            trace.command
        );
    }

    Ok(())
}

fn collect_traces(base_root: &Path, archived: bool, traces: &mut Vec<ListedTrace>) -> Result<()> {
    if !base_root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(base_root)
        .with_context(|| format!("failed to read trace root {}", base_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        if is_reserved_trace_name(entry.file_name().to_str().unwrap_or_default()) {
            continue;
        }

        if !is_trace_bundle_dir(&path) {
            continue;
        }

        let trace_id = entry.file_name().to_string_lossy().to_string();

        let Ok(manifest) = load_manifest(&path) else {
            // A partially written or foreign directory should not make listing
            // unusable. Inspection can report detailed errors for a specific ID.
            continue;
        };

        traces.push(ListedTrace {
            trace_id,
            archived,
            started_at: manifest.started_at,
            exit_code: manifest.exit_code,
            duration_ms: manifest.duration_ms,
            command: manifest.command.join(" "),
        });
    }

    Ok(())
}

fn load_manifest(trace_path: &Path) -> Result<TraceManifest> {
    let manifest_path = trace_path.join("manifest.json");
    let json = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;

    let manifest = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    Ok(manifest)
}

fn is_reserved_trace_name(name: &str) -> bool {
    matches!(name, "archive" | "reports" | "tmp" | "index.json")
}

fn is_trace_bundle_dir(path: &Path) -> bool {
    path.is_dir() && path.join("manifest.json").is_file()
}

fn empty_message(
    store: &FilesystemTraceStore,
    include_active: bool,
    include_archived: bool,
) -> String {
    match (include_active, include_archived) {
        (true, true) => format!("No traces found at {}", store.root().display()),
        (true, false) => format!("No active traces found at {}", store.root().display()),
        (false, true) => format!(
            "No archived traces found at {}",
            store.archive_root().display()
        ),
        (false, false) => format!("No traces found at {}", store.root().display()),
    }
}
