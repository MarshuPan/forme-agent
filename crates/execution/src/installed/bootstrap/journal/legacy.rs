//! Pure schema-2 claim projection. Replay performs no data or service IO.
use std::collections::{BTreeMap, BTreeSet};

use super::{BootstrapError, BootstrapResult, Code, Fact, Reservation};
use forme_protocol as p;

#[derive(Clone, Default)]
pub(super) struct LegacyClaims {
    claims: BTreeMap<String, Claim>,
    last_epoch: u64,
}

#[derive(Clone, Default)]
struct Claim {
    catalog: Option<p::BootstrapCandidateCatalogV2>,
    selected: Option<p::BootstrapCandidateViewV2>,
    prepared: Option<p::BootstrapClaimDataPreparedV2>,
    commit: Option<p::BootstrapClaimCommitV2>,
    activation: Option<p::BootstrapClaimActivationV2>,
    aborted: Option<String>,
}

impl LegacyClaims {
    pub fn is_committed(&self) -> bool {
        self.claims.values().any(|claim| claim.commit.is_some())
    }

    pub fn has_selection(&self) -> bool {
        self.claims
            .values()
            .any(|claim| claim.selected.is_some() && claim.aborted.is_none())
    }

    pub fn pending_operations(&self) -> BTreeSet<String> {
        self.claims
            .iter()
            .filter(|(_, claim)| claim.prepared.is_some() && claim.activation.is_none())
            .map(|(operation, _)| operation.clone())
            .collect()
    }

