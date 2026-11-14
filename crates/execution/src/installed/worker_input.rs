//! Executor-local resolution and ephemeral delivery of worker inputs.
//!
//! The resolver is deliberately injected by the Host. The default resolver
//! rejects every grant, so a service cannot accidentally turn a typed grant
//! into an ambient credential or a file-backed secret.

use std::collections::BTreeMap;
use std::sync::Arc;

use forme_protocol as p;
use zeroize::Zeroizing;

use crate::{ContentResolver, ResolvedSecret, SecretResolver};

const INPUT_FRAME_MAGIC: &[u8] = b"FORME-WORKER-INPUT-V1\0";

pub trait WorkerInputResolver: Send + Sync {
    fn resolve(
        &self,
        grant: &p::WorkerInputGrantV1,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        intent: &p::ActionIntent,
        now: p::Timestamp,
    ) -> p::Result<WorkerInputMaterial>;

    fn resolve_content(
        &self,
        _grant: &p::WorkerInputGrantV1,
        _job: &p::WorkerJob,
        _permit: &p::WorkerPermit,
        _now: p::Timestamp,
    ) -> p::Result<WorkerInputMaterial> {
        Err(p::Error(
            "worker content resolver is unavailable; execution is blocked".into(),
        ))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RejectingWorkerInputResolver;

impl WorkerInputResolver for RejectingWorkerInputResolver {
    fn resolve(
        &self,
        _grant: &p::WorkerInputGrantV1,
        _job: &p::WorkerJob,
        _permit: &p::WorkerPermit,
        _intent: &p::ActionIntent,
        _now: p::Timestamp,
    ) -> p::Result<WorkerInputMaterial> {
        Err(p::Error(
            "worker input resolver is unavailable; execution is blocked".into(),
        ))
    }
}

/// A Host-registered secret slot. The slot name is safe to store in product
/// configuration; the underlying SecretRef is never copied into a grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSecretSlot {
    pub slot_ref: String,
    pub secret_ref: p::SecretRef,
    pub owner_ref: p::VerifiedPrincipal,
    pub workspace_scope: p::Scope,
    pub input_path: String,
    pub expires_at: p::Timestamp,
}

impl WorkerSecretSlot {
    pub fn validate(&self) -> p::Result<()> {
        if !self.slot_ref.starts_with("secret-slot:")
            || self.slot_ref.len() > 128
            || self.slot_ref.chars().any(char::is_control)
            || self.secret_ref.0.trim().is_empty()
            || self.owner_ref.0.trim().is_empty()
            || self.workspace_scope.0.trim().is_empty()
            || self.input_path.trim().is_empty()
            || self.expires_at <= 0
        {
            return Err(p::Error("worker secret slot is invalid".into()));
        }
        Ok(())
    }
}

/// A resolver backed by the Host's executor-local content and secret stores.
pub struct ManagedWorkerInputResolver {
    contents: Arc<dyn ContentResolver>,
    secrets: Arc<dyn SecretResolver>,
    slots: BTreeMap<String, WorkerSecretSlot>,
}

impl ManagedWorkerInputResolver {
    pub fn new(
        contents: Arc<dyn ContentResolver>,
        secrets: Arc<dyn SecretResolver>,
        slots: impl IntoIterator<Item = WorkerSecretSlot>,
    ) -> p::Result<Self> {
        let mut registered = BTreeMap::new();
        for slot in slots {
            slot.validate()?;
            if registered.insert(slot.slot_ref.clone(), slot).is_some() {
                return Err(p::Error("worker secret slot is duplicated".into()));
            }
        }
        Ok(Self {
            contents,
            secrets,
            slots: registered,
        })
    }
}

impl WorkerInputResolver for ManagedWorkerInputResolver {
    fn resolve(
        &self,
        grant: &p::WorkerInputGrantV1,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        intent: &p::ActionIntent,
        now: p::Timestamp,
    ) -> p::Result<WorkerInputMaterial> {
        grant.validate_for(job, permit, intent, now)?;
        match grant.input_kind {
            p::WorkerInputKind::Content => self.resolve_content(grant, job, permit, now),
            p::WorkerInputKind::Secret => {
                let slot_ref = grant
                    .secret_slot_ref
                    .as_ref()
                    .ok_or_else(|| p::Error("secret input slot is missing".into()))?;
                let slot = self
                    .slots
                    .get(slot_ref)
                    .ok_or_else(|| p::Error("worker secret slot is not registered".into()))?;
                if slot.owner_ref != job.owner_ref
                    || slot.workspace_scope != job.workspace_scope
                    || slot.input_path != grant.input_path
                    || now >= slot.expires_at
                    || slot.expires_at > grant.expires_at
                {
                    return Err(p::Error("worker secret slot is outside its grant".into()));
                }
                let secret = self.secrets.resolve(&slot.secret_ref)?;
                Ok(WorkerInputMaterial::secret(
                    grant.grant_ref.clone(),
                    grant.input_path.clone(),
                    secret,
                ))
            }
        }
    }

