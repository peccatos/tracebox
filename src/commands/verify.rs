use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::evidence::integrity::sha256_file;
use crate::evidence::manifest::TraceManifest;
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

/// Verify immutable trace bundle integrity.
///
/// This command is intentionally filesystem-level. It does not trust the trace
/// just because `manifest.json` exists. It recomputes artifact hashes from the
/// actual bytes on disk and compares them against the evidence contract.
///
/// Exit codes:
///
/// - `0`: verification passed;
/// - `1`: trace exists but verification failed;
/// - `2`: trace is missing or structurally invalid.
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

    let mut checks = Vec::new();

    verify_manifest_sidecar(&mut checks, &paths.manifest, &paths.manifest_sha256);

    verify_artifact(
        &mut checks,
        &paths.root,
        "stdout.log",
        &manifest.artifacts.stdout,
        &manifest.integrity.stdout_sha256,
    );

    verify_artifact(
        &mut checks,
        &paths.root,
        "stderr.log",
        &manifest.artifacts.stderr,
        &manifest.integrity.stderr_sha256,
    );

    verify_optional_pty(&mut checks, &paths.root, &manifest);

    let all_ok = checks.iter().all(VerificationCheck::is_ok);

    if json_output {
        let report = VerificationReport {
            trace_id,
            trace_path: paths.root.display().to_string(),
            status: if all_ok { "OK" } else { "FAILED" },
            reason: None,
            checks: checks.iter().map(VerificationCheck::to_json).collect(),
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
struct VerificationReport {
    trace_id: String,
    trace_path: String,
    status: &'static str,
    reason: Option<String>,
    checks: Vec<JsonVerificationCheck>,
}

#[derive(Debug, Serialize)]
struct JsonVerificationCheck {
    label: String,
    path: Option<String>,
    status: String,
    expected: Option<String>,
    actual: Option<String>,
    detail: Option<String>,
}

#[derive(Debug)]
struct VerificationCheck {
    label: String,
    path: Option<PathBuf>,
    status: CheckStatus,
    expected: Option<String>,
    actual: Option<String>,
    detail: Option<String>,
}

impl VerificationCheck {
    fn ok(label: impl Into<String>, path: impl Into<PathBuf>, hash: String) -> Self {
        Self {
            label: label.into(),
            path: Some(path.into()),
            status: CheckStatus::Ok,
            expected: Some(hash.clone()),
            actual: Some(hash),
            detail: None,
        }
    }

    fn failed(
        label: impl Into<String>,
        path: Option<PathBuf>,
        expected: Option<String>,
        actual: Option<String>,
        detail: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            path,
            status: CheckStatus::Failed,
            expected,
            actual,
            detail,
        }
    }

    fn missing(
        label: impl Into<String>,
        path: impl Into<PathBuf>,
        expected: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            path: Some(path.into()),
            status: CheckStatus::Missing,
            expected,
            actual: None,
            detail: None,
        }
    }

    fn invalid(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            path: None,
            status: CheckStatus::Invalid,
            expected: None,
            actual: None,
            detail: Some(detail.into()),
        }
    }

    fn is_ok(&self) -> bool {
        matches!(self.status, CheckStatus::Ok)
    }

    fn to_json(&self) -> JsonVerificationCheck {
        JsonVerificationCheck {
            label: self.label.clone(),
            path: self.path.as_ref().map(|path| path.display().to_string()),
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
    Missing,
    Invalid,
}

impl CheckStatus {
    fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Ok => "OK",
            CheckStatus::Failed => "FAILED",
            CheckStatus::Missing => "MISSING",
            CheckStatus::Invalid => "INVALID",
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
        let report = VerificationReport {
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

fn verify_manifest_sidecar(checks: &mut Vec<VerificationCheck>, manifest: &Path, sidecar: &Path) {
    let expected = match fs::read_to_string(sidecar) {
        Ok(hash) => hash.trim().to_string(),
        Err(_) => {
            checks.push(VerificationCheck::missing(
                "manifest.json",
                sidecar.to_path_buf(),
                None,
            ));
            return;
        }
    };

    verify_hash(checks, "manifest.json", manifest, expected);
}

fn verify_artifact(
    checks: &mut Vec<VerificationCheck>,
    trace_dir: &Path,
    label: &str,
    relative_path: &str,
    expected_hash: &str,
) {
    let path = match resolve_artifact_path(trace_dir, relative_path) {
        Ok(path) => path,
        Err(error) => {
            checks.push(VerificationCheck::invalid(
                label,
                format!("invalid artifact path {relative_path:?}: {error}"),
            ));
            return;
        }
    };

    verify_hash(checks, label, &path, expected_hash.to_string());
}

fn verify_optional_pty(
    checks: &mut Vec<VerificationCheck>,
    trace_dir: &Path,
    manifest: &TraceManifest,
) {
    match (&manifest.artifacts.pty, &manifest.integrity.pty_sha256) {
        (Some(relative_path), Some(expected_hash)) => {
            verify_artifact(checks, trace_dir, "pty.log", relative_path, expected_hash);
        }

        (Some(relative_path), None) => {
            checks.push(VerificationCheck::failed(
                "pty.log",
                resolve_artifact_path(trace_dir, relative_path).ok(),
                None,
                None,
                Some("PTY artifact is present but pty_sha256 is missing".to_string()),
            ));
        }

        (None, Some(expected_hash)) => {
            checks.push(VerificationCheck::failed(
                "pty.log",
                None,
                Some(expected_hash.clone()),
                None,
                Some("pty_sha256 is present but PTY artifact path is missing".to_string()),
            ));
        }

        (None, None) => {}
    }
}

fn verify_hash(
    checks: &mut Vec<VerificationCheck>,
    label: &str,
    path: &Path,
    expected_hash: String,
) {
    if !path.exists() {
        checks.push(VerificationCheck::missing(
            label,
            path.to_path_buf(),
            Some(expected_hash),
        ));
        return;
    }

    let actual_hash = match sha256_file(path) {
        Ok(hash) => hash,
        Err(error) => {
            checks.push(VerificationCheck::failed(
                label,
                Some(path.to_path_buf()),
                Some(expected_hash),
                None,
                Some(error.to_string()),
            ));
            return;
        }
    };

    if actual_hash == expected_hash {
        checks.push(VerificationCheck::ok(
            label,
            path.to_path_buf(),
            actual_hash,
        ));
    } else {
        checks.push(VerificationCheck::failed(
            label,
            Some(path.to_path_buf()),
            Some(expected_hash),
            Some(actual_hash),
            None,
        ));
    }
}

fn resolve_artifact_path(trace_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);

    if path.is_absolute() {
        anyhow::bail!("absolute artifact paths are not allowed");
    }

    for component in path.components() {
        match component {
            Component::ParentDir => {
                anyhow::bail!("artifact path must not contain '..'");
            }
            Component::Prefix(_) | Component::RootDir => {
                anyhow::bail!("artifact path must be relative");
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    Ok(trace_dir.join(path))
}

fn print_check(check: &VerificationCheck) {
    println!("  {}: {}", check.label, check.status.as_str());

    if let Some(path) = &check.path {
        println!("    path: {}", path.display());
    }

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
