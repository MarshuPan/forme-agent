//! Exact, one-shot input grants for installed workers.

use super::worker::{
    validate_worker_action, validate_worker_nonce, validate_worker_plan_digest,
    validate_worker_principal, validate_worker_ref, validate_workspace_scope, WorkerJob,
    WorkerPermit,
};
use super::{validate_product_digest, version};
use crate::{
    canonical_digest, ActionIntent, ActionParameters, AuthorityEpoch, ContentRef, Error,
    ExternalInput, Nonce, PlanDigest, Result, SchemaDigest, SchemaVersion, Scope, Timestamp,
};
use serde::{Deserialize, Serialize};

pub const WORKER_INPUT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const WORKER_INPUT_MAX_GRANTS: usize = 16;
pub const WORKER_INPUT_MAX_CONTENT_BYTES: u64 = 16 * 1024 * 1024;
pub const WORKER_INPUT_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
pub const WORKER_INPUT_MAX_CHUNK_BYTES: u64 = 64 * 1024;
pub const INSTALLED_PTY_PROFILE: &str = "sandbox:installed-pty-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerInputKind {
    Content,
    Secret,
}

/// A single plan-bound input authorization. Resolved bytes and secret values
/// never appear in this object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerInputGrantV1 {
    pub schema_version: SchemaVersion,
    pub grant_ref: String,
    pub installation_ref: String,
    pub owner_ref: crate::VerifiedPrincipal,
    pub job_id: String,
    pub action_ref: crate::ActionId,
    pub plan_digest: PlanDigest,
    pub normalized_action_digest: SchemaDigest,
    pub permit_digest: SchemaDigest,
    pub authority_epoch: AuthorityEpoch,
    pub nonce: Nonce,
    pub workspace_scope: Scope,
    pub input_path: String,
    pub input_kind: WorkerInputKind,
    #[serde(default)]
    pub content_ref: Option<ContentRef>,
    #[serde(default)]
    pub content_digest: Option<SchemaDigest>,
    #[serde(default)]
    pub content_bytes: Option<u64>,
    #[serde(default)]
    pub secret_slot_ref: Option<String>,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}

impl WorkerInputGrantV1 {
    pub fn expected_ref(&self) -> Result<String> {
        let mut material = self.clone();
        material.grant_ref.clear();
        let digest = canonical_digest(&("forme.product.worker.input-grant.v1", material))?;
        Ok(format!("worker-input-grant:{}", &digest.0[7..]))
    }

    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.grant_ref, "worker input grant")?;
        validate_worker_ref(&self.installation_ref, "worker input installation")?;
        validate_worker_principal(&self.owner_ref)?;
        validate_worker_ref(&self.job_id, "worker input job")?;
        validate_worker_action(&self.action_ref)?;
        validate_worker_plan_digest(&self.plan_digest)?;
        validate_product_digest(&self.normalized_action_digest)?;
        validate_product_digest(&self.permit_digest)?;
        if self.authority_epoch.0 == 0 {
            return Err(Error("worker input authority epoch is invalid".into()));
        }
        validate_worker_nonce(&self.nonce)?;
        validate_workspace_scope(&self.workspace_scope)?;
        validate_input_path(&self.input_path)?;
        if self.issued_at <= 0 || self.expires_at <= self.issued_at {
            return Err(Error("worker input grant lifetime is invalid".into()));
        }
        if self.grant_ref != self.expected_ref()? {
            return Err(Error(
                "worker input grant reference is not canonical".into(),
            ));
        }
        match self.input_kind {
            WorkerInputKind::Content => {
                let Some(reference) = &self.content_ref else {
                    return Err(Error("content input reference is missing".into()));
                };
                if reference.0.trim().is_empty() || reference.0.len() > 512 {
                    return Err(Error("content input reference is invalid".into()));
                }
                let Some(digest) = &self.content_digest else {
                    return Err(Error("content input digest is missing".into()));
                };
                validate_product_digest(digest)?;
                let Some(bytes) = self.content_bytes else {
                    return Err(Error("content input size is missing".into()));
                };
                if bytes == 0 || bytes > WORKER_INPUT_MAX_CONTENT_BYTES {
                    return Err(Error("content input size is out of bounds".into()));
                }
                if self.secret_slot_ref.is_some() {
                    return Err(Error("content input contains a secret slot".into()));
                }
            }
            WorkerInputKind::Secret => {
                let Some(slot) = &self.secret_slot_ref else {
                    return Err(Error("secret input slot is missing".into()));
                };
                validate_worker_ref(slot, "worker secret slot")?;
                if !slot.starts_with("secret-slot:")
                    || self.content_ref.is_some()
                    || self.content_digest.is_some()
                    || self.content_bytes.is_some()
                {
                    return Err(Error("secret input grant contains content material".into()));
                }
            }
        }
        Ok(())
    }

    pub fn validate_for(
        &self,
        job: &WorkerJob,
        permit: &WorkerPermit,
        intent: &ActionIntent,
        now: Timestamp,
    ) -> Result<()> {
        self.validate()?;
        job.validate()?;
        permit.validate_for(job, now)?;
        if self.job_id != job.job_id
            || self.owner_ref != job.owner_ref
            || self.action_ref != job.action_ref
            || self.plan_digest != job.plan_digest
            || self.normalized_action_digest != crate::normalized_action_digest(intent)?
            || self.permit_digest != canonical_digest(permit)?
            || self.authority_epoch != job.authority_epoch
            || self.nonce != job.nonce
            || self.workspace_scope != job.workspace_scope
            || now >= self.expires_at
            || self.expires_at > job.deadline.min(permit.deadline)
            || !input_matches_intent(self, intent)
        {
            return Err(Error(
                "worker input grant is not bound to the exact plan".into(),
            ));
        }
        Ok(())
    }
}

