use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::evidence::integrity::sha256_file;
use crate::evidence::manifest::{TraceManifest, WorkspaceChanges, WorkspaceEvidence};
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

pub fn execute(trace_root: PathBuf, trace_id: String, output: Option<PathBuf>) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let resolved = store.resolve_trace(&trace_id)?;
    let paths = resolved.paths;

    if !paths.root.is_dir() {
        bail!("trace directory does not exist: {}", paths.root.display());
    }

    let manifest = store.load_manifest_at(&paths)?;
    let report_path = output.unwrap_or_else(|| paths.root.join("report.md"));
    let report = build_report(&paths.root, &manifest, &paths)?;

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(&report_path, report)
        .with_context(|| format!("failed to write {}", report_path.display()))?;

    println!("Report written: {}", report_path.display());

    Ok(())
}

pub(crate) fn build_report(
    trace_path: &Path,
    manifest: &TraceManifest,
    paths: &crate::evidence::store::TracePaths,
) -> Result<String> {
    let mut report = String::new();

    push_line(&mut report, "# Trace Report".to_string());
    push_line(&mut report, String::new());
    push_line(
        &mut report,
        format!("Generated for trace `{}`.", manifest.trace_id),
    );

    section(&mut report, "Summary");
    push_kv(&mut report, "Trace ID", &manifest.trace_id);
    push_kv(&mut report, "Trace path", &trace_path.display().to_string());
    push_kv(&mut report, "Started", &manifest.started_at);
    push_kv(&mut report, "Finished", &manifest.finished_at);
    push_kv(
        &mut report,
        "Duration",
        &format!("{} ms", manifest.duration_ms),
    );
    push_kv(&mut report, "Tool kind", &manifest.tool_kind);

    section(&mut report, "Command");
    push_code_block(&mut report, "text", &manifest.command.join(" "));

    section(&mut report, "Exit status");
    push_kv(
        &mut report,
        "Exit code",
        &manifest
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unavailable".to_string()),
    );

    section(&mut report, "Duration");
    push_kv(
        &mut report,
        "Duration",
        &format!("{} ms", manifest.duration_ms),
    );
    if let Some(hint) = duration_hint(manifest.duration_ms) {
        push_kv(&mut report, "Rule", &hint);
    }

    section(&mut report, "Working directory");
    push_kv(&mut report, "cwd", &manifest.cwd);

    section(&mut report, "Git evidence");
    push_kv(
        &mut report,
        "Branch",
        &format!(
            "{} -> {}",
            display_opt(&manifest.git.branch_before),
            display_opt(&manifest.git.branch_after)
        ),
    );
    push_kv(
        &mut report,
        "Commit",
        &format!(
            "{} -> {}",
            display_opt(&manifest.git.commit_before),
            display_opt(&manifest.git.commit_after)
        ),
    );
    push_kv(
        &mut report,
        "Dirty",
        &format!(
            "{} -> {}",
            manifest.git.dirty_before, manifest.git.dirty_after
        ),
    );

    section(&mut report, "Workspace evidence");
    render_workspace_state(
        &mut report,
        "Dirty before",
        &manifest.workspace.dirty_before,
    );
    render_workspace_state(&mut report, "Dirty after", &manifest.workspace.dirty_after);
    render_changes(&mut report, &manifest.workspace.changes);

    section(&mut report, "Environment evidence");
    if manifest.env.is_empty() {
        push_line(
            &mut report,
            "- No allowlisted environment variables were captured.".to_string(),
        );
    } else {
        for entry in &manifest.env {
            push_line(
                &mut report,
                format!("- `{}` = `{}`", entry.key, entry.value),
            );
        }
    }

    section(&mut report, "Artifacts");
    render_artifact(
        &mut report,
        "stdout",
        &paths.stdout,
        Some(&manifest.artifacts.stdout),
    )?;
    render_artifact(
        &mut report,
        "stderr",
        &paths.stderr,
        Some(&manifest.artifacts.stderr),
    )?;
    if let Some(pty) = &manifest.artifacts.pty {
        let pty_path = paths.root.join(pty);
        render_artifact(&mut report, "pty", &pty_path, Some(pty))?;
    }

    section(&mut report, "Integrity");
    push_kv(
        &mut report,
        "stdout sha256",
        &manifest.integrity.stdout_sha256,
    );
    push_kv(
        &mut report,
        "stderr sha256",
        &manifest.integrity.stderr_sha256,
    );
    if let Some(pty_sha256) = &manifest.integrity.pty_sha256 {
        push_kv(&mut report, "pty sha256", pty_sha256);
    }

    match manifest_sha256_status(paths)? {
        Some(status) => {
            push_kv(&mut report, "manifest verification", &status);
        }
        None => {
            push_kv(&mut report, "manifest verification", "unavailable");
        }
    }

    section(&mut report, "Diagnosis hints");
    for hint in diagnosis_hints(manifest, paths)? {
        push_line(&mut report, format!("- {hint}"));
    }

    Ok(report)
}

