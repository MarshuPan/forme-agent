//! Product-only setup messages. No brain events, arbitrary paths or owner input.
use serde::{Deserialize, Serialize};

use crate::{OwnerPrincipalRef, Result, SchemaDigest, SchemaVersion, Timestamp};

pub const BOOTSTRAP_CANDIDATE_SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);

mod legacy_claim;
pub use legacy_claim::*;
mod data_activation;
pub use data_activation::*;

/// Sensitive authenticated-service input; intentionally has no Debug.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapReserveRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub proof: String,
    pub setup_digest: SchemaDigest,
}

/// Only the trusted Web service may use this binding. It is not a browser grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSessionBinding {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub setup_digest: SchemaDigest,
}

impl BootstrapSessionBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(crate::Error("unsupported product setup version".into()));
        }
        crate::validate_product_id(&self.operation_id)?;
        crate::validate_product_digest(&self.setup_digest)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataChoice {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBinding,
    pub candidate_ref: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapCommitRequest {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBinding,
    pub candidate_ref: String,
    pub account_commitment: SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapReservationView {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataCandidateView {
    pub schema_version: SchemaVersion,
    pub candidate_ref: String,
    pub selected: bool,
    pub available: bool,
    pub issue_code: Option<BootstrapProblemCode>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapProofRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
}

/// Delivered exclusively to an authenticated local management caller.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapProofView {
    pub schema_version: SchemaVersion,
    pub proof: String,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapProblemCode {
    InvalidInput,
    Unauthenticated,
    Conflict,
    Expired,
    DataUnavailable,
    Unavailable,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapProblem {
    pub schema_version: SchemaVersion,
    pub code: BootstrapProblemCode,
}

/// A candidate kind is a product-data concern, not a brain event or a model
/// decision. The browser may display this value but cannot manufacture one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapCandidateKind {
    NewBrainTarget,
    ExistingBrain,
    VerifiedBackup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapCandidateStatus {
    Available,
    Rejected,
    Unverifiable,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapOwnerSource {
    GeneratedForNewBrain,
    VerifiedFromCandidate,
    /// Historical identity is not established. This is only valid for a
    /// rejected or unverifiable historical candidate and can never authorize
    /// selection or claim.
    NotEstablished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapProtectionState {
    Protected,
    NeedsMigration,
    Unprotected,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSessionBindingV2 {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub setup_digest: SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapCandidateCatalogRequestV2 {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBindingV2,
}

impl BootstrapCandidateCatalogRequestV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported candidate catalog version".into()));
        }
        self.session.validate()
    }
}

impl BootstrapSessionBindingV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported candidate setup version".into()));
        }
        crate::validate_product_id(&self.operation_id)?;
        crate::validate_product_digest(&self.setup_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapCandidateViewV2 {
    pub schema_version: SchemaVersion,
    pub candidate_ref: String,
    pub kind: BootstrapCandidateKind,
    pub status: BootstrapCandidateStatus,
    pub candidate_digest: SchemaDigest,
    pub verification_epoch: u64,
    pub owner_source: BootstrapOwnerSource,
    /// A safe owner principal reference produced by the verifier. It is not
    /// accepted from a browser and never contains a credential or key.
    pub owner_ref: Option<OwnerPrincipalRef>,
    pub owner_binding_digest: Option<SchemaDigest>,
    pub store_identity_digest: Option<SchemaDigest>,
    pub schema_digest: Option<SchemaDigest>,
    pub protection_state: BootstrapProtectionState,
    pub source_observation_digest: SchemaDigest,
    pub issue_ref: Option<String>,
}

impl BootstrapCandidateViewV2 {
    pub fn validate(&self, install_ref: &str) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION || self.verification_epoch == 0
        {
            return Err(crate::Error(
                "invalid bootstrap candidate version or epoch".into(),
            ));
        }
        crate::validate_product_id(install_ref)?;
        crate::validate_product_id(&self.candidate_ref)?;
        crate::validate_product_digest(&self.candidate_digest)?;
        crate::validate_product_digest(&self.source_observation_digest)?;
        for digest in [
            self.owner_binding_digest.as_ref(),
            self.store_identity_digest.as_ref(),
            self.schema_digest.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            crate::validate_product_digest(digest)?;
        }
        if let Some(owner_ref) = &self.owner_ref {
            owner_ref.validate()?;
        }
        if let (Some(owner_ref), Some(owner_binding_digest)) =
            (&self.owner_ref, &self.owner_binding_digest)
        {
            let expected_owner_binding =
                crate::canonical_digest(&("forme-bootstrap-owner-binding-v2", owner_ref))?;
            if owner_binding_digest != &expected_owner_binding {
                return Err(crate::Error(
                    "bootstrap candidate owner binding is invalid".into(),
                ));
            }
        }
        if let Some(issue_ref) = &self.issue_ref {
            crate::validate_product_id(issue_ref)?;
        }
        match self.kind {
            BootstrapCandidateKind::NewBrainTarget => {
                if self.owner_source != BootstrapOwnerSource::GeneratedForNewBrain
                    || self.owner_ref.is_some()
                    || self.owner_binding_digest.is_some()
                    || self.store_identity_digest.is_some()
                    || self.schema_digest.is_some()
                {
                    return Err(crate::Error(
                        "new brain candidate has historical identity".into(),
                    ));
                }
            }
            BootstrapCandidateKind::ExistingBrain | BootstrapCandidateKind::VerifiedBackup => {
                let established = self.owner_source == BootstrapOwnerSource::VerifiedFromCandidate
                    && self.owner_ref.is_some()
                    && self.owner_binding_digest.is_some()
                    && self.store_identity_digest.is_some()
                    && self.schema_digest.is_some();
                let unavailable = self.owner_source == BootstrapOwnerSource::NotEstablished
                    && self.owner_ref.is_none()
                    && self.owner_binding_digest.is_none()
                    && self.store_identity_digest.is_none()
                    && self.schema_digest.is_none();
                if self.status == BootstrapCandidateStatus::Available && !established
                    || self.status != BootstrapCandidateStatus::Available
                        && !(established || unavailable)
                    || self.status == BootstrapCandidateStatus::Available
                        && self.issue_ref.is_some()
                    || self.status != BootstrapCandidateStatus::Available
                        && self.issue_ref.is_none()
                {
                    return Err(crate::Error(
                        "historical candidate lacks verified identity".into(),
                    ));
                }
            }
        }
        let expected = self.expected_digest(install_ref)?;
        if expected != self.candidate_digest {
            return Err(crate::Error("bootstrap candidate digest is invalid".into()));
        }
        if self.status == BootstrapCandidateStatus::Available
            && (self.issue_ref.is_some()
                || self.protection_state != BootstrapProtectionState::Protected
                || (self.kind != BootstrapCandidateKind::NewBrainTarget
                    && self.owner_source != BootstrapOwnerSource::VerifiedFromCandidate))
        {
            return Err(crate::Error(
                "available candidate has unresolved issues".into(),
            ));
        }
        Ok(())
    }

    pub fn expected_digest(&self, install_ref: &str) -> Result<SchemaDigest> {
        crate::canonical_digest(&(
            "forme-bootstrap-candidate-v2",
            BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
            install_ref,
            &self.candidate_ref,
            self.kind,
            &self.source_observation_digest,
            &self.owner_binding_digest,
            &self.owner_ref,
            &self.store_identity_digest,
            &self.schema_digest,
            self.protection_state,
            self.verification_epoch,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapCandidateCatalogV2 {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub verification_epoch: u64,
    pub candidates: Vec<BootstrapCandidateViewV2>,
    pub catalog_digest: SchemaDigest,
}

impl BootstrapCandidateCatalogV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION
            || self.verification_epoch == 0
            || self.candidates.is_empty()
        {
            return Err(crate::Error("invalid bootstrap candidate catalog".into()));
        }
        crate::validate_product_id(&self.install_ref)?;
        let mut refs = std::collections::BTreeSet::new();
        for candidate in &self.candidates {
            candidate.validate(&self.install_ref)?;
            if candidate.verification_epoch != self.verification_epoch
                || !refs.insert(candidate.candidate_ref.clone())
            {
                return Err(crate::Error(
                    "bootstrap catalog has inconsistent candidates".into(),
                ));
            }
        }
        crate::validate_product_digest(&self.catalog_digest)?;
        if self.catalog_digest != self.expected_digest()? {
            return Err(crate::Error("bootstrap catalog digest is invalid".into()));
        }
        Ok(())
    }

    pub fn expected_digest(&self) -> Result<SchemaDigest> {
        crate::canonical_digest(&(
            "forme-bootstrap-catalog-v2",
            self.schema_version,
            &self.install_ref,
            self.verification_epoch,
            &self.candidates,
        ))
    }

    pub fn candidate(&self, candidate_ref: &str) -> Option<&BootstrapCandidateViewV2> {
        self.candidates
            .iter()
            .find(|candidate| candidate.candidate_ref == candidate_ref)
    }
}
