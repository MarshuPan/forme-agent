//! Context hydration for owner-uploaded, session-scoped workbench artifacts.

use std::collections::BTreeSet;
use std::sync::Arc;

use forme_context::ContextSlice;
use forme_protocol as p;
use forme_store::{ProtectedContentRecord, SqliteEventStore, StoreContentProtection};
use serde::{Deserialize, Serialize};

use crate::ProductionRuntime;

const MAX_ARTIFACT_BYTES: usize = p::ArtifactUploadCommand::MAX_CONTENT_BYTES;
const MAX_METADATA_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactMetadata {
    schema_version: p::SchemaVersion,
    session: p::SessionId,
    workspace: Option<p::WorkspaceRef>,
    receipt: p::ArtifactUploadReceipt,
}

fn scope(session: &p::SessionId) -> p::Scope {
    p::Scope(format!("workbench-artifact:{}", session.0))
}
fn metadata_object(id: &p::ContentRef) -> String {
    format!("workbench-artifact-metadata-v1:{}", id.0)
}
fn metadata_ref(id: &p::ContentRef) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(metadata_object(id).as_bytes()).0
    ))
}
fn subject(metadata: &ArtifactMetadata) -> p::Result<p::SchemaDigest> {
    p::canonical_digest(&(
        &metadata.session,
        &metadata.workspace,
        &metadata.receipt.file_name,
        &metadata.receipt.media_type,
        &metadata.receipt.content_digest,
    ))
}

/// The namespace binding prevents an uploaded JSON document from masquerading
/// as metadata, even when its bytes are identical to a real companion.
fn metadata(record: &ProtectedContentRecord, bytes: &[u8]) -> p::Result<Option<ArtifactMetadata>> {
    if bytes.len() > MAX_METADATA_BYTES {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_slice::<ArtifactMetadata>(bytes) else {
        return Ok(None);
    };
    if record.content_ref != metadata_ref(&value.receipt.content_ref) {
        return Ok(None);
    }
    p::ArtifactUploadCommand {
        schema_version: value.schema_version,
        command: p::IdempotencyKey("metadata:validation".into()),
        session: value.session.clone(),
        workspace: value.workspace.clone(),
        file_name: value.receipt.file_name.clone(),
        media_type: value.receipt.media_type.clone(),
        bytes_hex: "00".into(),
    }
    .validate()?;
    p::sha256_digest_bytes(&value.receipt.content_digest)?;
    if value.receipt.schema_version != p::SchemaVersion(1)
        || value.receipt.scope != scope(&value.session)
        || record.scope != value.receipt.scope
        || record.subject_digest != subject(&value)?
        || value.receipt.bytes == 0
        || value.receipt.bytes > MAX_ARTIFACT_BYTES as u64
    {
        return Err(p::Error("artifact metadata binding is invalid".into()));
    }
    Ok(Some(value))
}

fn companion(
    store: &SqliteEventStore,
    protection: &StoreContentProtection,
    owner: &p::VerifiedPrincipal,
    id: &p::ContentRef,
) -> p::Result<Option<ArtifactMetadata>> {
    let lifecycle = protection.lifecycle_service(store.clone());
    let Some(record) = lifecycle.protected_content(owner, &metadata_ref(id))? else {
        return Ok(None);
    };
    if record.envelope.ciphertext.len() > MAX_METADATA_BYTES + 16 {
        return Err(p::Error(
            "artifact metadata exceeds its bounded size".into(),
        ));
    }
    let bytes = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    metadata(&record, &bytes)?
        .filter(|value| value.receipt.content_ref == *id)
        .map(Some)
        .ok_or_else(|| p::Error("artifact metadata is corrupt".into()))
}

fn decrypt(
    protection: &StoreContentProtection,
    record: &ProtectedContentRecord,
) -> p::Result<Vec<u8>> {
    if record.envelope.ciphertext.len() > MAX_ARTIFACT_BYTES + 16 {
        return Err(p::Error("artifact exceeds its bounded size".into()));
    }
    protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)
}

fn check_original(
    store: &SqliteEventStore,
    protection: &StoreContentProtection,
    owner: &p::VerifiedPrincipal,
    record: &ProtectedContentRecord,
    bytes: &[u8],
) -> p::Result<bool> {
    if metadata(record, bytes)?.is_some() {
        return Ok(false);
    }
    if let Some(value) = companion(store, protection, owner, &record.content_ref)? {
        if value.receipt.scope != record.scope
            || subject(&value)? != record.subject_digest
            || value.receipt.bytes != bytes.len() as u64
            || value.receipt.content_digest != p::sha256_content_digest(bytes)
        {
            return Err(p::Error("artifact bytes do not match their receipt".into()));
        }
    }
    Ok(true)
}

