//! M5 governed capability-ecosystem portable artifact set.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: p::SchemaVersion = p::M5_SCHEMA_VERSION;
const PUBLISHER_KIND: &str = "forme-m5-publisher";
const ADMISSION_KIND: &str = "forme-m5-admission";
const INSTALL_KIND: &str = "forme-m5-install";
const DISTRIBUTION_KIND: &str = "forme-m5-distribution";
const TRACE_KIND: &str = "forme-m5-trace";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5PublisherGrantEvidence {
    pub schema_version: p::SchemaVersion,
    pub reference: p::CapabilityPublisherGrantRef,
    pub publisher: p::CapabilityPublisherRef,
    pub public_key_digest: p::SchemaDigest,
    pub allowed_kinds: Vec<p::CapabilityPackageKind>,
    pub scope: p::Scope,
    pub version: p::Version,
    pub status: p::CapabilityPublisherStatus,
    pub valid_at_execution: p::RequiredTrue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5PublisherArtifact {
    pub schema_version: p::SchemaVersion,
    pub grant: M5PublisherGrantEvidence,
    pub committed_version: p::EcosystemAggregateVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5AdmissionEvidence {
    pub schema_version: p::SchemaVersion,
    pub reference: p::CapabilityAdmissionRef,
    pub package: p::CapabilityPackageRef,
    pub release: p::CapabilityReleaseRef,
    pub package_digest: p::SchemaDigest,
    pub publisher_grant: p::CapabilityPublisherGrantRef,
    pub publisher_version: p::Version,
    pub policy: p::CapabilityPolicyRef,
    pub policy_version: p::Version,
    pub checks: Vec<p::CapabilityAdmissionCheck>,
    pub dependencies: Vec<p::CapabilityPackageDependency>,
    pub sbom_digest: p::SchemaDigest,
    pub license_expression: String,
    pub verified: p::RequiredTrue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5AdmissionArtifact {
    pub schema_version: p::SchemaVersion,
    pub admission: M5AdmissionEvidence,
    pub catalog_receipt: p::ContentRef,
    pub committed_version: p::EcosystemAggregateVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5ApprovalEvidence {
    pub schema_version: p::SchemaVersion,
    pub approval: p::ApprovalId,
    pub plan_digest: p::PlanDigest,
    pub principal: p::VerifiedPrincipal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5InstallArtifact {
    pub schema_version: p::SchemaVersion,
    pub plan: p::CapabilityInstallPlan,
    pub approval: M5ApprovalEvidence,
    pub state: p::CapabilityPackageState,
    pub committed_version: p::EcosystemAggregateVersion,
    pub registry_digest: p::SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5ExecutorRecordEvidence {
    pub schema_version: p::SchemaVersion,
    pub peer: p::FederatedPeerRef,
    pub package: p::CapabilityPackageRef,
    pub release: p::CapabilityReleaseRef,
    pub package_digest: p::SchemaDigest,
    pub installed_generation: u64,
    pub semantic_digest: p::SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5DistributionReceiptEvidence {
    pub schema_version: p::SchemaVersion,
    pub reference: p::CapabilityDistributionReceiptRef,
    pub package: p::CapabilityPackageRef,
    pub release: p::CapabilityReleaseRef,
    pub package_digest: p::SchemaDigest,
    pub peer: p::FederatedPeerRef,
    pub authority_epoch: p::AuthorityEpoch,
    pub capability_plan_digest: p::PlanDigest,
    pub lease: p::RemoteExecutionLeaseRef,
    pub fence_token: u64,
    pub installed_generation: u64,
    pub ground_truth: p::EvidenceRef,
    pub peer_grant_binding_verified: p::RequiredTrue,
    pub remote_plan_binding_verified: p::RequiredTrue,
    pub authority_verified: p::RequiredTrue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5DistributionArtifact {
    pub schema_version: p::SchemaVersion,
    pub capability_plan: p::CapabilityInstallPlanRef,
    pub capability_plan_digest: p::PlanDigest,
    pub semantic_envelope_digest: p::SchemaDigest,
    pub executor_record: M5ExecutorRecordEvidence,
    pub receipt: M5DistributionReceiptEvidence,
    pub committed_version: p::EcosystemAggregateVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M5TraceArtifact {
    pub schema_version: p::SchemaVersion,
    pub scenario: String,
    pub run: p::RunId,
    pub package: p::CapabilityPackageRef,
    pub release: p::CapabilityReleaseRef,
    pub package_digest: p::SchemaDigest,
    pub publisher_grant: p::CapabilityPublisherGrantRef,
    pub admission: p::CapabilityAdmissionRef,
    pub install_plan: p::CapabilityInstallPlanRef,
    pub distribution_receipt: p::CapabilityDistributionReceiptRef,
    pub event_kinds: Vec<p::EventKind>,
    pub stream_seq: Vec<u64>,
    pub event_taxonomy: Vec<p::EventKind>,
    pub registry_fetches: u64,
    pub authority_driver_calls: u64,
    pub executor_installs: u64,
    pub distribution_events: u64,
    pub unknown_retry_count: u64,
    pub post_revoke_visible_contributions: u64,
    pub post_revoke_distribution_attempts: u64,
    pub restricted_material_matches: u64,
    pub ground_truth_verified: bool,
}

impl M5PublisherGrantEvidence {
    pub fn from_runtime(
        grant: &p::CapabilityPublisherGrant,
        observed_at: p::Timestamp,
    ) -> p::Result<Self> {
        grant.validate()?;
        if observed_at <= 0
            || observed_at >= grant.expires_at
            || grant.status != p::CapabilityPublisherStatus::Active
        {
            return Err(p::Error(
                "publisher grant was not active during artifact observation".into(),
            ));
        }
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            reference: grant.reference.clone(),
            publisher: grant.publisher.clone(),
            public_key_digest: grant.public_key_digest.clone(),
            allowed_kinds: grant.allowed_kinds.clone(),
            scope: grant.scope.clone(),
            version: grant.version,
            status: grant.status,
            valid_at_execution: p::RequiredTrue,
        })
    }

    fn validate(&self) -> p::Result<()> {
        validate_schema(self.schema_version, "publisher grant evidence")?;
        required(&self.reference.0, "publisher grant reference")?;
        required(&self.publisher.0, "publisher")?;
        validate_digest(&self.public_key_digest, "publisher public key digest")?;
        required(&self.scope.0, "publisher scope")?;
        if self.allowed_kinds.is_empty()
            || self.allowed_kinds.windows(2).any(|pair| pair[0] >= pair[1])
            || self.version.0 == 0
            || self.status != p::CapabilityPublisherStatus::Active
        {
            return Err(p::Error(
                "publisher grant evidence is incomplete or inactive".into(),
            ));
        }
        Ok(())
    }
}

impl M5PublisherArtifact {
    pub fn from_runtime(
        grant: &p::CapabilityPublisherGrant,
        observed_at: p::Timestamp,
        committed_version: p::EcosystemAggregateVersion,
    ) -> p::Result<Self> {
        let artifact = Self {
            schema_version: SCHEMA_VERSION,
            grant: M5PublisherGrantEvidence::from_runtime(grant, observed_at)?,
            committed_version,
        };
        artifact.grant.validate()?;
        validate_committed_version(&artifact.committed_version, "publisher artifact")?;
        Ok(artifact)
    }
}

impl M5AdmissionEvidence {
    pub fn from_runtime(
        admission: &p::CapabilityPackageAdmission,
        manifest: &p::CapabilityPackageManifest,
    ) -> p::Result<Self> {
        admission.validate()?;
        manifest.validate()?;
        if admission.package != manifest.package
            || admission.release != manifest.release
            || admission.dependencies != manifest.dependencies
        {
            return Err(p::Error(
                "admission evidence does not match the package manifest".into(),
            ));
        }
        let checks = admission
            .checks
            .iter()
            .map(portable_admission_check)
            .collect::<p::Result<Vec<_>>>()?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            reference: admission.reference.clone(),
            package: admission.package.clone(),
            release: admission.release.clone(),
            package_digest: admission.package_digest.clone(),
            publisher_grant: admission.publisher_grant.clone(),
            publisher_version: admission.publisher_version,
            policy: admission.policy.clone(),
            policy_version: admission.policy_version,
            checks,
            dependencies: admission.dependencies.clone(),
            sbom_digest: manifest.sbom_digest.clone(),
            license_expression: manifest.license_expression.clone(),
            verified: p::RequiredTrue,
        })
    }

    fn validate(&self) -> p::Result<()> {
        validate_schema(self.schema_version, "admission evidence")?;
        required(&self.reference.0, "admission reference")?;
        required(&self.package.0, "admitted package")?;
        required(&self.release.0, "admitted release")?;
        validate_digest(&self.package_digest, "admitted package digest")?;
        required(&self.publisher_grant.0, "admission publisher grant")?;
        required(&self.policy.0, "admission policy")?;
        validate_digest(&self.sbom_digest, "admission SBOM digest")?;
        required(&self.license_expression, "admission license")?;
        if self.publisher_version.0 == 0
            || self.policy_version.0 == 0
            || self.checks.len() != p::CapabilityAdmissionCheckKind::ALL.len()
        {
            return Err(p::Error("admission evidence is incomplete".into()));
        }
        for (check, expected) in self.checks.iter().zip(p::CapabilityAdmissionCheckKind::ALL) {
            check.validate()?;
            if check.kind != expected || check.verdict != p::CapabilityAdmissionVerdict::Pass {
                return Err(p::Error(
                    "admission evidence does not contain every passing hard check".into(),
                ));
            }
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
        }
        if self.dependencies.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(p::Error(
                "admission evidence dependencies are not sorted and unique".into(),
            ));
        }
        Ok(())
    }
}

fn portable_admission_check(
    check: &p::CapabilityAdmissionCheck,
) -> p::Result<p::CapabilityAdmissionCheck> {
    check.validate()?;
    let evidence_digest = p::canonical_digest(&(check.kind, &check.evidence))?;
    Ok(p::CapabilityAdmissionCheck {
        schema_version: SCHEMA_VERSION,
        kind: check.kind,
        verdict: check.verdict,
        evidence: p::EvidenceRef(format!("admission-check-evidence:{}", evidence_digest.0)),
    })
}

impl M5AdmissionArtifact {
    pub fn from_runtime(
        admission: &p::CapabilityPackageAdmission,
        manifest: &p::CapabilityPackageManifest,
        catalog_receipt: p::ContentRef,
        committed_version: p::EcosystemAggregateVersion,
    ) -> p::Result<Self> {
        let artifact = Self {
            schema_version: SCHEMA_VERSION,
            admission: M5AdmissionEvidence::from_runtime(admission, manifest)?,
            catalog_receipt,
            committed_version,
        };
        artifact.admission.validate()?;
        required(&artifact.catalog_receipt.0, "catalog receipt")?;
        validate_committed_version(&artifact.committed_version, "admission artifact")?;
        Ok(artifact)
    }
}

impl M5ExecutorRecordEvidence {
    pub fn from_runtime(record: &p::CapabilityExecutorInstallRecord) -> p::Result<Self> {
        record.validate()?;
        let mut evidence = Self {
            schema_version: SCHEMA_VERSION,
            peer: record.peer.clone(),
            package: record.package.clone(),
            release: record.release.clone(),
            package_digest: record.package_digest.clone(),
            installed_generation: record.installed_generation,
            semantic_digest: p::SchemaDigest(String::new()),
        };
        evidence.semantic_digest = evidence.computed_digest()?;
        evidence.validate()?;
        Ok(evidence)
    }

    fn validate(&self) -> p::Result<()> {
        validate_schema(self.schema_version, "executor record evidence")?;
        required(&self.peer.0, "executor record peer")?;
        required(&self.package.0, "executor record package")?;
        required(&self.release.0, "executor record release")?;
        validate_digest(&self.package_digest, "executor record package digest")?;
        validate_digest(&self.semantic_digest, "executor semantic record digest")?;
        if self.installed_generation == 0 || self.semantic_digest != self.computed_digest()? {
            return Err(p::Error(
                "executor semantic record evidence is inconsistent".into(),
            ));
        }
        Ok(())
    }

    fn computed_digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(
            self.schema_version,
            &self.peer,
            &self.package,
            &self.release,
            &self.package_digest,
            self.installed_generation,
        ))
    }
}

impl M5DistributionReceiptEvidence {
    pub fn from_runtime(
        capability_plan: &p::CapabilityInstallPlan,
        runtime_record: &p::CapabilityExecutorInstallRecord,
        record: &M5ExecutorRecordEvidence,
        receipt: &p::CapabilityPackageDistributionReceipt,
    ) -> p::Result<Self> {
        capability_plan.validate()?;
        runtime_record.validate()?;
        record.validate()?;
        receipt.validate()?;
        if receipt.package != record.package
            || receipt.release != record.release
            || receipt.package_digest != record.package_digest
            || receipt.peer != record.peer
            || receipt.installed_generation != record.installed_generation
            || receipt.ground_truth.0
                != format!("executor-package-record:{}", runtime_record.record_digest.0)
        {
            return Err(p::Error(
                "authority receipt does not match executor runtime ground truth".into(),
            ));
        }
        let mut evidence = Self {
            schema_version: SCHEMA_VERSION,
            reference: p::CapabilityDistributionReceiptRef(String::new()),
            package: receipt.package.clone(),
            release: receipt.release.clone(),
            package_digest: receipt.package_digest.clone(),
            peer: receipt.peer.clone(),
            authority_epoch: receipt.authority_epoch,
            capability_plan_digest: capability_plan.digest.clone(),
            lease: receipt.lease.clone(),
            fence_token: receipt.fence_token,
            installed_generation: receipt.installed_generation,
            ground_truth: p::EvidenceRef(format!(
                "executor-package-semantic-record:{}",
                record.semantic_digest.0
            )),
            peer_grant_binding_verified: p::RequiredTrue,
            remote_plan_binding_verified: p::RequiredTrue,
            authority_verified: p::RequiredTrue,
        };
        evidence.reference = evidence.computed_reference()?;
        evidence.validate()?;
        Ok(evidence)
    }

    fn validate(&self) -> p::Result<()> {
        validate_schema(self.schema_version, "distribution receipt evidence")?;
        required(&self.reference.0, "distribution evidence reference")?;
        required(&self.package.0, "distributed package")?;
        required(&self.release.0, "distributed release")?;
        validate_digest(&self.package_digest, "distributed package digest")?;
        required(&self.peer.0, "distribution peer")?;
        required(
            &self.capability_plan_digest.0,
            "distribution capability plan digest",
        )?;
        required(&self.lease.0, "distribution lease")?;
        required(&self.ground_truth.0, "distribution ground truth")?;
        if self.authority_epoch.0 == 0
            || self.fence_token == 0
            || self.installed_generation == 0
            || self.reference != self.computed_reference()?
        {
            return Err(p::Error(
                "distribution receipt evidence is incomplete or inconsistent".into(),
            ));
        }
        Ok(())
    }

    fn computed_reference(&self) -> p::Result<p::CapabilityDistributionReceiptRef> {
        let digest = p::canonical_digest(&(
            self.schema_version,
            &self.package,
            &self.release,
            &self.package_digest,
            &self.peer,
            self.authority_epoch,
            &self.capability_plan_digest,
            &self.lease,
            self.fence_token,
            self.installed_generation,
            &self.ground_truth,
            self.peer_grant_binding_verified,
            self.remote_plan_binding_verified,
            self.authority_verified,
        ))?;
        Ok(p::CapabilityDistributionReceiptRef(format!(
            "distribution-evidence:{}",
            digest.0
        )))
    }
}

impl M5DistributionArtifact {
    pub fn from_runtime(
        capability_plan: &p::CapabilityInstallPlan,
        envelope: &p::CapabilityPackageDistributionEnvelope,
        runtime_record: &p::CapabilityExecutorInstallRecord,
        receipt: &p::CapabilityPackageDistributionReceipt,
        committed_version: p::EcosystemAggregateVersion,
    ) -> p::Result<Self> {
        capability_plan.validate()?;
        envelope.validate()?;
        runtime_record.validate()?;
        receipt.validate()?;
        if capability_plan.operation != p::CapabilityPackageOperation::Distribute
            || envelope.install_plan != *capability_plan
            || runtime_record.envelope_digest != envelope.content_digest
            || runtime_record.peer != envelope.target_peer
            || runtime_record.package != envelope.package.manifest.package
            || runtime_record.release != envelope.package.manifest.release
            || runtime_record.package_digest != envelope.package.package_digest
            || receipt.package != capability_plan.package
            || receipt.release != capability_plan.release
            || receipt.package_digest != capability_plan.package_digest
            || receipt.peer != envelope.target_peer
            || receipt.authority_epoch != envelope.authority_epoch
            || committed_version != capability_plan.expected_version.next()?
        {
            return Err(p::Error(
                "runtime distribution evidence does not have exact bindings".into(),
            ));
        }
        let executor_record = M5ExecutorRecordEvidence::from_runtime(runtime_record)?;
        let receipt = M5DistributionReceiptEvidence::from_runtime(
            capability_plan,
            runtime_record,
            &executor_record,
            receipt,
        )?;
        let semantic_envelope_digest = p::canonical_digest(&(
            &capability_plan.reference,
            &capability_plan.digest,
            &envelope.package.manifest.package,
            &envelope.package.manifest.release,
            &envelope.package.package_digest,
            &envelope.publisher_grant.reference,
            &envelope.publisher_grant.public_key_digest,
            &envelope.authority_policy.reference,
            envelope.authority_policy.version,
            &envelope.target_peer,
            envelope.authority_epoch,
        ))?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            capability_plan: capability_plan.reference.clone(),
            capability_plan_digest: capability_plan.digest.clone(),
            semantic_envelope_digest,
            executor_record,
            receipt,
            committed_version,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M5ArtifactBundle {
    pub publisher: M5PublisherArtifact,
    pub admission: M5AdmissionArtifact,
    pub install: M5InstallArtifact,
    pub distribution: M5DistributionArtifact,
    pub trace: M5TraceArtifact,
}

impl M5ArtifactBundle {
    pub fn validate(&self) -> p::Result<()> {
        validate_schema(self.publisher.schema_version, "publisher artifact")?;
        self.publisher.grant.validate()?;
        validate_committed_version(&self.publisher.committed_version, "publisher artifact")?;

        validate_schema(self.admission.schema_version, "admission artifact")?;
        self.admission.admission.validate()?;
        required(&self.admission.catalog_receipt.0, "catalog receipt")?;
        validate_committed_version(&self.admission.committed_version, "admission artifact")?;

        validate_schema(self.install.schema_version, "install artifact")?;
        self.install.plan.validate()?;
        validate_schema(
            self.install.approval.schema_version,
            "install approval evidence",
        )?;
        required(&self.install.approval.approval.0, "install approval")?;
        required(
            &self.install.approval.plan_digest.0,
            "install approval plan digest",
        )?;
        required(
            &self.install.approval.principal.0,
            "install approval principal",
        )?;
        self.install.state.validate()?;
        validate_committed_version(&self.install.committed_version, "install artifact")?;
        validate_digest(&self.install.registry_digest, "registry digest")?;

        validate_schema(self.distribution.schema_version, "distribution artifact")?;
        required(
            &self.distribution.capability_plan.0,
            "distribution capability plan",
        )?;
        required(
            &self.distribution.capability_plan_digest.0,
            "distribution capability plan digest",
        )?;
        validate_digest(
            &self.distribution.semantic_envelope_digest,
            "distribution semantic envelope digest",
        )?;
        self.distribution.executor_record.validate()?;
        self.distribution.receipt.validate()?;
        validate_committed_version(
            &self.distribution.committed_version,
            "distribution artifact",
        )?;

        self.trace.validate()?;
        self.validate_cross_references()?;
        let value = serde_json::to_value(self)
            .map_err(|_| p::Error("M5 artifact bundle could not be inspected".into()))?;
        scan_portable_value(&value)
    }

    fn validate_cross_references(&self) -> p::Result<()> {
        let grant = &self.publisher.grant;
        let admission = &self.admission.admission;
        let install = &self.install;
        let distribution = &self.distribution;
        let record = &distribution.executor_record;
        let receipt = &distribution.receipt;
        let trace = &self.trace;

        if admission.publisher_grant != grant.reference
            || admission.publisher_version != grant.version
            || admission.package != install.plan.package
            || admission.release != install.plan.release
            || admission.package_digest != install.plan.package_digest
            || install.plan.admission != admission.reference
            || install.state.approval.as_ref() != Some(&install.approval.approval)
            || install.approval.plan_digest != install.plan.digest
            || install.state.plan.as_ref() != Some(&install.plan.reference)
            || install.state.package != admission.package
            || install.state.release != admission.release
            || install.state.package_digest != admission.package_digest
            || install.state.lifecycle != p::CapabilityLifecycleState::Enabled
            || install.state.active_generation == 0
            || distribution.capability_plan_digest.0.trim().is_empty()
            || record.package != admission.package
            || record.release != admission.release
            || record.package_digest != admission.package_digest
            || receipt.package != admission.package
            || receipt.release != admission.release
            || receipt.package_digest != admission.package_digest
            || receipt.peer != record.peer
            || receipt.capability_plan_digest != distribution.capability_plan_digest
            || receipt.installed_generation != record.installed_generation
            || receipt.ground_truth.0
                != format!(
                    "executor-package-semantic-record:{}",
                    record.semantic_digest.0
                )
            || trace.package != admission.package
            || trace.release != admission.release
            || trace.package_digest != admission.package_digest
            || trace.publisher_grant != grant.reference
            || trace.admission != admission.reference
            || trace.install_plan != install.plan.reference
            || trace.distribution_receipt != receipt.reference
        {
            return Err(p::Error(
                "M5 artifact cross-reference binding is inconsistent".into(),
            ));
        }
        if !(self.publisher.committed_version.value < self.admission.committed_version.value
            && self.admission.committed_version.value < self.install.committed_version.value
            && self.install.committed_version.value < self.distribution.committed_version.value)
        {
            return Err(p::Error(
                "M5 artifact ecosystem versions are not monotonic".into(),
            ));
        }
        Ok(())
    }
}

impl M5TraceArtifact {
    pub fn validate(&self) -> p::Result<()> {
        validate_schema(self.schema_version, "trace artifact")?;
        required(&self.scenario, "trace scenario")?;
        required(&self.run.0, "trace run")?;
        required(&self.package.0, "trace package")?;
        required(&self.release.0, "trace release")?;
        validate_digest(&self.package_digest, "trace package digest")?;
        required(&self.publisher_grant.0, "trace publisher grant")?;
        required(&self.admission.0, "trace admission")?;
        required(&self.install_plan.0, "trace install plan")?;
        required(&self.distribution_receipt.0, "trace distribution receipt")?;
        if self.event_kinds.is_empty()
            || self.event_kinds.len() != self.stream_seq.len()
            || self.stream_seq.windows(2).any(|pair| pair[0] >= pair[1])
            || self.event_taxonomy.as_slice() != p::EventKind::ALL.as_slice()
            || self.registry_fetches != 1
            || self.authority_driver_calls != 1
            || self.executor_installs != 1
            || self.distribution_events != 1
            || self.unknown_retry_count != 0
            || self.post_revoke_visible_contributions != 0
            || self.post_revoke_distribution_attempts != 0
            || self.restricted_material_matches != 0
            || !self.ground_truth_verified
        {
            return Err(p::Error(
                "M5 trace counts, taxonomy, or ground truth are invalid".into(),
            ));
        }
        let required = [
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ActionPlanned,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
            p::EventKind::ApprovalResolved,
            p::EventKind::RunResumed,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutcomeUnknown,
            p::EventKind::ActionCompleted,
            p::EventKind::CapabilityPackageDistributionRecorded,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ];
        if !is_subsequence(&self.event_kinds, &required) {
            return Err(p::Error(
                "M5 trace does not contain the governed distribution sequence".into(),
            ));
        }
        Ok(())
    }
}

impl Serialize for M5ArtifactBundle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (
            &self.publisher,
            &self.admission,
            &self.install,
            &self.distribution,
            &self.trace,
        )
            .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableM5Artifact<T> {
    pub schema_version: p::SchemaVersion,
    pub artifact_type: String,
    pub digest: p::SchemaDigest,
    pub body: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M5ArtifactReceipt {
    pub schema_version: p::SchemaVersion,
    pub root: PathBuf,
    pub digest: p::SchemaDigest,
    pub content_ref: p::ContentRef,
}

pub struct M5ArtifactStore {
    root: PathBuf,
}

impl M5ArtifactStore {
    pub fn new(root: impl AsRef<Path>) -> p::Result<Self> {
        fs::create_dir_all(root.as_ref())
            .map_err(|_| p::Error("M5 artifact root could not be created".into()))?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|_| p::Error("M5 artifact root could not be resolved".into()))?;
        if !root.is_dir() {
            return Err(p::Error("M5 artifact root is not a directory".into()));
        }
        Ok(Self { root })
    }

    pub fn write(&self, bundle: &M5ArtifactBundle) -> p::Result<M5ArtifactReceipt> {
        bundle.validate()?;
        if !directory_entries(&self.root)?.is_empty() {
            return Err(p::Error(
                "M5 artifact root must be empty before generation".into(),
            ));
        }
        write_artifact(&self.root, "publisher", PUBLISHER_KIND, &bundle.publisher)?;
        write_artifact(&self.root, "admission", ADMISSION_KIND, &bundle.admission)?;
        write_artifact(&self.root, "install", INSTALL_KIND, &bundle.install)?;
        write_artifact(
            &self.root,
            "distribution",
            DISTRIBUTION_KIND,
            &bundle.distribution,
        )?;
        write_artifact(&self.root, "trace", TRACE_KIND, &bundle.trace)?;
        self.verify_complete_set()
    }

    pub fn verify_complete_set(&self) -> p::Result<M5ArtifactReceipt> {
        let entries = directory_entries(&self.root)?;
        if entries.len() != 5 || entries.iter().any(|entry| !entry.is_file()) {
            return Err(p::Error(
                "M5 artifact root is not a closed five-file set".into(),
            ));
        }
        let mut values = BTreeMap::<String, (PathBuf, serde_json::Value)>::new();
        let mut digests = Vec::<p::SchemaDigest>::new();
        for path in entries {
            if path.parent() != Some(self.root.as_path())
                || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                return Err(p::Error("M5 artifact path or type is invalid".into()));
            }
            let bytes =
                fs::read(&path).map_err(|_| p::Error("M5 artifact could not be read".into()))?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| p::Error("M5 artifact is malformed".into()))?;
            scan_portable_value(&value)?;
            let kind = value
                .get("artifact_type")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| p::Error("M5 artifact type is absent".into()))?
                .to_owned();
            if values.insert(kind, (path, value)).is_some() {
                return Err(p::Error("M5 artifact type is duplicated".into()));
            }
        }
        let publisher = decode_artifact::<M5PublisherArtifact>(
            values.remove(PUBLISHER_KIND),
            PUBLISHER_KIND,
            "publisher",
            &mut digests,
        )?;
        let admission = decode_artifact::<M5AdmissionArtifact>(
            values.remove(ADMISSION_KIND),
            ADMISSION_KIND,
            "admission",
            &mut digests,
        )?;
        let install = decode_artifact::<M5InstallArtifact>(
            values.remove(INSTALL_KIND),
            INSTALL_KIND,
            "install",
            &mut digests,
        )?;
        let distribution = decode_artifact::<M5DistributionArtifact>(
            values.remove(DISTRIBUTION_KIND),
            DISTRIBUTION_KIND,
            "distribution",
            &mut digests,
        )?;
        let trace = decode_artifact::<M5TraceArtifact>(
            values.remove(TRACE_KIND),
            TRACE_KIND,
            "trace",
            &mut digests,
        )?;
        if !values.is_empty() {
            return Err(p::Error("M5 artifact set contains an unknown type".into()));
        }
        M5ArtifactBundle {
            publisher,
            admission,
            install,
            distribution,
            trace,
        }
        .validate()?;
        digests.sort_by(|left, right| left.0.cmp(&right.0));
        let digest = p::canonical_digest(&digests)?;
        Ok(M5ArtifactReceipt {
            schema_version: SCHEMA_VERSION,
            root: self.root.clone(),
            content_ref: p::ContentRef(format!("artifact:{}", digest.0)),
            digest,
        })
    }
}

fn write_artifact<T: Serialize>(root: &Path, prefix: &str, kind: &str, body: &T) -> p::Result<()> {
    let digest = p::canonical_digest(body)?;
    let artifact = PortableM5Artifact {
        schema_version: SCHEMA_VERSION,
        artifact_type: kind.to_owned(),
        digest: digest.clone(),
        body,
    };
    let path = root.join(format!("{prefix}-{}.json", digest_suffix(&digest)?));
    if path.parent() != Some(root) {
        return Err(p::Error("M5 artifact escaped its root".into()));
    }
    let bytes = serde_json::to_vec_pretty(&artifact)
        .map_err(|_| p::Error("M5 artifact could not be encoded".into()))?;
    fs::write(path, bytes).map_err(|_| p::Error("M5 artifact could not be persisted".into()))
}

fn decode_artifact<T: DeserializeOwned + Serialize>(
    entry: Option<(PathBuf, serde_json::Value)>,
    expected_kind: &str,
    prefix: &str,
    digests: &mut Vec<p::SchemaDigest>,
) -> p::Result<T> {
    let (path, value) = entry.ok_or_else(|| p::Error("M5 artifact type is missing".into()))?;
    let artifact: PortableM5Artifact<T> = serde_json::from_value(value)
        .map_err(|_| p::Error("M5 artifact schema is invalid".into()))?;
    let digest = p::canonical_digest(&artifact.body)?;
    if artifact.schema_version != SCHEMA_VERSION
        || artifact.artifact_type != expected_kind
        || artifact.digest != digest
    {
        return Err(p::Error(
            "M5 artifact type, schema, or digest is invalid".into(),
        ));
    }
    let expected_name = format!("{prefix}-{}.json", digest_suffix(&digest)?);
    if path.file_name().and_then(|value| value.to_str()) != Some(expected_name.as_str()) {
        return Err(p::Error(
            "M5 artifact filename is not bound to its body digest".into(),
        ));
    }
    digests.push(digest);
    Ok(artifact.body)
}

fn directory_entries(root: &Path) -> p::Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(root)
        .map_err(|_| p::Error("M5 artifact root could not be listed".into()))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|_| p::Error("M5 artifact entry could not be inspected".into()))?
        .into_iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn digest_suffix(digest: &p::SchemaDigest) -> p::Result<&str> {
    digest
        .0
        .strip_prefix("sha256:")
        .filter(|suffix| suffix.len() == 64 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| p::Error("M5 artifact digest is invalid".into()))
}

