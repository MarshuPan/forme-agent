//! Encrypted execution artifacts and their server-authored material receipts.
use forme_execution::{ArtifactRecord, ArtifactStore, FileArtifactStore};
use forme_protocol as p;
use forme_store::{ProtectedContentRecord, SqliteEventStore, StoreContentProtection};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct ProtectedRuntimeArtifacts {
    pub(super) inner: FileArtifactStore,
    pub(super) protection: StoreContentProtection,
    pub(super) store: SqliteEventStore,
    pub(super) owner: p::VerifiedPrincipal,
}

impl ArtifactStore for ProtectedRuntimeArtifacts {
    fn write(&self, scope: &p::Scope, extension: &str, bytes: &[u8]) -> p::Result<ArtifactRecord> {
        let digest = p::sha256_content_digest(bytes);
        let safe_scope = p::Scope(format!(
            "artifact:{}",
            p::sha256_content_digest(scope.0.as_bytes()).0
        ));
        static ARTIFACT_NONCE: AtomicU64 = AtomicU64::new(1);
        let (sealed, record) = self.protection.seal_record(
            bytes,
            scope,
            &format!(
                "artifact:{}:{extension}:{}:{}",
                digest.0,
                crate::now_ms(),
                ARTIFACT_NONCE.fetch_add(1, Ordering::Relaxed)
            ),
            &p::sha256_content_digest(scope.0.as_bytes()),
        )?;
        let file = self.inner.write(&safe_scope, "sealed", &sealed)?;
        self.protection
            .lifecycle_service(self.store.clone())
            .put_protected_content(&self.owner, record.clone())?;
        register_material(&self.store, &self.protection, &self.owner, &record, bytes)?;
        Ok(ArtifactRecord {
            content_ref: record.content_ref,
            ..file
        })
    }
}

const MAX_RECEIPT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialReceipt {
    schema_version: p::SchemaVersion,
    content_ref: p::ContentRef,
    scope: p::Scope,
    subject: p::SchemaDigest,
    content_digest: p::SchemaDigest,
    bytes: u64,
}

fn receipt_object(id: &p::ContentRef) -> String {
    format!("runtime-artifact-material-v1:{}", id.0)
}
pub(super) fn receipt_ref(id: &p::ContentRef) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(receipt_object(id).as_bytes()).0
    ))
}

/// Called only by actual artifact writers, never by a browser query or model.
pub(super) fn register_material(
    store: &SqliteEventStore,
    protection: &StoreContentProtection,
    owner: &p::VerifiedPrincipal,
    original: &ProtectedContentRecord,
    bytes: &[u8],
) -> p::Result<()> {
    original.validate()?;
    let value = MaterialReceipt {
        schema_version: p::SchemaVersion(1),
        content_ref: original.content_ref.clone(),
        scope: original.scope.clone(),
        subject: original.subject_digest.clone(),
        content_digest: p::sha256_content_digest(bytes),
        bytes: bytes.len() as u64,
    };
    if let Some(record) =
        store.protected_content_for_current_owner(&receipt_ref(&value.content_ref))?
    {
        if decode_receipt(protection, &record)? != value {
            return Err(p::Error("artifact material receipt is inconsistent".into()));
        }
        return Ok(());
    }
    let encoded = serde_json::to_vec(&value)
        .map_err(|_| p::Error("artifact material receipt encoding failed".into()))?;
    if encoded.len() > MAX_RECEIPT_BYTES {
        return Err(p::Error(
            "artifact material receipt exceeds its size bound".into(),
        ));
    }
    let (_, record) = protection.seal_record(
        &encoded,
        &value.scope,
        &receipt_object(&value.content_ref),
        &value.subject,
    )?;
    protection
        .lifecycle_service(store.clone())
        .put_protected_content(owner, record)
}

fn decode_receipt(
    protection: &StoreContentProtection,
    record: &ProtectedContentRecord,
) -> p::Result<MaterialReceipt> {
    if record.envelope.ciphertext.len() > MAX_RECEIPT_BYTES + 16 {
        return Err(p::Error(
            "artifact material receipt exceeds its size bound".into(),
        ));
    }
    let bytes = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    let value: MaterialReceipt = serde_json::from_slice(&bytes)
        .map_err(|_| p::Error("artifact material receipt is invalid".into()))?;
    validate_receipt(record, &value)?;
    Ok(value)
}

fn validate_receipt(record: &ProtectedContentRecord, value: &MaterialReceipt) -> p::Result<()> {
    if value.schema_version != p::SchemaVersion(1)
        || record.content_ref != receipt_ref(&value.content_ref)
        || value.scope != record.scope
        || value.subject != record.subject_digest
    {
        return Err(p::Error(
            "artifact material receipt binding is invalid".into(),
        ));
    }
    p::sha256_digest_bytes(&value.content_digest)?;
    Ok(())
}

/// Prevent this receipt from entering legacy artifact downloads or context.
/// Uploaded JSON with the same shape has a different immutable object address.
pub(super) fn is_material_receipt(
    record: &ProtectedContentRecord,
    bytes: &[u8],
) -> p::Result<bool> {
    let Ok(value) = serde_json::from_slice::<MaterialReceipt>(bytes) else {
        return Ok(false);
    };
    if record.content_ref != receipt_ref(&value.content_ref) {
        return Ok(false);
    }
    validate_receipt(record, &value)?;
    Ok(true)
}

pub(super) struct MaterialBody {
    pub(super) state: p::WorkbenchMaterialState,
    pub(super) bytes: Option<u64>,
    pub(super) digest: Option<p::SchemaDigest>,
    pub(super) text: Option<String>,
}

/// The caller first binds this server-selected ref to an authorized memory row.
pub(super) fn read_material(
    store: &SqliteEventStore,
    id: &p::ContentRef,
    scope: &p::Scope,
) -> p::Result<MaterialBody> {
    let mut body = MaterialBody {
        state: p::WorkbenchMaterialState::ReferenceOnly,
        bytes: None,
        digest: None,
        text: None,
    };
    let Some(metadata) = store.protected_content_for_current_owner(&receipt_ref(id))? else {
        return Ok(body);
    };
    let protection = store
        .content_protection()?
        .ok_or_else(|| p::Error("material content protection is unavailable".into()))?;
    let value = decode_receipt(&protection, &metadata)?;
    if value.content_ref != *id || value.scope != *scope {
        return Err(p::Error("memory material scope binding is invalid".into()));
    }
    let Some(record) = store.protected_content_for_current_owner(id)? else {
        body.state = p::WorkbenchMaterialState::MissingOrErased;
        return Ok(body);
    };
    if record.scope != value.scope || record.subject_digest != value.subject {
        return Err(p::Error(
            "memory material content binding is invalid".into(),
        ));
    }
    body.bytes = Some(value.bytes);
    body.digest = Some(value.content_digest.clone());
    if value.bytes > p::WORKBENCH_MATERIAL_MAX_BYTES as u64
        || record.envelope.ciphertext.len() > p::WORKBENCH_MATERIAL_MAX_BYTES + 16
    {
        body.state = p::WorkbenchMaterialState::TooLarge;
        return Ok(body);
    }
    let bytes = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    if value.bytes != bytes.len() as u64 || value.content_digest != p::sha256_content_digest(&bytes)
    {
        return Err(p::Error("memory material content digest is invalid".into()));
    }
    match String::from_utf8(bytes) {
        Ok(text) => {
            body.state = p::WorkbenchMaterialState::Available;
            body.text = Some(text);
        }
        Err(_) => body.state = p::WorkbenchMaterialState::NonText,
    }
    Ok(body)
}
