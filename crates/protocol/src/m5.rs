//! M5 governed capability-ecosystem protocol contracts.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::*;

pub const M5_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

fn required(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error(format!("{name} is incomplete")))
    } else {
        Ok(())
    }
}

fn required_version(value: Version, name: &str) -> Result<()> {
    if value.0 == 0 {
        Err(Error(format!("{name} must be non-zero")))
    } else {
        Ok(())
    }
}

fn validate_sorted_unique<T: Ord>(values: &[T], name: &str) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(Error(format!("{name} must be sorted and unique")))
    } else {
        Ok(())
    }
}

pub fn sha256_content_digest(bytes: &[u8]) -> SchemaDigest {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::from("sha256:");
    for byte in digest {
        use core::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    SchemaDigest(encoded)
}

pub fn sha256_digest_bytes(digest: &SchemaDigest) -> Result<[u8; 32]> {
    let Some(encoded) = digest.0.strip_prefix("sha256:") else {
        return Err(Error("digest is not sha256-prefixed".into()));
    };
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error("digest is not a canonical sha256 value".into()));
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let text = core::str::from_utf8(pair)
            .map_err(|_| Error("digest contains invalid bytes".into()))?;
        bytes[index] = u8::from_str_radix(text, 16)
            .map_err(|_| Error("digest contains invalid hexadecimal".into()))?;
    }
    Ok(bytes)
}

fn required_digest(digest: &SchemaDigest, name: &str) -> Result<()> {
    sha256_digest_bytes(digest)
        .map(|_| ())
        .map_err(|_| Error(format!("{name} is not a canonical sha256 digest")))
}

fn scope_contains(granted: &Scope, requested: &Scope) -> bool {
    if granted.0 == "*" || granted == requested {
        return true;
    }
    requested
        .0
        .strip_prefix(&granted.0)
        .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}

fn normalize_resource_path(path: &str) -> Result<String> {
    required(path, "package resource path")?;
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\\') || path.contains(':')
    {
        return Err(Error("package resource path is not relative".into()));
    }
    let segments = path.split('/').collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(Error(
            "package resource path escapes or is not normalized".into(),
        ));
    }
    Ok(segments.join("/"))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackageSignature(pub String);

