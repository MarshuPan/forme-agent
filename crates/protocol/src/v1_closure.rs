//! V1 core-brain closure protocol contracts.
//!
//! These objects close the two facts that could not be represented by the
//! existing M0-M5 taxonomy: a versioned workspace charter and a redacted data
//! lifecycle tombstone. They are deliberately data-only. Authority,
//! compare-and-swap, and projection commits remain store responsibilities.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use crate::*;

pub const V1_CLOSURE_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OwnerPrincipalRef(pub String);

impl OwnerPrincipalRef {
    pub fn validate(&self) -> Result<()> {
        required_ref(&self.0, "owner principal")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectionRef(pub String);

impl ProjectionRef {
    pub fn validate(&self) -> Result<()> {
        validate_safe_reference(&self.0, "projection reference")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceCharterRecord {
    pub schema_version: SchemaVersion,
    pub workspace: WorkspaceRef,
    pub version: u64,
    pub goals: Vec<GoalRef>,
    pub constraints: Vec<Constraint>,
    pub prohibitions: Vec<Constraint>,
    pub done_contract: Option<DoneContractRef>,
    pub review_cadence: Option<DurationMs>,
    pub actor: Actor,
    pub digest: SchemaDigest,
}

impl WorkspaceCharterRecord {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.version == 0 {
            return Err(Error(
                "workspace charter schema or version is invalid".into(),
            ));
        }
        required_ref(&self.workspace.0, "workspace charter workspace")?;
        validate_refs(&self.goals, "workspace charter goal", |value| &value.0)?;
        validate_refs(&self.constraints, "workspace charter constraint", |value| {
            &value.0
        })?;
        validate_refs(
            &self.prohibitions,
            "workspace charter prohibition",
            |value| &value.0,
        )?;
        if let Some(done_contract) = &self.done_contract {
            required_ref(&done_contract.0, "workspace charter done contract")?;
        }
        if self.review_cadence.is_some_and(|cadence| cadence.0 == 0) {
            return Err(Error(
                "workspace charter review cadence must be non-zero".into(),
            ));
        }
        if !matches!(self.actor, Actor::Owner) {
            return Err(Error("workspace charter actor must be the owner".into()));
        }
        validate_digest(&self.digest, "workspace charter digest")?;
        if self.digest != self.expected_digest()? {
            return Err(Error(
                "workspace charter digest does not match its contents".into(),
            ));
        }
        Ok(())
    }

    pub fn expected_digest(&self) -> Result<SchemaDigest> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        canonical_digest(&material)
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        self.digest = self.expected_digest()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataLifecycleOperation {
    Retain,
    Delete,
    CryptoShred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemoteDeletionDisposition {
    NotApplicable,
    Requested,
    Verified,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataLifecycleReceipt {
    pub schema_version: SchemaVersion,
    pub aggregate: RunId,
    pub operation: DataLifecycleOperation,
    pub scope: Scope,
    pub subject_digest: SchemaDigest,
    pub cleaned_projections: Vec<ProjectionRef>,
    pub destroyed_key_digests: Vec<SchemaDigest>,
    pub remote_disposition: RemoteDeletionDisposition,
    pub evidence: Vec<EvidenceRef>,
}

impl DataLifecycleReceipt {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("data lifecycle receipt schema is invalid".into()));
        }
        validate_safe_reference(&self.aggregate.0, "data lifecycle aggregate")?;
        validate_safe_reference(&self.scope.0, "data lifecycle scope")?;
        validate_digest(&self.subject_digest, "data lifecycle subject digest")?;
        for projection in &self.cleaned_projections {
            projection.validate()?;
        }
        for digest in &self.destroyed_key_digests {
            validate_digest(digest, "data lifecycle destroyed key digest")?;
        }
        validate_refs(&self.evidence, "data lifecycle evidence", |value| &value.0)?;
        if matches!(self.operation, DataLifecycleOperation::CryptoShred)
            && self.destroyed_key_digests.is_empty()
        {
            return Err(Error(
                "crypto-shred receipt must name destroyed key digests".into(),
            ));
        }
        Ok(())
    }
}

impl WorkspaceCharterChangedPayload {
    pub fn validate(&self) -> Result<()> {
        self.charter.validate()?;
        validate_version_step(
            self.expected_version,
            self.committed_version,
            "workspace charter",
        )?;
        if self.charter.version != self.committed_version {
            return Err(Error(
                "workspace charter committed version disagrees with record".into(),
            ));
        }
        Ok(())
    }
}

impl DataLifecycleAppliedPayload {
    pub fn validate(&self) -> Result<()> {
        self.receipt.validate()?;
        validate_version_step(
            self.expected_version,
            self.committed_version,
            "data lifecycle",
        )
    }
}

fn required_ref(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error(format!("{name} is incomplete")))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &SchemaDigest, name: &str) -> Result<()> {
    sha256_digest_bytes(value)
        .map(|_| ())
        .map_err(|_| Error(format!("{name} is not a canonical sha256 digest")))
}

fn validate_refs<T>(values: &[T], name: &str, value: impl Fn(&T) -> &String) -> Result<()> {
    for item in values {
        required_ref(value(item), name)?;
    }
    Ok(())
}

fn validate_safe_reference(value: &str, name: &str) -> Result<()> {
    required_ref(value, name)?;
    let lowered = value.to_ascii_lowercase();
    let forbidden_markers = [
        "secret",
        "credential",
        "password",
        "api_key",
        "apikey",
        "key_id",
        "keyid",
        "private_key",
        "endpoint",
    ];
    if forbidden_markers
        .iter()
        .any(|marker| lowered.contains(marker))
        || lowered.starts_with("file:")
        || lowered.contains("://")
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(":\\")
    {
        return Err(Error(format!(
            "{name} contains a secret, endpoint, or private-path marker"
        )));
    }
    Ok(())
}

fn validate_version_step(expected: u64, committed: u64, name: &str) -> Result<()> {
    let next = expected
        .checked_add(1)
        .ok_or_else(|| Error(format!("{name} expected version is exhausted")))?;
    if committed != next {
        return Err(Error(format!(
            "{name} committed version must be exactly expected version plus one"
        )));
    }
    Ok(())
}
