//! Offline data activation is separate from a running core acknowledgement.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataBindingConfirmationV2 {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub receipt_digest: SchemaDigest,
    pub expected_config_revision: u64,
    pub config_binding_digest: SchemaDigest,
    pub target_binding_digest: SchemaDigest,
    pub execution_fence_digest: SchemaDigest,
    pub verification_digest: SchemaDigest,
}

impl BootstrapDataBindingConfirmationV2 {
    pub fn validate_against_receipt(&self, receipt: &BootstrapClaimReceiptV2) -> Result<()> {
        receipt.validate()?;
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION
            || self.expected_config_revision != 0
            || self.install_ref != receipt.install_ref
            || self.operation_id != receipt.operation_id
            || self.receipt_digest != receipt.digest()?
            || self.target_binding_digest != receipt.target_binding_digest
            || receipt.migration_outcome != BootstrapMigrationOutcome::Recovered
        {
            return Err(crate::Error(
                "offline claim confirmation binding is invalid".into(),
            ));
        }
        for digest in [
            &self.config_binding_digest,
            &self.execution_fence_digest,
            &self.verification_digest,
        ] {
            crate::validate_product_digest(digest)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedBootstrapClaimActivationV2 {
    pub schema_version: SchemaVersion,
    pub receipt: BootstrapClaimReceiptV2,
    pub activation: BootstrapClaimActivationV2,
    pub signature: Vec<u8>,
}

impl SignedBootstrapClaimActivationV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION || self.signature.len() != 64 {
            return Err(crate::Error("invalid signed claim activation".into()));
        }
        self.activation.validate_against_receipt(&self.receipt)
    }

    pub fn signing_digest(
        receipt: &BootstrapClaimReceiptV2,
        activation: &BootstrapClaimActivationV2,
    ) -> Result<SchemaDigest> {
        activation.validate_against_receipt(receipt)?;
        crate::canonical_digest(&("forme-bootstrap-claim-activation-v2", receipt, activation))
    }
}

/// Private Host/worker input. Not accepted by the browser setup API.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataIntentV2 {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub request: BootstrapClaimRequestV2,
    pub candidate: BootstrapCandidateViewV2,
}

impl BootstrapDataIntentV2 {
    pub fn validate(&self) -> Result<()> {
        self.request.validate()?;
        self.candidate.validate(&self.install_ref)?;
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION
            || self.candidate.status != BootstrapCandidateStatus::Available
            || self.candidate.kind == BootstrapCandidateKind::NewBrainTarget
            || self.request.candidate_ref != self.candidate.candidate_ref
            || self.request.expected_candidate_digest != self.candidate.candidate_digest
        {
            return Err(crate::Error("invalid bootstrap data intent".into()));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme-bootstrap-data-intent-v2", self))
    }

    pub fn target_ref(&self) -> Result<String> {
        self.validate()?;
        Ok(crate::canonical_digest(&(
            "forme-bootstrap-data-target-v2",
            &self.install_ref,
            &self.request.session.operation_id,
        ))?
        .0[7..]
            .to_owned())
    }
}

/// Actual storage bindings stay on the private pipe, never in setup views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataPreparedResultV2 {
    pub schema_version: SchemaVersion,
    pub prepared: BootstrapClaimDataPreparedV2,
    pub binding: crate::ProductDataBinding,
    pub artifact_digest: SchemaDigest,
    pub verification_digest: SchemaDigest,
}

impl BootstrapDataPreparedResultV2 {
    pub fn worker_digest(&self, intent: &BootstrapDataIntentV2) -> Result<SchemaDigest> {
        crate::canonical_digest(&(
            "forme-bootstrap-worker-receipt-v2",
            intent.digest()?,
            &self.binding,
            &self.artifact_digest,
            &self.verification_digest,
        ))
    }
    pub fn target_digest(&self) -> Result<SchemaDigest> {
        self.binding.validate()?;
        crate::validate_product_digest(&self.artifact_digest)?;
        crate::canonical_digest(&(
            "forme-bootstrap-active-data-v2",
            &self.binding,
            &self.artifact_digest,
        ))
    }

    pub fn validate_for(&self, intent: &BootstrapDataIntentV2) -> Result<()> {
        intent.validate()?;
        self.prepared.validate()?;
        crate::validate_product_digest(&self.verification_digest)?;
        let candidate = &intent.candidate;
        if self.schema_version != BOOTSTRAP_CANDIDATE_SCHEMA_VERSION
            || self.prepared.operation_id != intent.request.session.operation_id
            || self.prepared.candidate_ref != candidate.candidate_ref
            || self.prepared.candidate_digest != candidate.candidate_digest
            || self.prepared.verification_epoch != candidate.verification_epoch
            || self.prepared.migration_outcome != BootstrapMigrationOutcome::Recovered
            || self.prepared.target_binding_digest != self.target_digest()?
            || self.prepared.worker_receipt_digest != self.worker_digest(intent)?
            || self.binding.candidate_ref != candidate.candidate_ref
            || Some(&self.binding.store_identity) != candidate.store_identity_digest.as_ref()
            || self.binding.owner_digest
                != crate::sha256_content_digest(
                    candidate
                        .owner_ref
                        .as_ref()
                        .ok_or_else(|| crate::Error("missing source owner".into()))?
                        .0
                        .as_bytes(),
                )
            || self.binding.execution_fence_digest.is_none()
        {
            return Err(crate::Error(
                "prepared bootstrap result differs from its intent".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case", deny_unknown_fields)]
pub enum BootstrapDataWorkerRequestV2 {
    Prepare {
        schema_version: SchemaVersion,
        intent: BootstrapDataIntentV2,
    },
    Confirm {
        schema_version: SchemaVersion,
        receipt: BootstrapClaimReceiptV2,
        snapshot_ref: String,
        config_binding_digest: SchemaDigest,
    },
    Release {
        schema_version: SchemaVersion,
        operation_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum BootstrapDataWorkerResponseV2 {
    Prepared {
        schema_version: SchemaVersion,
        data: BootstrapDataPreparedResultV2,
    },
    Confirmed {
        schema_version: SchemaVersion,
        confirmation: BootstrapDataBindingConfirmationV2,
    },
    Released {
        schema_version: SchemaVersion,
        operation_id: String,
    },
}
