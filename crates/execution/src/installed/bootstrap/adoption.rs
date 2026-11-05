//! One Host-owned config -> claim transaction. Execution never opens brain data.
use super::legacy_candidates::session_v1;
use super::*;
use crate::installed::ProductConfigStore;
use std::time::{Duration, Instant};

impl BootstrapAuthority {
    pub(crate) fn adopt_data(
        &self,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapClaimRequestV2,
        now: i64,
        config: &mut ProductConfigStore,
    ) -> BootstrapResult<p::SignedBootstrapClaimActivationV2> {
        self.check_service(auth)?;
        request
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let started = Instant::now();
        let mut journal = self.lock_after_wait(now, started)?;
        let operation = &request.session.operation_id;
        self.check_session(
            &journal,
            &session_v1(&request.session),
            completed_at(now, started)?,
            true,
        )?;
        let view = journal.claim_view_v2(operation)?;
        if view.phase == p::BootstrapClaimPhaseV2::Activated {
            if journal
                .state
                .legacy_intent(operation)
                .is_none_or(|original| original.request != request)
            {
                return Err(BootstrapError(Code::Conflict));
            }
            return self.sign_activation(view);
        }
        if journal.state.legacy_intent(operation).is_some()
            || view.phase != p::BootstrapClaimPhaseV2::Selected
        {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        self.check_session(
            &journal,
            &session_v1(&request.session),
            completed_at(now, started)?,
            false,
        )?;
        let candidate = journal
            .state
            .candidate_catalog(operation)
            .and_then(|catalog| catalog.candidate(&request.candidate_ref))
            .cloned()
            .ok_or(BootstrapError(Code::Conflict))?;
        if view.candidate_ref.as_ref() != Some(&request.candidate_ref)
            || candidate.candidate_digest != request.expected_candidate_digest
        {
            return Err(BootstrapError(Code::Conflict));
        }
        let intent = p::BootstrapDataIntentV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: self.config.install_ref.clone(),
            request,
            candidate,
        };
        intent
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        config
            .bootstrap_admission(&self.config.install_ref)
            .map_err(unavailable)?;
        let program = self
            .candidate_program
            .as_ref()
            .ok_or(BootstrapError(Code::Unavailable))?;
        journal.append(Fact::ClaimDataIntentV2 {
            intent: intent.clone(),
            at: completed_at(now, started)?,
        })?;
        let operation = intent.request.session.operation_id.clone();
        let result = self.complete_adoption(&mut journal, config, program, &intent, now, started);
        if result.is_err() {
            journal.require_recovery(&operation);
        }
        result
    }

