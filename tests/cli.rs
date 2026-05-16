use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use anyhow::{Context, Result};
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// Return the only active trace directory created inside `.traces`.
///
/// These tests intentionally inspect the filesystem, not internal Rust APIs.
/// Tracebox is an evidence runtime; the storage layout is part of the external
/// contract and must be tested end-to-end.
fn single_trace_dir(workspace: &Path) -> Result<PathBuf> {
    let dirs = trace_dirs(workspace, false)?;

    assert_eq!(
        dirs.len(),
        1,
        "expected exactly one active trace directory in {}",
        workspace.join(".traces").display()
    );

    Ok(dirs.into_iter().next().expect("single trace dir"))
}

fn trace_dirs(workspace: &Path, archived: bool) -> Result<Vec<PathBuf>> {
    let root = if archived {
        workspace.join(".traces/archive")
    } else {
        workspace.join(".traces")
    };

    let mut dirs = Vec::new();

    if !root.exists() {
        return Ok(dirs);
    }

    for entry in fs::read_dir(&root)
        .with_context(|| format!("failed to read traces root: {}", root.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;

        if !file_type.is_dir() {
            continue;
        }

        if !archived && entry.file_name() == OsStr::new("archive") {
            continue;
        }

        dirs.push(entry.path());
    }

    dirs.sort();
    Ok(dirs)
}

/// Load `manifest.json` from a trace directory as generic JSON.
///
/// We use `serde_json::Value` here on purpose. These are CLI integration tests,
/// not model unit tests. They should verify serialized evidence as users and
/// future tools will see it on disk.
fn read_manifest(trace_dir: &Path) -> Result<Value> {
    let manifest_path = trace_dir.join("manifest.json");

    let json = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;

    let value = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    Ok(value)
}

/// Return true if `git` is available.
///
/// Workspace mutation tests require git because Tracebox v0.1 intentionally
/// derives workspace evidence from before/after git snapshots. If git is not
/// available on a system running tests, this specific test exits successfully
/// instead of making the entire suite environment-dependent.
fn git_available() -> bool {
    StdCommand::new("git")
        .arg("--version")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Run a git command inside `cwd`.
fn git(cwd: &Path, args: &[&str]) -> Result<bool> {
    let status = StdCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .with_context(|| format!("failed to run git {:?}", args))?;

    Ok(status.success())
}

/// Initialize a tiny clean git repository for workspace mutation tests.
fn init_git_repo(workspace: &Path) -> Result<bool> {
    if !git_available() {
        return Ok(false);
    }

    fs::create_dir_all(workspace.join("src"))?;
    fs::write(workspace.join("README.md"), "initial\n")?;
    fs::write(
        workspace.join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )?;

    // `-q` keeps test output clean.
    // `-b main` avoids Git's default-branch warning on fresh systems.
    if !git(workspace, &["init", "-q", "-b", "main"])? {
        return Ok(false);
    }

    if !git(workspace, &["add", "."])? {
        return Ok(false);
    }

    let committed = git(
        workspace,
        &[
            "-c",
            "user.name=Tracebox Test",
            "-c",
            "user.email=tracebox@example.invalid",
            "commit",
            "-m",
            "initial test repo",
        ],
    )?;

    Ok(committed)
}

#[test]
fn run_captures_stdout_stderr_manifest_and_preserves_exit_code() -> Result<()> {
    let temp = TempDir::new()?;

    let mut cmd = Command::cargo_bin("tracebox")?;

    cmd.current_dir(temp.path()).args([
        "run",
        "--",
        "sh",
        "-c",
        "echo ok && echo err >&2 && exit 7",
    ]);

    cmd.assert()
        .code(7)
        .stdout(predicate::str::contains("Trace created:"))
        .stdout(predicate::str::contains("Trace path:"));

    let trace_dir = single_trace_dir(temp.path())?;

    let stdout = fs::read_to_string(trace_dir.join("stdout.log"))?;
    let stderr = fs::read_to_string(trace_dir.join("stderr.log"))?;
    let manifest = read_manifest(&trace_dir)?;

    assert_eq!(stdout, "ok\n");
    assert_eq!(stderr, "err\n");

    assert_eq!(manifest["manifest_version"], 1);
    assert_eq!(manifest["tool_kind"], "process");
    assert_eq!(manifest["exit_code"], 7);

    assert_eq!(manifest["artifacts"]["stdout"], "stdout.log");
    assert_eq!(manifest["artifacts"]["stderr"], "stderr.log");

    assert!(manifest["trace_id"]
        .as_str()
        .context("trace_id should be a string")?
        .starts_with("trc_"));

    Ok(())
}

#[test]
fn inspect_prints_stdout_tail_when_requested() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;

    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf hello"]);

    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;

    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    let mut inspect = Command::cargo_bin("tracebox")?;

    inspect
        .current_dir(temp.path())
        .args(["inspect", &trace_id, "--stdout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Trace ID:"))
        .stdout(predicate::str::contains("Exit Code: 0"))
        .stdout(predicate::str::contains("stdout tail"))
        .stdout(predicate::str::contains("hello"));

    Ok(())
}

#[test]
fn list_prints_created_trace_ids() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;

    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf listed"]);

    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;

    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    let mut list = Command::cargo_bin("tracebox")?;

    list.current_dir(temp.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains(trace_id));

    Ok(())
}

