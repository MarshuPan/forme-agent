//! Offline observation of an existing original, with no writer or repair path.
use super::*;
use crate::installed::{management_repair::storage::ReadCustody, validate_installation_paths};

/// Observed facts only. This cannot be deserialized into an authorization,
/// recovery review, repair receipt or a new journal writer.
#[derive(Debug, Serialize)]
pub struct LegacyRepairSourceSnapshot {
    pub schema_version: p::SchemaVersion,
    pub observed_at: p::Timestamp,
    pub install_ref: String,
    pub installation_digest: p::SchemaDigest,
    pub journal_seq: u64,
    pub journal_head: p::SchemaDigest,
    pub journal_bytes: u64,
    pub journal_digest: p::SchemaDigest,
    pub journal_schema_floor: u32,
    pub owner: p::OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: p::SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: p::SchemaDigest,
    pub stage: p::MaintenanceStage,
    pub pending_stage: Option<p::MaintenanceStage>,
    pub config_revision: u64,
    pub config_digest: p::SchemaDigest,
    pub takeover_digest: p::SchemaDigest,
    pub takeover_deadline: p::Timestamp,
    pub images_claimed: bool,
    pub checking_prepared: bool,
    pub recovery_baseline_present: bool,
}

/// Reads only the installation's exact manifest and fixed original journal.
/// Missing/empty files, links, occupied originals or replay failures are errors;
/// this does not open managed configuration or start any runtime/service.
pub fn inspect_existing_legacy_source(
    installation_path: &Path,
    original_id: &str,
    now: p::Timestamp,
) -> p::Result<LegacyRepairSourceSnapshot> {
    let mut custody = ReadCustody::default();
    let installation: p::InstallationManifest = custody.json(installation_path, 64 * 1024)?;
    validate_installation_paths(&installation)?;
    if installation_path != installation.paths.config.join("installation.json") {
        return Err(invalid_journal());
    }
    let snapshot = inspect_at(
        &mut custody,
        &installation.paths.host_state.join("host.journal"),
        &installation.install_id,
        &installation.digest()?,
        original_id,
        now,
    )?;
    if snapshot.original_release_ref != installation.release_id
        || snapshot.original_release_digest != installation.release_digest
    {
        return Err(invalid_journal());
    }
    custody.recheck_records()?;
    Ok(snapshot)
}

pub(super) fn inspect_at(
    custody: &mut ReadCustody,
    path: &Path,
    install_id: &str,
    installation_digest: &p::SchemaDigest,
    original_id: &str,
    now: p::Timestamp,
) -> p::Result<LegacyRepairSourceSnapshot> {
    p::validate_product_id(install_id)?;
    p::validate_product_id(original_id)?;
    p::validate_product_digest(installation_digest)?;
    if now < 0 {
        return Err(invalid_journal());
    }
    let bytes = custody.read_exclusive(path, MAX_JOURNAL_BYTES)?;
    if bytes.is_empty() {
        return Err(invalid_journal());
    }
    let projection = replay(&bytes)?;
    if projection.install_id.as_deref() != Some(install_id)
        || projection
            .installation_verification
            .as_ref()
            .is_none_or(|(digest, _)| digest != installation_digest)
    {
        return Err(invalid_journal());
    }
    let operation = projection
        .operations
        .get(original_id)
        .ok_or_else(invalid_journal)?;
    let takeover = operation
        .release_takeover
        .as_ref()
        .ok_or_else(invalid_journal)?;
    let release = &takeover.release;
    let active = projection
        .active_release
        .as_ref()
        .ok_or_else(invalid_journal)?;
    if takeover.installation_digest != *installation_digest
        || release.install_ref != install_id
        || release.operation_id != original_id
        || release.command_digest != operation.view.command_digest
        || release.original_release_ref != active.release_id
        || release.original_release_digest != active.release_digest
    {
        return Err(invalid_journal());
    }
    custody.recheck_records()?;
    Ok(LegacyRepairSourceSnapshot {
        schema_version: p::SchemaVersion(1),
        observed_at: now,
        install_ref: install_id.into(),
        installation_digest: installation_digest.clone(),
        journal_seq: projection.sequence,
        journal_head: projection.head()?,
        journal_bytes: bytes.len() as u64,
        journal_digest: p::sha256_content_digest(&bytes),
        journal_schema_floor: projection.schema_floor,
        owner: takeover.owner.clone(),
        original_operation_id: original_id.into(),
        original_command_digest: operation.view.command_digest.clone(),
        original_release_ref: active.release_id.clone(),
        original_release_digest: active.release_digest.clone(),
        stage: operation.view.stage,
        pending_stage: operation.view.pending_stage,
        config_revision: takeover.expected_config_revision,
        config_digest: takeover.expected_config_digest.clone(),
        takeover_digest: takeover.digest()?,
        takeover_deadline: takeover.deadline,
        images_claimed: operation.release_images_claimed,
        checking_prepared: operation.release_checking.is_some(),
        recovery_baseline_present: operation.release_recovery_baseline.is_some(),
    })
}

fn replay(bytes: &[u8]) -> p::Result<Projection> {
    let mut projection = Projection::default();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.len() > MAX_RECORD_BYTES || line.last() != Some(&b'\n') {
            return Err(invalid_journal());
        }
        let envelope: JournalEnvelope =
            serde_json::from_slice(line).map_err(|_| invalid_journal())?;
        let raw: PersistedFact<'_> = serde_json::from_slice(line).map_err(|_| invalid_journal())?;
        let digest = p::canonical_digest(&(
            envelope.schema_version,
            envelope.product_seq,
            &envelope.previous,
            raw.fact,
        ))?;
        projection.fold_with_digest_and_max(&envelope, digest, PRODUCT_JOURNAL_READER_VERSION)?;
    }
    Ok(projection)
}
