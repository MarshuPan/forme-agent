//! Executor-local resolution and ephemeral delivery of worker inputs.
//!
//! The resolver is deliberately injected by the Host. The default resolver
//! rejects every grant, so a service cannot accidentally turn a typed grant
//! into an ambient credential or a file-backed secret.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

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
    slots: RwLock<BTreeMap<String, WorkerSecretSlot>>,
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
            slots: RwLock::new(registered),
        })
    }

    /// Revoke an executor-local slot before any subsequent worker admission.
    /// Existing material is held only by the short-lived material object and
    /// is zeroized when that object is dropped.
    pub fn revoke_slot(&self, slot_ref: &str) -> p::Result<()> {
        let mut slots = self
            .slots
            .write()
            .map_err(|_| p::Error("worker secret slot registry is unavailable".into()))?;
        if slots.remove(slot_ref).is_none() {
            return Err(p::Error("worker secret slot is not registered".into()));
        }
        Ok(())
    }

    /// Revoke every slot during logout, release replacement, or cleanup.
    pub fn revoke_all(&self) -> p::Result<()> {
        self.slots
            .write()
            .map_err(|_| p::Error("worker secret slot registry is unavailable".into()))?
            .clear();
        Ok(())
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
                let slots = self
                    .slots
                    .read()
                    .map_err(|_| p::Error("worker secret slot registry is unavailable".into()))?;
                let slot = slots
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
            || grant.action_ref != job.action_ref
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

/// Decode the closed stdin envelope inside an installed worker. The parser
/// exposes only the already-authorized path label and bytes; grant/slot
/// identifiers are intentionally discarded at this boundary.
#[derive(Debug)]
pub struct WorkerInputFrame {
    pub input_path: String,
    pub secret: bool,
    pub bytes: Zeroizing<Vec<u8>>,
}

pub fn decode_worker_input_frame(frame: &[u8]) -> p::Result<Vec<WorkerInputFrame>> {
    if frame.len() > (p::WORKER_INPUT_MAX_TOTAL_BYTES as usize + 64 * 1024)
        || !frame.starts_with(INPUT_FRAME_MAGIC)
    {
        return Err(p::Error("worker input frame is invalid".into()));
    }
    let mut cursor = INPUT_FRAME_MAGIC.len();
    let count = read_u32(frame, &mut cursor)? as usize;
    if count > p::WORKER_INPUT_MAX_GRANTS {
        return Err(p::Error("worker input frame has too many grants".into()));
    }
    let mut total = 0_u64;
    let mut decoded = Vec::with_capacity(count);
    let mut paths = BTreeSet::new();
    for _ in 0..count {
        let grant_len = read_u16(frame, &mut cursor)? as usize;
        let path_len = read_u16(frame, &mut cursor)? as usize;
        let secret = match *frame
            .get(cursor)
            .ok_or_else(|| p::Error("worker input frame is truncated".into()))?
        {
            0 => false,
            1 => true,
            _ => {
                return Err(p::Error(
                    "worker input frame secret marker is invalid".into(),
                ))
            }
        };
        cursor += 1;
        let bytes = read_u64(frame, &mut cursor)? as usize;
        if grant_len == 0 {
            return Err(p::Error("worker input grant reference is missing".into()));
        }
        let _grant = read_slice(frame, &mut cursor, grant_len)?;
        let path = std::str::from_utf8(read_slice(frame, &mut cursor, path_len)?)
            .map_err(|_| p::Error("worker input path is not UTF-8".into()))?;
        if !is_closed_input_path(path) {
            return Err(p::Error("worker input path is invalid".into()));
        }
        let path = path.to_owned();
        if !paths.insert(path.clone()) {
            return Err(p::Error("worker input path is duplicated".into()));
        }
        total = total
            .checked_add(bytes as u64)
            .filter(|size| *size <= p::WORKER_INPUT_MAX_TOTAL_BYTES)
            .ok_or_else(|| p::Error("worker input frame exceeds its byte budget".into()))?;
        let payload = read_slice(frame, &mut cursor, bytes)?.to_vec();
        decoded.push(WorkerInputFrame {
            input_path: path,
            secret,
            bytes: Zeroizing::new(payload),
        });
    }
    if cursor != frame.len() {
        return Err(p::Error("worker input frame has trailing bytes".into()));
    }
    Ok(decoded)
}

fn is_closed_input_path(path: &str) -> bool {
    if path.len() > 128 || path.chars().any(char::is_control) {
        return false;
    }
    matches!(
        path,
        "browser.type" | "computer.type" | "pty.stdin" | "app_api.body" | "notification.body"
    ) || path
        .strip_prefix("pty.environment/")
        .is_some_and(valid_environment_name)
}

fn valid_environment_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().enumerate().all(|(index, byte)| {
            (index == 0 && byte.is_ascii_alphabetic())
                || (index > 0 && (byte.is_ascii_alphanumeric() || byte == b'_'))
        })
}

