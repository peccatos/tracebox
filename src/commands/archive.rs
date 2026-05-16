use std::path::PathBuf;

use anyhow::Result;

use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

pub fn execute(trace_root: PathBuf, trace_id: String) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let archived = store.archive_trace(&trace_id)?;

    println!("Archived trace: {}", trace_id);
    println!("Archived path: {}", archived.root.display());

    Ok(())
}