impl ProductionRuntime {
    pub fn upload_workbench_artifact(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::ArtifactUploadCommand,
    ) -> p::Result<p::ArtifactUploadReceipt> {
        let _available = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        command.validate()?;
        if command
            .workspace
            .as_ref()
            .is_some_and(|w| *w != self.config().workspace)
        {
            return Err(p::Error(
                "artifact workspace is outside the configured brain".into(),
            ));
        }
        let lock = self
            .harness()
            .session_lock(&p::SessionId("maintenance:workbench-artifacts".into()))?;
        let _guard = lock
            .lock()
            .map_err(|_| p::Error("artifact queue unavailable".into()))?;
        let bytes = decode_hex(&command.bytes_hex)?;
        let scope = scope(&command.session);
        let protection = self
            .services()
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("artifact protection is unavailable".into()))?;
        let lifecycle = protection.lifecycle_service(self.services().store.as_ref().clone());
        let object = format!("artifact:{}", command.command.0);
        let id = p::ContentRef(format!(
            "content:{}",
            p::sha256_content_digest(object.as_bytes()).0
        ));
        let value = ArtifactMetadata {
            schema_version: p::SchemaVersion(1),
            session: command.session,
            workspace: command.workspace,
            receipt: p::ArtifactUploadReceipt {
                schema_version: p::SchemaVersion(1),
                content_ref: id.clone(),
                content_digest: p::sha256_content_digest(&bytes),
                scope: scope.clone(),
                bytes: bytes.len() as u64,
                file_name: command.file_name,
                media_type: command.media_type,
            },
        };
        let subject = subject(&value)?;
        if let Some(previous) = lifecycle.protected_content(owner, &id)? {
            if previous.scope != scope
                || previous.subject_digest != subject
                || decrypt(&protection, &previous)? != bytes
            {
                return Err(p::Error("artifact command identity conflict".into()));
            }
        } else {
            let (_, record) = protection.seal_record(&bytes, &scope, &object, &subject)?;
            lifecycle.put_protected_content(owner, record)?;
        }
        if let Some(previous) = companion(self.services().store.as_ref(), &protection, owner, &id)?
        {
            if previous != value {
                return Err(p::Error("artifact metadata identity conflict".into()));
            }
        } else {
            let encoded = serde_json::to_vec(&value)
                .map_err(|_| p::Error("artifact metadata encoding failed".into()))?;
            if encoded.len() > MAX_METADATA_BYTES {
                return Err(p::Error(
                    "artifact metadata exceeds its bounded size".into(),
                ));
            }
            let (_, record) =
                protection.seal_record(&encoded, &scope, &metadata_object(&id), &subject)?;
            lifecycle.put_protected_content(owner, record)?;
        }
        Ok(value.receipt)
    }

    pub fn read_workbench_artifact(
        &self,
        owner: &p::VerifiedPrincipal,
        id: &p::ContentRef,
        session: &p::SessionId,
    ) -> p::Result<Vec<u8>> {
        let _available = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        let protection = self
            .services()
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("artifact protection is unavailable".into()))?;
        let record = protection
            .lifecycle_service(self.services().store.as_ref().clone())
            .protected_content(owner, id)?
            .ok_or_else(|| p::Error("artifact is absent or erased".into()))?;
        if record.scope != scope(session) {
            return Err(p::Error("artifact is outside the requested session".into()));
        }
        let bytes = decrypt(&protection, &record)?;
        if !check_original(
            self.services().store.as_ref(),
            &protection,
            owner,
            &record,
            &bytes,
        )? {
            return Err(p::Error(
                "artifact metadata is not a downloadable object".into(),
            ));
        }
        Ok(bytes)
    }

    pub fn query_workbench_artifacts(
        &self,
        owner: &p::VerifiedPrincipal,
        query: p::ArtifactCatalogQuery,
    ) -> p::Result<p::ArtifactCatalogView> {
        let _available = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        query.validate()?;
        let lock = self
            .harness()
            .session_lock(&p::SessionId("maintenance:workbench-artifacts".into()))?;
        let _guard = lock
            .lock()
            .map_err(|_| p::Error("artifact queue unavailable".into()))?;
        let services = self.services();
        let store = services.store.as_ref();
        let protection = store
            .content_protection()?
            .ok_or_else(|| p::Error("artifact protection is unavailable".into()))?;
        let lifecycle = protection.lifecycle_service(store.clone());
        let scope = scope(&query.session);
        let refs = store.protected_content_refs(owner, &scope)?;
        if refs.len() > 20_000 {
            return Err(p::Error("artifact catalog exceeds its bounded size".into()));
        }
        let ids: BTreeSet<_> = refs.iter().map(|id| &id.0).collect();
        let metadata_ids: BTreeSet<_> = refs
            .iter()
            .map(metadata_ref)
            .filter(|id| ids.contains(&id.0))
            .map(|id| id.0)
            .collect();
        let mut entries = Vec::new();
        let mut legacy_bytes = 0;
        for id in refs.iter().filter(|id| !metadata_ids.contains(&id.0)) {
            let entry = if let Some(value) = companion(store, &protection, owner, id)? {
                if value.session != query.session {
                    return Err(p::Error("artifact catalog session mismatch".into()));
                }
                p::ArtifactCatalogEntry {
                    schema_version: p::SchemaVersion(1),
                    content_ref: id.clone(),
                    content_digest: value.receipt.content_digest,
                    scope: scope.clone(),
                    bytes: value.receipt.bytes,
                    file_name: Some(value.receipt.file_name),
                    media_type: Some(value.receipt.media_type),
                }
            } else {
                let record = lifecycle
                    .protected_content(owner, id)?
                    .ok_or_else(|| p::Error("artifact catalog changed".into()))?;
                let bytes = decrypt(&protection, &record)?;
                if metadata(&record, &bytes)?.is_some() {
                    return Err(p::Error("artifact metadata source is missing".into()));
                }
                legacy_bytes += bytes.len();
                if legacy_bytes > 32 * 1024 * 1024 {
                    return Err(p::Error(
                        "legacy artifact scan exceeds its bounded size".into(),
                    ));
                }
                p::ArtifactCatalogEntry {
                    schema_version: p::SchemaVersion(1),
                    content_ref: id.clone(),
                    content_digest: p::sha256_content_digest(&bytes),
                    scope: scope.clone(),
                    bytes: bytes.len() as u64,
                    file_name: None,
                    media_type: None,
                }
            };
            entries.push(entry);
        }
        let digest = p::canonical_digest(&(&query.session, &entries))?;
        if query
            .page
            .snapshot
            .as_ref()
            .is_some_and(|expected| *expected != digest)
        {
            return Err(p::Error("artifact catalog snapshot conflict".into()));
        }
        let total = entries.len();
        let end = query
            .page
            .offset
            .saturating_add(query.page.limit)
            .min(total);
        let artifacts = entries
            .into_iter()
            .skip(query.page.offset)
            .take(query.page.limit)
            .collect();
        Ok(p::ArtifactCatalogView {
            schema_version: p::SchemaVersion(1),
            session: query.session,
            digest,
            total,
            next_offset: (end < total).then_some(end),
            artifacts,
        })
    }
}

