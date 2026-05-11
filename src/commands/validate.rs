use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, FixedOffset};
use serde::Serialize;

use crate::evidence::manifest::{EnvVar, TraceManifest, WorkspaceFileState, MANIFEST_VERSION};
use crate::evidence::store::{FilesystemTraceStore, TracePaths, TraceStoreConfig};

/// Validate trace manifest schema and semantic consistency.
///
/// This command does not verify artifact bytes. That is the job of
/// `tracebox verify`.
///
/// `validate` answers a different question:
///
/// "Is this manifest structurally and semantically sane?"
///
/// Exit codes:
///
/// - `0`: validation passed;
/// - `1`: trace exists but semantic validation failed;
/// - `2`: trace is missing or cannot be parsed.
pub fn execute(trace_root: PathBuf, trace_id: String, json_output: bool) -> Result<i32> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let paths = store.paths_for(&trace_id);

    if !paths.root.is_dir() {
        return finish_invalid(
            json_output,
            &trace_id,
            &paths.root,
            "trace directory does not exist",
        );
    }

    let manifest_json = match fs::read_to_string(&paths.manifest) {
        Ok(json) => json,
        Err(error) => {
            return finish_invalid(
                json_output,
                &trace_id,
                &paths.root,
                &format!("failed to read {}: {error}", paths.manifest.display()),
            );
        }
    };

    let manifest: TraceManifest = match serde_json::from_str(&manifest_json) {
        Ok(manifest) => manifest,
        Err(error) => {
            return finish_invalid(
                json_output,
                &trace_id,
                &paths.root,
                &format!("failed to parse {}: {error}", paths.manifest.display()),
            );
        }
    };

    let checks = validate_manifest(&trace_id, &paths, &manifest);
    let all_ok = checks.iter().all(ValidationCheck::is_ok);

    if json_output {
        let report = ValidationReport {
            trace_id,
            trace_path: paths.root.display().to_string(),
            status: if all_ok { "OK" } else { "FAILED" },
            reason: None,
            checks: checks.iter().map(ValidationCheck::to_json).collect(),
        };

        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Trace ID: {trace_id}");
        println!("Trace path: {}", paths.root.display());

        if all_ok {
            println!("Status: OK");
        } else {
            println!("Status: FAILED");
        }

        println!();
        println!("Checks:");

        for check in &checks {
            print_check(check);
        }
    }

    if all_ok {
        Ok(0)
    } else {
        Ok(1)
    }
}

#[derive(Debug, Serialize)]
struct ValidationReport {
    trace_id: String,
    trace_path: String,
    status: &'static str,
    reason: Option<String>,
    checks: Vec<JsonValidationCheck>,
}

#[derive(Debug, Serialize)]
struct JsonValidationCheck {
    name: String,
    status: String,
    expected: Option<String>,
    actual: Option<String>,
    detail: Option<String>,
}

#[derive(Debug)]
struct ValidationCheck {
    name: String,
    status: CheckStatus,
    expected: Option<String>,
    actual: Option<String>,
    detail: Option<String>,
}

impl ValidationCheck {
    fn ok(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Ok,
            expected: None,
            actual: None,
            detail: None,
        }
    }

    fn failed(
        name: impl Into<String>,
        expected: Option<String>,
        actual: Option<String>,
        detail: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Failed,
            expected,
            actual,
            detail,
        }
    }

    fn is_ok(&self) -> bool {
        matches!(self.status, CheckStatus::Ok)
    }

    fn to_json(&self) -> JsonValidationCheck {
        JsonValidationCheck {
            name: self.name.clone(),
            status: self.status.as_str().to_string(),
            expected: self.expected.clone(),
            actual: self.actual.clone(),
            detail: self.detail.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CheckStatus {
    Ok,
    Failed,
}

impl CheckStatus {
    fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Ok => "OK",
            CheckStatus::Failed => "FAILED",
        }
    }
}