#[test]
fn report_writes_markdown_file_by_default() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;
    run.current_dir(temp.path()).args([
        "run",
        "--",
        "sh",
        "-c",
        "echo ok && echo err >&2 && exit 7",
    ]);
    run.assert().code(7);

    let trace_dir = single_trace_dir(temp.path())?;
    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    let mut report = Command::cargo_bin("tracebox")?;
    report
        .current_dir(temp.path())
        .args(["report", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Report written:"));

    let report_path = trace_dir.join("report.md");
    let report_text = fs::read_to_string(&report_path)
        .with_context(|| format!("failed to read {}", report_path.display()))?;

    assert!(report_text.contains(&trace_id));
    assert!(report_text.contains("## Summary"));
    assert!(report_text.contains("## Command"));
    assert!(report_text.contains("## Exit status"));
    assert!(report_text.contains("## Artifacts"));
    assert!(report_text.contains("echo ok && echo err >&2 && exit 7"));
    assert!(report_text.contains("Exit code"));
    assert!(report_text.contains("stdout path"));
    assert!(report_text.contains("stderr path"));
    assert!(report_text.contains("non-zero exit code: 7"));

    Ok(())
}

#[test]
fn report_supports_custom_output_path() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;
    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf custom"]);
    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;
    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    let output_path = temp.path().join("nested").join("trace-report.md");

    let mut report = Command::cargo_bin("tracebox")?;
    report
        .current_dir(temp.path())
        .args([
            "report",
            &trace_id,
            "--output",
            output_path
                .to_str()
                .context("output path should be valid UTF-8")?,
        ])
        .assert()
        .success();

    assert!(output_path.is_file());

    let report_text = fs::read_to_string(&output_path)
        .with_context(|| format!("failed to read {}", output_path.display()))?;

    assert!(report_text.contains(&trace_id));
    assert!(report_text.contains("## Diagnosis hints"));

    Ok(())
}

#[test]
fn report_returns_clean_error_for_missing_trace() -> Result<()> {
    let temp = TempDir::new()?;

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["report", "trc_missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("trace not found"));

    Ok(())
}

#[test]
fn service_directories_are_never_treated_as_traces() -> Result<()> {
    let temp = TempDir::new()?;

    fs::create_dir_all(temp.path().join(".traces/archive"))?;

    let mut run = Command::cargo_bin("tracebox")?;
    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf active"]);
    run.assert().success();

    let active_dir = single_trace_dir(temp.path())?;
    let active_trace = active_dir
        .file_name()
        .context("active trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .arg("list")
        .output()
        .map(|output| {
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).expect("valid UTF-8");
            assert!(text.contains(&active_trace));
            assert!(!text.contains("archive"));
        })?;

    for args in [
        vec!["inspect", "archive"],
        vec!["verify", "archive"],
        vec!["validate", "archive"],
        vec!["report", "archive"],
        vec!["diff", "archive", &active_trace],
    ] {
        Command::cargo_bin("tracebox")?
            .current_dir(temp.path())
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains("trace not found"));
    }

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["inspect", &active_trace])
        .assert()
        .success()
        .stdout(predicate::str::contains("Trace ID:"))
        .stdout(predicate::str::contains(&active_trace));

    Ok(())
}

