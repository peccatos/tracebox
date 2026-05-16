use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::evidence::integrity::{sha256_bytes, sha256_file};
use crate::evidence::manifest::TraceManifest;

/// Filesystem storage configuration.
///
/// The root should usually be:
///
/// - `.traces` for local development;
/// - a CI artifact directory;
/// - a runtime-specific trace directory for embedded integrations.
#[derive(Debug, Clone)]
pub struct TraceStoreConfig {
    pub root: PathBuf,
}

impl TraceStoreConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

/// Resolved filesystem paths for a single trace bundle.
#[derive(Debug, Clone)]
pub struct TracePaths {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub manifest_sha256: PathBuf,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    pub pty: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceLocation {
    Active,
    Archived,
}

#[derive(Debug, Clone)]
pub struct ResolvedTrace {
    pub trace_id: String,
    pub location: TraceLocation,
    pub paths: TracePaths,
}

/// Minimal append-only filesystem store.
///
/// This type intentionally does not know anything about process execution. It
/// only creates trace directories and writes immutable artifacts.
#[derive(Debug, Clone)]
pub struct FilesystemTraceStore {
    config: TraceStoreConfig,
}

impl FilesystemTraceStore {
    pub fn new(config: TraceStoreConfig) -> Self {
        Self { config }
    }

    pub fn root(&self) -> &Path {
        &self.config.root
    }

    pub fn paths_for(&self, trace_id: &str) -> TracePaths {
        let root = self.config.root.join(trace_id);

        TracePaths {
            manifest: root.join("manifest.json"),
            manifest_sha256: root.join("manifest.sha256"),
            stdout: root.join("stdout.log"),
            stderr: root.join("stderr.log"),
            pty: root.join("pty.log"),
            root,
        }
    }

    pub fn archived_paths_for(&self, trace_id: &str) -> TracePaths {
        let root = self.archive_root().join(trace_id);

        TracePaths {
            manifest: root.join("manifest.json"),
            manifest_sha256: root.join("manifest.sha256"),
            stdout: root.join("stdout.log"),
            stderr: root.join("stderr.log"),
            pty: root.join("pty.log"),
            root,
        }
    }

    pub fn archive_root(&self) -> PathBuf {
        self.config.root.join("archive")
    }

    pub fn trace_state(&self, trace_id: &str) -> TraceState {
        let active = self.paths_for(trace_id).root.is_dir();
        let archived = self.archived_paths_for(trace_id).root.is_dir();

        TraceState { active, archived }
    }

    pub fn resolve_trace(&self, trace_id: &str) -> Result<ResolvedTrace> {
        let active = self.paths_for(trace_id);

        if active.root.is_dir() {
            return Ok(ResolvedTrace {
                trace_id: trace_id.to_string(),
                location: TraceLocation::Active,
                paths: active,
            });
        }

        let archived = self.archived_paths_for(trace_id);

        if archived.root.is_dir() {
            return Ok(ResolvedTrace {
                trace_id: trace_id.to_string(),
                location: TraceLocation::Archived,
                paths: archived,
            });
        }

        bail!("trace not found: {trace_id}")
    }

    /// Create a new immutable trace directory.
    ///
    /// `create_dir` is used for the final trace directory so accidental trace ID
    /// collisions fail loudly instead of reusing an existing bundle.
    pub fn create_trace_dir(&self, trace_id: &str) -> Result<TracePaths> {
        fs::create_dir_all(&self.config.root).with_context(|| {
            format!("failed to create trace root {}", self.config.root.display())
        })?;

        let paths = self.paths_for(trace_id);

        if paths.root.exists() {
            bail!(
                "trace directory already exists and would violate append-only semantics: {}",
                paths.root.display()
            );
        }

        fs::create_dir(&paths.root).with_context(|| {
            format!("failed to create trace directory {}", paths.root.display())
        })?;

        Ok(paths)
    }