fn finish_invalid(
    json_output: bool,
    trace_id: &str,
    trace_path: &Path,
    reason: &str,
) -> Result<i32> {
    if json_output {
        let report = ValidationReport {
            trace_id: trace_id.to_string(),
            trace_path: trace_path.display().to_string(),
            status: "INVALID",
            reason: Some(reason.to_string()),
            checks: Vec::new(),
        };

        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Trace ID: {trace_id}");
        println!("Trace path: {}", trace_path.display());
        println!("Status: INVALID");
        println!();
        println!("Reason:");
        println!("  {reason}");
    }

    Ok(2)
}

fn validate_manifest(
    requested_trace_id: &str,
    paths: &TracePaths,
    manifest: &TraceManifest,
) -> Vec<ValidationCheck> {
    let mut checks = Vec::new();

    push_bool(
        &mut checks,
        "manifest_version",
        manifest.manifest_version == MANIFEST_VERSION,
        Some(MANIFEST_VERSION.to_string()),
        Some(manifest.manifest_version.to_string()),
        Some("manifest schema version must match the current supported version".to_string()),
    );

    push_bool(
        &mut checks,
        "trace_id_matches_argument",
        manifest.trace_id == requested_trace_id,
        Some(requested_trace_id.to_string()),
        Some(manifest.trace_id.clone()),
        Some("manifest trace_id must match the requested trace ID".to_string()),
    );

    let directory_name = paths
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    push_bool(
        &mut checks,
        "trace_id_matches_directory",
        manifest.trace_id == directory_name,
        Some(directory_name.to_string()),
        Some(manifest.trace_id.clone()),
        Some("manifest trace_id must match the trace directory name".to_string()),
    );

    push_bool(
        &mut checks,
        "trace_id_prefix",
        manifest.trace_id.starts_with("trc_"),
        Some("trc_*".to_string()),
        Some(manifest.trace_id.clone()),
        Some("trace_id should use the Tracebox `trc_` prefix".to_string()),
    );

    push_bool(
        &mut checks,
        "parent_trace_id_not_self",
        manifest.context.parent_trace_id.as_deref() != Some(manifest.trace_id.as_str()),
        None,
        manifest.context.parent_trace_id.clone(),
        Some("parent_trace_id must not point to the same trace".to_string()),
    );

    push_bool(
        &mut checks,
        "tool_kind_non_empty",
        !manifest.tool_kind.trim().is_empty(),
        Some("non-empty string".to_string()),
        Some(manifest.tool_kind.clone()),
        Some("tool_kind is required for downstream filtering and analysis".to_string()),
    );

    push_bool(
        &mut checks,
        "command_non_empty",
        !manifest.command.is_empty(),
        Some("at least one argv element".to_string()),
        Some(format!("{} elements", manifest.command.len())),
        Some("command argv must not be empty".to_string()),
    );

    push_bool(
        &mut checks,
        "command_entries_non_empty",
        manifest
            .command
            .iter()
            .all(|entry| !entry.trim().is_empty()),
        Some("all argv elements non-empty".to_string()),
        Some(format!("{:?}", manifest.command)),
        Some("empty argv elements make command evidence ambiguous".to_string()),
    );

    push_bool(
        &mut checks,
        "cwd_non_empty",
        !manifest.cwd.trim().is_empty(),
        Some("non-empty path string".to_string()),
        Some(manifest.cwd.clone()),
        Some("cwd is required to interpret relative workspace evidence".to_string()),
    );

    validate_timestamps(&mut checks, manifest);

    validate_artifact(
        &mut checks,
        "stdout_artifact",
        &paths.root,
        &manifest.artifacts.stdout,
    );

    validate_artifact(
        &mut checks,
        "stderr_artifact",
        &paths.root,
        &manifest.artifacts.stderr,
    );

    match (&manifest.artifacts.pty, &manifest.integrity.pty_sha256) {
        (Some(path), Some(_)) => {
            validate_artifact(&mut checks, "pty_artifact", &paths.root, path);
        }
        (Some(path), None) => checks.push(ValidationCheck::failed(
            "pty_integrity_pair",
            Some("pty path and pty_sha256 both present".to_string()),
            Some(format!("pty path present: {path}, pty_sha256 missing")),
            Some("PTY artifact metadata must be internally consistent".to_string()),
        )),
        (None, Some(hash)) => checks.push(ValidationCheck::failed(
            "pty_integrity_pair",
            Some("pty path and pty_sha256 both absent or both present".to_string()),
            Some(format!("pty path missing, pty_sha256 present: {hash}")),
            Some("PTY artifact metadata must be internally consistent".to_string()),
        )),
        (None, None) => checks.push(ValidationCheck::ok("pty_integrity_pair")),
    }

    push_bool(
        &mut checks,
        "stdout_sha256_format",
        is_sha256_hex(&manifest.integrity.stdout_sha256),
        Some("64 lowercase hexadecimal characters".to_string()),
        Some(manifest.integrity.stdout_sha256.clone()),
        Some("stdout_sha256 must be a valid SHA-256 hex digest".to_string()),
    );

    push_bool(
        &mut checks,
        "stderr_sha256_format",
        is_sha256_hex(&manifest.integrity.stderr_sha256),
        Some("64 lowercase hexadecimal characters".to_string()),
        Some(manifest.integrity.stderr_sha256.clone()),
        Some("stderr_sha256 must be a valid SHA-256 hex digest".to_string()),
    );

    if let Some(hash) = &manifest.integrity.pty_sha256 {
        push_bool(
            &mut checks,
            "pty_sha256_format",
            is_sha256_hex(hash),
            Some("64 lowercase hexadecimal characters".to_string()),
            Some(hash.clone()),
            Some("pty_sha256 must be a valid SHA-256 hex digest".to_string()),
        );
    }

    push_bool(
        &mut checks,
        "manifest_sha256_sidecar_exists",
        paths.manifest_sha256.is_file(),
        Some(paths.manifest_sha256.display().to_string()),
        Some(if paths.manifest_sha256.exists() {
            "exists".to_string()
        } else {
            "missing".to_string()
        }),
        Some("manifest.sha256 sidecar is part of the trace bundle contract".to_string()),
    );

    validate_workspace_strings(
        &mut checks,
        "workspace.changes.created_files",
        &manifest.workspace.changes.created_files,
    );

    validate_workspace_strings(
        &mut checks,
        "workspace.changes.modified_files",
        &manifest.workspace.changes.modified_files,
    );

    validate_workspace_strings(
        &mut checks,
        "workspace.changes.deleted_files",
        &manifest.workspace.changes.deleted_files,
    );

    validate_workspace_states(
        &mut checks,
        "workspace.dirty_before",
        &manifest.workspace.dirty_before,
    );

    validate_workspace_states(
        &mut checks,
        "workspace.dirty_after",
        &manifest.workspace.dirty_after,
    );

    validate_env(&mut checks, &manifest.env);

    checks
}

