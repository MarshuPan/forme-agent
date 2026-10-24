//! Prepared, one-shot native service configuration; not application readiness.
use super::{
    file_digest, reject_linked_ancestors, ProductJournal, ReleaseStager, VerifiedStagedRelease,
};
use forme_protocol as p;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

pub struct ReleaseServiceImageChange {
    service: p::BuiltinServiceId,
    source: PathBuf,
    target: PathBuf,
    config: PathBuf,
    config_digest: p::SchemaDigest,
    source_digest: p::SchemaDigest,
    target_digest: p::SchemaDigest,
}

impl ReleaseServiceImageChange {
    pub fn service(&self) -> p::BuiltinServiceId {
        self.service
    }
    pub fn source_image(&self) -> &Path {
        &self.source
    }
    pub fn target_image(&self) -> &Path {
        &self.target
    }
    pub fn configuration_path(&self) -> &Path {
        &self.config
    }
}

/// Trusted platform injection. Both methods must inspect actual Stopped state,
/// the fixed image/arguments/identity, dependencies and recovery policy. A lost
/// observation is an error, never permission to recreate a service.
pub trait ReleaseServiceImageManager {
    fn verify_original(&self, change: &ReleaseServiceImageChange) -> p::Result<p::SchemaDigest>;
    fn switch_and_verify(
        &self,
        change: &ReleaseServiceImageChange,
        original_snapshot: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest>;
}

pub struct SystemReleaseServiceImageManager;
impl ReleaseServiceImageManager for SystemReleaseServiceImageManager {
    fn verify_original(&self, change: &ReleaseServiceImageChange) -> p::Result<p::SchemaDigest> {
        #[cfg(windows)]
        {
            super::registration::release::verify_original(change)
        }
        #[cfg(not(windows))]
        {
            let _ = change;
            Err(unavailable())
        }
    }
    fn switch_and_verify(
        &self,
        change: &ReleaseServiceImageChange,
        original_snapshot: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        #[cfg(windows)]
        {
            super::registration::release::switch_and_verify(change, original_snapshot)
        }
        #[cfg(not(windows))]
        {
            let _ = (change, original_snapshot);
            Err(unavailable())
        }
    }
}

/// No Deserialize/Clone: only signed staging + the original journal can create
/// this handle. The durable claim also prevents reconstruction after a crash.
pub struct PreparedReleaseServiceSwitch {
    request: p::ProductReleaseRequest,
    target: VerifiedStagedRelease,
    original: Option<VerifiedStagedRelease>,
    installation_digest: p::SchemaDigest,
    initial_release_digest: p::SchemaDigest,
    changes: Vec<ReleaseServiceImageChange>,
    revision: u64,
    expires_at: p::Timestamp,
}

impl ReleaseStager {
    pub fn prepare_service_switch(
        &self,
        journal: &ProductJournal,
        request: p::ProductReleaseRequest,
    ) -> p::Result<PreparedReleaseServiceSwitch> {
        request.validate()?;
        if request.install_ref != self.installation.install_id
            || journal.release_images_claimed(&request.operation_id)?
        {
            return Err(unavailable());
        }
        let revision = journal.revision()?;
        let target = self.verified_staged(
            journal,
            &request.target_release_ref,
            &request.target_release_digest,
        )?;
        let original = if request.original_release_ref == self.installation.release_id {
            if request.original_release_digest != self.installation.release_digest {
                return Err(unavailable());
            }
            None
        } else {
            Some(self.verified_staged(
                journal,
                &request.original_release_ref,
                &request.original_release_digest,
            )?)
        };
        let source = original
            .as_ref()
            .map_or(self.installation.paths.releases.as_path(), |r| {
                r.content_root()
            });
        let mut changes = Vec::new();
        for service in [
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
            p::BuiltinServiceId::Host,
        ] {
            let suffix = if self.target() == p::ProductTarget::WindowsX86_64 {
                ".exe"
            } else {
                ""
            };
            let binary = format!("bin/{}{suffix}", service.binary_name());
            let config = self.installation.paths.config.join(match service {
                p::BuiltinServiceId::Host => "host.json",
                p::BuiltinServiceId::Web => "web.json",
                p::BuiltinServiceId::Brain => "core/core.json",
            });
            reject_linked_ancestors(&config)?;
            if !config.is_file() {
                return Err(unavailable());
            }
            changes.push(ReleaseServiceImageChange {
                service,
                source_digest: file_digest(&source.join(&binary))?,
                target_digest: file_digest(&target.content_root().join(&binary))?,
                config_digest: file_digest(&config)?,
                source: source.join(&binary),
                target: target.content_root().join(binary),
                config,
            });
        }
        let plan = PreparedReleaseServiceSwitch {
            expires_at: journal.command(&request.operation_id)?.requested_deadline,
            request,
            target,
            original,
            installation_digest: self.installation.digest()?,
            initial_release_digest: self.installation.release_digest.clone(),
            changes,
            revision,
        };
        plan.verify_binding(journal, revision)?;
        Ok(plan)
    }
}

impl PreparedReleaseServiceSwitch {
    pub fn execute(
        self,
        journal: &ProductJournal,
        manager: &dyn ReleaseServiceImageManager,
        deadline: Instant,
    ) -> p::Result<p::ProductReleaseServiceReceipt> {
        self.verify_binding(journal, self.revision)?;
        self.check_deadline(deadline)?;
        self.recheck_files()?;
        let mut observations = Vec::new();
        for change in &self.changes {
            let snapshot = manager.verify_original(change)?;
            p::validate_product_digest(&snapshot)?;
            observations.push(snapshot);
        }
        self.check_deadline(deadline)?;
        let prepared = p::canonical_digest(&(
            &self.request,
            &observations,
            self.changes
                .iter()
                .map(|c| {
                    (
                        &c.source_digest,
                        &c.target_digest,
                        &c.config,
                        &c.config_digest,
                    )
                })
                .collect::<Vec<_>>(),
        ))?;
        journal.claim_release_images(
            self.revision,
            &self.request.operation_id,
            self.request.command_digest.clone(),
            prepared,
            current_time()?,
        )?;
        let claimed_revision = journal.revision()?;
        let mut images = Vec::new();
        for (change, observed) in self.changes.iter().zip(observations.iter()) {
            self.verify_binding(journal, claimed_revision)?;
            self.check_deadline(deadline)?;
            self.recheck_files()?;
            let configuration_digest = manager.switch_and_verify(change, observed)?;
            p::validate_product_digest(&configuration_digest)?;
            self.check_deadline(deadline)?;
            images.push(p::ProductReleaseServiceImageReceipt {
                schema_version: p::SchemaVersion(1),
                service: change.service,
                source_image_digest: change.source_digest.clone(),
                target_image_digest: change.target_digest.clone(),
                configuration_digest,
            });
        }
        self.verify_binding(journal, claimed_revision)?;
        let receipt = p::ProductReleaseServiceReceipt {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.operation_id.clone(),
            command_digest: self.request.command_digest.clone(),
            images,
        };
        receipt.validate_for(&self.request)?;
        Ok(receipt)
    }