fn validate_schema(schema: p::SchemaVersion, name: &str) -> p::Result<()> {
    if schema != SCHEMA_VERSION {
        return Err(p::Error(format!("{name} schema is unsupported")));
    }
    Ok(())
}

fn validate_committed_version(version: &p::EcosystemAggregateVersion, name: &str) -> p::Result<()> {
    version.validate()?;
    if version.value == 0 {
        return Err(p::Error(format!("{name} has no committed version")));
    }
    Ok(())
}

fn validate_digest(digest: &p::SchemaDigest, name: &str) -> p::Result<()> {
    p::sha256_digest_bytes(digest)
        .map(|_| ())
        .map_err(|_| p::Error(format!("{name} is not a canonical digest")))
}

fn required(value: &str, name: &str) -> p::Result<()> {
    if value.trim().is_empty() {
        return Err(p::Error(format!("{name} is incomplete")));
    }
    Ok(())
}

fn is_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) -> bool {
    let mut cursor = 0;
    for expected_kind in expected {
        let Some(offset) = actual[cursor..]
            .iter()
            .position(|kind| kind == expected_kind)
        else {
            return false;
        };
        cursor += offset + 1;
    }
    true
}

fn scan_portable_value(value: &serde_json::Value) -> p::Result<()> {
    match value {
        serde_json::Value::String(value) => scan_string(value),
        serde_json::Value::Array(values) => {
            for value in values {
                scan_portable_value(value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                scan_string(key)?;
                scan_portable_value(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn scan_string(value: &str) -> p::Result<()> {
    let lower = value.to_ascii_lowercase();
    if [
        "secretref",
        "secret_ref",
        "credential",
        "authorization:",
        "bearer ",
        "private key",
        "private-key",
        "session key",
        "http://",
        "https://",
        "ws://",
        "wss://",
        "ssh://",
        "127.0.0.1:",
        "localhost:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || is_absolute_path(value)
    {
        return Err(p::Error(
            "M5 artifact contains restricted or host-private material".into(),
        ));
    }
    Ok(())
}

fn is_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}
