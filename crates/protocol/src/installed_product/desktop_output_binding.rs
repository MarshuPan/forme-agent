//! Original-job metadata for the separate desktop output protocol.
use crate as p;
use serde::{Deserialize, Serialize};

pub const DESKTOP_OUTPUT_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(2);
pub const DESKTOP_OUTPUT_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const DESKTOP_OUTPUT_MAX_PIXELS: u64 = 16_777_216;
pub const DESKTOP_OUTPUT_CHUNK_BYTES: u32 = 32 * 1024;
pub const DESKTOP_OUTPUT_MAX_CHUNKS: usize = 2048;
pub const DESKTOP_OUTPUT_MAX_HEADER_BYTES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopExchangeBinding {
    pub schema_version: p::SchemaVersion,
    pub installation_ref: String,
    pub owner_ref: p::VerifiedPrincipal,
    pub job_id: String,
    pub permit_ref: String,
    pub permit_digest: p::SchemaDigest,
    pub action_ref: p::ActionId,
    pub workspace_scope: p::Scope,
    pub plan_digest: p::PlanDigest,
    pub normalized_action_digest: p::SchemaDigest,
    pub authority_epoch: p::AuthorityEpoch,
    pub nonce: p::Nonce,
    pub grant_ref: String,
    pub os_session_ref: String,
    pub session_generation: u64,
    pub deadline: p::Timestamp,
}