    fn recheck_files(&self) -> p::Result<()> {
        self.target.recheck()?;
        if let Some(original) = &self.original {
            original.recheck()?;
        }
        for change in &self.changes {
            reject_linked_ancestors(&change.config)?;
            if !change.config.is_file()
                || file_digest(&change.config)? != change.config_digest
                || file_digest(&change.source)? != change.source_digest
                || file_digest(&change.target)? != change.target_digest
            {
                return Err(unavailable());
            }
        }
        Ok(())
    }

    fn check_deadline(&self, deadline: Instant) -> p::Result<()> {
        let now = current_time()?;
        if Instant::now() >= deadline || now >= self.expires_at {
            return Err(unavailable());
        }
        Ok(())
    }

    fn verify_binding(&self, journal: &ProductJournal, expected: u64) -> p::Result<()> {
        let op = journal.operation(&self.request.operation_id)?;
        let command = journal.command(&self.request.operation_id)?;
        let kind_matches = match (&command.kind, self.request.change) {
            (
                p::MaintenanceKind::Upgrade {
                    release_id,
                    release_digest,
                    backup_ref,
                },
                p::ProductReleaseChange::Upgrade,
            )
            | (
                p::MaintenanceKind::RollbackCompatibleRelease {
                    release_id,
                    release_digest,
                    backup_ref,
                },
                p::ProductReleaseChange::RollbackCompatibleRelease,
            ) => {
                release_id == &self.request.target_release_ref
                    && release_digest == &self.request.target_release_digest
                    && backup_ref == &self.request.backup_ref
            }
            _ => false,
        };
        if !kind_matches
            || journal.revision()? != expected
            || op.stage != p::MaintenanceStage::BackedUp
            || op.pending_stage != Some(p::MaintenanceStage::Switching)
            || op.command_digest != self.request.command_digest
            || op.original_release_ref != self.request.original_release_ref
            || op.target_release_ref != self.request.target_release_ref
            || op.backup_ref.as_ref() != Some(&self.request.backup_ref)
            || journal.installation_verification()?
                != Some((
                    self.installation_digest.clone(),
                    self.initial_release_digest.clone(),
                ))
            || journal.active_release()?
                != Some((
                    self.request.original_release_ref.clone(),
                    self.request.original_release_digest.clone(),
                ))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

fn unavailable() -> p::Error {
    p::Error("native release image switch requires the original one-shot transaction".into())
}

fn current_time() -> p::Result<p::Timestamp> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| unavailable())?
        .as_millis()
        .try_into()
        .map_err(|_| unavailable())
}