fn validate_timestamps(checks: &mut Vec<ValidationCheck>, manifest: &TraceManifest) {
    let started = DateTime::parse_from_rfc3339(&manifest.started_at);
    let finished = DateTime::parse_from_rfc3339(&manifest.finished_at);

    push_bool(
        checks,
        "started_at_rfc3339",
        started.is_ok(),
        Some("RFC3339 timestamp".to_string()),
        Some(manifest.started_at.clone()),
        Some("started_at must be parseable as RFC3339".to_string()),
    );

    push_bool(
        checks,
        "finished_at_rfc3339",
        finished.is_ok(),
        Some("RFC3339 timestamp".to_string()),
        Some(manifest.finished_at.clone()),
        Some("finished_at must be parseable as RFC3339".to_string()),
    );

    let Ok(started) = started else {
        checks.push(ValidationCheck::failed(
            "timestamp_order",
            Some("started_at <= finished_at".to_string()),
            None,
            Some("cannot check timestamp order because started_at is invalid".to_string()),
        ));
        checks.push(ValidationCheck::failed(
            "duration_consistency",
            Some("duration_ms approximately equals finished_at - started_at".to_string()),
            None,
            Some("cannot check duration because started_at is invalid".to_string()),
        ));
        return;
    };

    let Ok(finished) = finished else {
        checks.push(ValidationCheck::failed(
            "timestamp_order",
            Some("started_at <= finished_at".to_string()),
            None,
            Some("cannot check timestamp order because finished_at is invalid".to_string()),
        ));
        checks.push(ValidationCheck::failed(
            "duration_consistency",
            Some("duration_ms approximately equals finished_at - started_at".to_string()),
            None,
            Some("cannot check duration because finished_at is invalid".to_string()),
        ));
        return;
    };

    validate_timestamp_order(checks, started, finished);
    validate_duration(checks, manifest, started, finished);
}

