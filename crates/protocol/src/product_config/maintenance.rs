//! Installed-owner maintenance adapter. Not an Agent or a brain event.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductMaintenanceAction {
    StartCore {},
    StopCore {},
    RestartCore {},
    StageSignedRelease {
        release_id: String,
        release_digest: SchemaDigest,
    },
    UninstallPlan {},
}

impl ProductMaintenanceAction {
    pub fn maintenance_kind(&self) -> MaintenanceKind {
        match self {
            Self::StartCore {} => MaintenanceKind::StartCore,
            Self::StopCore {} => MaintenanceKind::StopCore,
            Self::RestartCore {} => MaintenanceKind::RestartCore,
            Self::StageSignedRelease {
                release_id,
                release_digest,
            } => MaintenanceKind::StageSignedRelease {
                release_id: release_id.clone(),
                release_digest: release_digest.clone(),
            },
            Self::UninstallPlan {} => MaintenanceKind::UninstallPlan {
                preserve_owner_data: true,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMaintenanceRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub action: ProductMaintenanceAction,
    pub requested_deadline: Timestamp,
}

impl ProductMaintenanceRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.expected_revision == 0
            || self.requested_deadline <= 0
        {
            return Err(Error("product maintenance request is invalid".into()));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()?;
        if let ProductMaintenanceAction::StageSignedRelease {
            release_id,
            release_digest,
        } = &self.action
        {
            validate_release_id(release_id)?;
            validate_product_digest(release_digest)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-product-owner-maintenance-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMaintenanceSnapshot {
    pub schema_version: SchemaVersion,
    pub product_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub host: HostStatus,
    pub operations: Vec<MaintenanceOperation>,
}
