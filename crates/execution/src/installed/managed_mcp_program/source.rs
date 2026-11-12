//! The current-source read holds config exclusion, but never admits a job.
use super::*;
use crate::installed::{
    managed_mcp_proof::DiscoveryProofReader, worker::WorkerJournal,
    worker_results::WorkerResultStore,
};
use std::{
    sync::Weak,
    time::{Duration, Instant},
};

pub(super) struct SourceBinding {
    pub(super) journal: Weak<WorkerJournal>,
    pub(super) results: Weak<WorkerResultStore>,
    pub(super) installation: String,
}

impl InstalledManagedMcpPrograms {
    pub(crate) fn bind_source_reader(
        &self,
        journal: &Arc<WorkerJournal>,
        results: &Arc<WorkerResultStore>,
        installation: &str,
        epoch: p::AuthorityEpoch,
    ) -> p::Result<()> {
        if epoch != self.epoch {
            return Err(invalid());
        }
        p::validate_product_id(installation)?;
        let mut binding = self.source.lock().map_err(|_| invalid())?;
        if let Some(old) = binding.as_ref() {
            if old.journal.upgrade().is_some() || old.results.upgrade().is_some() {
                return Err(invalid());
            }
        }
        *binding = Some(SourceBinding {
            journal: Arc::downgrade(journal),
            results: Arc::downgrade(results),
            installation: installation.into(),
        });
        Ok(())
    }

    pub(crate) fn verify_source(
        &self,
        manifest: &p::WorkerResultManifest,
    ) -> p::Result<p::ProductMcpServerConfig> {
        manifest.validate()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let cancel = crate::CancelToken::default();
        let mut config = self.configuration_until(deadline, &cancel)?;
        let reader = {
            let binding = self.source.lock().map_err(|_| invalid())?;
            let binding = binding.as_ref().ok_or_else(invalid)?;
            DiscoveryProofReader::new(
                binding.journal.upgrade().ok_or_else(invalid)?,
                binding.results.upgrade().ok_or_else(invalid)?,
                binding.installation.clone(),
            )
        };
        let (job, catalog) = reader.verify_manifest(manifest)?;
        self.check_discovery(&mut config, &job, &catalog.spec)?;
        // A current-source query cannot bootstrap a missing program journal.
        if !self.journal.is_file() {
            return Err(invalid());
        }
        let registry = self.registry_for(&config.snapshot()?)?;
        let lease = registry.acquire(
            &p::OwnerPrincipalRef(job.owner_ref.0.clone()),
            self.epoch,
            &catalog.spec.program_ref,
            &catalog.spec.registration_digest,
            &job.workspace_scope,
        )?;
        registry.with_active(&lease, || {
            admission::live(deadline, &cancel)?;
            config
                .snapshot_settings_v3()?
                .managed_capabilities
                .mcp_servers
                .into_iter()
                .find(|s| s.server_ref == catalog.spec.server.0)
                .ok_or_else(invalid)
        })
    }
}
