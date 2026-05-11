use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

pub fn execute(
    trace_root: PathBuf,
    trace_id: String,
    show_stdout: bool,
    show_stderr: bool,
    tail: usize,
) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let paths = store.paths_for(&trace_id);
    let manifest = store.load_manifest(&trace_id)?;

    println!("Trace ID: {}", manifest.trace_id);

    if let Some(parent) = &manifest.context.parent_trace_id {
        println!("Parent Trace ID: {parent}");
    }

    println!("Tool Kind: {}", manifest.tool_kind);
    println!("Exit Code: {}", format_exit_code(manifest.exit_code));
    println!("Duration: {}ms", manifest.duration_ms);
    println!("Started: {}", manifest.started_at);
    println!("Finished: {}", manifest.finished_at);
    println!("CWD: {}", manifest.cwd);

    println!();
    println!("Command:");
    println!("  {}", manifest.command.join(" "));

    println!();
    println!("Git:");
    println!(
        "  branch: {} -> {}",
        display_opt(&manifest.git.branch_before),
        display_opt(&manifest.git.branch_after)
    );
    println!(
        "  commit: {} -> {}",
        display_opt(&manifest.git.commit_before),
        display_opt(&manifest.git.commit_after)
    );
    println!(
        "  dirty: {} -> {}",
        manifest.git.dirty_before, manifest.git.dirty_after
    );

    println!();
    println!("Workspace changes:");
    print_file_list("  created", &manifest.workspace.changes.created_files);
    print_file_list("  modified", &manifest.workspace.changes.modified_files);
    print_file_list("  deleted", &manifest.workspace.changes.deleted_files);

    println!();
    println!("Artifacts:");
    println!(
        "  stdout: {} ({})",
        paths.stdout.display(),
        artifact_summary(&paths.stdout)?
    );
    println!(
        "  stderr: {} ({})",
        paths.stderr.display(),
        artifact_summary(&paths.stderr)?
    );

    if let Some(pty) = &manifest.artifacts.pty {
        println!(
            "  pty: {} ({})",
            paths.root.join(pty).display(),
            artifact_summary(&paths.root.join(pty))?
        );
    }

    println!();
    println!("Integrity:");
    println!("  stdout_sha256: {}", manifest.integrity.stdout_sha256);
    println!("  stderr_sha256: {}", manifest.integrity.stderr_sha256);

    if let Some(pty_hash) = &manifest.integrity.pty_sha256 {
        println!("  pty_sha256: {pty_hash}");
    }

    if paths.manifest_sha256.exists() {
        let hash = fs::read_to_string(&paths.manifest_sha256)
            .with_context(|| format!("failed to read {}", paths.manifest_sha256.display()))?;
        println!("  manifest_sha256: {}", hash.trim());
    }

    let should_show_stderr_by_default =
        !show_stdout && !show_stderr && file_nonempty(&paths.stderr);

    if show_stdout {
        print_tail("stdout", &paths.stdout, tail)?;
    }

    if show_stderr || should_show_stderr_by_default {
        print_tail("stderr", &paths.stderr, tail)?;
    }

    Ok(())
}

fn display_opt(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("-")
}

fn format_exit_code(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn print_file_list(label: &str, files: &[String]) {
    if files.is_empty() {
        println!("{label}: -");
    } else {
        println!("{label}:");

        for file in files {
            println!("    - {file}");
        }
    }
}

fn artifact_summary(path: &Path) -> Result<String> {
    let bytes = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .len();

    Ok(format!("{bytes} bytes"))
}

fn file_nonempty(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
}

fn print_tail(label: &str, path: &Path, lines: usize) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read {} artifact {}", label, path.display()))?;

    let collected = text.lines().collect::<Vec<_>>();
    let start = collected.len().saturating_sub(lines);

    println!();
    println!("{label} tail (last {lines} lines):");

    if collected.is_empty() {
        println!("  <empty>");
        return Ok(());
    }

    for line in &collected[start..] {
        println!("{line}");
    }

    Ok(())
}