#[test]
fn archive_restore_and_archived_resolution_work_end_to_end() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;
    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "echo archived && echo err >&2"]);
    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;
    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["archive", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Archived trace:"));

    assert!(!trace_dir.exists());

    let archived_dir = temp.path().join(".traces/archive").join(&trace_id);
    assert!(archived_dir.is_dir());

    let archived_dirs = trace_dirs(temp.path(), true)?;
    assert_eq!(archived_dirs.len(), 1);
    assert_eq!(archived_dirs[0], archived_dir);

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["inspect", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Trace ID:"))
        .stdout(predicate::str::contains(&trace_id));

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["verify", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status: OK"));

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["validate", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status: OK"));

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["report", &trace_id])
        .assert()
        .success();

    let report_path = archived_dir.join("report.md");
    assert!(report_path.is_file());

    let report_text = fs::read_to_string(&report_path)
        .with_context(|| format!("failed to read {}", report_path.display()))?;

    assert!(report_text.contains(&trace_id));
    assert!(report_text.contains("## Diagnosis hints"));

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["restore", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored trace:"));

    assert!(temp.path().join(".traces").join(&trace_id).is_dir());
    assert!(!archived_dir.exists());

    Ok(())
}

#[test]
fn list_modes_and_diff_work_across_active_and_archived_traces() -> Result<()> {
    let temp = TempDir::new()?;

    let mut first = Command::cargo_bin("tracebox")?;
    first
        .current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf first"]);
    first.assert().success();

    let mut second = Command::cargo_bin("tracebox")?;
    second
        .current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf second"]);
    second.assert().success();

    let trace_ids = trace_ids_from_list_json(temp.path(), false, false)?;
    assert_eq!(trace_ids.len(), 2);

    let archived_trace = &trace_ids[0];
    let active_trace = &trace_ids[1];

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["archive", archived_trace])
        .assert()
        .success();

    let default_list = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .arg("list")
        .output()?;

    assert!(default_list.status.success());
    let default_text = String::from_utf8(default_list.stdout)?;
    assert!(default_text.contains(active_trace));
    assert!(!default_text.contains(archived_trace));

    let archived_list = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["list", "--archived"])
        .output()?;

    assert!(archived_list.status.success());
    let archived_text = String::from_utf8(archived_list.stdout)?;
    assert!(archived_text.contains(archived_trace));
    assert!(!archived_text.contains(active_trace));

    let all_list = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["list", "--all"])
        .output()?;

    assert!(all_list.status.success());
    let all_text = String::from_utf8(all_list.stdout)?;
    assert!(all_text.contains(active_trace));
    assert!(all_text.contains(archived_trace));
    assert!(all_text.contains("ARCHIVED"));
    assert!(all_text.contains("ACTIVE"));

    let diff_output = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["diff", active_trace, archived_trace])
        .output()?;

    assert!(diff_output.status.success());
    let diff_text = String::from_utf8(diff_output.stdout)?;
    assert!(diff_text.contains(active_trace));
    assert!(diff_text.contains(archived_trace));

    Ok(())
}

#[test]
fn archive_and_restore_error_cases_are_clean() -> Result<()> {
    let temp = TempDir::new()?;

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["archive", "trc_missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("trace not found"));

    let mut run = Command::cargo_bin("tracebox")?;
    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf error-cases"]);
    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;
    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["restore", &trace_id])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "active destination already exists",
        ));

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["archive", &trace_id])
        .assert()
        .success();

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["archive", &trace_id])
        .assert()
        .failure()
        .stderr(predicate::str::contains("trace is already archived"));

    let active_dir = temp.path().join(".traces").join(&trace_id);
    fs::create_dir_all(&active_dir)?;

    Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["restore", &trace_id])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "active destination already exists",
        ));

    Ok(())
}

