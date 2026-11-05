//! Fixed-layout, read-only candidate discovery. Never creates a brain or claim.
use super::bootstrap_files::BootstrapTree;
use super::{invalid, OfflineBrainMaintenance, StorageConfig};
use forme_harness::offline_data::BootstrapSourceCustody;
use forme_protocol as p;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const ROOT_LIMIT: usize = 4;

// Closed projection: ignored credential paths are never opened by discovery.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceConfig {
    schema_version: p::SchemaVersion,
    install_ref: String,
    journal_path: PathBuf,
    signing_key_path: PathBuf,
    web_credential_path: PathBuf,
    new_brain_directory: PathBuf,
    candidate_ref: String,
    #[serde(default)]
    historical_brain_directory: Option<PathBuf>,
    #[serde(default)]
    verified_backup_directory: Option<PathBuf>,
}

/// Retains the actual source locks until the private worker sends its result.
/// A catalog alone is an observation, not authorization to copy or activate.
pub struct OfflineBootstrapInspection {
    catalog: p::BootstrapCandidateCatalogV2,
    custody: BootstrapSourceCustody,
    sources: Vec<(String, VerifiedSource)>,
}

pub(super) struct VerifiedSource {
    pub data: OfflineBrainMaintenance,
    pub root: PathBuf,
    pub owner: p::OwnerPrincipalRef,
    pub workspace: p::WorkspaceRef,
    pub keys: BootstrapTree,
    pub artifacts: BootstrapTree,
    custody: BootstrapSourceCustody,
}

impl OfflineBootstrapInspection {
    pub fn open(
        manifest_path: &Path,
        request: &p::BootstrapCandidateCatalogRequestV2,
        epoch: u64,
    ) -> p::Result<Self> {
        request.validate()?;
        if epoch == 0 {
            return Err(invalid());
        }
        let mut custody = BootstrapSourceCustody::default();
        let installation: p::InstallationManifest =
            serde_json::from_slice(&custody.read(manifest_path, 256 * 1024)?)
                .map_err(|_| invalid())?;
        installation.digest()?;
        if manifest_path != installation.paths.config.join("installation.json") {
            return Err(invalid());
        }
        let config_path = installation.paths.config.join("bootstrap.json");
        let config: SourceConfig = serde_json::from_slice(&custody.read(&config_path, 32 * 1024)?)
            .map_err(|_| invalid())?;
        config.validate(&installation)?;
        let mut catalog = p::BootstrapCandidateCatalogV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: installation.install_id.clone(),
            verification_epoch: epoch,
            candidates: vec![new_candidate(&config, epoch)?],
            catalog_digest: p::sha256_content_digest(b"pending"),
        };
        let mut sources = Vec::new();
        for (kind, root) in [
            (
                p::BootstrapCandidateKind::ExistingBrain,
                config.historical_brain_directory.as_ref(),
            ),
            (
                p::BootstrapCandidateKind::VerifiedBackup,
                config.verified_backup_directory.as_ref(),
            ),
        ] {
            let Some(root) = root else { continue };
            for entry in source_entries(root)? {
                let reference = p::canonical_digest(&(
                    "forme-bootstrap-source-reference-v2",
                    &config.install_ref,
                    kind,
                    entry
                        .file_name()
                        .ok_or_else(invalid)?
                        .to_str()
                        .ok_or_else(invalid)?,
                ))?;
                let mut candidate =
                    unavailable(kind, format!("candidate:{}", &reference.0[7..]), epoch);
                if let Ok((source, owner, config_digest, key_digest)) = inspect_source(&entry, kind)
                {
                    let identity = source.data.identity();
                    candidate.status = p::BootstrapCandidateStatus::Available;
                    candidate.owner_source = p::BootstrapOwnerSource::VerifiedFromCandidate;
                    candidate.owner_binding_digest = Some(p::canonical_digest(&(
                        "forme-bootstrap-owner-binding-v2",
                        &owner,
                    ))?);
                    candidate.owner_ref = Some(owner);
                    candidate.store_identity_digest = Some(identity.store_identity.clone());
                    candidate.schema_digest = Some(identity.schema_digest.clone());
                    candidate.protection_state = p::BootstrapProtectionState::Protected;
                    candidate.source_observation_digest = p::canonical_digest(&(
                        "forme-bootstrap-source-observation-v2",
                        identity,
                        source.data.database_schema_version()?,
                        config_digest,
                        key_digest,
                        source.artifacts.digest()?,
                        if kind == p::BootstrapCandidateKind::VerifiedBackup {
                            Some(p::canonical_digest(&source.data.verify()?.manifest)?)
                        } else {
                            None
                        },
                    ))?;
                    candidate.issue_ref = None;
                    sources.push((candidate.candidate_ref.clone(), source));
                }
                candidate.candidate_digest = candidate.expected_digest(&config.install_ref)?;
                catalog.candidates.push(candidate);
            }
        }
        catalog.catalog_digest = catalog.expected_digest()?;
        catalog.validate()?;
        let inspection = Self {
            catalog,
            custody,
            sources,
        };
        inspection.recheck()?;
        Ok(inspection)
    }

    pub fn catalog(&self) -> p::Result<&p::BootstrapCandidateCatalogV2> {
        self.recheck()?;
        Ok(&self.catalog)
    }

    pub(super) fn source(&self, reference: &str) -> p::Result<&VerifiedSource> {
        self.sources
            .iter()
            .find(|(candidate, _)| candidate == reference)
            .map(|(_, source)| source)
            .ok_or_else(invalid)
    }

    pub(super) fn recheck(&self) -> p::Result<()> {
        self.custody.recheck()?;
        for (_, source) in &self.sources {
            source.custody.recheck()?;
            source.data.database_schema_version()?;
            source.keys.recheck()?;
            source.artifacts.recheck()?;
        }
        Ok(())
    }
}