fn validate_timestamp_order(
    checks: &mut Vec<ValidationCheck>,
    started: DateTime<FixedOffset>,
    finished: DateTime<FixedOffset>,
) {
    push_bool(
        checks,
        "timestamp_order",
        started <= finished,
        Some("started_at <= finished_at".to_string()),
        Some(format!("{started} <= {finished}")),
        Some("finished_at must not be earlier than started_at".to_string()),
    );
}

fn validate_duration(
    checks: &mut Vec<ValidationCheck>,
    manifest: &TraceManifest,
    started: DateTime<FixedOffset>,
    finished: DateTime<FixedOffset>,
) {
    let elapsed_ms = (finished - started).num_milliseconds();

    if elapsed_ms < 0 {
        checks.push(ValidationCheck::failed(
            "duration_consistency",
            Some("non-negative elapsed duration".to_string()),
            Some(elapsed_ms.to_string()),
            Some("finished_at is earlier than started_at".to_string()),
        ));
        return;
    }

    let actual = elapsed_ms as u128;
    let recorded = manifest.duration_ms;
    let drift = recorded.abs_diff(actual);

    push_bool(
        checks,
        "duration_consistency",
        drift <= 1_000,
        Some("duration_ms within 1000ms of finished_at - started_at".to_string()),
        Some(format!(
            "recorded={recorded}, computed={actual}, drift={drift}"
        )),
        Some("duration_ms should be consistent with timestamps".to_string()),
    );
}

fn validate_artifact(
    checks: &mut Vec<ValidationCheck>,
    name: &str,
    trace_dir: &Path,
    relative_path: &str,
) {
    match validate_relative_path(relative_path) {
        Ok(()) => checks.push(ValidationCheck::ok(format!("{name}.path"))),
        Err(error) => {
            checks.push(ValidationCheck::failed(
                format!("{name}.path"),
                Some("relative path without parent traversal".to_string()),
                Some(relative_path.to_string()),
                Some(error),
            ));
            return;
        }
    }

    let path = trace_dir.join(relative_path);

    push_bool(
        checks,
        format!("{name}.exists"),
        path.is_file(),
        Some(path.display().to_string()),
        Some(if path.exists() {
            "exists but is not a regular file".to_string()
        } else {
            "missing".to_string()
        }),
        Some("artifact path in manifest must resolve to a regular file".to_string()),
    );
}

fn validate_relative_path(path: &str) -> std::result::Result<(), String> {
    if path.trim().is_empty() {
        return Err("path is empty".to_string());
    }

    let path = Path::new(path);

    if path.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }

    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err("parent traversal `..` is not allowed".to_string());
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err("artifact path must be relative".to_string());
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    Ok(())
}

