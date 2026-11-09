//! Host-issued consent is separate from brain approval and worker permission.
use super::*;
use crate::{ApprovalId, AuthorityEpoch, RunId, VerifiedPrincipal, Version, WorkerExecutionSpec};
use std::collections::BTreeSet;

pub const DESKTOP_AUTHORITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const DESKTOP_AUTHORITY_REQUEST_MAX_BYTES: usize = 16 * 1024;
pub const DESKTOP_CATALOG_MAX_ENTRIES: usize = 64;
pub const DESKTOP_CATALOG_TTL_MS: i64 = 30_000;

/// A locator only: Host must still find and validate the registered original.
pub fn desktop_pending_ref(
    approval: &ApprovalId,
    plan: &crate::PlanDigest,
) -> crate::Result<String> {
    validate_product_id(&approval.0)?;
    validate_product_digest(&SchemaDigest(plan.0.clone()))?;
    let digest = crate::canonical_digest(&(
        "forme.product.desktop.pending.v1",
        "reference",
        approval,
        plan,
    ))?;
    Ok(format!("desktop-pending:{}", &digest.0[7..]))
}

pub fn desktop_consent_ref(pending_ref: &str) -> crate::Result<String> {
    validate_product_id(pending_ref)?;
    let digest =
        crate::canonical_digest(&("forme.product.desktop.consent.v1", "reference", pending_ref))?;
    Ok(format!("desktop-consent:{}", &digest.0[7..]))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSessionChoice {
    pub schema_version: SchemaVersion,
    pub session_ref: String,
    pub platform: DesktopPlatform,
    pub label: String,
    pub available: bool,
    pub supported_capabilities: BTreeSet<DesktopCapability>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSessionCatalog {
    pub schema_version: SchemaVersion,
    pub installation_ref: String,
    pub owner_ref: VerifiedPrincipal,
    pub authority_epoch: AuthorityEpoch,
    pub catalog_revision: u64,
    pub catalog_digest: SchemaDigest,
    pub entries: Vec<DesktopSessionChoice>,
    pub observed_at: Timestamp,
    pub expires_at: Timestamp,
}
impl DesktopSessionCatalog {
    pub fn digest(&self) -> crate::Result<SchemaDigest> {
        crate::canonical_digest(&(
            "forme.product.desktop.catalog.v1",
            self.schema_version,
            &self.installation_ref,
            &self.owner_ref,
            self.authority_epoch,
            self.catalog_revision,
            &self.entries,
            self.observed_at,
            self.expires_at,
        ))
    }
    pub fn validate_at(&self, now: Timestamp) -> crate::Result<()> {
        authority_version(self.schema_version)?;
        validate_product_id(&self.installation_ref)?;
        validate_product_id(&self.owner_ref.0)?;
        validate_product_digest(&self.catalog_digest)?;
        if self.authority_epoch.0 == 0
            || self.catalog_revision == 0
            || self.entries.len() > DESKTOP_CATALOG_MAX_ENTRIES
            || self.observed_at <= 0
            || now < self.observed_at
            || now >= self.expires_at
            || self.expires_at.saturating_sub(self.observed_at) > DESKTOP_CATALOG_TTL_MS
            || self.catalog_digest != self.digest()?
        {
            return Err(authority_invalid());
        }
        let mut seen = BTreeSet::new();
        for entry in &self.entries {
            authority_version(entry.schema_version)?;
            validate_product_id(&entry.session_ref)?;
            if !seen.insert(&entry.session_ref)
                || entry.label.trim().is_empty()
                || entry.label.len() > 128
                || entry.label.chars().any(char::is_control)
                || (entry.available && entry.supported_capabilities.is_empty())
            {
                return Err(authority_invalid());
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopPendingAction {
    pub schema_version: SchemaVersion,
    pub pending_ref: String,
    pub owner_ref: VerifiedPrincipal,
    pub authority_epoch: AuthorityEpoch,
    pub run_ref: RunId,
    pub approval_ref: ApprovalId,
    pub action_ref: crate::ActionId,
    pub plan_digest: crate::PlanDigest,
    pub normalized_action_digest: SchemaDigest,
    pub affected_resources: Vec<crate::ResourceRef>,
    pub capability: DesktopCapability,
    pub policy_version: Version,
    pub tool_schema_version: Version,
    pub expires_at: Timestamp,
}
impl DesktopPendingAction {
    pub fn validate(&self) -> crate::Result<()> {
        authority_version(self.schema_version)?;
        for id in [
            &self.pending_ref,
            &self.owner_ref.0,
            &self.run_ref.0,
            &self.approval_ref.0,
            &self.action_ref.0,
        ] {
            validate_product_id(id)?;
        }
        validate_product_digest(&SchemaDigest(self.plan_digest.0.clone()))?;
        validate_product_digest(&self.normalized_action_digest)?;
        if self.authority_epoch.0 == 0
            || self.policy_version.0 == 0
            || self.tool_schema_version.0 == 0
            || self.expires_at <= 0
            || self.affected_resources.is_empty()
            || self.affected_resources.len() > 256
            || crate::normalize_resource_set(&self.affected_resources) != self.affected_resources
            || self.capability == DesktopCapability::PtyInput
        {
            return Err(authority_invalid());
        }
        for resource in &self.affected_resources {
            if resource.0.len() > 512
                || resource.0.contains(['\\', '\0'])
                || resource.0.chars().any(char::is_control)
                || resource
                    .0
                    .split('/')
                    .any(|part| part == "." || part == "..")
            {
                return Err(authority_invalid());
            }
        }
        bounded(self)
    }
    pub fn digest(&self) -> crate::Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.desktop.pending.v1", self))
    }
    pub fn validate_for(&self, request: &DesktopGrantRequest, now: Timestamp) -> crate::Result<()> {
        self.validate()?;
        request.validate_at(now)?;
        let job = &request.job;
        let execution = &request.execution;
        if self.expires_at <= now
            || self.owner_ref != job.owner_ref
            || self.authority_epoch != job.authority_epoch
            || self.action_ref != job.action_ref
            || self.plan_digest != job.plan_digest
            || self.normalized_action_digest != execution.normalized_action_digest
            || self.affected_resources != job.affected_resources
            || self.capability != desktop_intent_capability(&execution.intent)?
        {
            return Err(authority_invalid());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopConsentRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub pending_ref: String,
    pub catalog_revision: u64,
    pub catalog_digest: SchemaDigest,
    pub session_ref: String,
    pub expires_at: Timestamp,
}
impl DesktopConsentRequest {
    pub fn validate(&self) -> crate::Result<()> {
        authority_version(self.schema_version)?;
        for id in [&self.operation_id, &self.pending_ref, &self.session_ref] {
            validate_product_id(id)?;
        }
        validate_product_digest(&self.catalog_digest)?;
        if self.catalog_revision == 0 || self.expires_at <= 0 {
            return Err(authority_invalid());
        }
        bounded(self)
    }
    pub fn digest(&self) -> crate::Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.desktop.consent.v1", self))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopConsentStatus {
    Prepared,
    Committed,
    Revoked,
    Expired,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopConsentView {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub revision: u64,
    pub pending_ref: String,
    pub consent_ref: String,
    pub status: DesktopConsentStatus,
    pub expires_at: Timestamp,
    pub reason_ref: Option<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopGrantRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub consent_ref: String,
    pub job: WorkerJob,
    pub permit: WorkerPermit,
    pub execution: WorkerExecutionSpec,
}
impl DesktopGrantRequest {
    pub fn validate_at(&self, now: Timestamp) -> crate::Result<()> {
        authority_version(self.schema_version)?;
        validate_product_id(&self.operation_id)?;
        validate_product_id(&self.consent_ref)?;
        self.job.validate()?;
        self.permit.validate_for(&self.job, now)?;
        self.execution.validate_for(&self.job, &self.permit, now)?;
        if self.execution.desktop_session.is_some()
            || self.execution.backend != crate::BackendKind::Computer
        {
            return Err(authority_invalid());
        }
        desktop_intent_capability(&self.execution.intent)?;
        bounded(self)
    }
    pub fn digest(&self) -> crate::Result<SchemaDigest> {
        crate::canonical_digest(&("forme.product.desktop.grant.v1", self))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopGrantStatus {
    Issued,
    Consumed,
    Revoked,
    Expired,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopGrantView {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub revision: u64,
    pub binding: DesktopSessionBinding,
    pub status: DesktopGrantStatus,
    pub reason_ref: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopRevokeRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub consent_ref: String,
}
impl DesktopRevokeRequest {
    pub fn validate(&self) -> crate::Result<()> {
        authority_version(self.schema_version)?;
        validate_product_id(&self.operation_id)?;
        validate_product_id(&self.consent_ref)?;
        bounded(self)
    }
}
/// Maps only the Computer operations supported by the desktop contract.
/// Reference input and PTY require separate input contracts.
pub fn desktop_intent_capability(intent: &crate::ActionIntent) -> crate::Result<DesktopCapability> {
    let crate::ActionParameters::Computer(spec) = &intent.parameters else {
        return Err(authority_invalid());
    };
    Ok(match &spec.operation {
        crate::ComputerOperation::Move { .. } => DesktopCapability::PointerMove,
        crate::ComputerOperation::Click { .. } => DesktopCapability::PointerClick,
        crate::ComputerOperation::Type {
            input: crate::ExternalInput::Literal(_),
        } => DesktopCapability::KeyboardType,
        crate::ComputerOperation::Type { .. } => return Err(authority_invalid()),
        crate::ComputerOperation::Key { .. } => DesktopCapability::KeyboardKey,
        crate::ComputerOperation::Scroll { .. } => DesktopCapability::Scroll,
        crate::ComputerOperation::Screenshot => DesktopCapability::Screenshot,
    })
}
fn authority_version(version: SchemaVersion) -> crate::Result<()> {
    if version != DESKTOP_AUTHORITY_SCHEMA_VERSION {
        return Err(authority_invalid());
    }
    Ok(())
}
fn bounded(value: &impl Serialize) -> crate::Result<()> {
    if serde_json::to_vec(value)
        .map_err(|_| authority_invalid())?
        .len()
        > DESKTOP_AUTHORITY_REQUEST_MAX_BYTES
    {
        return Err(authority_invalid());
    }
    Ok(())
}
fn authority_invalid() -> crate::Error {
    crate::Error(
        "desktop authority request is invalid or does not match the original action".into(),
    )
}
