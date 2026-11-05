//! Schema 2 legacy-brain claim messages.
//!
//! These product messages separate data preparation, account commit, and
//! active-data activation. They are not brain events and are never built from
//! browser-supplied owner or path values.
use serde::{Deserialize, Serialize};

use super::{
    BootstrapCandidateKind, BootstrapOwnerSource, BootstrapSessionBindingV2,
    BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
};
use crate::{OwnerPrincipalRef, Result, SchemaDigest, SchemaVersion, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapCandidateSelectionV2 {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBindingV2,
    pub candidate_ref: String,
    pub expected_candidate_digest: SchemaDigest,
}

impl BootstrapCandidateSelectionV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error(
                "unsupported candidate selection version".into(),
            ));
        }
        self.session.validate()?;
        crate::validate_product_id(&self.candidate_ref)?;
        crate::validate_product_digest(&self.expected_candidate_digest)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimRequestV2 {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBindingV2,
    pub candidate_ref: String,
    pub expected_candidate_digest: SchemaDigest,
    pub account_commitment: SchemaDigest,
}

impl BootstrapClaimRequestV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported candidate claim version".into()));
        }
        self.session.validate()?;
        crate::validate_product_id(&self.candidate_ref)?;
        crate::validate_product_digest(&self.expected_candidate_digest)?;
        crate::validate_product_digest(&self.account_commitment)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapMigrationOutcome {
    NotStarted,
    Prepared,
    Recovered,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimReceiptV2 {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub candidate_ref: String,
    pub candidate_kind: BootstrapCandidateKind,
    pub candidate_digest: SchemaDigest,
    pub owner_source: BootstrapOwnerSource,
    pub owner_ref: OwnerPrincipalRef,
    pub owner_binding_digest: SchemaDigest,
    pub account_commitment: SchemaDigest,
    pub target_binding_digest: SchemaDigest,
    pub migration_outcome: BootstrapMigrationOutcome,
    pub expires_at: Timestamp,
}

impl BootstrapClaimReceiptV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported candidate receipt version".into()));
        }
        crate::validate_product_id(&self.install_ref)?;
        crate::validate_product_id(&self.operation_id)?;
        crate::validate_product_id(&self.candidate_ref)?;
        for digest in [
            &self.candidate_digest,
            &self.owner_binding_digest,
            &self.account_commitment,
            &self.target_binding_digest,
        ] {
            crate::validate_product_digest(digest)?;
        }
        self.owner_ref.validate()?;
        let expected_owner_binding =
            crate::canonical_digest(&("forme-bootstrap-owner-binding-v2", &self.owner_ref))?;
        if self.owner_binding_digest != expected_owner_binding {
            return Err(crate::Error(
                "historical claim owner binding is invalid".into(),
            ));
        }
        if self.expires_at <= 0
            || self.candidate_kind == BootstrapCandidateKind::NewBrainTarget
            || self.owner_source != BootstrapOwnerSource::VerifiedFromCandidate
        {
            return Err(crate::Error("invalid historical claim receipt".into()));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme-bootstrap-claim-receipt-v2", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimCommitV2 {
    pub schema_version: SchemaVersion,
    pub receipt: BootstrapClaimReceiptV2,
}

