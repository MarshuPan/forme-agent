//! Fixed independent updater, outside the lifetime of all three business services.
use super::{ProductJournal, ReleaseStager, VerifiedStagedRelease};
use forme_protocol as p;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

mod capabilities;
mod checking;
mod diagnostics;
mod platform;
mod recovery;
mod storage;
mod transaction;
pub use platform::SystemReleaseUpdaterPlatform;
pub(crate) use recovery::RecoveryObservation;
pub use recovery::ReleaseRecoveryDataInspection;

pub const RELEASE_UPDATER_SERVICE: &str = "forme-release-updater";
pub const RELEASE_UPDATER_VERSION: u32 = 2;

/// Trusted injection for component testing; no request can select a platform.
pub trait ReleaseUpdaterPlatform: Send + Sync {
    /// Trusted component injection; the wire cannot choose this inspector.
    /// Production uses the fixed installed child and holds its exclusive lock.
    fn inspect_recovery_data(
        &self,
        worker: super::InstalledReleaseDataWorker,
        request: p::ProductReleaseDataRequest,
        deadline: Instant,
    ) -> p::Result<Box<dyn ReleaseRecoveryDataInspection>> {
        Ok(Box::new(worker.start(request, deadline)?))
    }
    /// Read-only configuration fingerprint; verifies stopped updater/Brain and
    /// exact service identities. Never launches, stops or changes an image.
    fn observe_recovery_boundary(
        &self,
        _installation: &p::InstallationManifest,
        _root: &Path,
    ) -> p::Result<p::SchemaDigest> {
        Err(invalid())
    }
    fn verify_host_image(&self, image: &Path, digest: &p::SchemaDigest) -> p::Result<()> {
        if std::env::current_exe().map_err(|_| invalid())? != image
            || super::file_digest(image)? != *digest
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn switch_images(
        &self,
        prepared: super::PreparedReleaseServiceSwitch,
        journal: &ProductJournal,
        deadline: Instant,
    ) -> p::Result<p::ProductReleaseServiceReceipt> {
        prepared.execute(journal, &super::SystemReleaseServiceImageManager, deadline)
    }
    fn verify_updater(
        &self,
        installation: &p::InstallationManifest,
        host_config: &Path,
    ) -> p::Result<()>;
    fn launch(&self, installation: &p::InstallationManifest, host_config: &Path) -> p::Result<()>;
    fn observe(
        &self,
        installation: &p::InstallationManifest,
        root: &Path,
        service: p::BuiltinServiceId,
        state: Option<p::NativeServiceState>,
    ) -> p::Result<p::SchemaDigest>;
    fn change(
        &self,
        installation: &p::InstallationManifest,
        root: &Path,
        service: p::BuiltinServiceId,
        action: p::NativeServiceAction,
        deadline: Instant,
    ) -> p::Result<p::SchemaDigest>;
}

pub struct InstalledReleaseUpdater {
    installation: p::InstallationManifest,
    host_config: PathBuf,
    stager: Arc<ReleaseStager>,
    platform: Arc<dyn ReleaseUpdaterPlatform>,
}

impl InstalledReleaseUpdater {
    pub fn new(
        installation: p::InstallationManifest,
        host_config: PathBuf,
        stager: Arc<ReleaseStager>,
        platform: Arc<dyn ReleaseUpdaterPlatform>,
    ) -> p::Result<Self> {
        super::validate_installation_paths(&installation)?;
        if host_config != installation.paths.config.join("host.json")
            || stager.installation_digest()? != installation.digest()?
        {
            return Err(invalid());
        }
        super::reject_linked_ancestors(&host_config)?;
        Ok(Self {
            installation,
            host_config,
            stager,
            platform,
        })
    }
    pub fn installation(&self) -> &p::InstallationManifest {
        &self.installation
    }
    pub fn available(&self) -> bool {
        self.platform
            .verify_updater(&self.installation, &self.host_config)
            .is_ok()
    }
    pub(crate) fn preflight(
        &self,
        journal: &ProductJournal,
        request: &p::ProductReleaseRequest,
    ) -> p::Result<()> {
        request.validate()?;
        if request.install_ref != self.installation.install_id {
            return Err(invalid());
        }
        self.platform
            .verify_updater(&self.installation, &self.host_config)?;
        let target = self.target(journal, request)?;
        if target.manifest().minimum_upgrader > RELEASE_UPDATER_VERSION {
            return Err(invalid());
        }
        let original = self.original(journal, request)?;
        let root = original
            .as_ref()
            .map_or(self.installation.paths.releases.as_path(), |r| {
                r.content_root()
            });
        for id in services() {
            self.platform.observe(&self.installation, root, id, None)?;
        }
        super::InstalledReleaseDataWorker::new(self.installation.clone())?.preflight()
    }
    pub(crate) fn prepare_handoff(&self, takeover: &p::ProductReleaseTakeover) -> p::Result<()> {
        takeover.validate()?;
        if takeover.installation_digest != self.installation.digest()? {
            return Err(invalid());
        }
        storage::publish(&self.handoff_path(), takeover)
    }
    pub(crate) fn launch(
        &self,
        journal: &ProductJournal,
        takeover: &p::ProductReleaseTakeover,
    ) -> p::Result<()> {
        self.verify_takeover(journal, takeover)?;
        if storage::read::<p::ProductReleaseTakeover>(&self.handoff_path())? != *takeover {
            return Err(invalid());
        }
        self.preflight(journal, &takeover.release)?;
        self.platform.launch(&self.installation, &self.host_config)
    }
    /// Called only by the fixed SCM updater mode. A marker is consumed before
    /// the first stop and is never inferred as a successful transaction receipt.
    pub fn run(&self) -> p::Result<()> {
        transaction::run(self)
    }

