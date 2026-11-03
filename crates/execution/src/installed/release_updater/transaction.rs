//! Runs after a one-shot SCM dispatch; no automatic repeat on any uncertain effect.
use super::diagnostics::{Progress, Stage};
use super::*;
use crate::installed::{InstalledReleaseDataWorker, ProductConfigStore};

pub(super) fn run(updater: &InstalledReleaseUpdater) -> p::Result<()> {
    let mut progress = Progress::default();
    let result = execute(updater, &mut progress);
    if let Err(error) = &result {
        // A diagnostic never claims a journal writer, retries an OS effect or
        // replaces the original failure when the private log cannot be saved.
        let _ = diagnostics::record(
            &updater.installation.paths.logs.join("host"),
            &progress,
            error,
        );
    }
    result
}

fn execute(updater: &InstalledReleaseUpdater, progress: &mut Progress) -> p::Result<()> {
    let takeover: p::ProductReleaseTakeover = storage::read(&updater.handoff_path())?;
    takeover.validate()?;
    if takeover.installation_digest != updater.installation.digest()?
        || takeover.release.install_ref != updater.installation.install_id
    {
        return Err(invalid());
    }
    progress.bind(&takeover);
    progress.set(Stage::VerifyUpdater, None);
    let limit = deadline(takeover.deadline)?;
    updater
        .platform
        .verify_updater(&updater.installation, &updater.host_config)?;
    progress.set(Stage::VerifyTarget, None);
    let target = updater.stager.takeover_content(
        &takeover.release.target_release_ref,
        &takeover.release.target_release_digest,
    )?;
    target.recheck()?;
    if target.manifest().minimum_upgrader > RELEASE_UPDATER_VERSION {
        return Err(invalid());
    }
    let original = source_root(updater, &takeover.release)?;
    progress.set(Stage::ClaimDispatch, None);
    storage::claim(updater, &takeover)?;
    // All preconditions before the first stop. A protected handoff has already
    // been journal-bound by Host; writer transfer cannot happen before Host exits.
    for id in services() {
        progress.set(Stage::ObserveOriginalService, Some(id));
        updater
            .platform
            .observe(&updater.installation, &original, id, None)?;
    }
    for id in services().into_iter().rev() {
        progress.set(Stage::StopOriginalService, Some(id));
        valid_time(takeover.deadline, limit)?;
        updater.platform.change(
            &updater.installation,
            &original,
            id,
            p::NativeServiceAction::Stop,
            limit,
        )?;
    }
    progress.set(Stage::AcquireJournal, None);
    let journal = ProductJournal::open(
        &updater.installation.paths.host_state.join("host.journal"),
        &updater.installation.install_id,
    )?;
    let result = prepare_target(updater, &journal, &takeover, limit, progress);
    if let Err(error) = result {
        journal.require_recovery(
            &takeover.release.operation_id,
            journal.revision()?,
            p::canonical_digest(&error.to_string())?,
            now()?,
        )?;
        return Err(error);
    }
    let root = result?;
    // Both writer locks have been released before starting the new Host.
    drop(journal);
    for id in services() {
        progress.set(Stage::StartTargetService, Some(id));
        valid_time(takeover.deadline, limit)?;
        if let Err(error) = updater.platform.change(
            &updater.installation,
            &root,
            id,
            p::NativeServiceAction::Start,
            limit,
        ) {
            record_failed_start(updater, &takeover, &root, &error)?;
            return Err(error);
        }
    }
    Ok(())
}

