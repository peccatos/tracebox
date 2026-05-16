use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::evidence::manifest::TraceManifest;
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};
use crate::evidence::validation::{validate_manifest, ValidationCheck, ValidationReport};

/// Validate trace manifest schema and semantic consistency.
///
/// This command intentionally stays thin. The validation logic lives in
/// `crate::evidence::validation` so it can be reused by non-CLI integrations.
pub fn execute(trace_root: PathBuf, trace_id: String, json_output: bool) -> Result<i32> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let resolved = store.resolve_trace(&trace_id)?;
    let paths = resolved.paths;

    if !paths.root.is_dir() {
        let report =
            ValidationReport::invalid(&trace_id, &paths.root, "trace directory does not exist");

        print_report(&report, json_output)?;
        return Ok(report.exit_code());
    }

    let manifest_json = match fs::read_to_string(&paths.manifest) {
        Ok(json) => json,
        Err(error) => {
            let report = ValidationReport::invalid(
                &trace_id,
                &paths.root,
                &format!("failed to read {}: {error}", paths.manifest.display()),
            );

            print_report(&report, json_output)?;
            return Ok(report.exit_code());
        }
    };

    let manifest: TraceManifest = match serde_json::from_str(&manifest_json) {
        Ok(manifest) => manifest,
        Err(error) => {
            let report = ValidationReport::invalid(
                &trace_id,
                &paths.root,
                &format!("failed to parse {}: {error}", paths.manifest.display()),
            );

            print_report(&report, json_output)?;
            return Ok(report.exit_code());
        }
    };

    let report = validate_manifest(&trace_id, &paths, &manifest);

    print_report(&report, json_output)?;

    Ok(report.exit_code())
}

fn print_report(report: &ValidationReport, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("Trace ID: {}", report.trace_id);
    println!("Trace path: {}", report.trace_path);
    println!("Status: {}", report.status);

    if let Some(reason) = &report.reason {
        println!();
        println!("Reason:");
        println!("  {reason}");
        return Ok(());
    }

    println!();
    println!("Checks:");

    for check in &report.checks {
        print_check(check);
    }

    Ok(())
}

fn print_check(check: &ValidationCheck) {
    println!("  {}: {}", check.name, check.status);

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