#[test]
fn workspace_mutations_are_detected_from_before_after_git_snapshots() -> Result<()> {
    let temp = TempDir::new()?;

    if !init_git_repo(temp.path())? {
        return Ok(());
    }

    let mut run = Command::cargo_bin("tracebox")?;

    run.current_dir(temp.path()).args([
        "run",
        "--",
        "sh",
        "-c",
        "printf changed > src/lib.rs; printf generated > generated.txt; rm README.md",
    ]);

    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;
    let manifest = read_manifest(&trace_dir)?;

    let modified = manifest["workspace"]["changes"]["modified_files"]
        .as_array()
        .context("workspace.changes.modified_files should be an array")?;

    let created = manifest["workspace"]["changes"]["created_files"]
        .as_array()
        .context("workspace.changes.created_files should be an array")?;

    let deleted = manifest["workspace"]["changes"]["deleted_files"]
        .as_array()
        .context("workspace.changes.deleted_files should be an array")?;

    assert!(
        modified.iter().any(|value| value == "src/lib.rs"),
        "expected src/lib.rs to be detected as modified"
    );

    assert!(
        created.iter().any(|value| value == "generated.txt"),
        "expected generated.txt to be detected as created"
    );

    assert!(
        deleted.iter().any(|value| value == "README.md"),
        "expected README.md to be detected as deleted"
    );

    assert_eq!(manifest["git"]["dirty_before"], false);
    assert_eq!(manifest["git"]["dirty_after"], true);

    Ok(())
}

#[test]
fn verify_passes_for_intact_trace_and_fails_after_tampering() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;

    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf verified"]);

    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;

    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    let mut verify = Command::cargo_bin("tracebox")?;

    verify
        .current_dir(temp.path())
        .args(["verify", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status: OK"))
        .stdout(predicate::str::contains("manifest.json: OK"))
        .stdout(predicate::str::contains("stdout.log: OK"))
        .stdout(predicate::str::contains("stderr.log: OK"));

    fs::write(trace_dir.join("stdout.log"), "tampered\n")?;

    let mut verify_after_tamper = Command::cargo_bin("tracebox")?;

    verify_after_tamper
        .current_dir(temp.path())
        .args(["verify", &trace_id])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Status: FAILED"))
        .stdout(predicate::str::contains("stdout.log: FAILED"));

    Ok(())
}

#[test]
fn inspect_verify_and_diff_support_json_output() -> Result<()> {
    let temp = TempDir::new()?;

    let mut first = Command::cargo_bin("tracebox")?;
    first
        .current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf first"]);
    first.assert().success();

    let mut second = Command::cargo_bin("tracebox")?;
    second
        .current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf second && exit 3"]);
    second.assert().code(3);

    let trace_ids = trace_ids_from_list_json(temp.path(), false, false)?;

    assert_eq!(trace_ids.len(), 2);

    let first_trace = &trace_ids[0];
    let second_trace = &trace_ids[1];

    let inspect_output = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["inspect", first_trace, "--json", "--stdout"])
        .output()?;

    assert!(inspect_output.status.success());

    let inspect_json: Value = serde_json::from_slice(&inspect_output.stdout)?;

    assert_eq!(inspect_json["trace"]["trace_id"], *first_trace);
    assert_eq!(inspect_json["trace"]["exit_code"], 0);
    assert_eq!(inspect_json["stdout_tail"][0], "first");

    let verify_output = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["verify", first_trace, "--json"])
        .output()?;

    assert!(verify_output.status.success());

    let verify_json: Value = serde_json::from_slice(&verify_output.stdout)?;

    assert_eq!(verify_json["trace_id"], *first_trace);
    assert_eq!(verify_json["status"], "OK");
    assert!(
        verify_json["checks"]
            .as_array()
            .context("checks should be an array")?
            .len()
            >= 3
    );

    let diff_output = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["diff", first_trace, second_trace, "--json"])
        .output()?;

    assert!(diff_output.status.success());

    let diff_json: Value = serde_json::from_slice(&diff_output.stdout)?;

    assert_eq!(diff_json["left_trace_id"], *first_trace);
    assert_eq!(diff_json["right_trace_id"], *second_trace);
    assert_eq!(diff_json["fields"]["exit_code"]["changed"], true);
    assert_eq!(diff_json["summary"]["stdout_changed"], true);

    Ok(())
}

