//! Real read-only composition; missing protected provenance leaves it unavailable.
#[path = "native/material.rs"]
mod material;
#[path = "native/objects.rs"]
mod objects;
#[cfg(test)]
#[path = "native/tests.rs"]
mod tests;

use std::{sync::Arc, time::Instant};

use ed25519_dalek::VerifyingKey;
use forme_protocol as p;

use super::{
    LegacyReleaseReviewer, LegacyReviewCollector, LegacyReviewInspection, LegacyReviewObservations,
};
use crate::installed::{
    journal::LegacySourceContext as SourceContext,
    legacy_data_archive::{LegacyDataArchiveEvidence, LegacyDataArchiveTrust},
    InstalledReleaseDataWorker, ManagementReaderRepairEvidence, ManagementReaderRepairInspection,
    NativeServiceManager, ProductHostBroker, ReleaseRecoveryDataInspection, ReleaseStagingConfig,
};
use material::Materials;

impl<M: NativeServiceManager> ProductHostBroker<M> {
    /// Managed composition only. The fixed protected directory is optional;
    /// nothing is provisioned, signed or recovered by this builder. A configured
    /// but invalid source is an error rather than a silent fixture fallback.
    pub fn with_native_legacy_review(mut self, trust: ReleaseStagingConfig) -> p::Result<Self> {
        let installation = self
            .release_updater
            .as_ref()
            .ok_or_else(invalid)?
            .installation()
            .clone();
        trust.validate()?;
        if trust.install_ref != self.install_id || installation.install_id != self.install_id {
            return Err(invalid());
        }
        let Some(materials) = Materials::open(installation, &self.journal)? else {
            return Ok(self);
        };
        let repair = ManagementReaderRepairEvidence::load(
            &materials.installation,
            &trust,
            &materials.signed.review.binding,
            super::timestamp()?,
        )?;
        repair.check_legacy_sources(
            &materials.sources.image_bindings(),
            &materials.sources.config_files_digest,
            &materials.sources.service_configuration_digest,
        )?;
        let key =
            VerifyingKey::from_bytes(&materials.trust.review_public_key).map_err(|_| invalid())?;
        self.legacy_release_reviewer = Some(LegacyReleaseReviewer {
            key,
            collector: Arc::new(NativeCollector {
                materials: Arc::new(materials),
                release_trust: trust,
            }),
        });
        Ok(self)
    }
}

struct NativeCollector {
    materials: Arc<Materials>,
    release_trust: ReleaseStagingConfig,
}

impl LegacyReviewCollector for NativeCollector {
    fn load(&self, reference: &str) -> p::Result<p::SignedLegacyReleaseReview> {
        let report = &self.materials.signed;
        if reference != report.review.binding.review_id
            && reference != report.review.binding.original_operation_id
        {
            return Err(invalid());
        }
        self.materials
            .recheck(Instant::now() + std::time::Duration::from_secs(5))?;
        Ok(report.clone())
    }

    fn inspect(
        &self,
        review: &p::LegacyReleaseReview,
        worker_digest: &p::SchemaDigest,
        deadline: Instant,
    ) -> p::Result<Box<dyn LegacyReviewInspection>> {
        self.materials.recheck(deadline)?;
        if review != &self.materials.signed.review
            || worker_digest != &self.materials.sources.worker_state_digest
        {
            return Err(invalid());
        }
        let materials = &self.materials;
        let origin = LegacyDataArchiveTrust::from_pinned_origin(
            materials.trust.archive_digest.clone(),
            materials.trust.archive_origin_digest.clone(),
            materials.trust.archive_origin_public_key,
        )?;
        let archive = LegacyDataArchiveEvidence::load(
            &materials.installation,
            &origin,
            &review.binding,
            &materials.context.takeover.release,
            materials.context.planned_at,
            deadline,
        )?;
        let repair = ManagementReaderRepairEvidence::load(
            &materials.installation,
            &self.release_trust,
            &review.binding,
            super::timestamp()?,
        )?;
        repair.check_legacy_sources(
            &materials.sources.image_bindings(),
            &materials.sources.config_files_digest,
            &materials.sources.service_configuration_digest,
        )?;
        let repair = repair.inspect_current(deadline)?;
        let data = InstalledReleaseDataWorker::new(materials.installation.clone())?.start(
            p::ProductReleaseDataRequest {
                schema_version: objects::SCHEMA,
                release: materials.context.takeover.release.clone(),
                owner: materials.context.takeover.owner.clone(),
                expected_config_revision: materials.context.takeover.expected_config_revision,
            },
            deadline,
        )?;
        let identity = data.identity().clone();
        let mut inspection = NativeInspection {
            materials: materials.clone(),
            archive,
            repair,
            data: Box::new(data),
            identity,
            worker_digest: worker_digest.clone(),
        };
        let result = inspection
            .recheck(deadline)
            .and_then(|_| inspection.ensure_held(deadline));
        if let Err(error) = result {
            let _ = inspection.data.release();
            return Err(error);
        }
        Ok(Box::new(inspection))
    }
}