    pub fn fold(
        &mut self,
        fact: &Fact,
        install: &str,
        reservation: &Reservation,
        at: i64,
    ) -> BootstrapResult<()> {
        if reservation.selection.is_some() {
            return Err(invalid());
        }
        let operation = match fact {
            Fact::CandidateInventoryObserved { operation, .. }
            | Fact::ClaimAbortedV2 { operation, .. } => operation,
            Fact::CandidateSelectedV2 { selection, .. } => &selection.session.operation_id,
            Fact::ClaimDataPrepared { prepared, .. } => &prepared.operation_id,
            Fact::ClaimCommittedV2 { commit, .. } => &commit.receipt.operation_id,
            Fact::ClaimActivatedV2 { activation, .. } => &activation.operation_id,
            _ => return Err(invalid()),
        };
        p::validate_product_id(operation).map_err(|_| invalid())?;
        if operation != &reservation.operation {
            return Err(invalid());
        }
        let claim = self.claims.entry(operation.clone()).or_default();
        if claim.aborted.is_some() || claim.activation.is_some() {
            return Err(invalid());
        }
        match fact {
            Fact::CandidateInventoryObserved { catalog, .. } => {
                catalog.validate().map_err(|_| invalid())?;
                if claim.selected.is_some()
                    || at >= reservation.expires
                    || catalog.install_ref != install
                    || catalog.verification_epoch <= self.last_epoch
                {
                    return Err(invalid());
                }
                self.last_epoch = catalog.verification_epoch;
                claim.catalog = Some(catalog.clone());
            }
            Fact::CandidateSelectedV2 {
                selection,
                candidate,
                ..
            } => {
                selection.validate().map_err(|_| invalid())?;
                candidate.validate(install).map_err(|_| invalid())?;
                let catalog = claim.catalog.as_ref().ok_or_else(invalid)?;
                if at >= reservation.expires
                    || claim.selected.is_some()
                    || selection.session.setup_digest != reservation.setup
                    || selection.candidate_ref != candidate.candidate_ref
                    || selection.expected_candidate_digest != candidate.candidate_digest
                    || candidate.status != p::BootstrapCandidateStatus::Available
                    || candidate.kind == p::BootstrapCandidateKind::NewBrainTarget
                    || catalog.candidate(&candidate.candidate_ref) != Some(candidate)
                {
                    return Err(invalid());
                }
                claim.selected = Some(candidate.clone());
            }
            Fact::ClaimDataPrepared { prepared, .. } => {
                prepared.validate().map_err(|_| invalid())?;
                let selected = claim.selected.as_ref().ok_or_else(invalid)?;
                if claim.prepared.is_some()
                    || claim.commit.is_some()
                    || prepared.candidate_ref != selected.candidate_ref
                    || prepared.candidate_digest != selected.candidate_digest
                    || prepared.verification_epoch != selected.verification_epoch
                {
                    return Err(invalid());
                }
                claim.prepared = Some(prepared.clone());
            }
            Fact::ClaimCommittedV2 { commit, .. } => {
                let prepared = claim.prepared.as_ref().ok_or_else(invalid)?;
                commit
                    .validate_against_prepared(prepared)
                    .map_err(|_| invalid())?;
                let selected = claim.selected.as_ref().ok_or_else(invalid)?;
                let receipt = &commit.receipt;
                if claim.commit.is_some()
                    || at >= reservation.expires
                    || receipt.install_ref != install
                    || receipt.candidate_kind != selected.kind
                    || receipt.owner_source != selected.owner_source
                    || Some(&receipt.owner_ref) != selected.owner_ref.as_ref()
                    || Some(&receipt.owner_binding_digest) != selected.owner_binding_digest.as_ref()
                    || receipt.expires_at != at.checked_add(5 * 60 * 1000).ok_or_else(invalid)?
                {
                    return Err(invalid());
                }
                claim.commit = Some(commit.clone());
            }
            Fact::ClaimActivatedV2 { activation, .. } => {
                activation
                    .validate_against_commit(claim.commit.as_ref().ok_or_else(invalid)?)
                    .map_err(|_| invalid())?;
                claim.activation = Some(activation.clone());
            }
            Fact::ClaimAbortedV2 {
                candidate_ref,
                candidate_digest,
                issue_ref,
                ..
            } => {
                p::validate_product_id(issue_ref).map_err(|_| invalid())?;
                let selected = claim.selected.as_ref().ok_or_else(invalid)?;
                if claim.prepared.is_some()
                    || claim.commit.is_some()
                    || candidate_ref != &selected.candidate_ref
                    || candidate_digest != &selected.candidate_digest
                {
                    return Err(invalid());
                }
                claim.aborted = Some(issue_ref.clone());
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }

    pub fn view(
        &self,
        install: &str,
        operation: &str,
        recovered: bool,
    ) -> BootstrapResult<p::BootstrapClaimViewV2> {
        let claim = self.claims.get(operation);
        let selected = claim.and_then(|c| c.selected.as_ref());
        let receipt = claim
            .and_then(|c| c.commit.as_ref())
            .map(|c| c.receipt.clone());
        let activation = claim.and_then(|c| c.activation.clone());
        let aborted = claim.and_then(|c| c.aborted.clone());
        let (phase, issue_ref) = if activation.is_some() {
            (p::BootstrapClaimPhaseV2::Activated, None)
        } else if let Some(issue) = aborted {
            (p::BootstrapClaimPhaseV2::Aborted, Some(issue))
        } else if recovered {
            (
                p::BootstrapClaimPhaseV2::RecoveryRequired,
                Some("issue:bootstrap-claim-interrupted".into()),
            )
        } else if receipt.is_some() {
            (p::BootstrapClaimPhaseV2::Committed, None)
        } else if claim.is_some_and(|c| c.prepared.is_some()) {
            (p::BootstrapClaimPhaseV2::DataPrepared, None)
        } else if selected.is_some() {
            (p::BootstrapClaimPhaseV2::Selected, None)
        } else {
            (p::BootstrapClaimPhaseV2::Observed, None)
        };
        let view = p::BootstrapClaimViewV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: install.into(),
            operation_id: operation.into(),
            phase,
            candidate_ref: selected.map(|c| c.candidate_ref.clone()),
            candidate_digest: selected.map(|c| c.candidate_digest.clone()),
            verification_epoch: selected.map(|c| c.verification_epoch),
            receipt,
            active_data_binding_digest: activation
                .as_ref()
                .map(|a| a.active_data_binding_digest.clone()),
            activation,
            issue_ref,
        };
        view.validate().map_err(|_| invalid())?;
        Ok(view)
    }
}

fn invalid() -> BootstrapError {
    BootstrapError(Code::RecoveryRequired)
}
