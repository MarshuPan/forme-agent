//! Product workbench records that are deliberately outside the brain event log.
#![forbid(unsafe_code)]

use crate::{
    ContentRef, IdempotencyKey, SchemaDigest, SchemaVersion, SessionId, Timestamp, WorkspaceRef,
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