impl DesktopExchangeBinding {
    pub fn from_request(
        installation_ref: String,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        request: &p::DesktopActionRequest,
    ) -> p::Result<Self> {
        let value = Self {
            schema_version: DESKTOP_OUTPUT_SCHEMA_VERSION,
            installation_ref,
            owner_ref: job.owner_ref.clone(),
            job_id: job.job_id.clone(),
            permit_ref: permit.permit_ref.clone(),
            permit_digest: p::canonical_digest(permit)?,
            action_ref: job.action_ref.clone(),
            workspace_scope: job.workspace_scope.clone(),
            plan_digest: job.plan_digest.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            grant_ref: request.binding.grant_ref.clone(),
            os_session_ref: request.binding.os_session_ref.clone(),
            session_generation: request.binding.session_generation,
            deadline: request.deadline,
        };
        value.validate_request(request)?;
        value.result_binding(1).validate_for(job, permit)?;
        permit.validate_for(job, request.binding.issued_at)?;
        request
            .binding
            .validate_for(job, permit, request.binding.issued_at)?;
        if request.binding.issued_at < permit.issued_at {
            return Err(invalid());
        }
        if value.deadline > job.deadline || value.deadline > permit.deadline {
            return Err(invalid());
        }
        Ok(value)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION
            || self.session_generation == 0
            || self.deadline <= 0
        {
            return Err(invalid());
        }
        self.result_binding(1).validate()?;
        p::validate_product_id(&self.grant_ref)?;
        p::validate_product_id(&self.os_session_ref)?;
        Ok(())
    }

    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        self.validate()?;
        p::canonical_digest(&("forme.desktop.output.binding.v2", self))
    }

    pub fn validate_request(&self, request: &p::DesktopActionRequest) -> p::Result<()> {
        self.validate()?;
        request.binding.validate()?;
        if request.schema_version != p::DESKTOP_SESSION_SCHEMA_VERSION
            || request.capability != p::DesktopCapability::Screenshot
            || !request
                .binding
                .capabilities
                .contains(&p::DesktopCapability::Screenshot)
            || self.job_id != request.job_id
            || self.permit_ref != request.permit_ref
            || self.owner_ref != request.binding.owner_ref
            || self.action_ref != request.binding.action_ref
            || self.plan_digest != request.binding.plan_digest
            || self.normalized_action_digest != request.normalized_action_digest
            || self.authority_epoch != request.binding.authority_epoch
            || self.nonce != request.binding.nonce
            || self.grant_ref != request.binding.grant_ref
            || self.os_session_ref != request.binding.os_session_ref
            || self.session_generation != request.binding.session_generation
            || self.deadline != request.deadline
            || self.deadline > request.binding.expires_at
            || self.deadline <= request.binding.issued_at
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn validate_result(&self, binding: &p::WorkerResultBinding) -> p::Result<()> {
        self.validate()?;
        binding.validate()?;
        let mut expected = self.result_binding(binding.output_budget_bytes);
        expected.receipt_ref = binding.receipt_ref.clone();
        if expected != *binding {
            return Err(invalid());
        }
        Ok(())
    }

    fn result_binding(&self, output_budget_bytes: u64) -> p::WorkerResultBinding {
        p::WorkerResultBinding {
            schema_version: p::SchemaVersion(1),
            installation_ref: self.installation_ref.clone(),
            job_id: self.job_id.clone(),
            owner_ref: self.owner_ref.clone(),
            action_ref: self.action_ref.clone(),
            workspace_scope: self.workspace_scope.clone(),
            plan_digest: self.plan_digest.clone(),
            normalized_action_digest: Some(self.normalized_action_digest.clone()),
            permit_ref: self.permit_ref.clone(),
            permit_digest: self.permit_digest.clone(),
            authority_epoch: self.authority_epoch,
            nonce: self.nonce.clone(),
            receipt_ref: None,
            output_budget_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOutputTransfer {
    pub schema_version: p::SchemaVersion,
    pub binding: DesktopExchangeBinding,
    pub transfer_ref: String,
    pub output_budget_bytes: u64,
    pub width: u32,
    pub height: u32,
}

impl DesktopOutputTransfer {
    pub fn new(
        binding: DesktopExchangeBinding,
        intent: &p::ActionIntent,
        budget: u64,
    ) -> p::Result<Self> {
        let (width, height) = dimensions(intent)?;
        let transfer_ref = format!("desktop-transfer:{}", &binding.digest()?.0[7..]);
        let value = Self {
            schema_version: DESKTOP_OUTPUT_SCHEMA_VERSION,
            binding,
            transfer_ref,
            output_budget_bytes: budget.min(DESKTOP_OUTPUT_MAX_BYTES),
            width,
            height,
        };
        value.validate_intent(intent)?;
        Ok(value)
    }

    pub fn validate(&self) -> p::Result<()> {
        self.binding.validate()?;
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION
            || self.transfer_ref != format!("desktop-transfer:{}", &self.binding.digest()?.0[7..])
            || self.output_budget_bytes == 0
            || self.output_budget_bytes > DESKTOP_OUTPUT_MAX_BYTES
        {
            return Err(invalid());
        }
        validate_dimensions(self.width, self.height)
    }

    pub fn validate_intent(&self, intent: &p::ActionIntent) -> p::Result<()> {
        self.validate()?;
        if dimensions(intent)? != (self.width, self.height)
            || intent.scope != self.binding.workspace_scope
            || intent.intent_id != self.binding.action_ref
            || p::normalized_action_digest(intent)? != self.binding.normalized_action_digest
        {
            return Err(invalid());
        }
        Ok(())
    }
}

pub fn desktop_output_dimensions(intent: &p::ActionIntent) -> p::Result<(u32, u32)> {
    dimensions(intent)
}
fn dimensions(intent: &p::ActionIntent) -> p::Result<(u32, u32)> {
    let p::ActionParameters::Computer(spec) = &intent.parameters else {
        return Err(invalid());
    };
    if intent.schema_version != p::SchemaVersion(1)
        || intent.backend_hint != p::BackendKind::Computer
        || spec.operation != p::ComputerOperation::Screenshot
    {
        return Err(invalid());
    }
    spec.bounds.validate()?;
    if spec.schema_version != p::SchemaVersion(1)
        || spec.bounds.schema_version != p::SchemaVersion(1)
        || spec.artifact_scope != intent.scope
    {
        return Err(invalid());
    }
    let width =
        u32::try_from(i64::from(spec.bounds.max_x_exclusive) - i64::from(spec.bounds.min_x))
            .map_err(|_| invalid())?;
    let height =
        u32::try_from(i64::from(spec.bounds.max_y_exclusive) - i64::from(spec.bounds.min_y))
            .map_err(|_| invalid())?;
    validate_dimensions(width, height)?;
    Ok((width, height))
}
pub(super) fn validate_dimensions(width: u32, height: u32) -> p::Result<()> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > DESKTOP_OUTPUT_MAX_PIXELS
    {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn invalid() -> p::Error {
    p::Error("desktop output binding is invalid".into())
}