/// Host-side, read-only content transfer. Secret grants are deliberately
/// excluded: secret material is resolved in the Host's final launch step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerInputReadRequest {
    pub schema_version: SchemaVersion,
    pub grant: WorkerInputGrantV1,
    pub offset: u64,
    pub max_bytes: u64,
}

impl WorkerInputReadRequest {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        self.grant.validate()?;
        if self.grant.input_kind != WorkerInputKind::Content
            || self.offset > self.grant.content_bytes.unwrap_or(0)
            || !(1..=WORKER_INPUT_MAX_CHUNK_BYTES).contains(&self.max_bytes)
        {
            return Err(Error("worker input read request is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerInputChunk {
    pub schema_version: SchemaVersion,
    pub grant_ref: String,
    pub offset: u64,
    pub next_offset: u64,
    pub bytes: Vec<u8>,
    pub finished: bool,
    pub content_digest: SchemaDigest,
}

impl WorkerInputChunk {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.grant_ref, "worker input grant")?;
        validate_product_digest(&self.content_digest)?;
        if self.next_offset < self.offset
            || self.next_offset - self.offset != self.bytes.len() as u64
            || self.bytes.len() as u64 > WORKER_INPUT_MAX_CHUNK_BYTES
            || (!self.finished && self.bytes.is_empty())
        {
            return Err(Error("worker input chunk is invalid".into()));
        }
        Ok(())
    }
}

fn validate_input_path(path: &str) -> Result<()> {
    let valid = matches!(
        path,
        "browser.type" | "computer.type" | "pty.stdin" | "app_api.body" | "notification.body"
    ) || path
        .strip_prefix("pty.environment/")
        .is_some_and(valid_environment_name);
    if !valid || path.len() > 128 || path.chars().any(char::is_control) {
        return Err(Error(
            "worker input path is not a closed input location".into(),
        ));
    }
    Ok(())
}

fn valid_environment_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().enumerate().all(|(index, byte)| {
            (index == 0 && byte.is_ascii_alphabetic())
                || (index > 0 && (byte.is_ascii_alphanumeric() || byte == b'_'))
        })
}