fn read_slice<'a>(frame: &'a [u8], cursor: &mut usize, len: usize) -> p::Result<&'a [u8]> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| p::Error("worker input frame length overflowed".into()))?;
    let slice = frame
        .get(*cursor..end)
        .ok_or_else(|| p::Error("worker input frame is truncated".into()))?;
    *cursor = end;
    Ok(slice)
}

fn read_u16(frame: &[u8], cursor: &mut usize) -> p::Result<u16> {
    Ok(u16::from_le_bytes(
        read_slice(frame, cursor, 2)?
            .try_into()
            .map_err(|_| p::Error("worker input frame is truncated".into()))?,
    ))
}

fn read_u32(frame: &[u8], cursor: &mut usize) -> p::Result<u32> {
    Ok(u32::from_le_bytes(
        read_slice(frame, cursor, 4)?
            .try_into()
            .map_err(|_| p::Error("worker input frame is truncated".into()))?,
    ))
}

fn read_u64(frame: &[u8], cursor: &mut usize) -> p::Result<u64> {
    Ok(u64::from_le_bytes(
        read_slice(frame, cursor, 8)?
            .try_into()
            .map_err(|_| p::Error("worker input frame is truncated".into()))?,
    ))
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

    #[test]
    fn secret_slots_revoke_before_future_resolution() {
        let slot = WorkerSecretSlot {
            slot_ref: "secret-slot:test".into(),
            secret_ref: p::SecretRef("secret:test".into()),
            owner_ref: p::VerifiedPrincipal("owner:test".into()),
            workspace_scope: p::Scope("workspace:test".into()),
            input_path: "pty.environment/TOKEN".into(),
            expires_at: 100,
        };
        let resolver = ManagedWorkerInputResolver::new(
            Arc::new(crate::InMemoryContentResolver::default()),
            Arc::new(crate::RejectingSecretResolver),
            [slot],
        )
        .unwrap();
        resolver.revoke_slot("secret-slot:test").unwrap();
        assert!(resolver.revoke_slot("secret-slot:test").is_err());
        resolver.revoke_all().unwrap();
    }

    #[test]
    fn input_frame_decoder_rejects_trailing_or_oversized_data() {
        let material = WorkerInputMaterial::content(
            "worker-input-grant:test".into(),
            "pty.stdin".into(),
            b"hello".to_vec(),
        );
        let frame = encode_input_frame(&[material]).unwrap();
        assert_eq!(
            decode_worker_input_frame(&frame).unwrap()[0]
                .bytes
                .as_slice(),
            b"hello"
        );
        let mut trailing = frame.clone();
        trailing.push(0);
        assert!(decode_worker_input_frame(&trailing).is_err());

        let duplicate = encode_input_frame(&[
            WorkerInputMaterial::content(
                "worker-input-grant:a".into(),
                "pty.stdin".into(),
                b"a".to_vec(),
            ),
            WorkerInputMaterial::content(
                "worker-input-grant:b".into(),
                "pty.stdin".into(),
                b"b".to_vec(),
            ),
        ])
        .unwrap();
        assert!(decode_worker_input_frame(&duplicate).is_err());

        let invalid_path = encode_input_frame(&[WorkerInputMaterial::content(
            "worker-input-grant:c".into(),
            "pty.environment/1TOKEN".into(),
            b"x".to_vec(),
        )])
        .unwrap();
        assert!(decode_worker_input_frame(&invalid_path).is_err());
    }
}