impl PackageSignature {
    pub fn validate(&self) -> Result<()> {
        let Some(encoded) = self.0.strip_prefix("ed25519:") else {
            return Err(Error("package signature is not ed25519-prefixed".into()));
        };
        if encoded.len() != 128 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error("package signature is malformed".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityPackageKind {
    Connector,
    Plugin,
    Skill,
    McpServer,
    AgentProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityPublisherStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityLifecycleState {
    Quarantined,
    Admitted,
    Installed,
    Enabled,
    Disabled,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityPackageOperation {
    Install,
    Enable,
    Disable,
    Update,
    Rollback,
    Revoke,
    Distribute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityAdmissionVerdict {
    Pass,
    Fail,
    Unverifiable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityAdmissionCheckKind {
    Schema,
    Size,
    ClosedSet,
    Path,
    Digest,
    Publisher,
    Signature,
    Dependency,
    Sbom,
    License,
    Secret,
    Risk,
    Policy,
}

impl CapabilityAdmissionCheckKind {
    pub const ALL: [Self; 13] = [
        Self::Schema,
        Self::Size,
        Self::ClosedSet,
        Self::Path,
        Self::Digest,
        Self::Publisher,
        Self::Signature,
        Self::Dependency,
        Self::Sbom,
        Self::License,
        Self::Secret,
        Self::Risk,
        Self::Policy,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcosystemAggregateVersion {
    pub schema_version: SchemaVersion,
    pub value: u64,
}

impl EcosystemAggregateVersion {
    pub const fn zero() -> Self {
        Self {
            schema_version: M5_SCHEMA_VERSION,
            value: 0,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error(
                "ecosystem aggregate version schema is invalid".into(),
            ));
        }
        Ok(())
    }

    pub fn next(&self) -> Result<Self> {
        self.validate()?;
        Ok(Self {
            schema_version: self.schema_version,
            value: self
                .value
                .checked_add(1)
                .ok_or_else(|| Error("ecosystem aggregate version is exhausted".into()))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPublisherGrant {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityPublisherGrantRef,
    pub publisher: CapabilityPublisherRef,
    pub public_key_digest: SchemaDigest,
    pub allowed_kinds: Vec<CapabilityPackageKind>,
    pub scope: Scope,
    pub expires_at: Timestamp,
    pub version: Version,
    pub status: CapabilityPublisherStatus,
}

impl CapabilityPublisherGrant {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= 0 || self.allowed_kinds.is_empty() {
            return Err(Error("capability publisher grant is incomplete".into()));
        }
        required(&self.reference.0, "publisher grant reference")?;
        required(&self.publisher.0, "publisher")?;
        required(&self.scope.0, "publisher scope")?;
        required_digest(&self.public_key_digest, "publisher public key digest")?;
        required_version(self.version, "publisher grant version")?;
        validate_sorted_unique(&self.allowed_kinds, "publisher allowed kinds")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageDependency {
    pub schema_version: SchemaVersion,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub version: Version,
    pub digest: SchemaDigest,
}

impl CapabilityPackageDependency {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("package dependency schema is invalid".into()));
        }
        required(&self.package.0, "dependency package")?;
        required(&self.release.0, "dependency release")?;
        required_version(self.version, "dependency version")?;
        required_digest(&self.digest, "dependency digest")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityContributionDescriptor {
    pub schema_version: SchemaVersion,
    pub kind: CapabilityPackageKind,
    pub capability: CapabilityRef,
    pub payload_digest: SchemaDigest,
    pub required_permissions: Vec<PermissionRef>,
    pub risk: Risk,
    pub network: bool,
    pub hook: bool,
}

impl CapabilityContributionDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("package contribution schema is invalid".into()));
        }
        required(&self.capability.0, "package contribution capability")?;
        required_digest(&self.payload_digest, "package contribution payload digest")?;
        validate_sorted_unique(
            &self.required_permissions,
            "package contribution permissions",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageManifest {
    pub schema_version: SchemaVersion,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub version: Version,
    pub kind: CapabilityPackageKind,
    pub publisher: CapabilityPublisherRef,
    pub scope: Scope,
    pub contributions: Vec<CapabilityContributionDescriptor>,
    pub dependencies: Vec<CapabilityPackageDependency>,
    pub sbom_digest: SchemaDigest,
    pub license_expression: String,
    pub body_digest: SchemaDigest,
    pub max_unpacked_bytes: u64,
    pub contains_executable: bool,
}

impl CapabilityPackageManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.contributions.is_empty()
            || self.max_unpacked_bytes == 0
            || self.contains_executable
        {
            return Err(Error(
                "capability package manifest is incomplete or executable".into(),
            ));
        }
        required(&self.package.0, "package")?;
        required(&self.release.0, "package release")?;
        required(&self.publisher.0, "package publisher")?;
        required(&self.scope.0, "package scope")?;
        required(&self.license_expression, "package license expression")?;
        required_version(self.version, "package version")?;
        required_digest(&self.sbom_digest, "package SBOM digest")?;
        required_digest(&self.body_digest, "package body digest")?;
        for contribution in &self.contributions {
            contribution.validate()?;
            if contribution.kind != self.kind {
                return Err(Error(
                    "package contribution kind does not match manifest".into(),
                ));
            }
        }
        if self
            .contributions
            .windows(2)
            .any(|pair| pair[0].capability >= pair[1].capability)
        {
            return Err(Error(
                "package contributions must be sorted and unique".into(),
            ));
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
            if dependency.package == self.package && dependency.release == self.release {
                return Err(Error("package cannot depend on itself".into()));
            }
        }
        validate_sorted_unique(&self.dependencies, "package dependencies")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageResource {
    pub schema_version: SchemaVersion,
    pub relative_path: String,
    pub content: String,
    pub digest: SchemaDigest,
}

impl CapabilityPackageResource {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("package resource schema is invalid".into()));
        }
        if normalize_resource_path(&self.relative_path)? != self.relative_path {
            return Err(Error("package resource path is not canonical".into()));
        }
        required_digest(&self.digest, "package resource digest")?;
        if self.digest != sha256_content_digest(self.content.as_bytes()) {
            return Err(Error(
                "package resource digest does not match content".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCapabilityPackage {
    pub schema_version: SchemaVersion,
    pub manifest: CapabilityPackageManifest,
    pub resources: Vec<CapabilityPackageResource>,
    pub package_digest: SchemaDigest,
    pub signature: PackageSignature,
}

impl SignedCapabilityPackage {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.resources.is_empty() {
            return Err(Error("signed capability package is incomplete".into()));
        }
        self.manifest.validate()?;
        self.signature.validate()?;
        required_digest(&self.package_digest, "package digest")?;
        let mut casefolded = BTreeSet::new();
        let mut resource_digests = BTreeSet::new();
        let mut total = 0_u64;
        for resource in &self.resources {
            resource.validate()?;
            if !casefolded.insert(resource.relative_path.to_ascii_lowercase()) {
                return Err(Error("package resource path is duplicated".into()));
            }
            if !resource_digests.insert(resource.digest.clone()) {
                return Err(Error(
                    "package resource content digest is duplicated".into(),
                ));
            }
            total = total
                .checked_add(resource.content.len() as u64)
                .ok_or_else(|| Error("package resource size overflowed".into()))?;
        }
        if self
            .resources
            .windows(2)
            .any(|pair| pair[0].relative_path >= pair[1].relative_path)
            || total > self.manifest.max_unpacked_bytes
        {
            return Err(Error("package resources are unordered or oversized".into()));
        }
        let declared = self
            .manifest
            .contributions
            .iter()
            .map(|contribution| contribution.payload_digest.clone())
            .collect::<BTreeSet<_>>();
        if declared != resource_digests {
            return Err(Error(
                "package resources are not a closed contribution payload set".into(),
            ));
        }
        let resource_refs = self
            .resources
            .iter()
            .map(|resource| (&resource.relative_path, &resource.digest))
            .collect::<Vec<_>>();
        if self.manifest.body_digest != canonical_digest(&resource_refs)? {
            return Err(Error("package body digest does not match resources".into()));
        }
        if self.package_digest != canonical_digest(&(&self.manifest, &resource_refs))? {
            return Err(Error(
                "package digest does not match manifest and resources".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digests(&mut self) -> Result<()> {
        for resource in &mut self.resources {
            resource.digest = sha256_content_digest(resource.content.as_bytes());
        }
        self.resources
            .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let resource_refs = self
            .resources
            .iter()
            .map(|resource| (&resource.relative_path, &resource.digest))
            .collect::<Vec<_>>();
        self.manifest.body_digest = canonical_digest(&resource_refs)?;
        self.package_digest = canonical_digest(&(&self.manifest, &resource_refs))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityAdmissionPolicy {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityPolicyRef,
    pub version: Version,
    pub allowed_kinds: Vec<CapabilityPackageKind>,
    pub allowed_licenses: Vec<String>,
    pub max_package_bytes: u64,
    pub max_dependencies: u32,
    pub max_depth: u32,
    pub allow_network: bool,
    pub allow_hooks: bool,
}

impl CapabilityAdmissionPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.allowed_kinds.is_empty()
            || self.allowed_licenses.is_empty()
            || self.max_package_bytes == 0
            || self.max_depth == 0
        {
            return Err(Error("capability admission policy is incomplete".into()));
        }
        required(&self.reference.0, "capability admission policy")?;
        required_version(self.version, "capability admission policy version")?;
        validate_sorted_unique(&self.allowed_kinds, "allowed package kinds")?;
        validate_sorted_unique(&self.allowed_licenses, "allowed package licenses")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityAdmissionCheck {
    pub schema_version: SchemaVersion,
    pub kind: CapabilityAdmissionCheckKind,
    pub verdict: CapabilityAdmissionVerdict,
    pub evidence: EvidenceRef,
}

impl CapabilityAdmissionCheck {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("capability admission check schema is invalid".into()));
        }
        required(&self.evidence.0, "capability admission evidence")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageAdmission {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityAdmissionRef,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub publisher_grant: CapabilityPublisherGrantRef,
    pub publisher_version: Version,
    pub policy: CapabilityPolicyRef,
    pub policy_version: Version,
    pub checks: Vec<CapabilityAdmissionCheck>,
    pub dependencies: Vec<CapabilityPackageDependency>,
    pub admitted_at: Timestamp,
}

impl CapabilityPackageAdmission {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.admitted_at <= 0 {
            return Err(Error("capability package admission is incomplete".into()));
        }
        required(&self.reference.0, "capability admission reference")?;
        required(&self.package.0, "admitted package")?;
        required(&self.release.0, "admitted release")?;
        required_digest(&self.package_digest, "admitted package digest")?;
        required(&self.publisher_grant.0, "admission publisher grant")?;
        required_version(self.publisher_version, "admission publisher version")?;
        required(&self.policy.0, "admission policy")?;
        required_version(self.policy_version, "admission policy version")?;
        if self.checks.len() != CapabilityAdmissionCheckKind::ALL.len() {
            return Err(Error("admission does not contain every hard check".into()));
        }
        for (check, expected) in self.checks.iter().zip(CapabilityAdmissionCheckKind::ALL) {
            check.validate()?;
            if check.kind != expected || check.verdict != CapabilityAdmissionVerdict::Pass {
                return Err(Error("admission hard checks did not all pass".into()));
            }
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
        }
        validate_sorted_unique(&self.dependencies, "admitted dependencies")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityInstallPlan {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityInstallPlanRef,
    pub operation: CapabilityPackageOperation,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub admission: CapabilityAdmissionRef,
    pub scope: Scope,
    pub policy: CapabilityPolicyRef,
    pub policy_version: Version,
    pub expected_version: EcosystemAggregateVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_release: Option<CapabilityReleaseRef>,
    pub rollback_boundary: RollbackBoundary,
    pub digest: PlanDigest,
}

impl CapabilityInstallPlan {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("capability install plan schema is invalid".into()));
        }
        required(&self.reference.0, "capability install plan")?;
        required(&self.package.0, "plan package")?;
        required(&self.release.0, "plan release")?;
        required_digest(&self.package_digest, "plan package digest")?;
        required(&self.admission.0, "plan admission")?;
        required(&self.scope.0, "plan scope")?;
        required(&self.policy.0, "plan policy")?;
        required_version(self.policy_version, "plan policy version")?;
        self.expected_version.validate()?;
        required(&self.rollback_boundary.0, "plan rollback boundary")?;
        if matches!(
            self.operation,
            CapabilityPackageOperation::Update | CapabilityPackageOperation::Rollback
        ) && self.previous_release.is_none()
        {
            return Err(Error(
                "update or rollback plan has no previous release".into(),
            ));
        }
        if let Some(previous) = &self.previous_release {
            required(&previous.0, "previous release")?;
        }
        let mut material = self.clone();
        material.digest = PlanDigest(String::new());
        let expected = canonical_digest(&material)?;
        if self.digest.0 != expected.0 {
            return Err(Error(
                "capability install plan digest does not match".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = PlanDigest(String::new());
        self.digest = PlanDigest(canonical_digest(&material)?.0);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageApproval {
    pub schema_version: SchemaVersion,
    pub approval: ApprovalId,
    pub plan_digest: PlanDigest,
    pub principal: VerifiedPrincipal,
    pub nonce: Nonce,
    pub expires_at: Timestamp,
}

impl CapabilityPackageApproval {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= 0 {
            return Err(Error("capability package approval is incomplete".into()));
        }
        required(&self.approval.0, "package approval")?;
        required(&self.plan_digest.0, "approved plan digest")?;
        required(&self.principal.0, "package approval principal")?;
        required(&self.nonce.0, "package approval nonce")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageStateChange {
    pub schema_version: SchemaVersion,
    pub plan: CapabilityInstallPlanRef,
    pub approval: ApprovalId,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub from: CapabilityLifecycleState,
    pub to: CapabilityLifecycleState,
    pub active_generation: u64,
    pub reason: ReasonRef,
    pub external_effects_reverted: bool,
}

impl CapabilityPackageStateChange {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.active_generation == 0 {
            return Err(Error(
                "capability package state change is incomplete".into(),
            ));
        }
        required(&self.plan.0, "package state plan")?;
        required(&self.approval.0, "package state approval")?;
        required(&self.package.0, "package state package")?;
        required(&self.release.0, "package state release")?;
        required(&self.reason.0, "package state reason")?;
        if self.external_effects_reverted || !legal_lifecycle_transition(self.from, self.to) {
            return Err(Error(
                "capability package lifecycle transition is invalid".into(),
            ));
        }
        Ok(())
    }
}

fn legal_lifecycle_transition(
    from: CapabilityLifecycleState,
    to: CapabilityLifecycleState,
) -> bool {
    matches!(
        (from, to),
        (
            CapabilityLifecycleState::Admitted,
            CapabilityLifecycleState::Installed
        ) | (
            CapabilityLifecycleState::Installed,
            CapabilityLifecycleState::Enabled
        ) | (
            CapabilityLifecycleState::Disabled,
            CapabilityLifecycleState::Enabled
        ) | (
            CapabilityLifecycleState::Enabled,
            CapabilityLifecycleState::Disabled
        ) | (
            CapabilityLifecycleState::Enabled,
            CapabilityLifecycleState::Enabled
        ) | (
            CapabilityLifecycleState::Admitted
                | CapabilityLifecycleState::Installed
                | CapabilityLifecycleState::Enabled
                | CapabilityLifecycleState::Disabled,
            CapabilityLifecycleState::Revoked
        )
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageDistributionReceipt {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityDistributionReceiptRef,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub peer: FederatedPeerRef,
    pub peer_grant: FederatedPeerGrantRef,
    pub authority_epoch: AuthorityEpoch,
    pub plan_digest: PlanDigest,
    pub lease: RemoteExecutionLeaseRef,
    pub fence_token: u64,
    pub installed_generation: u64,
    pub ground_truth: EvidenceRef,
    pub verified: RequiredTrue,
}

impl CapabilityPackageDistributionReceipt {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.authority_epoch.0 == 0
            || self.fence_token == 0
            || self.installed_generation == 0
        {
            return Err(Error(
                "capability distribution receipt is incomplete".into(),
            ));
        }
        required(&self.reference.0, "capability distribution receipt")?;
        required(&self.package.0, "distributed package")?;
        required(&self.release.0, "distributed release")?;
        required_digest(&self.package_digest, "distributed package digest")?;
        required(&self.peer.0, "distribution peer")?;
        required(&self.peer_grant.0, "distribution peer grant")?;
        required(&self.plan_digest.0, "distribution plan digest")?;
        required(&self.lease.0, "distribution lease")?;
        required(&self.ground_truth.0, "distribution ground truth")
    }
}

/// Non-fact authority-to-executor payload. It carries an exact admitted
/// release through the existing M4 remote operation; the receiver must still
/// perform local admission and policy checks before persisting anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageDistributionEnvelope {
    pub schema_version: SchemaVersion,
    pub install_plan: CapabilityInstallPlan,
    pub package: SignedCapabilityPackage,
    pub admission: CapabilityPackageAdmission,
    #[serde(default)]
    pub dependency_admissions: Vec<CapabilityPackageAdmission>,
    pub publisher_grant: CapabilityPublisherGrant,
    pub authority_policy: CapabilityAdmissionPolicy,
    pub target_peer: FederatedPeerRef,
    pub authority_epoch: AuthorityEpoch,
    pub content_digest: SchemaDigest,
}

impl CapabilityPackageDistributionEnvelope {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != M5_SCHEMA_VERSION || self.authority_epoch.0 == 0 {
            return Err(Error(
                "capability distribution envelope is incomplete".into(),
            ));
        }
        required(&self.target_peer.0, "distribution target peer")?;
        self.install_plan.validate()?;
        if self.install_plan.operation != CapabilityPackageOperation::Distribute {
            return Err(Error(
                "distribution envelope install plan has the wrong operation".into(),
            ));
        }
        self.package.validate()?;
        self.admission.validate()?;
        for dependency in &self.dependency_admissions {
            dependency.validate()?;
        }
        if self
            .dependency_admissions
            .windows(2)
            .any(|pair| pair[0].release >= pair[1].release)
        {
            return Err(Error(
                "distribution dependency admissions must be sorted and unique".into(),
            ));
        }
        let expected_dependencies = self
            .package
            .manifest
            .dependencies
            .iter()
            .map(|dependency| dependency.release.clone())
            .collect::<BTreeSet<_>>();
        let actual_dependencies = self
            .dependency_admissions
            .iter()
            .map(|dependency| dependency.release.clone())
            .collect::<BTreeSet<_>>();
        if expected_dependencies != actual_dependencies
            || self
                .dependency_admissions
                .iter()
                .zip(&self.package.manifest.dependencies)
                .any(|(admission, dependency)| {
                    admission.package != dependency.package
                        || admission.release != dependency.release
                        || admission.package_digest != dependency.digest
                })
        {
            return Err(Error(
                "distribution dependency admissions do not close the package".into(),
            ));
        }
        self.publisher_grant.validate()?;
        self.authority_policy.validate()?;
        if self.install_plan.package != self.package.manifest.package
            || self.install_plan.release != self.package.manifest.release
            || self.install_plan.package_digest != self.package.package_digest
            || self.install_plan.admission != self.admission.reference
            || self.install_plan.scope != self.package.manifest.scope
            || self.install_plan.policy != self.authority_policy.reference
            || self.install_plan.policy_version != self.authority_policy.version
            || self.admission.package != self.package.manifest.package
            || self.admission.release != self.package.manifest.release
            || self.admission.package_digest != self.package.package_digest
            || self.admission.publisher_grant != self.publisher_grant.reference
            || self.admission.publisher_version != self.publisher_grant.version
            || self.package.manifest.publisher != self.publisher_grant.publisher
            || !scope_contains(&self.publisher_grant.scope, &self.package.manifest.scope)
        {
            return Err(Error(
                "distribution envelope authority bindings do not match".into(),
            ));
        }
        let mut material = self.clone();
        material.content_digest = SchemaDigest(String::new());
        if self.content_digest != canonical_digest(&material)? {
            return Err(Error(
                "distribution envelope content digest does not match".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.content_digest = SchemaDigest(String::new());
        self.content_digest = canonical_digest(&material)?;
        Ok(())
    }
}

/// Executor-local, content-addressed ground-truth record. This is not an
/// authority event and contains no package body or credential material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityExecutorInstallRecord {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub envelope_digest: SchemaDigest,
    pub installed_generation: u64,
    pub record_digest: SchemaDigest,
}

impl CapabilityExecutorInstallRecord {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != M5_SCHEMA_VERSION || self.installed_generation == 0 {
            return Err(Error("executor package record is incomplete".into()));
        }
        required(&self.peer.0, "executor record peer")?;
        required(&self.package.0, "executor record package")?;
        required(&self.release.0, "executor record release")?;
        required_digest(&self.package_digest, "executor record package digest")?;
        required_digest(&self.envelope_digest, "executor record envelope digest")?;
        required_digest(&self.record_digest, "executor record digest")?;
        let mut material = self.clone();
        material.record_digest = SchemaDigest(String::new());
        if self.record_digest != canonical_digest(&material)? {
            return Err(Error(
                "executor package record digest does not match".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.record_digest = SchemaDigest(String::new());
        self.record_digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackageState {
    pub schema_version: SchemaVersion,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub lifecycle: CapabilityLifecycleState,
    pub active_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CapabilityInstallPlanRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalId>,
}

impl CapabilityPackageState {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("capability package state schema is invalid".into()));
        }
        required(&self.package.0, "package state package")?;
        required(&self.release.0, "package state release")?;
        required_digest(&self.package_digest, "package state digest")?;
        if self.lifecycle == CapabilityLifecycleState::Enabled && self.active_generation == 0 {
            return Err(Error("enabled package has no active generation".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEcosystemSnapshot {
    pub schema_version: SchemaVersion,
    pub scope: Scope,
    pub version: EcosystemAggregateVersion,
    pub publishers: Vec<CapabilityPublisherGrantRef>,
    pub admissions: Vec<CapabilityAdmissionRef>,
    pub packages: Vec<CapabilityPackageState>,
    pub distributions: Vec<CapabilityDistributionReceiptRef>,
    pub digest: SchemaDigest,
}

impl CapabilityEcosystemSnapshot {
    pub fn empty(scope: Scope) -> Self {
        let mut snapshot = Self {
            schema_version: M5_SCHEMA_VERSION,
            scope,
            version: EcosystemAggregateVersion::zero(),
            publishers: Vec::new(),
            admissions: Vec::new(),
            packages: Vec::new(),
            distributions: Vec::new(),
            digest: SchemaDigest(String::new()),
        };
        snapshot.digest = snapshot
            .computed_digest()
            .unwrap_or_else(|_| SchemaDigest("sha256:invalid".into()));
        snapshot
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error(
                "capability ecosystem snapshot schema is invalid".into(),
            ));
        }
        required(&self.scope.0, "capability ecosystem snapshot scope")?;
        self.version.validate()?;
        validate_sorted_unique(&self.publishers, "snapshot publishers")?;
        validate_sorted_unique(&self.admissions, "snapshot admissions")?;
        validate_sorted_unique(&self.distributions, "snapshot distributions")?;
        for package in &self.packages {
            package.validate()?;
        }
        if self
            .packages
            .windows(2)
            .any(|pair| pair[0].package >= pair[1].package)
        {
            return Err(Error("snapshot package states are not sorted".into()));
        }
        required_digest(&self.digest, "ecosystem snapshot digest")?;
        if self.digest != self.computed_digest()? {
            return Err(Error("ecosystem snapshot digest does not match".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        self.digest = self.computed_digest()?;
        Ok(())
    }

    fn computed_digest(&self) -> Result<SchemaDigest> {
        canonical_digest(&(
            &self.scope,
            &self.version,
            &self.publishers,
            &self.admissions,
            &self.packages,
            &self.distributions,
        ))
    }
}