impl BootstrapClaimCommitV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported candidate commit version".into()));
        }
        self.receipt.validate()?;
        if matches!(
            self.receipt.migration_outcome,
            BootstrapMigrationOutcome::NotStarted | BootstrapMigrationOutcome::Unknown
        ) {
            return Err(crate::Error(
                "claim commit lacks a prepared migration result".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_against_prepared(&self, prepared: &BootstrapClaimDataPreparedV2) -> Result<()> {
        self.validate()?;
        prepared.validate()?;
        if self.receipt.operation_id != prepared.operation_id
            || self.receipt.candidate_ref != prepared.candidate_ref
            || self.receipt.candidate_digest != prepared.candidate_digest
            || self.receipt.target_binding_digest != prepared.target_binding_digest
            || self.receipt.migration_outcome != prepared.migration_outcome
        {
            return Err(crate::Error(
                "claim commit is not bound to prepared migration data".into(),
            ));
        }
        Ok(())
    }
}

/// Trusted result from the fixed legacy-data worker. This object is never a
/// browser request: the Host constructs it only after re-validating the
/// source, target and worker receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimDataPreparedV2 {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub candidate_ref: String,
    pub candidate_digest: SchemaDigest,
    pub verification_epoch: u64,
    pub target_binding_digest: SchemaDigest,
    pub migration_outcome: BootstrapMigrationOutcome,
    pub worker_receipt_digest: SchemaDigest,
}

impl BootstrapClaimDataPreparedV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION
            || self.verification_epoch == 0
            || self.migration_outcome == BootstrapMigrationOutcome::NotStarted
            || self.migration_outcome == BootstrapMigrationOutcome::Unknown
        {
            return Err(crate::Error("invalid prepared bootstrap data".into()));
        }
        crate::validate_product_id(&self.operation_id)?;
        crate::validate_product_id(&self.candidate_ref)?;
        for digest in [
            &self.candidate_digest,
            &self.target_binding_digest,
            &self.worker_receipt_digest,
        ] {
            crate::validate_product_digest(digest)?;
        }
        Ok(())
    }
}

/// Trusted Host-side active-data proof. A commit is not activation; this is
/// appended only after the product configuration CAS and post-switch probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimActivationV2 {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub receipt_digest: SchemaDigest,
    pub active_data_binding_digest: SchemaDigest,
}