    fn resolve_content(
        &self,
        grant: &p::WorkerInputGrantV1,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<WorkerInputMaterial> {
        grant.validate()?;
        if grant.input_kind != p::WorkerInputKind::Content
            || grant.job_id != job.job_id
            || grant.owner_ref != job.owner_ref
            || grant.plan_digest != job.plan_digest
            || grant.permit_digest != p::canonical_digest(permit)?
            || grant.authority_epoch != job.authority_epoch
            || grant.nonce != job.nonce
            || grant.workspace_scope != job.workspace_scope
            || now >= grant.expires_at
        {
            return Err(p::Error("worker content grant is not job-bound".into()));
        }
        let reference = grant
            .content_ref
            .as_ref()
            .ok_or_else(|| p::Error("content input reference is missing".into()))?;
        let bytes = self.contents.resolve(reference)?;
        let expected_bytes = grant
            .content_bytes
            .ok_or_else(|| p::Error("content input size is missing".into()))?;
        let expected_digest = grant
            .content_digest
            .as_ref()
            .ok_or_else(|| p::Error("content input digest is missing".into()))?;
        if bytes.len() as u64 != expected_bytes
            || p::sha256_content_digest(&bytes) != *expected_digest
        {
            return Err(p::Error("content input binding changed".into()));
        }
        Ok(WorkerInputMaterial::content(
            grant.grant_ref.clone(),
            grant.input_path.clone(),
            bytes,
        ))
    }
}

#[derive(Debug)]
pub enum WorkerInputMaterialKind {
    Content(Zeroizing<Vec<u8>>),
    Secret(Zeroizing<Vec<u8>>),
}

#[derive(Debug)]
pub struct WorkerInputMaterial {
    grant_ref: String,
    input_path: String,
    kind: WorkerInputMaterialKind,
}

impl WorkerInputMaterial {
    pub fn content(grant_ref: String, input_path: String, bytes: Vec<u8>) -> Self {
        Self {
            grant_ref,
            input_path,
            kind: WorkerInputMaterialKind::Content(Zeroizing::new(bytes)),
        }
    }

    pub fn secret(grant_ref: String, input_path: String, secret: ResolvedSecret) -> Self {
        Self {
            grant_ref,
            input_path,
            kind: WorkerInputMaterialKind::Secret(Zeroizing::new(
                secret.expose().as_bytes().to_vec(),
            )),
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        match &self.kind {
            WorkerInputMaterialKind::Content(bytes) | WorkerInputMaterialKind::Secret(bytes) => {
                bytes
            }
        }
    }

    pub(crate) fn is_secret(&self) -> bool {
        matches!(self.kind, WorkerInputMaterialKind::Secret(_))
    }
}

/// Encode a bounded, one-shot stdin envelope. It contains no SecretRef or
/// value labels; the worker receives only the closed input path and bytes.
pub(crate) fn encode_input_frame(materials: &[WorkerInputMaterial]) -> p::Result<Vec<u8>> {
    if materials.len() > p::WORKER_INPUT_MAX_GRANTS {
        return Err(p::Error("worker input material is unbounded".into()));
    }
    let total = materials.iter().try_fold(0_u64, |total, material| {
        total
            .checked_add(material.bytes().len() as u64)
            .filter(|bytes| *bytes <= p::WORKER_INPUT_MAX_TOTAL_BYTES)
            .ok_or_else(|| p::Error("worker input material exceeds its byte budget".into()))
    })?;
    let mut frame = Vec::with_capacity(total as usize + 128);
    frame.extend_from_slice(INPUT_FRAME_MAGIC);
    frame.extend_from_slice(&(materials.len() as u32).to_le_bytes());
    for material in materials {
        let grant = material.grant_ref.as_bytes();
        let path = material.input_path.as_bytes();
        if grant.len() > u16::MAX as usize || path.len() > u16::MAX as usize {
            return Err(p::Error(
                "worker input material reference is too large".into(),
            ));
        }
        frame.extend_from_slice(&(grant.len() as u16).to_le_bytes());
        frame.extend_from_slice(&(path.len() as u16).to_le_bytes());
        frame.push(if material.is_secret() { 1 } else { 0 });
        frame.extend_from_slice(&(material.bytes().len() as u64).to_le_bytes());
        frame.extend_from_slice(grant);
        frame.extend_from_slice(path);
        frame.extend_from_slice(material.bytes());
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_frame_is_bounded_and_does_not_copy_slot_identifiers() {
        let material = WorkerInputMaterial::secret(
            "worker-input-grant:test".into(),
            "pty.environment/TOKEN".into(),
            ResolvedSecret::new("secret-value").unwrap(),
        );
        let frame = encode_input_frame(&[material]).unwrap();
        assert!(frame.starts_with(INPUT_FRAME_MAGIC));
        assert!(!frame
            .windows("secret-slot:".len())
            .any(|window| window == b"secret-slot:"));
        let too_large = WorkerInputMaterial::content(
            "worker-input-grant:large".into(),
            "pty.stdin".into(),
            vec![b'x'; (p::WORKER_INPUT_MAX_TOTAL_BYTES + 1) as usize],
        );
        assert!(encode_input_frame(&[too_large]).is_err());
    }
}