fn validate_workspace_strings(checks: &mut Vec<ValidationCheck>, name: &str, values: &[String]) {
    push_bool(
        checks,
        format!("{name}.paths_non_empty"),
        values.iter().all(|value| !value.trim().is_empty()),
        Some("all paths non-empty".to_string()),
        Some(format!("{values:?}")),
        Some("workspace path evidence must not contain empty paths".to_string()),
    );

    let duplicates = duplicates(values.iter().map(String::as_str));

    push_bool(
        checks,
        format!("{name}.deduplicated"),
        duplicates.is_empty(),
        Some("no duplicate paths".to_string()),
        Some(format!("{duplicates:?}")),
        Some("duplicate paths make diff output ambiguous".to_string()),
    );
}

fn validate_workspace_states(
    checks: &mut Vec<ValidationCheck>,
    name: &str,
    states: &[WorkspaceFileState],
) {
    let paths = states
        .iter()
        .map(|state| state.path.as_str())
        .collect::<Vec<_>>();

    push_bool(
        checks,
        format!("{name}.paths_non_empty"),
        paths.iter().all(|path| !path.trim().is_empty()),
        Some("all paths non-empty".to_string()),
        Some(format!("{paths:?}")),
        Some("workspace state paths must not be empty".to_string()),
    );

    let duplicates = duplicates(paths.iter().copied());

    push_bool(
        checks,
        format!("{name}.deduplicated"),
        duplicates.is_empty(),
        Some("no duplicate paths".to_string()),
        Some(format!("{duplicates:?}")),
        Some("dirty workspace state must not contain duplicate paths".to_string()),
    );

    let invalid_statuses = states
        .iter()
        .filter(|state| !matches!(state.status.as_str(), "created" | "modified" | "deleted"))
        .map(|state| format!("{}={}", state.path, state.status))
        .collect::<Vec<_>>();

    push_bool(
        checks,
        format!("{name}.statuses_valid"),
        invalid_statuses.is_empty(),
        Some("created | modified | deleted".to_string()),
        Some(format!("{invalid_statuses:?}")),
        Some("workspace state status must be one of the known coarse states".to_string()),
    );
}

fn validate_env(checks: &mut Vec<ValidationCheck>, env: &[EnvVar]) {
    push_bool(
        checks,
        "env.keys_non_empty",
        env.iter().all(|entry| !entry.key.trim().is_empty()),
        Some("all env keys non-empty".to_string()),
        Some(format!(
            "{:?}",
            env.iter().map(|entry| &entry.key).collect::<Vec<_>>()
        )),
        Some("environment evidence must not contain empty keys".to_string()),
    );

    let duplicates = duplicates(env.iter().map(|entry| entry.key.as_str()));

    push_bool(
        checks,
        "env.keys_deduplicated",
        duplicates.is_empty(),
        Some("no duplicate env keys".to_string()),
        Some(format!("{duplicates:?}")),
        Some("duplicate env keys make environment evidence ambiguous".to_string()),
    );
}

fn duplicates<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();

    for value in values {
        if !seen.insert(value.to_string()) {
            duplicates.insert(value.to_string());
        }
    }

    duplicates.into_iter().collect()
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn push_bool(
    checks: &mut Vec<ValidationCheck>,
    name: impl Into<String>,
    ok: bool,
    expected: Option<String>,
    actual: Option<String>,
    detail: Option<String>,
) {
    let name = name.into();

    if ok {
        checks.push(ValidationCheck::ok(name));
    } else {
        checks.push(ValidationCheck::failed(name, expected, actual, detail));
    }
}

fn print_check(check: &ValidationCheck) {
    println!("  {}: {}", check.name, check.status.as_str());

    if let Some(expected) = &check.expected {
        println!("    expected: {expected}");
    }

    if let Some(actual) = &check.actual {
        println!("    actual:   {actual}");
    }

    if let Some(detail) = &check.detail {
        println!("    detail:   {detail}");
    }
}
