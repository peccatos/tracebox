use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

const RESERVED_TRACE_NAMES: &[&str] = &["archive", "reports", "tmp", "index.json"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceTab {
    Active,
    Archived,
}

impl TraceTab {
    pub fn toggle(self) -> Self {
        match self {
            TraceTab::Active => TraceTab::Archived,
            TraceTab::Archived => TraceTab::Active,
        }
    }

    pub fn is_archived(self) -> bool {
        matches!(self, TraceTab::Archived)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub archived: bool,
    pub started_at: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub command: String,
    pub trace_path: PathBuf,
}

impl TraceSummary {
    pub fn matches_filter(&self, filter: &str) -> bool {
        let filter = filter.trim().to_lowercase();

        if filter.is_empty() {
            return true;
        }

        self.trace_id.to_lowercase().contains(&filter)
            || self.command.to_lowercase().contains(&filter)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TraceCatalog {
    pub active: Vec<TraceSummary>,
    pub archived: Vec<TraceSummary>,
}

impl TraceCatalog {
    pub fn traces_for_tab(&self, tab: TraceTab) -> &[TraceSummary] {
        match tab {
            TraceTab::Active => &self.active,
            TraceTab::Archived => &self.archived,
        }
    }

    pub fn filtered_for_tab(&self, tab: TraceTab, filter: &str) -> Vec<&TraceSummary> {
        self.traces_for_tab(tab)
            .iter()
            .filter(|trace| trace.matches_filter(filter))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty() && self.archived.is_empty()
    }
}

pub fn load_trace_catalog(trace_root: &Path) -> Result<TraceCatalog> {
    let active = load_trace_summaries(trace_root, false)?;
    let archived = load_trace_summaries(&trace_root.join("archive"), true)?;

    Ok(TraceCatalog { active, archived })
}

pub fn load_trace_summaries(base_root: &Path, archived: bool) -> Result<Vec<TraceSummary>> {
    if !base_root.exists() {
        return Ok(Vec::new());
    }

    let mut traces = Vec::new();

    for entry in fs::read_dir(base_root)
        .with_context(|| format!("failed to read trace root {}", base_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let trace_id = entry.file_name().to_string_lossy().to_string();

        if is_reserved_trace_name(&trace_id) {
            continue;
        }

        if !is_trace_bundle_dir(&path) {
            continue;
        }

        let Some(summary) = read_trace_summary(&path, archived) else {
            continue;
        };

        traces.push(summary);
    }

    traces.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then(a.trace_id.cmp(&b.trace_id))
    });

    Ok(traces)
}

fn read_trace_summary(trace_path: &Path, archived: bool) -> Option<TraceSummary> {
    let manifest_path = trace_path.join("manifest.json");
    let json = fs::read_to_string(&manifest_path).ok()?;
    let manifest = serde_json::from_str::<crate::evidence::manifest::TraceManifest>(&json).ok()?;

    Some(TraceSummary {
        trace_id: manifest.trace_id,
        archived,
        started_at: manifest.started_at,
        exit_code: manifest.exit_code,
        duration_ms: manifest.duration_ms,
        command: manifest.command.join(" "),
        trace_path: trace_path.to_path_buf(),
    })
}

fn is_reserved_trace_name(name: &str) -> bool {
    RESERVED_TRACE_NAMES.contains(&name)
}

fn is_trace_bundle_dir(path: &Path) -> bool {
    path.is_dir() && path.join("manifest.json").is_file()
}
