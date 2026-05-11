use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Value};

use crate::evidence::manifest::TraceManifest;
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

pub fn execute(trace_root: PathBuf, left: String, right: String, json_output: bool) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));

    let left_manifest = store.load_manifest(&left)?;
    let right_manifest = store.load_manifest(&right)?;

    if json_output {
        let output = json_diff(&left_manifest, &right_manifest);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("Trace diff");
    println!("==========");
    println!("left: {}", left_manifest.trace_id);
    println!("right: {}", right_manifest.trace_id);

    println!();

    print_field_diff(
        "exit_code",
        &format_exit(left_manifest.exit_code),
        &format_exit(right_manifest.exit_code),
    );

    print_field_diff(
        "duration_ms",
        &left_manifest.duration_ms.to_string(),
        &right_manifest.duration_ms.to_string(),
    );

    print_field_diff(
        "command",
        &left_manifest.command.join(" "),
        &right_manifest.command.join(" "),
    );

    print_field_diff("cwd", &left_manifest.cwd, &right_manifest.cwd);

    println!();
    println!("Git:");

    print_field_diff(
        "  branch",
        &display_opt(&left_manifest.git.branch_after),
        &display_opt(&right_manifest.git.branch_after),
    );

    print_field_diff(
        "  commit",
        &display_opt(&left_manifest.git.commit_after),
        &display_opt(&right_manifest.git.commit_after),
    );

    print_field_diff(
        "  dirty",
        &left_manifest.git.dirty_after.to_string(),
        &right_manifest.git.dirty_after.to_string(),
    );

    println!();
    println!("Workspace changes:");

    diff_file_set(
        "created",
        &left_manifest.workspace.changes.created_files,
        &right_manifest.workspace.changes.created_files,
    );

    diff_file_set(
        "modified",
        &left_manifest.workspace.changes.modified_files,
        &right_manifest.workspace.changes.modified_files,
    );

    diff_file_set(
        "deleted",
        &left_manifest.workspace.changes.deleted_files,
        &right_manifest.workspace.changes.deleted_files,
    );

    println!();
    println!("Artifacts:");

    print_field_diff(
        "  stdout_sha256",
        &left_manifest.integrity.stdout_sha256,
        &right_manifest.integrity.stdout_sha256,
    );

    print_field_diff(
        "  stderr_sha256",
        &left_manifest.integrity.stderr_sha256,
        &right_manifest.integrity.stderr_sha256,
    );

    println!();
    println!("Summary:");
    print_summary(&left_manifest, &right_manifest);

    Ok(())
}

fn json_diff(left: &TraceManifest, right: &TraceManifest) -> Value {
    json!({
        "left_trace_id": left.trace_id,
        "right_trace_id": right.trace_id,
        "fields": {
            "exit_code": value_diff(left.exit_code, right.exit_code),
            "duration_ms": value_diff(left.duration_ms, right.duration_ms),
            "command": value_diff(left.command.join(" "), right.command.join(" ")),
            "cwd": value_diff(&left.cwd, &right.cwd),
        },
        "git": {
            "branch_after": value_diff(&left.git.branch_after, &right.git.branch_after),
            "commit_after": value_diff(&left.git.commit_after, &right.git.commit_after),
            "dirty_after": value_diff(left.git.dirty_after, right.git.dirty_after),
        },
        "workspace": {
            "created": file_set_diff(
                &left.workspace.changes.created_files,
                &right.workspace.changes.created_files,
            ),
            "modified": file_set_diff(
                &left.workspace.changes.modified_files,
                &right.workspace.changes.modified_files,
            ),
            "deleted": file_set_diff(
                &left.workspace.changes.deleted_files,
                &right.workspace.changes.deleted_files,
            ),
        },
        "artifacts": {
            "stdout_sha256": value_diff(
                &left.integrity.stdout_sha256,
                &right.integrity.stdout_sha256,
            ),
            "stderr_sha256": value_diff(
                &left.integrity.stderr_sha256,
                &right.integrity.stderr_sha256,
            ),
            "pty_sha256": value_diff(
                &left.integrity.pty_sha256,
                &right.integrity.pty_sha256,
            ),
        },
        "summary": {
            "exit_code_changed": left.exit_code != right.exit_code,
            "stdout_changed": left.integrity.stdout_sha256 != right.integrity.stdout_sha256,
            "stderr_changed": left.integrity.stderr_sha256 != right.integrity.stderr_sha256,
            "workspace_change_count": {
                "left": count_changes(left),
                "right": count_changes(right),
                "changed": count_changes(left) != count_changes(right),
            }
        }
    })
}

