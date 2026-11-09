//! Closed metadata for bounded desktop screenshot exchange; pixel bytes are separate.
use crate as p;
use serde::{Deserialize, Serialize};
#[path = "desktop_output_binding.rs"]
mod binding;
pub use binding::*;
use binding::{invalid, validate_dimensions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOutputHeader {
    pub schema_version: p::SchemaVersion,
    pub binding: DesktopExchangeBinding,
    pub transfer_ref: String,
    pub media_type: String,
    pub bytes: u64,
    pub content_digest: p::SchemaDigest,
    pub width: u32,
    pub height: u32,
}
impl DesktopOutputHeader {
    pub fn validate_for(&self, transfer: &DesktopOutputTransfer) -> p::Result<()> {
        transfer.validate()?;
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION
            || self.binding != transfer.binding
            || self.transfer_ref != transfer.transfer_ref
            || self.media_type != "image/png"
            || self.bytes == 0
            || self.bytes > transfer.output_budget_bytes
            || self.width != transfer.width
            || self.height != transfer.height
            || serde_json::to_vec(self).map_err(|_| invalid())?.len()
                > DESKTOP_OUTPUT_MAX_HEADER_BYTES
        {
            return Err(invalid());
        }
        p::validate_product_digest(&self.content_digest)
    }
    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme.desktop.output.header.v2", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOutputChunkHeader {
    pub schema_version: p::SchemaVersion,
    pub binding_digest: p::SchemaDigest,
    pub transfer_ref: String,
    pub offset: u64,
    pub bytes: u32,
    pub chunk_digest: p::SchemaDigest,
    pub next_offset: u64,
    pub finished: bool,
}
impl DesktopOutputChunkHeader {
    pub fn validate_for(
        &self,
        header: &DesktopOutputHeader,
        offset: u64,
        bytes: &[u8],
    ) -> p::Result<()> {
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION
            || self.binding_digest != header.binding.digest()?
            || self.transfer_ref != header.transfer_ref
            || self.offset != offset
            || self.bytes == 0
            || self.bytes > DESKTOP_OUTPUT_CHUNK_BYTES
            || u64::from(self.bytes) != bytes.len() as u64
            || self.next_offset
                != self
                    .offset
                    .checked_add(u64::from(self.bytes))
                    .ok_or_else(invalid)?
            || self.next_offset > header.bytes
            || self.finished != (self.next_offset == header.bytes)
            || self.chunk_digest != p::sha256_content_digest(bytes)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOutputFinish {
    pub schema_version: p::SchemaVersion,
    pub binding_digest: p::SchemaDigest,
    pub transfer_ref: String,
    pub header_digest: p::SchemaDigest,
    pub bytes: u64,
    pub content_digest: p::SchemaDigest,
    pub receipt: p::DesktopActionReceipt,
}
impl DesktopOutputFinish {
    pub fn validate_for(
        &self,
        header: &DesktopOutputHeader,
        request: &p::DesktopActionRequest,
    ) -> p::Result<()> {
        header.binding.validate_request(request)?;
        self.receipt.validate_for(request)?;
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION
            || self.binding_digest != header.binding.digest()?
            || self.transfer_ref != header.transfer_ref
            || self.header_digest != header.digest()?
            || self.bytes != header.bytes
            || self.content_digest != header.content_digest
            || self.receipt.outcome != p::DesktopActionOutcome::Committed
            || self.receipt.result_ref.as_deref() != Some(header.transfer_ref.as_str())
            || self.receipt.observed_at < request.binding.issued_at
            || self.receipt.observed_at > request.deadline
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopOutputDescriptor {
    pub schema_version: p::SchemaVersion,
    pub binding: DesktopExchangeBinding,
    pub manifest_digest: p::SchemaDigest,
    pub content_digest: p::SchemaDigest,
    pub bytes: u64,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}
impl DesktopOutputDescriptor {
    pub fn validate_for(&self, manifest: &p::WorkerResultManifest) -> p::Result<()> {
        manifest.validate()?;
        self.binding.validate_result(&manifest.binding)?;
        validate_dimensions(self.width, self.height)?;
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION
            || self.manifest_digest != manifest.manifest_digest
            || self.media_type != "image/png"
            || self.bytes == 0
            || self.bytes != manifest.bytes
            || Some(&self.content_digest) != manifest.content_digest.as_ref()
            || manifest.status != p::WorkerResultStatus::Available
            || manifest.encoding != p::WorkerResultEncoding::Binary
            || manifest.outcome != Some(p::WorkerOutcome::Completed)
            || manifest.binding.receipt_ref.is_none()
            || manifest.produced_at > self.binding.deadline
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme.desktop.output.descriptor.v2", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerArtifactBinding {
    pub schema_version: p::SchemaVersion,
    pub manifest: p::WorkerResultManifest,
    pub descriptor: DesktopOutputDescriptor,
}
impl WorkerArtifactBinding {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != DESKTOP_OUTPUT_SCHEMA_VERSION {
            return Err(invalid());
        }
        self.descriptor.validate_for(&self.manifest)
    }
    pub fn object_id(&self) -> p::Result<String> {
        self.validate()?;
        Ok(format!(
            "worker-artifact:{}",
            p::canonical_digest(&("forme.runtime.worker-artifact.v2", self))?.0
        ))
    }
}
