//! Read-only source evidence. Its data guard keeps the original store locked.
use super::*;

pub trait ReleaseRecoveryDataInspection {
    fn identity(&self) -> &p::ProductReleaseDataIdentity;
    /// Confirm the original exclusive guard, without reacquiring or extending
    /// it. Trusted test adapters must implement the same failure boundary.
    fn ensure_held(&mut self, deadline: Instant) -> p::Result<()>;
    fn release(&mut self) -> p::Result<()>;
}

impl ReleaseRecoveryDataInspection for super::super::ReleaseDataWorkerProcess {
    fn identity(&self) -> &p::ProductReleaseDataIdentity {
        self.identity()
    }
    fn ensure_held(&mut self, deadline: Instant) -> p::Result<()> {
        self.ensure_read_only_hold(deadline)
    }
    fn release(&mut self) -> p::Result<()> {
        self.step(p::ProductReleaseDataStep::Release).map(|_| ())
    }
}

pub(crate) struct RecoveryObservation {
    pub baseline: p::ProductReleaseRecoveryBaseline,
    data: Box<dyn ReleaseRecoveryDataInspection>,
}
impl RecoveryObservation {
    pub fn ensure_held(&mut self, deadline: Instant) -> p::Result<()> {
        self.data.ensure_held(deadline)?;
        if Instant::now() >= deadline || self.data.identity() != &self.baseline.data_identity {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn release(mut self) -> p::Result<()> {
        self.data.release()
    }
}

impl InstalledReleaseUpdater {
    pub(crate) fn observe_release_recovery(
        &self,
        journal: &ProductJournal,
        takeover: &p::ProductReleaseTakeover,
        worker_state_digest: p::SchemaDigest,
        captured_at: p::Timestamp,
        deadline: Instant,
    ) -> p::Result<RecoveryObservation> {
        takeover.validate()?;
        if takeover.installation_digest != self.installation.digest()? || Instant::now() >= deadline
        {
            return Err(invalid());
        }
        let original = self.original(journal, &takeover.release)?;
        let root = original
            .as_ref()
            .map_or(self.installation.paths.releases.as_path(), |r| {
                r.content_root()
            });
        let (images, configs, services) = self.recovery_files(root)?;
        let worker = match original {
            Some(verified) => super::super::InstalledReleaseDataWorker::from_verified(
                self.installation.clone(),
                verified,
            )?,
            None => super::super::InstalledReleaseDataWorker::new(self.installation.clone())?,
        };
        let data = self.platform.inspect_recovery_data(
            worker,
            p::ProductReleaseDataRequest {
                schema_version: p::SchemaVersion(1),
                release: takeover.release.clone(),
                owner: takeover.owner.clone(),
                expected_config_revision: takeover.expected_config_revision,
            },
            deadline,
        )?;
        let identity = data.identity().clone();
        identity.validate()?;
        if identity.owner_digest != p::sha256_content_digest(takeover.owner.0.as_bytes())
            || Instant::now() >= deadline
        {
            return Err(invalid());
        }
        let mut observation = RecoveryObservation {
            baseline: p::ProductReleaseRecoveryBaseline {
                schema_version: p::SchemaVersion(1),
                operation_id: takeover.release.operation_id.clone(),
                command_digest: takeover.release.command_digest.clone(),
                takeover_digest: takeover.digest()?,
                source_images_digest: images,
                config_files_digest: configs,
                service_configuration_digest: services,
                worker_state_digest,
                data_identity: identity,
                captured_at,
            },
            data,
        };
        self.recheck_recovery_files(journal, takeover, &observation.baseline, deadline)?;
        observation.ensure_held(deadline)?;
        Ok(observation)
    }
    pub(crate) fn recheck_recovery_files(
        &self,
        journal: &ProductJournal,
        takeover: &p::ProductReleaseTakeover,
        baseline: &p::ProductReleaseRecoveryBaseline,
        deadline: Instant,
    ) -> p::Result<()> {
        let original = self.original(journal, &takeover.release)?;
        let root = original
            .as_ref()
            .map_or(self.installation.paths.releases.as_path(), |r| {
                r.content_root()
            });
        let (images, configs, services) = self.recovery_files(root)?;
        if images != baseline.source_images_digest
            || configs != baseline.config_files_digest
            || services != baseline.service_configuration_digest
            || Instant::now() >= deadline
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn recovery_files(
        &self,
        root: &Path,
    ) -> p::Result<(p::SchemaDigest, p::SchemaDigest, p::SchemaDigest)> {
        let services = self
            .platform
            .observe_recovery_boundary(&self.installation, root)?;
        let suffix = if self.installation.target == p::ProductTarget::WindowsX86_64 {
            ".exe"
        } else {
            ""
        };
        let mut images = Vec::new();
        for name in [
            "forme-hostd",
            "forme-webd",
            "forme-gatewayd",
            "forme-cli",
            "forme-hostctl",
            "forme-executord",
        ] {
            let path = root.join(format!("bin/{name}{suffix}"));
            images.push((name, bounded_digest(&path, 512 * 1024 * 1024)?));
        }
        let mut configs = Vec::new();
        for name in [
            "host.json",
            "web.json",
            "worker.json",
            "product-template.json",
            "core/core.json",
            "installation.json",
        ] {
            configs.push((
                name,
                bounded_digest(&self.installation.paths.config.join(name), 4 * 1024 * 1024)?,
            ));
        }
        Ok((
            p::canonical_digest(&("forme-release-recovery-images-v1", images))?,
            p::canonical_digest(&("forme-release-recovery-configs-v1", configs))?,
            services,
        ))
    }
}

fn bounded_digest(path: &Path, max: u64) -> p::Result<p::SchemaDigest> {
    super::super::reject_linked_ancestors(path)?;
    let before = std::fs::metadata(path).map_err(|_| invalid())?;
    if !before.is_file() || before.len() == 0 || before.len() > max {
        return Err(invalid());
    }
    let digest = super::super::file_digest(path)?;
    super::super::reject_linked_ancestors(path)?;
    let after = std::fs::metadata(path).map_err(|_| invalid())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(invalid());
    }
    Ok(digest)
}
