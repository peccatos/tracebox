use std::path::PathBuf;

use anyhow::Result;

use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

pub fn execute(trace_root: PathBuf, trace_id: String) -> Result<()> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(&trace_root));
    let restored = store.restore_trace(&trace_id)?;

    println!("Restored trace: {}", trace_id);
    println!("Active path: {}", restored.root.display());

    Ok(())
}