impl SourceConfig {
    fn validate(&self, installation: &p::InstallationManifest) -> p::Result<()> {
        p::validate_product_id(&self.candidate_ref)?;
        if self.schema_version != p::SchemaVersion(1)
            || self.install_ref != installation.install_id
            || self.journal_path != installation.paths.host_state.join("bootstrap.journal")
            || self.signing_key_path != installation.paths.keys.join("bootstrap.key")
            || self.web_credential_path != installation.paths.web_auth.join("bootstrap-web.token")
            || self.new_brain_directory != installation.paths.brain
            || self
                .historical_brain_directory
                .as_ref()
                .is_some_and(|path| *path != installation.paths.backups.join("historical-brains"))
            || self
                .verified_backup_directory
                .as_ref()
                .is_some_and(|path| *path != installation.paths.backups.join("candidates"))
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn source_entries(root: &Path) -> p::Result<Vec<PathBuf>> {
    crate::web::config::check_path(root)?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(invalid()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        if paths.len() >= ROOT_LIMIT {
            return Err(invalid());
        }
        paths.push(entry.map_err(|_| invalid())?.path());
    }
    paths.sort();
    Ok(paths)
}

fn inspect_source(
    root: &Path,
    kind: p::BootstrapCandidateKind,
) -> p::Result<(
    VerifiedSource,
    p::OwnerPrincipalRef,
    p::SchemaDigest,
    p::SchemaDigest,
)> {
    let mut custody = BootstrapSourceCustody::default();
    let bytes = custody.read(&root.join("core.json"), 256 * 1024)?;
    let config: StorageConfig = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let database = root.join("forme.db");
    let keys = root.join("forme.content-keys");
    let artifacts = root.join("artifacts");
    if config.schema_version != p::SchemaVersion(1)
        || config.brain.schema_version != p::SchemaVersion(1)
        || config.revision == 0
        || config.brain.store_path != database
        || config.brain.artifact_root != artifacts
        || config
            .brain
            .content_key_directory
            .as_ref()
            .is_some_and(|path| *path != keys)
    {
        return Err(invalid());
    }
    config.brain.owner.validate()?;
    p::validate_product_id(&config.install_ref)?;
    crate::web::config::check_path(&artifacts)?;
    if !artifacts.is_dir() {
        return Err(invalid());
    }
    let key_tree = BootstrapTree::open(&keys, true)?;
    let key_digest = key_tree.digest()?;
    let artifact_tree = BootstrapTree::open(&artifacts, false)?;
    let backup =
        (kind == p::BootstrapCandidateKind::VerifiedBackup).then(|| root.join("backup.db"));
    let source = backup.as_ref().unwrap_or(&database);
    custody.pin(source, 64 * 1024 * 1024 * 1024)?;
    custody.pin_existing_fence(&source.with_extension("access-lock"))?;
    if let Some(backup) = &backup {
        // Store owns backup-manifest semantics; custody prevents aliases.
        let mut manifest = backup.as_os_str().to_os_string();
        manifest.push(".manifest.json");
        custody.read(Path::new(&manifest), 256 * 1024)?;
    }
    let data = OfflineBrainMaintenance::inspect_storage(
        &p::VerifiedPrincipal(config.brain.owner.0.clone()),
        p::Scope(config.brain.workspace.0.clone()),
        &database,
        &artifacts,
        &keys,
        backup.as_deref(),
    )?;
    // A plaintext authority digest by itself cannot establish the owner.
    // Historical identity requires at least one actually authenticated record.
    if data.identity().authenticated_content_rows == 0 {
        return Err(invalid());
    }
    if backup.is_none() {
        data.require_restoration_fence(config.brain.expected_restoration_fence.as_ref())?;
    }
    custody.recheck()?;
    Ok((
        VerifiedSource {
            data,
            root: root.to_path_buf(),
            owner: config.brain.owner.clone(),
            workspace: config.brain.workspace,
            keys: key_tree,
            artifacts: artifact_tree,
            custody,
        },
        config.brain.owner,
        p::sha256_content_digest(&bytes),
        key_digest,
    ))
}

fn new_candidate(config: &SourceConfig, epoch: u64) -> p::Result<p::BootstrapCandidateViewV2> {
    let mut candidate = unavailable(
        p::BootstrapCandidateKind::NewBrainTarget,
        config.candidate_ref.clone(),
        epoch,
    );
    candidate.owner_source = p::BootstrapOwnerSource::GeneratedForNewBrain;
    crate::web::config::check_path(&config.new_brain_directory)?;
    let empty = std::fs::read_dir(&config.new_brain_directory)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false);
    if empty {
        candidate.status = p::BootstrapCandidateStatus::Available;
        candidate.protection_state = p::BootstrapProtectionState::Protected;
        candidate.issue_ref = None;
    }
    candidate.source_observation_digest =
        p::canonical_digest(&("forme-bootstrap-empty-target-v2", empty))?;
    candidate.candidate_digest = candidate.expected_digest(&config.install_ref)?;
    Ok(candidate)
}

fn unavailable(
    kind: p::BootstrapCandidateKind,
    candidate_ref: String,
    epoch: u64,
) -> p::BootstrapCandidateViewV2 {
    p::BootstrapCandidateViewV2 {
        schema_version: p::SchemaVersion(2),
        candidate_ref,
        kind,
        status: p::BootstrapCandidateStatus::Unverifiable,
        candidate_digest: p::sha256_content_digest(b"pending"),
        verification_epoch: epoch,
        owner_source: p::BootstrapOwnerSource::NotEstablished,
        owner_ref: None,
        owner_binding_digest: None,
        store_identity_digest: None,
        schema_digest: None,
        protection_state: p::BootstrapProtectionState::Unknown,
        source_observation_digest: p::sha256_content_digest(
            b"forme-bootstrap-source-not-verified-v2",
        ),
        issue_ref: Some("issue:bootstrap-source-unverified".into()),
    }
}