fn diagnosis_hints(
    manifest: &TraceManifest,
    paths: &crate::evidence::store::TracePaths,
) -> Result<Vec<String>> {
    let mut hints = Vec::new();

    if let Some(exit_code) = manifest.exit_code {
        if exit_code != 0 {
            hints.push(format!("non-zero exit code: {exit_code}"));
        }
    }

    if let Some(stderr) = artifact_stats(&paths.stderr)? {
        if stderr.size_bytes > 0 {
            hints.push(format!(
                "stderr artifact exists and is non-empty: {} bytes",
                stderr.size_bytes
            ));
        }
    }

    if workspace_changed(&manifest.workspace) {
        hints.push(format!(
            "workspace changed during execution: {}",
            format_changes(&manifest.workspace.changes)
        ));
    }

    if manifest.git.dirty_before != manifest.git.dirty_after {
        hints.push(format!(
            "git dirty state changed during execution: {} -> {}",
            manifest.git.dirty_before, manifest.git.dirty_after
        ));
    }

    if !manifest.integrity.stdout_sha256.trim().is_empty() {
        hints.push(format!(
            "stdout hash exists: {}",
            manifest.integrity.stdout_sha256
        ));
    }

    if !manifest.integrity.stderr_sha256.trim().is_empty() {
        hints.push(format!(
            "stderr hash exists: {}",
            manifest.integrity.stderr_sha256
        ));
    }

    if let Some(hint) = duration_hint(manifest.duration_ms) {
        hints.push(hint);
    }

    if let Some(status) = manifest_sha256_status(paths)? {
        hints.push(format!("manifest verification status: {status}"));
    }

    Ok(hints)
}

struct ArtifactStats {
    size_bytes: u64,
}

fn artifact_stats(path: &Path) -> Result<Option<ArtifactStats>> {
    if !path.exists() {
        return Ok(None);
    }

    let size_bytes = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .len();

    Ok(Some(ArtifactStats { size_bytes }))
}

fn manifest_sha256_status(paths: &crate::evidence::store::TracePaths) -> Result<Option<String>> {
    if !paths.manifest_sha256.exists() {
        return Ok(None);
    }

    let expected = fs::read_to_string(&paths.manifest_sha256)
        .with_context(|| format!("failed to read {}", paths.manifest_sha256.display()))?
        .trim()
        .to_string();

    if expected.is_empty() {
        return Ok(Some("FAILED (empty manifest.sha256)".to_string()));
    }

    let actual = sha256_file(&paths.manifest)?;

    if actual == expected {
        Ok(Some("OK".to_string()))
    } else {
        Ok(Some(format!(
            "FAILED (expected {expected}, actual {actual})"
        )))
    }
}

fn workspace_changed(workspace: &WorkspaceEvidence) -> bool {
    !(workspace.changes.modified_files.is_empty()
        && workspace.changes.created_files.is_empty()
        && workspace.changes.deleted_files.is_empty())
}

fn format_changes(changes: &WorkspaceChanges) -> String {
    let mut parts = Vec::new();

    if !changes.modified_files.is_empty() {
        parts.push(format!("modified {}", changes.modified_files.len()));
    }

    if !changes.created_files.is_empty() {
        parts.push(format!("created {}", changes.created_files.len()));
    }

    if !changes.deleted_files.is_empty() {
        parts.push(format!("deleted {}", changes.deleted_files.len()));
    }

    if parts.is_empty() {
        "no attributed changes".to_string()
    } else {
        parts.join(", ")
    }
}

fn duration_hint(duration_ms: u128) -> Option<String> {
    if duration_ms <= 10 {
        Some(format!("duration suspiciously short: {} ms", duration_ms))
    } else if duration_ms >= 30 * 60 * 1000 {
        Some(format!("duration suspiciously long: {} ms", duration_ms))
    } else {
        None
    }
}

fn render_artifact(
    report: &mut String,
    label: &str,
    path: &Path,
    manifest_path: Option<&str>,
) -> Result<()> {
    push_kv(
        report,
        &format!("{label} path"),
        &path.display().to_string(),
    );

    if let Some(path_in_manifest) = manifest_path {
        push_kv(report, &format!("{label} manifest path"), path_in_manifest);
    }

    if path.exists() {
        let size = fs::metadata(path)
            .with_context(|| format!("failed to stat {}", path.display()))?
            .len();
        push_kv(report, &format!("{label} size"), &format!("{size} bytes"));
    } else {
        push_kv(report, &format!("{label} size"), "missing");
    }

    Ok(())
}

fn render_workspace_state(
    report: &mut String,
    label: &str,
    states: &[crate::evidence::manifest::WorkspaceFileState],
) {
    push_line(report, format!("- {label}:"));

    if states.is_empty() {
        push_line(report, "  - none".to_string());
        return;
    }

    for state in states {
        push_line(report, format!("  - `{}` ({})", state.path, state.status));
    }
}

fn render_changes(report: &mut String, changes: &WorkspaceChanges) {
    push_line(report, "- Changes:".to_string());

    if changes.modified_files.is_empty()
        && changes.created_files.is_empty()
        && changes.deleted_files.is_empty()
    {
        push_line(report, "  - none".to_string());
        return;
    }

    render_path_list(report, "modified", &changes.modified_files);
    render_path_list(report, "created", &changes.created_files);
    render_path_list(report, "deleted", &changes.deleted_files);
}

fn render_path_list(report: &mut String, label: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }

    push_line(report, format!("  - {label}:"));
    for value in values {
        push_line(report, format!("    - `{value}`"));
    }
}

fn push_code_block(report: &mut String, language: &str, body: &str) {
    push_line(report, format!("```{language}"));
    push_line(report, body.to_string());
    push_line(report, "```".to_string());
    push_line(report, String::new());
}

fn section(report: &mut String, title: &str) {
    push_line(report, String::new());
    push_line(report, format!("## {title}"));
}

fn push_kv(report: &mut String, key: &str, value: &str) {
    push_line(report, format!("- **{key}:** {value}"));
}

fn push_line(report: &mut String, line: String) {
    report.push_str(&line);
    report.push('\n');
}

fn display_opt(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("-")
}
