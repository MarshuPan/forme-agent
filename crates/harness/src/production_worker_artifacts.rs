//! Idempotent protected material registration for a verified worker result.
use super::*;

#[cfg(test)]
#[path = "production_worker_artifact_tests.rs"]
mod tests;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkerMaterial {
    pub(super) schema_version: p::SchemaVersion,
    pub(super) content_ref: p::ContentRef,
    pub(super) binding: p::WorkerArtifactBinding,
}
fn metadata_object(id: &p::ContentRef) -> String {
    format!("worker-artifact-metadata-v2:{}", id.0)
}
pub(super) fn worker_metadata_ref(id: &p::ContentRef) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(metadata_object(id).as_bytes()).0
    ))
}
impl ProtectedRuntimeArtifacts {
    pub(super) fn persist_worker_output(
        &self,
        binding: &p::WorkerArtifactBinding,
        bytes: &[u8],
    ) -> p::Result<ArtifactRecord> {
        binding.validate()?;
        if binding.manifest.binding.owner_ref != self.owner
            || bytes.len() as u64 != binding.manifest.bytes
            || Some(p::sha256_content_digest(bytes)) != binding.manifest.content_digest
        {
            return Err(invalid_worker());
        }
        forme_execution::validate_desktop_png(
            bytes,
            binding.descriptor.width,
            binding.descriptor.height,
        )?;
        let scope = &binding.manifest.binding.workspace_scope;
        let subject = p::sha256_content_digest(scope.0.as_bytes());
        let object = binding.object_id()?;
        let id = p::ContentRef(format!(
            "content:{}",
            p::sha256_content_digest(object.as_bytes()).0
        ));
        let lifecycle = self.protection.lifecycle_service(self.store.clone());
        let record = if let Some(record) = self.store.protected_content_for_current_owner(&id)? {
            if record.scope != *scope
                || record.subject_digest != subject
                || self
                    .protection
                    .content_service()
                    .decrypt(&record.envelope.binding(), &record.envelope)?
                    != bytes
            {
                return Err(invalid_worker());
            }
            record
        } else {
            // This sealed record is itself the durable content body in the
            // existing lifecycle catalog; no untracked plaintext copy exists.
            let (_, record) = self
                .protection
                .seal_record(bytes, scope, &object, &subject)?;
            lifecycle.put_protected_content(&self.owner, record.clone())?;
            record
        };
        let metadata = WorkerMaterial {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            content_ref: id.clone(),
            binding: binding.clone(),
        };
        if let Some(existing) = read_worker_metadata(&self.store, &self.protection, &id)? {
            if existing != metadata {
                return Err(invalid_worker());
            }
        } else {
            let encoded = serde_json::to_vec(&metadata).map_err(|_| invalid_worker())?;
            if encoded.len() > 16 * 1024 {
                return Err(invalid_worker());
            }
            let (_, metadata_record) =
                self.protection
                    .seal_record(&encoded, scope, &metadata_object(&id), &subject)?;
            lifecycle.put_protected_content(&self.owner, metadata_record)?;
        }
        register_material(&self.store, &self.protection, &self.owner, &record, bytes)?;
        // Re-read all durable components; a cached body is not proof that a
        // concurrent lifecycle fence left the original material available.
        let final_metadata =
            read_worker_metadata(&self.store, &self.protection, &id)?.ok_or_else(invalid_worker)?;
        let (receipt, body) =
            registered_material(&self.store, &id, scope)?.ok_or_else(invalid_worker)?;
        let body = body.ok_or_else(invalid_worker)?;
        if final_metadata != metadata
            || receipt.content_digest != p::sha256_content_digest(bytes)
            || receipt.bytes != bytes.len() as u64
            || body != record
        {
            return Err(invalid_worker());
        }
        self.protection
            .content_service()
            .decrypt(&body.envelope.binding(), &body.envelope)?;
        Ok(ArtifactRecord {
            schema_version: p::SchemaVersion(1),
            content_ref: id,
            digest: p::sha256_content_digest(bytes),
            bytes: bytes.len() as u64,
        })
    }
}
pub(super) fn read_worker_metadata(
    store: &SqliteEventStore,
    protection: &StoreContentProtection,
    id: &p::ContentRef,
) -> p::Result<Option<WorkerMaterial>> {
    let Some(record) = store.protected_content_for_current_owner(&worker_metadata_ref(id))? else {
        return Ok(None);
    };
    if record.envelope.ciphertext.len() > 16 * 1024 + 16 {
        return Err(invalid_worker());
    }
    let bytes = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    let value: WorkerMaterial = serde_json::from_slice(&bytes).map_err(|_| invalid_worker())?;
    value.binding.validate()?;
    let expected = p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(value.binding.object_id()?.as_bytes()).0
    ));
    if value.schema_version != p::DESKTOP_OUTPUT_SCHEMA_VERSION
        || value.content_ref != *id
        || expected != *id
        || record.owner_digest
            != p::sha256_content_digest(value.binding.manifest.binding.owner_ref.0.as_bytes())
        || record.scope != value.binding.manifest.binding.workspace_scope
        || record.subject_digest != p::sha256_content_digest(record.scope.0.as_bytes())
    {
        return Err(invalid_worker());
    }
    Ok(Some(value))
}
fn invalid_worker() -> p::Error {
    p::Error("worker artifact material is inconsistent".into())
}
