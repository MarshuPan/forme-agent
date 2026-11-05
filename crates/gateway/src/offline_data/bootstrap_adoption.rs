//! Finite first-adoption worker. It never creates an account or starts a core.
use super::{
    bindings,
    bootstrap_files::{self as files, BootstrapTree},
    invalid, OfflineBootstrapInspection, OfflineBrainMaintenance,
};
use forme_harness::offline_data::BootstrapSourceCustody;
use forme_protocol as p;
use std::path::{Path, PathBuf};

pub struct OfflineBootstrapDataWorker {
    inspection: OfflineBootstrapInspection,
    target: OfflineBrainMaintenance,
    intent: p::BootstrapDataIntentV2,
    data: p::BootstrapDataPreparedResultV2,
    template: p::ProductConfigTemplate,
    journal_root: PathBuf,
    custody: BootstrapSourceCustody,
    keys: BootstrapTree,
    artifacts: BootstrapTree,
    confirmed: bool,
}

impl OfflineBootstrapDataWorker {
    pub fn prepare(manifest_path: &Path, intent: p::BootstrapDataIntentV2) -> p::Result<Self> {
        intent.validate()?;
        let mut custody = BootstrapSourceCustody::default();
        let installation: p::InstallationManifest =
            serde_json::from_slice(&custody.read(manifest_path, 256 * 1024)?)
                .map_err(|_| invalid())?;
        installation.digest()?;
        if installation.install_id != intent.install_ref
            || manifest_path != installation.paths.config.join("installation.json")
        {
            return Err(invalid());
        }
        let template: p::ProductConfigTemplate = serde_json::from_slice(&custody.read(
            &installation.paths.config.join("product-template.json"),
            256 * 1024,
        )?)
        .map_err(|_| invalid())?;
        bindings::validate_template(&installation, &template)?;
        if template
            .core_config_path
            .try_exists()
            .map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        let inspection = OfflineBootstrapInspection::open(
            manifest_path,
            &p::BootstrapCandidateCatalogRequestV2 {
                schema_version: p::SchemaVersion(2),
                session: intent.request.session.clone(),
            },
            intent.candidate.verification_epoch,
        )?;
        if inspection
            .catalog()?
            .candidate(&intent.candidate.candidate_ref)
            != Some(&intent.candidate)
        {
            return Err(invalid());
        }
        let source = inspection.source(&intent.candidate.candidate_ref)?;
        let reference = intent.target_ref()?;
        let journal_base = installation.paths.host_state.join("bootstrap-data");
        let target_base = installation.paths.brain.join("bootstrap-data");
        let journal_root = journal_base.join(&reference);
        let target_root = target_base.join(&reference);
        for root in [&journal_root, &target_root] {
            crate::web::config::check_path(root)?;
            if root.try_exists().map_err(|_| invalid())? {
                return Err(invalid());
            }
        }
        crate::web::config::check_path(&template.artifact_root)?;
        if template.artifact_root.exists()
            && (!template.artifact_root.is_dir()
                || std::fs::read_dir(&template.artifact_root)
                    .map_err(|_| invalid())?
                    .next()
                    .is_some())
        {
            return Err(invalid());
        }
        for base in [&journal_base, &target_base] {
            crate::web::config::check_path(base)?;
            if !base.exists() {
                files::directory(base)?;
            }
            if !base.is_dir() {
                return Err(invalid());
            }
        }
        files::directory(&journal_root)?;
        // Intent precedes any source-data copy, and is never overwritten.
        files::immutable(
            &journal_root.join("intent.json"),
            &serde_json::to_vec(&intent).map_err(|_| invalid())?,
        )?;
        files::directory(&target_root)?;
        let database = target_root.join("forme.db");
        let keys_path = target_root.join("forme.content-keys");
        source.keys.copy_into_empty(&keys_path)?;
        source.artifacts.copy_into_empty(&template.artifact_root)?;
        let staging = journal_root.join("source.db");
        let restore = if intent.candidate.kind == p::BootstrapCandidateKind::ExistingBrain {
            source.data.backup(&staging)?;
            let backup = OfflineBrainMaintenance::inspect_storage(
                &p::VerifiedPrincipal(source.owner.0.clone()),
                p::Scope(source.workspace.0.clone()),
                &source.root.join("forme.db"),
                &source.root.join("artifacts"),
                &source.root.join("forme.content-keys"),
                Some(&staging),
            )?;
            let plan = backup.plan_restore(&database)?;
            backup.restore(&database, &plan, &plan.digest)?
        } else {
            let plan = source.data.plan_restore(&database)?;
            source.data.restore(&database, &plan, &plan.digest)?
        };
        if !restore.source_unchanged
            || restore.authority_switched
            || !restore.verification.is_valid()
        {
            return Err(invalid());
        }
        let keys = BootstrapTree::open(&keys_path, true)?;
        let artifacts = BootstrapTree::open(&template.artifact_root, false)?;
        custody.pin(&database, 64 * 1024 * 1024 * 1024)?;
        custody.pin_existing_fence(&database.with_extension("access-lock"))?;
        let target = OfflineBrainMaintenance::inspect_storage(
            &p::VerifiedPrincipal(source.owner.0.clone()),
            p::Scope(source.workspace.0.clone()),
            &database,
            &template.artifact_root,
            &keys_path,
            Some(&database),
        )?;
        if target.identity() != &restore.restored
            || target.identity().authenticated_content_rows == 0
        {
            return Err(invalid());
        }
        let verification_digest = verification(&target, &keys, &artifacts)?;
        let binding = p::ProductDataBinding {
            schema_version: p::PRODUCT_DATA_BINDING_SCHEMA_VERSION,
            candidate_ref: intent.candidate.candidate_ref.clone(),
            store_path: database,
            content_key_directory: keys_path,
            owner_digest: target.identity().owner_digest.clone(),
            store_identity: target.identity().store_identity.clone(),
            manifest_digest: p::canonical_digest(&target.verify()?.manifest)?,
            execution_fence_digest: Some(target.restored_candidate_fence_digest()?),
        };
        let mut data = p::BootstrapDataPreparedResultV2 {
            schema_version: p::SchemaVersion(2),
            prepared: p::BootstrapClaimDataPreparedV2 {
                schema_version: p::SchemaVersion(2),
                operation_id: intent.request.session.operation_id.clone(),
                candidate_ref: intent.candidate.candidate_ref.clone(),
                candidate_digest: intent.candidate.candidate_digest.clone(),
                verification_epoch: intent.candidate.verification_epoch,
                target_binding_digest: p::sha256_content_digest(b"pending"),
                migration_outcome: p::BootstrapMigrationOutcome::Recovered,
                worker_receipt_digest: p::sha256_content_digest(b"pending"),
            },
            binding,
            artifact_digest: artifacts.digest()?,
            verification_digest,
        };
        data.prepared.target_binding_digest = data.target_digest()?;
        data.prepared.worker_receipt_digest = data.worker_digest(&intent)?;
        data.validate_for(&intent)?;
        inspection.recheck()?;
        custody.recheck()?;
        files::immutable(
            &journal_root.join("prepared.json"),
            &serde_json::to_vec(&data).map_err(|_| invalid())?,
        )?;
        Ok(Self {
            inspection,
            target,
            intent,
            data,
            template,
            journal_root,
            custody,
            keys,
            artifacts,
            confirmed: false,
        })
    }

