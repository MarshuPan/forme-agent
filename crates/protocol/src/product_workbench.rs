//! Product workbench records that are deliberately outside the brain event log.
#![forbid(unsafe_code)]

use crate::{
    ContentRef, IdempotencyKey, RunRequest, SchemaDigest, SchemaVersion, SessionId, Timestamp,
    WorkspaceRef,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchSessionRecord {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub title: String,
    pub workspace: Option<WorkspaceRef>,
    pub archived: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl WorkbenchSessionRecord {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.session.0.trim().is_empty()
            || self.title.trim().is_empty()
            || self.title.len() > 200
            || self.title.chars().any(char::is_control)
            || self.created_at <= 0
            || self.updated_at < self.created_at
            || self.workspace.as_ref().is_some_and(|value| {
                value.0.trim().is_empty()
                    || value.0.len() > 512
                    || value.0.chars().any(char::is_control)
            })
        {
            return Err(crate::Error("workbench session record is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchSessionCreate {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub title: String,
    pub workspace: Option<WorkspaceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkbenchSessionMutation {
    Rename { title: String },
    Archive { archived: bool },
    AssociateWorkspace { workspace: Option<WorkspaceRef> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchSessionUpdate {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub session: SessionId,
    pub expected_updated_at: Timestamp,
    pub mutation: WorkbenchSessionMutation,
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

impl WorkbenchSessionCreate {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SchemaVersion(1)
            || !valid_id(&self.command.0)
            || self.title.trim().is_empty()
            || self.title.len() > 200
            || self.title.chars().any(char::is_control)
        {
            return Err(crate::Error("workbench session create is invalid".into()));
        }
        self.workspace.as_ref().map_or(Ok(()), |workspace| {
            if valid_id(&workspace.0) {
                Ok(())
            } else {
                Err(crate::Error(
                    "workbench session workspace is invalid".into(),
                ))
            }
        })
    }
}

impl WorkbenchSessionUpdate {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SchemaVersion(1)
            || !valid_id(&self.command.0)
            || !valid_id(&self.session.0)
            || self.expected_updated_at <= 0
        {
            return Err(crate::Error("workbench session update is invalid".into()));
        }
        match &self.mutation {
            WorkbenchSessionMutation::Rename { title } => {
                if title.trim().is_empty()
                    || title.len() > 200
                    || title.chars().any(char::is_control)
                {
                    return Err(crate::Error("workbench session title is invalid".into()));
                }
            }
            WorkbenchSessionMutation::AssociateWorkspace { workspace } => {
                if workspace.as_ref().is_some_and(|value| !valid_id(&value.0)) {
                    return Err(crate::Error(
                        "workbench session workspace is invalid".into(),
                    ));
                }
            }
            WorkbenchSessionMutation::Archive { .. } => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchSessionList {
    pub schema_version: SchemaVersion,
    pub sessions: Vec<WorkbenchSessionRecord>,
}

/// Signed Web-to-core envelope for a user run whose workspace was resolved
/// from the authenticated Web session record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchRunSubmission {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub workspace: WorkspaceRef,
    pub request: RunRequest,
}

impl WorkbenchRunSubmission {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.session.0.trim().is_empty()
            || self.workspace.0.trim().is_empty()
            || self.request.source != crate::Source::UserTurn
            || self.request.session.0 != self.session.0
            || self.request.agent_profile.0.trim().is_empty()
            || self.request.idempotency_key.as_ref().is_none_or(|key| {
                key.0.trim().is_empty() || key.0.len() > 256 || key.0.chars().any(char::is_control)
            })
        {
            return Err(crate::Error("workbench run submission is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactUploadReceipt {
    pub schema_version: SchemaVersion,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub scope: crate::Scope,
    pub bytes: u64,
    pub file_name: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCatalogQuery {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub page: crate::DataPageQuery,
}

impl ArtifactCatalogQuery {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SchemaVersion(1) || !valid_id(&self.session.0) {
            return Err(crate::Error("artifact catalog query is invalid".into()));
        }
        self.page.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCatalogEntry {
    pub schema_version: SchemaVersion,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub scope: crate::Scope,
    pub bytes: u64,
    /// Older uploads have no retained filename or media type. Reading them
    /// must not manufacture metadata or silently write a migration.
    pub file_name: Option<String>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCatalogView {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub digest: SchemaDigest,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub artifacts: Vec<ArtifactCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactUploadCommand {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub session: SessionId,
    pub workspace: Option<WorkspaceRef>,
    pub file_name: String,
    pub media_type: String,
    /// Hex is used at this closed JSON boundary so the browser never sends
    /// multipart data through the core bridge. The plaintext is encrypted
    /// before it reaches the protected content catalog.
    pub bytes_hex: String,
}

impl ArtifactUploadCommand {
    pub const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
    /// The dedicated upload transport accepts hex plus a bounded JSON envelope.
    /// This ceiling must not increase limits on ordinary control requests.
    pub const MAX_REQUEST_BYTES: usize = 2 * Self::MAX_CONTENT_BYTES + 8192;

    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SchemaVersion(1)
            || !valid_id(&self.command.0)
            || !valid_id(&self.session.0)
            || self.file_name.trim().is_empty()
            || self.file_name.len() > 200
            || self.file_name.chars().any(char::is_control)
            || self.media_type.trim().is_empty()
            || self.media_type.len() > 128
            || self
                .media_type
                .chars()
                .any(|value| value.is_control() || value == '\n' || value == '\r')
            || self.bytes_hex.is_empty()
            || self.bytes_hex.len() > 2 * Self::MAX_CONTENT_BYTES
            || !self.bytes_hex.len().is_multiple_of(2)
            || !self
                .bytes_hex
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
        {
            return Err(crate::Error("artifact upload command is invalid".into()));
        }
        if self
            .workspace
            .as_ref()
            .is_some_and(|value| !valid_id(&value.0))
        {
            return Err(crate::Error("artifact workspace is invalid".into()));
        }
        Ok(())
    }
}