    fn complete_adoption(
        &self,
        journal: &mut Journal,
        config: &mut ProductConfigStore,
        program: &super::super::maintenance_pipe::ReleaseDataProgram,
        intent: &p::BootstrapDataIntentV2,
        now: i64,
        started: Instant,
    ) -> BootstrapResult<p::SignedBootstrapClaimActivationV2> {
        let mut pipe = program
            .spawn_bootstrap_data(Instant::now() + Duration::from_secs(120))
            .map_err(unavailable)?;
        let response = pipe
            .exchange(&p::BootstrapDataWorkerRequestV2::Prepare {
                schema_version: p::SchemaVersion(2),
                intent: intent.clone(),
            })
            .map_err(unavailable)?;
        let p::BootstrapDataWorkerResponseV2::Prepared {
            schema_version: p::SchemaVersion(2),
            data,
        } = response
        else {
            return Err(BootstrapError(Code::RecoveryRequired));
        };
        data.validate_for(intent).map_err(unavailable)?;
        let at = completed_at(now, started)?;
        self.check_session(journal, &session_v1(&intent.request.session), at, false)?;
        journal.append(Fact::ClaimDataPrepared {
            prepared: data.prepared.clone(),
            at,
        })?;
        let candidate = &intent.candidate;
        let receipt = p::BootstrapClaimReceiptV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: intent.install_ref.clone(),
            operation_id: intent.request.session.operation_id.clone(),
            candidate_ref: candidate.candidate_ref.clone(),
            candidate_kind: candidate.kind,
            candidate_digest: candidate.candidate_digest.clone(),
            owner_source: candidate.owner_source,
            owner_ref: candidate
                .owner_ref
                .clone()
                .ok_or(BootstrapError(Code::RecoveryRequired))?,
            owner_binding_digest: candidate
                .owner_binding_digest
                .clone()
                .ok_or(BootstrapError(Code::RecoveryRequired))?,
            account_commitment: intent.request.account_commitment.clone(),
            target_binding_digest: data.target_digest().map_err(unavailable)?,
            migration_outcome: p::BootstrapMigrationOutcome::Recovered,
            expires_at: at
                .checked_add(5 * 60 * 1000)
                .ok_or(BootstrapError(Code::InvalidInput))?,
        };
        let commit = p::BootstrapClaimCommitV2 {
            schema_version: p::SchemaVersion(2),
            receipt,
        };
        journal.append(Fact::ClaimCommittedV2 {
            commit: commit.clone(),
            at,
        })?;
        let (snapshot_ref, config_digest) = config
            .publish_bootstrap_data(intent, &data, &commit)
            .map_err(unavailable)?;
        let response = pipe
            .exchange(&p::BootstrapDataWorkerRequestV2::Confirm {
                schema_version: p::SchemaVersion(2),
                receipt: commit.receipt.clone(),
                snapshot_ref,
                config_binding_digest: config_digest.clone(),
            })
            .map_err(unavailable)?;
        let p::BootstrapDataWorkerResponseV2::Confirmed {
            schema_version: p::SchemaVersion(2),
            confirmation,
        } = response
        else {
            return Err(BootstrapError(Code::RecoveryRequired));
        };
        confirmation
            .validate_against_receipt(&commit.receipt)
            .map_err(unavailable)?;
        if confirmation.config_binding_digest != config_digest
            || Some(&confirmation.execution_fence_digest)
                != data.binding.execution_fence_digest.as_ref()
            || confirmation.verification_digest != data.verification_digest
        {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let at = completed_at(now, started)?;
        journal.append(Fact::ClaimDataBindingConfirmedV2 { confirmation, at })?;
        journal.append(Fact::ClaimActivatedV2 {
            activation: p::BootstrapClaimActivationV2 {
                schema_version: p::SchemaVersion(2),
                operation_id: commit.receipt.operation_id.clone(),
                receipt_digest: commit.receipt.digest().map_err(unavailable)?,
                active_data_binding_digest: commit.receipt.target_binding_digest,
            },
            at,
        })?;
        let response = pipe
            .exchange(&p::BootstrapDataWorkerRequestV2::Release {
                schema_version: p::SchemaVersion(2),
                operation_id: commit.receipt.operation_id.clone(),
            })
            .map_err(unavailable)?;
        if !matches!(response, p::BootstrapDataWorkerResponseV2::Released { schema_version: p::SchemaVersion(2), ref operation_id } if operation_id == &commit.receipt.operation_id)
        {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        pipe.finish().map_err(unavailable)?;
        self.sign_activation(journal.claim_view_v2(&commit.receipt.operation_id)?)
    }

    pub(super) fn sign_activation(
        &self,
        view: p::BootstrapClaimViewV2,
    ) -> BootstrapResult<p::SignedBootstrapClaimActivationV2> {
        if view.phase != p::BootstrapClaimPhaseV2::Activated {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let receipt = view.receipt.ok_or(BootstrapError(Code::RecoveryRequired))?;
        let activation = view
            .activation
            .ok_or(BootstrapError(Code::RecoveryRequired))?;
        let message = p::SignedBootstrapClaimActivationV2::signing_digest(&receipt, &activation)
            .map_err(unavailable)?;
        Ok(p::SignedBootstrapClaimActivationV2 {
            schema_version: p::SchemaVersion(2),
            receipt,
            activation,
            signature: self
                .signing_key
                .sign(message.0.as_bytes())
                .to_bytes()
                .to_vec(),
        })
    }
}

fn completed_at(now: i64, started: Instant) -> BootstrapResult<i64> {
    now.checked_add(
        i64::try_from(started.elapsed().as_millis())
            .map_err(|_| BootstrapError(Code::InvalidInput))?,
    )
    .ok_or(BootstrapError(Code::InvalidInput))
}
fn unavailable(_: p::Error) -> BootstrapError {
    BootstrapError(Code::RecoveryRequired)
}
