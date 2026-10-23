//! A memory node, never an arbitrary content locator, authorizes this owner view.
use crate::*;
use serde::{Deserialize, Serialize};

pub const WORKBENCH_MATERIAL_MAX_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchMaterialQuery {
    pub schema_version: SchemaVersion,
    pub node: NodeId,
    pub run: RunId,
    pub event: EventId,
    pub scope: Option<Scope>,
    pub snapshot: SchemaDigest,
}

impl WorkbenchMaterialQuery {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || [&self.node.0, &self.run.0, &self.event.0]
                .iter()
                .any(|value| {
                    value.trim().is_empty()
                        || value.len() > 512
                        || value.chars().any(char::is_control)
                })
        {
            return Err(Error("memory material query is invalid".into()));
        }
        self.memory_query().validate()
    }

    pub fn memory_query(&self) -> WorkbenchQuery {
        WorkbenchQuery {
            schema_version: SchemaVersion(1),
            section: WorkbenchSection::Memory,
            scope: self.scope.clone(),
            offset: 0,
            limit: 100,
            snapshot: Some(self.snapshot.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkbenchMaterialState {
    Available,
    ReferenceOnly,
    MissingOrErased,
    TooLarge,
    NonText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchMaterialView {
    pub schema_version: SchemaVersion,
    pub node: NodeId,
    pub run: RunId,
    pub event: EventId,
    pub scope: Scope,
    pub content_ref: ContentRef,
    pub state: WorkbenchMaterialState,
    pub bytes: Option<u64>,
    pub content_digest: Option<SchemaDigest>,
    pub text: Option<String>,
}