    fn handoff_path(&self) -> PathBuf {
        self.installation
            .paths
            .host_state
            .join("release-handoff.json")
    }
    fn target(
        &self,
        journal: &ProductJournal,
        request: &p::ProductReleaseRequest,
    ) -> p::Result<VerifiedStagedRelease> {
        self.stager.verified_staged(
            journal,
            &request.target_release_ref,
            &request.target_release_digest,
        )
    }
    fn original(
        &self,
        journal: &ProductJournal,
        request: &p::ProductReleaseRequest,
    ) -> p::Result<Option<VerifiedStagedRelease>> {
        if request.original_release_ref == self.installation.release_id {
            if request.original_release_digest != self.installation.release_digest {
                return Err(invalid());
            }
            Ok(None)
        } else {
            self.stager
                .verified_staged(
                    journal,
                    &request.original_release_ref,
                    &request.original_release_digest,
                )
                .map(Some)
        }
    }
    fn verify_takeover(
        &self,
        journal: &ProductJournal,
        takeover: &p::ProductReleaseTakeover,
    ) -> p::Result<()> {
        takeover.validate()?;
        let request = &takeover.release;
        let operation = journal.operation(&request.operation_id)?;
        if takeover.installation_digest != self.installation.digest()?
            || journal.release_takeover(&request.operation_id)?.as_ref() != Some(takeover)
            || operation.command_digest != request.command_digest
            || operation.stage != p::MaintenanceStage::Validated
            || operation.pending_stage != Some(p::MaintenanceStage::Quiescing)
            || journal.active_release()?
                != Some((
                    request.original_release_ref.clone(),
                    request.original_release_digest.clone(),
                ))
            || now()? >= takeover.deadline
        {
            return Err(invalid());
        }
        Ok(())
    }
}
fn services() -> [p::BuiltinServiceId; 3] {
    [
        p::BuiltinServiceId::Host,
        p::BuiltinServiceId::Web,
        p::BuiltinServiceId::Brain,
    ]
}
fn now() -> p::Result<p::Timestamp> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .ok_or_else(invalid)
}
fn deadline(at: p::Timestamp) -> p::Result<Instant> {
    let remaining = at
        .checked_sub(now()?)
        .filter(|n| *n > 0 && *n <= 300_000)
        .ok_or_else(invalid)?;
    Ok(Instant::now() + Duration::from_millis(remaining as u64))
}
fn valid_time(at: p::Timestamp, limit: Instant) -> p::Result<()> {
    if Instant::now() >= limit || now()? >= at {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> p::Error {
    p::Error(
        "release takeover is unavailable, changed or requires original-operation recovery".into(),
    )
}