impl BootstrapClaimActivationV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported activation version".into()));
        }
        crate::validate_product_id(&self.operation_id)?;
        crate::validate_product_digest(&self.receipt_digest)?;
        crate::validate_product_digest(&self.active_data_binding_digest)
    }

    pub fn validate_against_receipt(&self, receipt: &BootstrapClaimReceiptV2) -> Result<()> {
        self.validate()?;
        receipt.validate()?;
        if receipt.operation_id != self.operation_id {
            return Err(crate::Error(
                "claim activation is bound to another operation".into(),
            ));
        }
        if receipt.digest()? != self.receipt_digest {
            return Err(crate::Error(
                "claim activation receipt binding is invalid".into(),
            ));
        }
        if receipt.migration_outcome != BootstrapMigrationOutcome::Recovered {
            return Err(crate::Error(
                "claim activation requires recovered migration data".into(),
            ));
        }
        if self.active_data_binding_digest != receipt.target_binding_digest {
            return Err(crate::Error(
                "claim activation data binding is not the prepared target".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_against_commit(&self, commit: &BootstrapClaimCommitV2) -> Result<()> {
        commit.validate()?;
        self.validate_against_receipt(&commit.receipt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimQueryV2 {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBindingV2,
}

impl BootstrapClaimQueryV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported claim query version".into()));
        }
        self.session.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapClaimPhaseV2 {
    Observed,
    Selected,
    DataPrepared,
    Committed,
    Activated,
    Aborted,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapClaimViewV2 {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub phase: BootstrapClaimPhaseV2,
    pub candidate_ref: Option<String>,
    pub candidate_digest: Option<SchemaDigest>,
    pub verification_epoch: Option<u64>,
    pub receipt: Option<BootstrapClaimReceiptV2>,
    pub activation: Option<BootstrapClaimActivationV2>,
    pub active_data_binding_digest: Option<SchemaDigest>,
    pub issue_ref: Option<String>,
}

impl BootstrapClaimViewV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION {
            return Err(crate::Error("unsupported claim view version".into()));
        }
        crate::validate_product_id(&self.install_ref)?;
        crate::validate_product_id(&self.operation_id)?;
        if let Some(candidate_ref) = &self.candidate_ref {
            crate::validate_product_id(candidate_ref)?;
        }
        for digest in [
            self.candidate_digest.as_ref(),
            self.active_data_binding_digest.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            crate::validate_product_digest(digest)?;
        }
        if self.verification_epoch == Some(0) {
            return Err(crate::Error("claim view has an invalid epoch".into()));
        }
        let candidate_fields = [
            self.candidate_ref.is_some(),
            self.candidate_digest.is_some(),
            self.verification_epoch.is_some(),
        ];
        if candidate_fields
            .iter()
            .any(|present| *present != candidate_fields[0])
        {
            return Err(crate::Error(
                "claim view has an incomplete candidate binding".into(),
            ));
        }
        let has_candidate = candidate_fields[0];
        let has_receipt = self.receipt.is_some();
        let has_activation = self.activation.is_some();
        let has_active_binding = self.active_data_binding_digest.is_some();
        if has_activation != has_active_binding {
            return Err(crate::Error(
                "claim view has an incomplete activation binding".into(),
            ));
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
            BootstrapClaimCommitV2 {
                schema_version: self.schema_version,
                receipt: receipt.clone(),
            }
            .validate()?;
            if receipt.install_ref != self.install_ref || receipt.operation_id != self.operation_id
            {
                return Err(crate::Error(
                    "claim receipt is not bound to the view".into(),
                ));
            }
            if self.candidate_ref.as_ref() != Some(&receipt.candidate_ref)
                || self.candidate_digest.as_ref() != Some(&receipt.candidate_digest)
            {
                return Err(crate::Error(
                    "claim receipt is not bound to the candidate view".into(),
                ));
            }
        }
        if let Some(activation) = &self.activation {
            let receipt = self
                .receipt
                .as_ref()
                .ok_or_else(|| crate::Error("claim activation lacks its receipt".into()))?;
            activation.validate_against_receipt(receipt)?;
            if self.active_data_binding_digest.as_ref()
                != Some(&activation.active_data_binding_digest)
            {
                return Err(crate::Error(
                    "claim view active binding is not the activation binding".into(),
                ));
            }
        }
        if let Some(issue_ref) = &self.issue_ref {
            crate::validate_product_id(issue_ref)?;
        }
        match self.phase {
            BootstrapClaimPhaseV2::Observed => {
                if has_candidate
                    || has_receipt
                    || has_activation
                    || has_active_binding
                    || self.issue_ref.is_some()
                {
                    return Err(crate::Error(
                        "observed claim has progressed-state bindings".into(),
                    ));
                }
            }
            BootstrapClaimPhaseV2::Selected | BootstrapClaimPhaseV2::DataPrepared => {
                if !has_candidate
                    || has_receipt
                    || has_activation
                    || has_active_binding
                    || self.issue_ref.is_some()
                {
                    return Err(crate::Error("claim phase lacks candidate binding".into()));
                }
            }
            BootstrapClaimPhaseV2::Committed => {
                if !has_candidate
                    || !has_receipt
                    || has_activation
                    || has_active_binding
                    || self.issue_ref.is_some()
                {
                    return Err(crate::Error(
                        "committed claim has an invalid binding".into(),
                    ));
                }
            }
            BootstrapClaimPhaseV2::Activated => {
                if !has_candidate
                    || !has_receipt
                    || !has_activation
                    || !has_active_binding
                    || self.issue_ref.is_some()
                {
                    return Err(crate::Error("activated claim lacks active binding".into()));
                }
            }
            BootstrapClaimPhaseV2::Aborted => {
                if !has_candidate
                    || has_receipt
                    || has_activation
                    || has_active_binding
                    || self.issue_ref.is_none()
                {
                    return Err(crate::Error("aborted claim lacks a reason".into()));
                }
            }
            BootstrapClaimPhaseV2::RecoveryRequired => {
                if !has_candidate
                    || has_activation
                    || has_active_binding
                    || self.issue_ref.is_none()
                {
                    return Err(crate::Error(
                        "recovering claim lacks its pending binding".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
