use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Result;

use crate::evidence::manifest::TraceManifest;
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

pub fn execute(trace_root: PathBuf, left: String, right: String) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let left_manifest = store.load_manifest(&left)?;
    let right_manifest = store.load_manifest(&right)?;

    println!("Trace diff");
    println!("==========");
    println!("left:  {}", left_manifest.trace_id);
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
        println!("  left:  {left}");
        println!("  right: {right}");
    }
}

fn diff_file_set(label: &str, left: &[String], right: &[String]) {
    let left_set = left.iter().cloned().collect::<BTreeSet<_>>();
    let right_set = right.iter().cloned().collect::<BTreeSet<_>>();

    let only_left = left_set.difference(&right_set).cloned().collect::<Vec<_>>();
    let only_right = right_set.difference(&left_set).cloned().collect::<Vec<_>>();

    if only_left.is_empty() && only_right.is_empty() {
        println!("  {label}: same");
        return;
    }

    println!("  {label}:");

    if !only_left.is_empty() {
        println!("    only left:");
        for file in only_left {
            println!("      - {file}");
        }
    }

    if !only_right.is_empty() {
        println!("    only right:");
        for file in only_right {
            println!("      - {file}");
        }
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