fn value_diff<L, R>(left: L, right: R) -> Value
where
    L: serde::Serialize,
    R: serde::Serialize,
{
    let left_value = serde_json::to_value(left).unwrap_or(Value::Null);
    let right_value = serde_json::to_value(right).unwrap_or(Value::Null);

    json!({
        "changed": left_value != right_value,
        "left": left_value,
        "right": right_value,
    })
}

fn format_exit(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn display_opt(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("-").to_string()
}

fn print_field_diff(label: &str, left: &str, right: &str) {
    if left == right {
        println!("{label}: {left}");
    } else {
        println!("{label}:");
        println!("  left: {left}");
        println!("  right: {right}");
    }
}

fn diff_file_set(label: &str, left: &[String], right: &[String]) {
    let diff = file_sets(left, right);

    if diff.only_left.is_empty() && diff.only_right.is_empty() {
        println!("  {label}: same");
        return;
    }

    println!("  {label}:");

    if !diff.only_left.is_empty() {
        println!("    only left:");
        for file in diff.only_left {
            println!("      - {file}");
        }
    }

    if !diff.only_right.is_empty() {
        println!("    only right:");
        for file in diff.only_right {
            println!("      - {file}");
        }
    }
}

fn file_set_diff(left: &[String], right: &[String]) -> Value {
    let diff = file_sets(left, right);

    json!({
        "changed": !diff.only_left.is_empty() || !diff.only_right.is_empty(),
        "only_left": diff.only_left,
        "only_right": diff.only_right,
    })
}

struct FileSetDiff {
    only_left: Vec<String>,
    only_right: Vec<String>,
}

fn file_sets(left: &[String], right: &[String]) -> FileSetDiff {
    let left_set = left.iter().cloned().collect::<BTreeSet<_>>();
    let right_set = right.iter().cloned().collect::<BTreeSet<_>>();

    FileSetDiff {
        only_left: left_set.difference(&right_set).cloned().collect(),
        only_right: right_set.difference(&left_set).cloned().collect(),
    }
}

fn print_summary(left: &TraceManifest, right: &TraceManifest) {
    match (left.exit_code, right.exit_code) {
        (Some(a), Some(b)) if a != b => {
            println!("  exit code changed: {a} -> {b}");
        }
        (Some(code), Some(_)) => {
            println!("  exit code unchanged: {code}");
        }
        _ => {
            println!("  at least one trace has no process exit code");
        }
    }

    if left.integrity.stderr_sha256 != right.integrity.stderr_sha256 {
        println!("  stderr artifact changed");
    }

    if left.integrity.stdout_sha256 != right.integrity.stdout_sha256 {
        println!("  stdout artifact changed");
    }

    let left_changes = count_changes(left);
    let right_changes = count_changes(right);

    if left_changes != right_changes {
        println!("  workspace change count changed: {left_changes} -> {right_changes}");
    } else {
        println!("  workspace change count unchanged: {left_changes}");
    }
}

fn count_changes(manifest: &TraceManifest) -> usize {
    manifest.workspace.changes.created_files.len()
        + manifest.workspace.changes.modified_files.len()
        + manifest.workspace.changes.deleted_files.len()
}
