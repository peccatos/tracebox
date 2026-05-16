use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::evidence::browser::{load_trace_catalog, TraceSummary};

pub fn execute(trace_root: PathBuf, json: bool, archived: bool, all: bool) -> Result<()> {
    let catalog = load_trace_catalog(&trace_root)?;
    let traces = match (archived, all) {
        (true, true) | (false, true) => catalog
            .active
            .iter()
            .chain(catalog.archived.iter())
            .collect::<Vec<_>>(),
        (true, false) => catalog.archived.iter().collect::<Vec<_>>(),
        (false, false) => catalog.active.iter().collect::<Vec<_>>(),
    };

    let mut traces = traces;
    traces.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then(a.archived.cmp(&b.archived))
            .then(a.trace_id.cmp(&b.trace_id))
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&traces)?);
        return Ok(());
    }

    if traces.is_empty() {
        println!("{}", empty_message(&trace_root, archived, all));
        return Ok(());
    }

    println!(
        "{:<42} {:<10} {:<25} {:<8} {:<10} COMMAND",
        "TRACE ID", "STATUS", "STARTED", "EXIT", "DURATION"
    );

    for trace in traces {
        print_trace_row(trace);
    }

    Ok(())
}

fn print_trace_row(trace: &TraceSummary) {
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

fn empty_message(trace_root: &Path, archived: bool, all: bool) -> String {
    match (archived, all) {
        (true, true) | (false, true) => format!("No traces found at {}", trace_root.display()),
        (true, false) => format!("No archived traces found at {}", trace_root.display()),
        (false, false) => format!("No active traces found at {}", trace_root.display()),
    }
}