fn input_matches_intent(grant: &WorkerInputGrantV1, intent: &ActionIntent) -> bool {
    let path = grant.input_path.as_str();
    let kind = grant.input_kind;
    let matches_input = |input: &ExternalInput| match (kind, input) {
        (WorkerInputKind::Content, ExternalInput::Content(reference)) => {
            grant.content_ref.as_ref() == Some(reference)
        }
        (WorkerInputKind::Secret, ExternalInput::Secret(_)) => true,
        _ => false,
    };
    match (&intent.parameters, path) {
        (ActionParameters::Browser(spec), "browser.type") => match &spec.operation {
            crate::BrowserOperation::Type { input, .. } => matches_input(input),
            _ => false,
        },
        (ActionParameters::Computer(spec), "computer.type") => match &spec.operation {
            crate::ComputerOperation::Type { input } => matches_input(input),
            _ => false,
        },
        (ActionParameters::Pty(spec), "pty.stdin") => {
            spec.input.as_ref().is_some_and(matches_input)
        }
        (ActionParameters::Pty(spec), path) if path.starts_with("pty.environment/") => {
            let name = &path["pty.environment/".len()..];
            spec.environment
                .iter()
                .any(|binding| binding.name == name && matches!(kind, WorkerInputKind::Secret))
        }
        (ActionParameters::AppApi(spec), "app_api.body") => match &spec.operation {
            crate::AppApiOperation::Mutation {
                body: Some(input), ..
            } => matches_input(input),
            _ => false,
        },
        (ActionParameters::Notification { body_ref, .. }, "notification.body") => {
            matches!(kind, WorkerInputKind::Content) && !body_ref.0.trim().is_empty()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_paths_are_closed() {
        assert!(validate_input_path("pty.stdin").is_ok());
        assert!(validate_input_path("pty.environment/TOKEN").is_ok());
        assert!(validate_input_path("pty.environment/1TOKEN").is_err());
        assert!(validate_input_path("../../secret").is_err());
    }

    #[test]
    fn secret_slot_cannot_carry_content_material() {
        let mut grant = sample_secret();
        grant.content_ref = Some(ContentRef("content:wrong".into()));
        grant.grant_ref = grant.expected_ref().unwrap();
        assert!(grant.validate().is_err());
    }

    #[test]
    fn input_chunk_requires_contiguous_bounded_bytes() {
        let digest = SchemaDigest(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        );
        let chunk = WorkerInputChunk {
            schema_version: WORKER_INPUT_SCHEMA_VERSION,
            grant_ref: "worker-input-grant:test".into(),
            offset: 4,
            next_offset: 7,
            bytes: b"abc".to_vec(),
            finished: false,
            content_digest: digest.clone(),
        };
        assert!(chunk.validate().is_ok());
        let mut invalid = chunk;
        invalid.next_offset = 8;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn read_request_rejects_secret_grants() {
        let mut grant = sample_secret();
        grant.grant_ref = grant.expected_ref().unwrap();
        let request = WorkerInputReadRequest {
            schema_version: WORKER_INPUT_SCHEMA_VERSION,
            grant,
            offset: 0,
            max_bytes: 1,
        };
        assert!(request.validate().is_err());
    }

    fn sample_secret() -> WorkerInputGrantV1 {
        let mut grant = WorkerInputGrantV1 {
            schema_version: WORKER_INPUT_SCHEMA_VERSION,
            grant_ref: "worker-input-grant:placeholder".into(),
            installation_ref: "installation:test".into(),
            owner_ref: crate::VerifiedPrincipal("owner:test".into()),
            job_id: "worker-job:test".into(),
            action_ref: crate::ActionId("action:test".into()),
            plan_digest: PlanDigest(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            ),
            normalized_action_digest: SchemaDigest(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ),
            permit_digest: SchemaDigest(
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
            ),
            authority_epoch: AuthorityEpoch(1),
            nonce: Nonce("worker-nonce:test".into()),
            workspace_scope: Scope("workspace:test".into()),
            input_path: "pty.environment/TOKEN".into(),
            input_kind: WorkerInputKind::Secret,
            content_ref: None,
            content_digest: None,
            content_bytes: None,
            secret_slot_ref: Some("secret-slot:test".into()),
            issued_at: 10,
            expires_at: 20,
        };
        grant.grant_ref = grant.expected_ref().unwrap();
        grant
    }
}