    pub fn prepared(&self) -> p::Result<p::BootstrapDataWorkerResponseV2> {
        self.recheck()?;
        Ok(p::BootstrapDataWorkerResponseV2::Prepared {
            schema_version: p::SchemaVersion(2),
            data: self.data.clone(),
        })
    }

    pub fn step(
        &mut self,
        request: p::BootstrapDataWorkerRequestV2,
    ) -> p::Result<p::BootstrapDataWorkerResponseV2> {
        match request {
            p::BootstrapDataWorkerRequestV2::Confirm {
                schema_version,
                receipt,
                snapshot_ref,
                config_binding_digest,
            } if schema_version == p::SchemaVersion(2) && !self.confirmed => {
                let commit = p::BootstrapClaimCommitV2 {
                    schema_version,
                    receipt: receipt.clone(),
                };
                commit.validate_against_prepared(&self.data.prepared)?;
                if receipt.install_ref != self.intent.install_ref
                    || Some(&receipt.owner_ref) != self.intent.candidate.owner_ref.as_ref()
                    || receipt.account_commitment != self.intent.request.account_commitment
                    || snapshot_ref.len() != 32
                    || !snapshot_ref
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(invalid());
                }
                self.recheck()?;
                let snapshot_path = self
                    .template
                    .core_config_path
                    .with_extension("revisions")
                    .join(snapshot_ref)
                    .join("state.json");
                let bytes = self.custody.read(&snapshot_path, 128 * 1024)?;
                let state: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if p::sha256_content_digest(&bytes) != config_binding_digest
                    || state["install_ref"] != serde_json::json!(self.intent.install_ref)
                    || state["owner"] != serde_json::json!(receipt.owner_ref)
                    || state["revision"] != serde_json::json!(0)
                    || state["models"] != serde_json::json!([])
                    || state["credentials"] != serde_json::json!({})
                    || state["active_data"] != serde_json::json!(self.data.binding)
                    || self
                        .template
                        .core_config_path
                        .try_exists()
                        .map_err(|_| invalid())?
                {
                    return Err(invalid());
                }
                let observed = verification(&self.target, &self.keys, &self.artifacts)?;
                if observed != self.data.verification_digest {
                    return Err(invalid());
                }
                let confirmation = p::BootstrapDataBindingConfirmationV2 {
                    schema_version,
                    install_ref: self.intent.install_ref.clone(),
                    operation_id: receipt.operation_id.clone(),
                    receipt_digest: receipt.digest()?,
                    expected_config_revision: 0,
                    config_binding_digest,
                    target_binding_digest: self.data.target_digest()?,
                    execution_fence_digest: self.target.restored_candidate_fence_digest()?,
                    verification_digest: observed,
                };
                confirmation.validate_against_receipt(&receipt)?;
                self.custody.recheck()?;
                files::immutable(
                    &self.journal_root.join("confirmation.json"),
                    &serde_json::to_vec(&confirmation).map_err(|_| invalid())?,
                )?;
                self.confirmed = true;
                Ok(p::BootstrapDataWorkerResponseV2::Confirmed {
                    schema_version,
                    confirmation,
                })
            }
            p::BootstrapDataWorkerRequestV2::Release {
                schema_version,
                operation_id,
            } if schema_version == p::SchemaVersion(2)
                && self.confirmed
                && operation_id == self.intent.request.session.operation_id =>
            {
                self.recheck()?;
                Ok(p::BootstrapDataWorkerResponseV2::Released {
                    schema_version,
                    operation_id,
                })
            }
            _ => Err(invalid()),
        }
    }

    fn recheck(&self) -> p::Result<()> {
        self.inspection.recheck()?;
        self.custody.recheck()?;
        if verification(&self.target, &self.keys, &self.artifacts)? != self.data.verification_digest
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn verification(
    data: &OfflineBrainMaintenance,
    keys: &BootstrapTree,
    artifacts: &BootstrapTree,
) -> p::Result<p::SchemaDigest> {
    keys.recheck()?;
    artifacts.recheck()?;
    let verification = data.verify()?;
    if !verification.is_valid() {
        return Err(invalid());
    }
    p::canonical_digest(&(
        "forme-bootstrap-target-verification-v2",
        data.identity(),
        data.database_schema_version()?,
        verification,
        data.restored_candidate_fence_digest()?,
        keys.digest()?,
        artifacts.digest()?,
    ))
}
