//! Independently signed management repair, separate from business release state.
//! Loading verifies artifacts. Only an actual native inspection proves the
//! current images, configuration, SCM registration and fixed updater capability.
mod objects;
mod storage;

use std::{path::PathBuf, time::Instant};

use forme_protocol as p;

use super::{
    file_digest, validate_installation_paths, ReleaseKeyring, ReleaseStagingConfig, VerifiedRelease,
};
use objects::{RepairPlan, RepairReceipt, SignedPlan, SignedReceipt, IMAGE_LIMIT, SCHEMA};
use storage::{closed_directory, ReadCustody};

const CONFIG_FILES: [&str; 6] = [
    "host.json",
    "web.json",
    "worker.json",
    "product-template.json",
    "core/core.json",
    "installation.json",
];

/// No Deserialize or caller-supplied paths/verification flags. The installation
/// and trust must come from the administrator-owned Host configuration. This
/// handle proves signed artifact binding, not a completed native repair.
pub struct ManagementReaderRepairEvidence {
    installation: p::InstallationManifest,
    plan: RepairPlan,
    receipt: RepairReceipt,
    receipt_digest: p::SchemaDigest,
    release: VerifiedRelease,
    distribution: PathBuf,
    custody: ReadCustody,
}

impl ManagementReaderRepairEvidence {
    /// Read-only, fixed-layout load; never stages or activates a release. The
    /// original journal checkpoint must be supplied by its sole writer, and its
    /// subsequent causal continuity must still be verified by the collector.
    pub fn load(
        installation: &p::InstallationManifest,
        trust: &ReleaseStagingConfig,
        binding: &p::LegacyReleaseReviewBinding,
        now: p::Timestamp,
    ) -> p::Result<Self> {
        validate_installation_paths(installation)?;
        trust.validate()?;
        binding.validate()?;
        if trust.install_ref != installation.install_id || now < 0 {
            return Err(invalid());
        }
        let mut custody = ReadCustody::default();
        let root = installation.paths.host_state.join("management-repair");
        let signed_receipt: SignedReceipt = custody.json(&root.join("receipt.json"), 64 * 1024)?;
        let receipt = signed_receipt.receipt;
        p::validate_product_id(&receipt.repair_id)?;
        let directory = p::sha256_content_digest(receipt.repair_id.as_bytes());
        let directory = &directory.0[7..];
        closed_directory(&root, &["receipt.json", directory])?;
        let repair = root.join(directory);
        closed_directory(&repair, &["plan.json", "distribution"])?;
        let signed_plan: SignedPlan = custody.json(&repair.join("plan.json"), 64 * 1024)?;
        let plan = signed_plan.plan;
        plan.validate_for(installation, binding)?;
        receipt.validate_for(&plan, now)?;
        if signed_receipt.schema_version != SCHEMA
            || signed_plan.schema_version != SCHEMA
            || signed_receipt.signature.len() != 64
            || signed_plan.signature.len() != 64
        {
            return Err(invalid());
        }
        let distribution = repair.join("distribution");
        closed_directory(
            &distribution,
            &["manifest.json", "signature.ed25519", "content"],
        )?;
        let manifest: p::ReleaseManifest =
            custody.json(&distribution.join("manifest.json"), 8 * 1024 * 1024)?;
        manifest.validate()?;
        if manifest.digest()? != plan.distribution_digest || manifest.minimum_upgrader != 3 {
            return Err(invalid());
        }
        let signature = custody.read(&distribution.join("signature.ed25519"), 64)?;
        let content = distribution.join("content");
        let mut total = 0_u64;
        for entry in &manifest.files {
            total = total.checked_add(entry.size).ok_or_else(invalid)?;
            if total > 2 * 1024 * 1024 * 1024
                || custody.pin(&content.join(&entry.relative_path), IMAGE_LIMIT)? != entry.size
            {
                return Err(invalid());
            }
        }
        let keys = ReleaseKeyring::from_pinned_keys(trust.pinned_keys.clone())?;
        let release = keys.verify(&manifest, &signature, &content, installation.target)?;
        keys.verify_digest(
            &manifest.signature_ref,
            &plan.digest()?,
            &signed_plan.signature,
        )?;
        let receipt_digest = receipt.digest()?;
        keys.verify_digest(
            &manifest.signature_ref,
            &receipt_digest,
            &signed_receipt.signature,
        )?;
        for image in plan.images.iter().filter(|entry| entry.image.may_change()) {
            let name = format!("bin/{}", image.image.binary());
            let entry = manifest
                .files
                .iter()
                .find(|entry| entry.relative_path == name)
                .ok_or_else(invalid)?;
            if image.target_size != entry.size || image.target_digest != entry.digest {
                return Err(invalid());
            }
        }
        Ok(Self {
            installation: installation.clone(),
            plan,
            receipt,
            receipt_digest,
            release,
            distribution: content,
            custody,
        })
    }