fn prepare_target(
    updater: &InstalledReleaseUpdater,
    journal: &ProductJournal,
    takeover: &p::ProductReleaseTakeover,
    limit: Instant,
    progress: &mut Progress,
) -> p::Result<PathBuf> {
    use p::MaintenanceStage as Stage;
    progress.set(diagnostics::Stage::VerifyTakeover, None);
    updater.verify_takeover(journal, takeover)?;
    updater.preflight(journal, &takeover.release)?;
    let mutex = ProductConfigStore::open(
        &updater
            .installation
            .paths
            .config
            .join("product-template.json"),
    )?;
    let mut config = mutex.lock().map_err(|_| invalid())?;
    if config.release_configuration_identity()?
        != (
            takeover.owner.clone(),
            takeover.expected_config_revision,
            takeover.expected_config_digest.clone(),
        )
    {
        return Err(invalid());
    }
    progress.set(diagnostics::Stage::InspectOriginalData, None);
    finish(
        journal,
        takeover,
        Stage::Quiescing,
        p::canonical_digest(&("release-services-stopped-v1", takeover))?,
    )?;
    begin(journal, takeover, Stage::BackedUp, limit)?;
    let request = p::ProductReleaseDataRequest {
        schema_version: p::SchemaVersion(1),
        release: takeover.release.clone(),
        owner: takeover.owner.clone(),
        expected_config_revision: takeover.expected_config_revision,
    };
    let original = updater.original(journal, &takeover.release)?;
    let worker = match original {
        Some(verified) => {
            InstalledReleaseDataWorker::from_verified(updater.installation.clone(), verified)?
        }
        None => InstalledReleaseDataWorker::new(updater.installation.clone())?,
    };
    let mut inspector = worker.start(request.clone(), limit)?;
    let identity = inspector.identity().clone();
    updater
        .target(journal, &takeover.release)?
        .check_runtime_compatibility(
            identity.config_schema_version,
            identity.database_schema_version,
            RELEASE_UPDATER_VERSION,
        )?;
    progress.set(diagnostics::Stage::BackupOriginalData, None);
    let backup = inspector.step(p::ProductReleaseDataStep::Backup)?;
    valid_time(takeover.deadline, limit)?;
    finish(
        journal,
        takeover,
        Stage::BackedUp,
        p::canonical_digest(&backup)?,
    )?;
    begin(journal, takeover, Stage::Switching, limit)?;
    progress.set(diagnostics::Stage::SwitchServiceImages, None);
    let receipt = updater.platform.switch_images(
        updater
            .stager
            .prepare_service_switch(journal, takeover.release.clone())?,
        journal,
        limit,
    )?;
    inspector.step(p::ProductReleaseDataStep::Release)?;
    drop(inspector);
    // Re-open via target code while the data is unchanged and core is stopped.
    progress.set(diagnostics::Stage::InspectTargetData, None);
    let verified = updater.target(journal, &takeover.release)?;
    let root = verified.content_root().to_owned();
    let target_worker =
        InstalledReleaseDataWorker::from_verified(updater.installation.clone(), verified)?;
    let mut target_inspector = target_worker.start(request, limit)?;
    if target_inspector.identity() != &identity {
        return Err(invalid());
    }
    target_inspector.step(p::ProductReleaseDataStep::Release)?;
    let core_image_digest = receipt
        .images
        .iter()
        .find(|r| r.service == p::BuiltinServiceId::Brain)
        .ok_or_else(invalid)?
        .target_image_digest
        .clone();
    let binding = p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: takeover.release.install_ref.clone(),
        operation_id: takeover.release.operation_id.clone(),
        command_digest: takeover.release.command_digest.clone(),
        target_release_ref: takeover.release.target_release_ref.clone(),
        target_release_digest: takeover.release.target_release_digest.clone(),
        core_image_digest,
        owner_digest: identity.owner_digest.clone(),
        store_identity: identity.store_identity.clone(),
        config_revision: takeover
            .expected_config_revision
            .checked_add(1)
            .ok_or_else(invalid)?,
        readiness_nonce: takeover.readiness_nonce.clone(),
    };
    valid_time(takeover.deadline, limit)?;
    progress.set(diagnostics::Stage::PublishReleaseBinding, None);
    config.publish_release_binding(takeover, binding.clone(), now()?)?;
    journal.change_active_release(
        &takeover.release.operation_id,
        journal.revision()?,
        &takeover.release.original_release_ref,
        &takeover.release.original_release_digest,
        &takeover.release.target_release_ref,
        &takeover.release.target_release_digest,
    )?;
    finish(
        journal,
        takeover,
        Stage::Switching,
        p::canonical_digest(&receipt)?,
    )?;
    begin(journal, takeover, Stage::Checking, limit)?;
    progress.set(diagnostics::Stage::PrepareChecking, None);
    journal.prepare_release_checking(
        journal.revision()?,
        p::ProductReleaseChecking {
            schema_version: p::SchemaVersion(1),
            takeover: takeover.clone(),
            binding,
            data_identity: identity,
            service_receipt: receipt,
        },
        now()?,
    )?;
    Ok(root)
}
fn source_root(
    updater: &InstalledReleaseUpdater,
    request: &p::ProductReleaseRequest,
) -> p::Result<PathBuf> {
    if request.original_release_ref == updater.installation.release_id {
        if request.original_release_digest != updater.installation.release_digest {
            return Err(invalid());
        }
        Ok(updater.installation.paths.releases.clone())
    } else {
        p::validate_release_id(&request.original_release_ref)?;
        let verified = updater.stager.takeover_content(
            &request.original_release_ref,
            &request.original_release_digest,
        )?;
        Ok(verified.content_root().to_owned())
    }
}
fn begin(
    journal: &ProductJournal,
    takeover: &p::ProductReleaseTakeover,
    stage: p::MaintenanceStage,
    limit: Instant,
) -> p::Result<()> {
    valid_time(takeover.deadline, limit)?;
    journal.begin_stage(
        &takeover.release.operation_id,
        journal.revision()?,
        stage,
        p::MaintenanceActorClass::OwnerReauthenticated,
        now()?,
    )
}
fn finish(
    journal: &ProductJournal,
    takeover: &p::ProductReleaseTakeover,
    stage: p::MaintenanceStage,
    receipt: p::SchemaDigest,
) -> p::Result<()> {
    journal.finish_stage(
        &takeover.release.operation_id,
        journal.revision()?,
        stage,
        vec![receipt],
        now()?,
    )
}
fn record_failed_start(
    updater: &InstalledReleaseUpdater,
    takeover: &p::ProductReleaseTakeover,
    root: &Path,
    error: &p::Error,
) -> p::Result<()> {
    // Never compete with a live Host writer or stop it merely to record failure.
    if updater
        .platform
        .observe(
            &updater.installation,
            root,
            p::BuiltinServiceId::Host,
            Some(p::NativeServiceState::Stopped),
        )
        .is_ok()
    {
        let journal = ProductJournal::open(
            &updater.installation.paths.host_state.join("host.journal"),
            &updater.installation.install_id,
        )?;
        journal.require_recovery(
            &takeover.release.operation_id,
            journal.revision()?,
            p::canonical_digest(&error.to_string())?,
            now()?,
        )?;
    }
    Ok(())
}
