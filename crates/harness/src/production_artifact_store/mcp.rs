//! Lifecycle-aware encrypted material for one verified discovery original.
use super::*;
use forme_execution::installed::{
    managed_mcp_material_object, validate_managed_mcp_material, ManagedMcpDiscoveryConsumer,
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryMaterial {
    schema_version: p::SchemaVersion,
    content_ref: p::ContentRef,
    request: p::WorkerDispatchRequest,
    observation: p::WorkerObservation,
    manifest: p::WorkerResultManifest,
}
fn metadata_object(id: &p::ContentRef) -> String {
    format!("managed-mcp-discovery-metadata-v1:{}", id.0)
}
fn metadata_ref(id: &p::ContentRef) -> p::ContentRef {
    content_ref(&metadata_object(id))
}
fn content_ref(object: &str) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(object.as_bytes()).0
    ))
}
const MAX_METADATA_BYTES: usize = 32 * 1024;

impl ManagedMcpDiscoveryConsumer for ProtectedRuntimeArtifacts {
    fn persist_discovery(
        &self,
        request: &p::WorkerDispatchRequest,
        observation: &p::WorkerObservation,
        manifest: &p::WorkerResultManifest,
        bytes: &[u8],
    ) -> p::Result<ArtifactRecord> {
        validate_managed_mcp_material(request, observation, manifest, bytes)?;
        if request.job.owner_ref != self.owner {
            return Err(invalid());
        }
        let object = managed_mcp_material_object(request, manifest)?;
        let id = content_ref(&object);
        let scope = &request.job.workspace_scope;
        let subject = p::sha256_content_digest(scope.0.as_bytes());
        let metadata = DiscoveryMaterial {
            schema_version: p::SchemaVersion(1),
            content_ref: id.clone(),
            request: request.clone(),
            observation: observation.clone(),
            manifest: manifest.clone(),
        };
        let encoded = serde_json::to_vec(&metadata).map_err(|_| invalid())?;
        if encoded.len() > MAX_METADATA_BYTES {
            return Err(invalid());
        }
        let body = self.persist_discovery_record(&object, scope, &subject, bytes)?;
        // The observation timestamp may advance while retrieving the same
        // original receipt. Compare immutable completion identity, not read time.
        if let Some(original) = read_metadata(&self.store, &self.protection, &id)? {
            let mut same = original.clone();
            same.observation.observed_at = metadata.observation.observed_at;
            if same != metadata {
                return Err(invalid());
            }
        } else {
            self.persist_discovery_record(&metadata_object(&id), scope, &subject, &encoded)?;
        }
        register_material(&self.store, &self.protection, &self.owner, &body, bytes)?;
        let (_, original) =
            read_material_original(&self.store, &id, &self.owner)?.ok_or_else(invalid)?;
        if original != bytes {
            return Err(invalid());
        }
        Ok(ArtifactRecord {
            schema_version: p::SchemaVersion(1),
            content_ref: id,
            digest: p::sha256_content_digest(bytes),
            bytes: bytes.len() as u64,
        })
    }
}
impl ProtectedRuntimeArtifacts {
    fn persist_discovery_record(
        &self,
        object: &str,
        scope: &p::Scope,
        subject: &p::SchemaDigest,
        bytes: &[u8],
    ) -> p::Result<ProtectedContentRecord> {
        if let Some(record) = self
            .store
            .protected_content_for_current_owner(&content_ref(object))?
        {
            if record.scope != *scope
                || record.subject_digest != *subject
                || record.envelope.ciphertext.len() != bytes.len() + 16
                || self
                    .protection
                    .content_service()
                    .decrypt(&record.envelope.binding(), &record.envelope)?
                    != bytes
            {
                return Err(invalid());
            }
            return Ok(record);
        }
        let (_, record) = self.protection.seal_record(bytes, scope, object, subject)?;
        self.protection
            .lifecycle_service(self.store.clone())
            .put_protected_content(&self.owner, record.clone())?;
        Ok(record)
    }
}
fn read_metadata(
    store: &SqliteEventStore,
    protection: &StoreContentProtection,
    id: &p::ContentRef,
) -> p::Result<Option<DiscoveryMaterial>> {
    let Some(record) = store.protected_content_for_current_owner(&metadata_ref(id))? else {
        return Ok(None);
    };
    if record.envelope.ciphertext.len() > MAX_METADATA_BYTES + 16 {
        return Err(invalid());
    }
    let encoded = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    let material: DiscoveryMaterial = serde_json::from_slice(&encoded).map_err(|_| invalid())?;
    let expected = content_ref(&managed_mcp_material_object(
        &material.request,
        &material.manifest,
    )?);
    if material.schema_version != p::SchemaVersion(1)
        || material.content_ref != *id
        || expected != *id
        || record.scope != material.request.job.workspace_scope
        || record.subject_digest != p::sha256_content_digest(record.scope.0.as_bytes())
        || record.owner_digest
            != p::sha256_content_digest(material.request.job.owner_ref.0.as_bytes())
    {
        return Err(invalid());
    }
    Ok(Some(material))
}
fn read_material_original(
    store: &SqliteEventStore,
    id: &p::ContentRef,
    owner: &p::VerifiedPrincipal,
) -> p::Result<Option<(DiscoveryMaterial, Vec<u8>)>> {
    let protection = store.content_protection()?.ok_or_else(invalid)?;
    let Some(material) = read_metadata(store, &protection, id)? else {
        return Ok(None);
    };
    if material.request.job.owner_ref != *owner {
        return Err(invalid());
    }
    let Some((receipt, Some(body))) =
        registered_material(store, id, &material.request.job.workspace_scope)?
    else {
        return Ok(None);
    };
    if receipt.bytes != material.manifest.bytes
        || Some(&receipt.content_digest) != material.manifest.content_digest.as_ref()
        || body.envelope.ciphertext.len() as u64 != receipt.bytes.saturating_add(16)
        || body.owner_digest != p::sha256_content_digest(owner.0.as_bytes())
    {
        return Err(invalid());
    }
    let bytes = protection
        .content_service()
        .decrypt(&body.envelope.binding(), &body.envelope)?;
    validate_managed_mcp_material(
        &material.request,
        &material.observation,
        &material.manifest,
        &bytes,
    )?;
    Ok(Some((material, bytes)))
}
fn invalid() -> p::Error {
    p::Error("protected MCP discovery material is unavailable or inconsistent".into())
}

pub(crate) mod source;
#[cfg(test)]
mod tests;