    pub fn archive_trace(&self, trace_id: &str) -> Result<TracePaths> {
        fs::create_dir_all(self.archive_root()).with_context(|| {
            format!(
                "failed to create archive root {}",
                self.archive_root().display()
            )
        })?;

        let active = self.paths_for(trace_id);
        let archived = self.archived_paths_for(trace_id);
        let state = self.trace_state(trace_id);

        match (state.active, state.archived) {
            (false, false) => bail!("trace not found: {trace_id}"),
            (false, true) => bail!("trace is already archived: {trace_id}"),
            (true, true) => bail!(
                "archive destination already exists: {}",
                archived.root.display()
            ),
            (true, false) => {
                fs::rename(&active.root, &archived.root).with_context(|| {
                    format!(
                        "failed to move trace {} to {}",
                        active.root.display(),
                        archived.root.display()
                    )
                })?;

                Ok(archived)
            }
        }
    }

    pub fn restore_trace(&self, trace_id: &str) -> Result<TracePaths> {
        fs::create_dir_all(&self.config.root).with_context(|| {
            format!("failed to create trace root {}", self.config.root.display())
        })?;

        let active = self.paths_for(trace_id);
        let archived = self.archived_paths_for(trace_id);
        let state = self.trace_state(trace_id);

        match (state.active, state.archived) {
            (false, false) => bail!("trace is not archived: {trace_id}"),
            (true, false) => bail!("trace is not archived: {trace_id}"),
            (true, true) => bail!(
                "active destination already exists: {}",
                active.root.display()
            ),
            (false, true) => {
                fs::rename(&archived.root, &active.root).with_context(|| {
                    format!(
                        "failed to move trace {} to {}",
                        archived.root.display(),
                        active.root.display()
                    )
                })?;

                Ok(active)
            }
        }
    }

    /// Open an artifact file with create-new semantics.
    pub fn create_artifact(path: &Path) -> Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("failed to create artifact {}", path.display()))
    }

    /// Write manifest and a `manifest.sha256` sidecar exactly once.
    ///
    /// The sidecar avoids recursive self-hashing inside `manifest.json`.
    pub fn write_manifest_and_hash(
        &self,
        paths: &TracePaths,
        manifest: &TraceManifest,
    ) -> Result<String> {
        let mut json =
            serde_json::to_vec_pretty(manifest).context("failed to serialize trace manifest")?;

        json.push(b'\n');

        let manifest_hash = sha256_bytes(&json);

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.manifest)
            .with_context(|| format!("failed to create manifest {}", paths.manifest.display()))?;

        file.write_all(&json)
            .with_context(|| format!("failed to write manifest {}", paths.manifest.display()))?;

        file.flush()
            .with_context(|| format!("failed to flush manifest {}", paths.manifest.display()))?;

        let mut hash_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.manifest_sha256)
            .with_context(|| {
                format!(
                    "failed to create manifest hash sidecar {}",
                    paths.manifest_sha256.display()
                )
            })?;

        writeln!(hash_file, "{manifest_hash}")
            .with_context(|| format!("failed to write {}", paths.manifest_sha256.display()))?;

        Ok(manifest_hash)
    }

    pub fn load_manifest(&self, trace_id: &str) -> Result<TraceManifest> {
        let paths = self.paths_for(trace_id);
        self.load_manifest_at(&paths)
    }

    pub fn load_manifest_at(&self, paths: &TracePaths) -> Result<TraceManifest> {
        let json = fs::read_to_string(&paths.manifest)
            .with_context(|| format!("failed to read {}", paths.manifest.display()))?;

        serde_json::from_str(&json)
            .with_context(|| format!("failed to parse {}", paths.manifest.display()))
    }

    pub fn artifact_size(path: &Path) -> Result<u64> {
        Ok(fs::metadata(path)
            .with_context(|| format!("failed to stat artifact {}", path.display()))?
            .len())
    }

    pub fn artifact_hash(path: &Path) -> Result<String> {
        sha256_file(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceState {
    pub active: bool,
    pub archived: bool,
}
