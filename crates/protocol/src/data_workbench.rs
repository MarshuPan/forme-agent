//! Closed data-maintenance views; internal plans and storage locators stay in core.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPageQuery {
    pub schema_version: SchemaVersion,
    pub offset: usize,
    pub limit: usize,
    pub snapshot: Option<SchemaDigest>,
}
impl DataPageQuery {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.limit == 0
            || self.limit > 100
            || self.offset > 1_000_000
            || self.offset > 0 && self.snapshot.is_none()
        {
            return Err(Error("data page is invalid or unbounded".into()));
        }
        if let Some(digest) = &self.snapshot {
            sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataWorkbenchQuery {
    pub schema_version: SchemaVersion,
    pub query: DataWorkbenchRead,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DataWorkbenchRead {
    Catalog { page: DataPageQuery },
    Operations { page: DataPageQuery },
    Inspect { command: IdempotencyKey },
    VerifyBackup { source: String },
}
impl DataWorkbenchQuery {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(Error("unsupported data schema".into()));
        }
        match &self.query {
            DataWorkbenchRead::Catalog { page } | DataWorkbenchRead::Operations { page } => {
                page.validate()
            }
            DataWorkbenchRead::Inspect { command } => validate_product_id(&command.0),
            DataWorkbenchRead::VerifyBackup { source } => validate_data_object_name(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataWorkbenchCommand {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub action: DataWorkbenchAction,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DataWorkbenchAction {
    Backup {
        target: String,
    },
    PrepareRestore {
        source: String,
        target: String,
    },
    PrepareLifecycle {
        scope: Scope,
        subject: SchemaDigest,
        operation: DataLifecycleOperation,
        expected_version: u64,
    },
    Apply {
        prepared_by: IdempotencyKey,
        plan_digest: SchemaDigest,
    },
}
impl DataWorkbenchCommand {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(Error("unsupported data schema".into()));
        }
        validate_product_id(&self.command.0)?;
        match &self.action {
            DataWorkbenchAction::Backup { target } => validate_data_object_name(target),
            DataWorkbenchAction::PrepareRestore { source, target } => {
                validate_data_object_name(source)?;
                validate_data_object_name(target)?;
                if source.eq_ignore_ascii_case(target) {
                    return Err(Error("restore target must be new".into()));
                }
                Ok(())
            }
            DataWorkbenchAction::PrepareLifecycle {
                scope,
                subject,
                expected_version,
                ..
            } => {
                if scope.0.is_empty()
                    || scope.0.len() > 512
                    || scope.0.chars().any(char::is_control)
                    || scope.0.starts_with("maintenance:data-workbench")
                    || *expected_version == u64::MAX
                {
                    return Err(Error("data scope or revision is invalid".into()));
                }
                sha256_digest_bytes(subject).map(|_| ())
            }
            DataWorkbenchAction::Apply {
                prepared_by,
                plan_digest,
            } => {
                validate_product_id(&prepared_by.0)?;
                sha256_digest_bytes(plan_digest).map(|_| ())
            }
        }
    }
}
pub fn validate_data_object_name(name: &str) -> Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 96
        || name.starts_with('.')
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
    {
        return Err(Error(
            "data object must be an opaque regular filename".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSubjectView {
    pub schema_version: SchemaVersion,
    pub scope: Scope,
    pub subject: SchemaDigest,
    pub objects: usize,
    pub deleted_objects: usize,
    pub projections: Vec<String>,
    pub state: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataCatalogView {
    pub schema_version: SchemaVersion,
    pub version: u64,
    pub pending_cleanup: bool,
    pub digest: SchemaDigest,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub subjects: Vec<DataSubjectView>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataPlanPreview {
    pub schema_version: SchemaVersion,
    pub digest: SchemaDigest,
    pub expires_at: Timestamp,
    pub operation: String,
    pub scope: Option<Scope>,
    pub subject: Option<SchemaDigest>,
    pub objects: usize,
    pub source: Option<String>,
    pub target: Option<String>,
    pub irreversible: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataCommandState {
    Prepared,
    Confirmed,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBackupView {
    pub schema_version: SchemaVersion,
    pub object: String,
    pub digest: SchemaDigest,
    pub events: u64,
    pub created_at: Timestamp,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restoration: Option<DataRestoreCandidateView>,
}

/// Safe output from an actually verified restore, never filesystem/key paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataRestoreCandidateView {
    pub schema_version: SchemaVersion,
    pub candidate_ref: String,
    pub candidate_digest: SchemaDigest,
    pub owner_digest: SchemaDigest,
    pub store_identity: SchemaDigest,
    pub execution_fence_digest: SchemaDigest,
}

impl DataRestoreCandidateView {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(Error("unsupported restore candidate view".into()));
        }
        validate_data_object_name(&self.candidate_ref)?;
        for digest in [
            &self.candidate_digest,
            &self.owner_digest,
            &self.store_identity,
            &self.execution_fence_digest,
        ] {
            sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataLifecycleView {
    pub schema_version: SchemaVersion,
    pub scope: Scope,
    pub subject: SchemaDigest,
    pub operation: DataLifecycleOperation,
    pub cleaned_projections: usize,
    pub destroyed_keys: usize,
    pub remote: RemoteDeletionDisposition,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataCommandReceipt {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub binding: SchemaDigest,
    pub action: String,
    pub prepared_by: Option<IdempotencyKey>,
    pub state: DataCommandState,
    pub plan: Option<DataPlanPreview>,
    pub backup: Option<DataBackupView>,
    pub lifecycle: Option<DataLifecycleView>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataOperationList {
    pub schema_version: SchemaVersion,
    pub digest: SchemaDigest,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub operations: Vec<DataCommandReceipt>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataReceiptView {
    pub schema_version: SchemaVersion,
    pub receipt: Option<DataCommandReceipt>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DataWorkbenchView {
    Catalog(DataCatalogView),
    Operations(DataOperationList),
    Receipt(Box<DataReceiptView>),
    Verification(DataBackupView),
}