#[test]
fn diff_reports_environment_changes_when_tracebox_mode_differs() -> Result<()> {
    let temp = TempDir::new()?;

    let mut first = Command::cargo_bin("tracebox")?;
    first
        .current_dir(temp.path())
        .env("TRACEBOX_MODE", "stable")
        .args(["run", "--", "sh", "-c", "printf first"]);
    first.assert().success();

    let mut second = Command::cargo_bin("tracebox")?;
    second
        .current_dir(temp.path())
        .env("TRACEBOX_MODE", "broken")
        .args(["run", "--", "sh", "-c", "printf second && exit 3"]);
    second.assert().code(3);

    let trace_ids = trace_ids_from_list_json(temp.path(), false, false)?;
    assert_eq!(trace_ids.len(), 2);

    let first_trace = &trace_ids[0];
    let second_trace = &trace_ids[1];

    let diff_output = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["diff", first_trace, second_trace, "--json"])
        .output()?;

    assert!(diff_output.status.success());

    let diff_json: Value = serde_json::from_slice(&diff_output.stdout)?;
    assert_eq!(diff_json["environment"]["changed"], true);
    assert_eq!(diff_json["environment"]["left"]["TRACEBOX_MODE"], "stable");
    assert_eq!(diff_json["environment"]["right"]["TRACEBOX_MODE"], "broken");

    Ok(())
}

#[test]
fn validate_accepts_valid_trace_and_rejects_semantically_invalid_manifest() -> Result<()> {
    let temp = TempDir::new()?;

    let mut run = Command::cargo_bin("tracebox")?;
    run.current_dir(temp.path())
        .args(["run", "--", "sh", "-c", "printf valid"]);

    run.assert().success();

    let trace_dir = single_trace_dir(temp.path())?;

    let trace_id = trace_dir
        .file_name()
        .context("trace directory should have a file name")?
        .to_string_lossy()
        .to_string();

    let mut validate = Command::cargo_bin("tracebox")?;

    validate
        .current_dir(temp.path())
        .args(["validate", &trace_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status: OK"))
        .stdout(predicate::str::contains("manifest_version: OK"))
        .stdout(predicate::str::contains("trace_id_matches_directory: OK"));

    let validate_json_output = Command::cargo_bin("tracebox")?
        .current_dir(temp.path())
        .args(["validate", &trace_id, "--json"])
        .output()?;

    assert!(validate_json_output.status.success());

    let validate_json: Value = serde_json::from_slice(&validate_json_output.stdout)?;

    assert_eq!(validate_json["trace_id"], trace_id);
    assert_eq!(validate_json["status"], "OK");
    assert!(validate_json["checks"]
        .as_array()
        .context("checks should be an array")?
        .iter()
        .any(|check| check["name"] == "manifest_version" && check["status"] == "OK"));

    let manifest_path = trace_dir.join("manifest.json");
    let mut manifest = read_manifest(&trace_dir)?;

    manifest["trace_id"] = Value::String("trc_wrong".to_string());

    fs::write(
        &manifest_path,
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;

    let mut validate_after_tamper = Command::cargo_bin("tracebox")?;

    validate_after_tamper
        .current_dir(temp.path())
        .args(["validate", &trace_id])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Status: FAILED"))
        .stdout(predicate::str::contains(
            "trace_id_matches_directory: FAILED",
        ));

    Ok(())
}

fn trace_ids_from_list_json(workspace: &Path, archived: bool, all: bool) -> Result<Vec<String>> {
    let mut cmd = Command::cargo_bin("tracebox")?;
    cmd.current_dir(workspace).arg("list").arg("--json");

    if archived {
        cmd.arg("--archived");
    }

    if all {
        cmd.arg("--all");
    }

    let output = cmd.output()?;

    assert!(output.status.success());

    let traces: Vec<Value> = serde_json::from_slice(&output.stdout)?;

    traces
        .into_iter()
        .map(|trace| {
            trace["trace_id"]
                .as_str()
                .map(ToOwned::to_owned)
                .context("trace_id should be a string")
        })
        .collect()
}