struct NativeInspection {
    materials: Arc<Materials>,
    archive: LegacyDataArchiveEvidence,
    repair: ManagementReaderRepairInspection,
    data: Box<dyn ReleaseRecoveryDataInspection>,
    identity: p::ProductReleaseDataIdentity,
    worker_digest: p::SchemaDigest,
}

impl LegacyReviewInspection for NativeInspection {
    fn recheck(&self, deadline: Instant) -> p::Result<LegacyReviewObservations> {
        self.materials.recheck(deadline)?;
        let binding = &self.materials.signed.review.binding;
        let identity = &self.identity;
        if self.data.identity() != identity
            || self.worker_digest != self.materials.sources.worker_state_digest
        {
            return Err(invalid());
        }
        let archived = self.archive.compare_current(identity, deadline)?;
        let (repair_digest, capabilities) = self.repair.recheck_capabilities(deadline)?;
        let mut evidence = vec![
            self.materials
                .signed
                .review
                .evidence
                .first()
                .ok_or_else(invalid)?
                .clone(),
            archived,
        ];
        let source = &self.materials.sources;
        let binding_digest = binding.digest()?;
        let entries = [
            (
                p::LegacyReleaseEvidenceKind::UpdaterCodeAudit,
                source.updater.code_audit_digest.clone(),
                p::canonical_digest(&(
                    "forme-legacy-updater-audit-observation-v1",
                    &binding_digest,
                    &self.materials.sources_digest,
                    &source.updater,
                ))?,
            ),
            (
                p::LegacyReleaseEvidenceKind::SourceImagesAndServices,
                self.materials.trust.source_review_digest.clone(),
                p::canonical_digest(&(
                    "forme-legacy-images-services-observation-v1",
                    &binding_digest,
                    &self.materials.sources_digest,
                    &source.images,
                    &source.service_configuration_digest,
                    &repair_digest,
                ))?,
            ),
            (
                p::LegacyReleaseEvidenceKind::ConfigurationContinuity,
                source.configuration_history_digest.clone(),
                p::canonical_digest(&(
                    "forme-legacy-config-continuity-observation-v1",
                    &binding_digest,
                    &self.materials.sources_digest,
                    &source.config_files_digest,
                    &repair_digest,
                ))?,
            ),
            (
                p::LegacyReleaseEvidenceKind::WorkerContinuity,
                source.worker_history_digest.clone(),
                p::canonical_digest(&(
                    "forme-legacy-worker-continuity-observation-v1",
                    &binding_digest,
                    &self.materials.sources_digest,
                    &self.worker_digest,
                ))?,
            ),
            (
                p::LegacyReleaseEvidenceKind::ManagementReaderRepair,
                self.repair.receipt_digest().clone(),
                repair_digest,
            ),
            (
                p::LegacyReleaseEvidenceKind::CurrentExclusiveInspection,
                p::canonical_digest(identity)?,
                p::canonical_digest(&(
                    "forme-legacy-current-inspection-v1",
                    &binding_digest,
                    self.materials.context.takeover.digest()?,
                    identity,
                    &self.worker_digest,
                ))?,
            ),
        ];
        evidence.extend(
            entries
                .into_iter()
                .map(
                    |(kind, artifact_digest, observation_digest)| p::LegacyReleaseEvidence {
                        schema_version: objects::SCHEMA,
                        kind,
                        artifact_digest,
                        binding_digest: binding_digest.clone(),
                        observation_digest,
                    },
                ),
        );
        ensure_time(deadline)?;
        Ok(LegacyReviewObservations {
            evidence,
            worker_digest: self.worker_digest.clone(),
            updater_capabilities: capabilities,
            // recheck_capabilities also checked the real stopped SCM boundary.
            updater_state: p::NativeServiceState::Stopped,
        })
    }

    fn ensure_held(&mut self, deadline: Instant) -> p::Result<()> {
        self.data.ensure_held(deadline)?;
        if &self.identity != self.data.identity() {
            return Err(invalid());
        }
        self.materials.recheck(deadline)?;
        self.archive.recheck(deadline)?;
        self.repair.recheck(deadline)?;
        self.data.ensure_held(deadline)?;
        ensure_time(deadline)
    }

    fn release(mut self: Box<Self>) -> p::Result<()> {
        self.data.release()
    }
}

fn ensure_time(deadline: Instant) -> p::Result<()> {
    if Instant::now() >= deadline {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn invalid() -> p::Error {
    p::Error("native legacy review material is unavailable or unverifiable".into())
}