fn decode_hex(value: &str) -> p::Result<Vec<u8>> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| p::Error("artifact bytes are not valid hex".into()))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| p::Error("artifact bytes are not valid hex".into()))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

pub(crate) fn workbench_artifact_slices(
    store: &Arc<SqliteEventStore>,
    owner: &p::OwnerPrincipalRef,
    session: &p::SessionId,
) -> p::Result<Vec<ContextSlice>> {
    let Some(protection) = store.content_protection()? else {
        return Ok(Vec::new());
    };
    let principal = p::VerifiedPrincipal(owner.0.clone());
    let scope = p::Scope(format!("workbench-artifact:{}", session.0));
    let lifecycle = protection.lifecycle_service(store.as_ref().clone());
    let mut slices = Vec::new();
    for content_ref in store.protected_content_refs(&principal, &scope)? {
        let Some(record) = lifecycle.protected_content(&principal, &content_ref)? else {
            continue;
        };
        let bytes = decrypt(&protection, &record)?;
        if !check_original(store.as_ref(), &protection, &principal, &record, &bytes)? {
            continue;
        }
        let text = String::from_utf8(bytes).unwrap_or_else(|_| {
            "binary content is retained as an authorized artifact; a compatible driver is required to inspect it".into()
        });
        slices.push(ContextSlice {
            schema_version: p::SchemaVersion(1),
            id: p::ContextSliceRef(format!("attachment:{}", content_ref.0)),
            content: format!(
                "owner-uploaded session artifact {}:\n{}",
                content_ref.0, text
            ),
            provenance: p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        });
    }
    Ok(slices)
}
