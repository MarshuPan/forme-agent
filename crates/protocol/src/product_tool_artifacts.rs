//! Safe tool output metadata and bounded reads; upload schema 1 stays separate.
use crate::*;
use serde::{Deserialize, Serialize};

pub const TOOL_ARTIFACT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);
pub const TOOL_ARTIFACT_CHUNK_BYTES: u32 = 262_144;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactCatalogQuery {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub run: RunId,
    pub page: DataPageQuery,
}
impl ToolArtifactCatalogQuery {
    pub fn validate(&self) -> Result<()> {
        identity(self.schema_version, &self.session, &self.run)?;
        self.page.validate()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactEntry {
    pub schema_version: SchemaVersion,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub action: ActionId,
    pub scope: Scope,
    pub bytes: u64,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}
impl ToolArtifactEntry {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != TOOL_ARTIFACT_SCHEMA_VERSION
            || self.media_type != "image/png"
            || self.bytes == 0
            || self.bytes > DESKTOP_OUTPUT_MAX_BYTES
            || self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > DESKTOP_OUTPUT_MAX_PIXELS
        {
            return Err(invalid());
        }
        content_id(&self.content_ref)?;
        sha256_digest_bytes(&self.content_digest)?;
        validate_product_id(&self.action.0)?;
        validate_product_id(&self.scope.0)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactCatalogView {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub run: RunId,
    pub digest: SchemaDigest,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub artifacts: Vec<ToolArtifactEntry>,
}
impl ToolArtifactCatalogView {
    pub fn validate_for(&self, query: &ToolArtifactCatalogQuery) -> Result<()> {
        query.validate()?;
        identity(self.schema_version, &self.session, &self.run)?;
        let end = query.page.offset.saturating_add(self.artifacts.len());
        if self.session != query.session
            || self.run != query.run
            || self.total > 20_000
            || self.artifacts.len()
                != self
                    .total
                    .saturating_sub(query.page.offset)
                    .min(query.page.limit)
            || self.next_offset != (end < self.total).then_some(end)
            || query
                .page
                .snapshot
                .as_ref()
                .is_some_and(|d| d != &self.digest)
        {
            return Err(invalid());
        }
        sha256_digest_bytes(&self.digest)?;
        let mut refs = std::collections::BTreeSet::new();
        for entry in &self.artifacts {
            entry.validate()?;
            if !refs.insert(&entry.content_ref) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactRead {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub run: RunId,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub offset: u64,
    pub max_bytes: u32,
}
impl ToolArtifactRead {
    pub fn validate(&self) -> Result<()> {
        identity(self.schema_version, &self.session, &self.run)?;
        content_id(&self.content_ref)?;
        sha256_digest_bytes(&self.content_digest)?;
        if self.offset >= DESKTOP_OUTPUT_MAX_BYTES
            || self.max_bytes == 0
            || self.max_bytes > TOOL_ARTIFACT_CHUNK_BYTES
        {
            return Err(invalid());
        }
        Ok(())
    }
}
// Raw body is intentionally not Debug. Only the authenticated internal bridge
// serializes these bounded bytes; browser HTTP responses carry raw PNG chunks.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolArtifactChunk {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub run: RunId,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub offset: u64,
    pub next_offset: u64,
    pub total_bytes: u64,
    pub finished: bool,
    pub bytes: Vec<u8>,
}
impl ToolArtifactChunk {
    pub fn validate_for(&self, request: &ToolArtifactRead) -> Result<()> {
        request.validate()?;
        if self.schema_version != TOOL_ARTIFACT_SCHEMA_VERSION
            || self.session != request.session
            || self.run != request.run
            || self.content_ref != request.content_ref
            || self.content_digest != request.content_digest
            || self.offset != request.offset
            || self.bytes.is_empty()
            || self.bytes.len() > request.max_bytes as usize
            || self.next_offset
                != self
                    .offset
                    .checked_add(self.bytes.len() as u64)
                    .ok_or_else(invalid)?
            || self.total_bytes == 0
            || self.total_bytes > DESKTOP_OUTPUT_MAX_BYTES
            || self.next_offset > self.total_bytes
            || self.finished != (self.next_offset == self.total_bytes)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
fn identity(schema: SchemaVersion, session: &SessionId, run: &RunId) -> Result<()> {
    if schema != TOOL_ARTIFACT_SCHEMA_VERSION {
        return Err(invalid());
    }
    validate_product_id(&session.0)?;
    validate_product_id(&run.0)
}
fn content_id(id: &ContentRef) -> Result<()> {
    sha256_digest_bytes(&SchemaDigest(
        id.0.strip_prefix("content:").ok_or_else(invalid)?.into(),
    ))
    .map(|_| ())
}
fn invalid() -> Error {
    Error("tool artifact binding or boundary is invalid".into())
}
