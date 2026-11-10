//! Optional, locally provisioned program sources in the protected Host domain.
use forme_execution::installed::ProductConfigStore;
use forme_protocol as p;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(windows)]
pub(super) fn load(
    journal: &Path,
    installation: &str,
    configuration: Option<Arc<Mutex<ProductConfigStore>>>,
    worker_config: Option<&Path>,
    protected_paths: &[PathBuf],
    managed: bool,
) -> p::Result<Option<Arc<forme_execution::installed::InstalledManagedMcpPrograms>>> {
    use forme_execution::installed::{
        reject_linked_ancestors, InstalledManagedMcpPrograms, WorkerRuntimeConfig,
    };
    if !managed {
        return Ok(None);
    }
    let (Some(configuration), Some(worker_config)) = (configuration, worker_config) else {
        return Ok(None);
    };
    let root = journal
        .parent()
        .ok_or_else(|| p::Error("Host journal parent is unavailable".into()))?
        .join("mcp-programs");
    reject_linked_ancestors(&root)?;
    if !root
        .try_exists()
        .map_err(|_| p::Error("managed MCP source directory is unavailable".into()))?
    {
        return Ok(None);
    }
    let worker = WorkerRuntimeConfig::load(worker_config)?;
    let mut protected = protected_paths.to_vec();
    protected.push(worker.runtime_root);
    protected.push(worker.journal_path);
    protected.extend(worker.workspace_roots.into_values());
    Ok(Some(Arc::new(InstalledManagedMcpPrograms::new(
        journal.with_extension("mcp-programs.jsonl"),
        installation.into(),
        worker.authority_epoch,
        configuration,
        root,
        worker.allowed_scopes,
        protected,
    )?)))
}