    /// Stable digest of the independent receipt; not an installation verdict.
    pub fn receipt_digest(&self) -> &p::SchemaDigest {
        &self.receipt_digest
    }

    /// Actual platform verification. Keeps the exact images/configuration under
    /// read custody until the caller releases/drops the inspection. It performs
    /// only an offline capabilities probe and read-only OS queries, never
    /// stop/start, data migration, journal append or an original upgrade retry.
    pub fn inspect_current(self, deadline: Instant) -> p::Result<ManagementReaderRepairInspection> {
        self.inspect_with(deadline, native_observation)
    }

    fn inspect_with(
        self,
        deadline: Instant,
        observe: fn(&p::InstallationManifest) -> p::Result<NativeObservation>,
    ) -> p::Result<ManagementReaderRepairInspection> {
        ensure_time(deadline)?;
        let mut custody = ReadCustody::default();
        for image in &self.plan.images {
            let path = self
                .installation
                .paths
                .releases
                .join("bin")
                .join(image.image.binary());
            if custody.pin(&path, IMAGE_LIMIT)? != image.target_size {
                return Err(invalid());
            }
        }
        for name in CONFIG_FILES {
            custody.pin(&self.installation.paths.config.join(name), 4 * 1024 * 1024)?;
        }
        let inspection = ManagementReaderRepairInspection {
            evidence: self,
            _custody: custody,
            observe,
        };
        inspection.recheck(deadline)?;
        Ok(inspection)
    }
}

/// A native inspection guard, distinct from a decoded/signed repair receipt.
pub struct ManagementReaderRepairInspection {
    evidence: ManagementReaderRepairEvidence,
    _custody: ReadCustody,
    observe: fn(&p::InstallationManifest) -> p::Result<NativeObservation>,
}

impl ManagementReaderRepairInspection {
    /// Repeat under the collector's same journal/data/worker critical section
    /// immediately before its independent decision is committed.
    pub fn recheck(&self, deadline: Instant) -> p::Result<p::SchemaDigest> {
        ensure_time(deadline)?;
        let evidence = &self.evidence;
        evidence.custody.recheck_records()?;
        let root = evidence
            .installation
            .paths
            .host_state
            .join("management-repair");
        let directory = p::sha256_content_digest(evidence.plan.repair_id.as_bytes());
        let directory = &directory.0[7..];
        closed_directory(&root, &["receipt.json", directory])?;
        closed_directory(&root.join(directory), &["plan.json", "distribution"])?;
        closed_directory(
            evidence.distribution.parent().ok_or_else(invalid)?,
            &["manifest.json", "signature.ed25519", "content"],
        )?;
        evidence.release.recheck(&evidence.distribution)?;
        for image in &evidence.plan.images {
            ensure_time(deadline)?;
            let path = evidence
                .installation
                .paths
                .releases
                .join("bin")
                .join(image.image.binary());
            if file_digest(&path)? != image.target_digest {
                return Err(invalid());
            }
        }
        let mut configurations = Vec::new();
        for name in CONFIG_FILES {
            ensure_time(deadline)?;
            configurations.push((
                name,
                file_digest(&evidence.installation.paths.config.join(name))?,
            ));
        }
        let config_digest =
            p::canonical_digest(&("forme-release-recovery-configs-v1", configurations))?;
        if config_digest != evidence.plan.config_files_digest {
            return Err(invalid());
        }
        let observation = (self.observe)(&evidence.installation)?;
        if observation.services != evidence.plan.service_configuration_digest
            || observation.capabilities.schema_version != SCHEMA
            || observation.capabilities.journal_read_max != 3
            || observation.capabilities.updater_version != 3
        {
            return Err(invalid());
        }
        observation.capabilities.validate()?;
        ensure_time(deadline)?;
        p::canonical_digest(&(
            "forme-management-reader-repair-inspection-v1",
            &evidence.receipt_digest,
            &evidence.receipt.installed_images_digest,
            config_digest,
            observation.services,
            observation.capabilities,
        ))
    }
}

struct NativeObservation {
    services: p::SchemaDigest,
    capabilities: p::ProductReleaseRecoveryCapabilities,
}

fn native_observation(installation: &p::InstallationManifest) -> p::Result<NativeObservation> {
    #[cfg(windows)]
    {
        use super::ReleaseUpdaterPlatform;
        let services = super::SystemReleaseUpdaterPlatform
            .observe_recovery_boundary(installation, &installation.paths.releases)?;
        let capabilities = super::release_updater::capabilities::probe(installation)?;
        Ok(NativeObservation {
            services,
            capabilities,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = installation;
        Err(invalid())
    }
}

fn ensure_time(deadline: Instant) -> p::Result<()> {
    if Instant::now() >= deadline {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> p::Error {
    p::Error("independent management repair evidence is unavailable or inconsistent".into())
}

#[cfg(test)]
mod tests;
