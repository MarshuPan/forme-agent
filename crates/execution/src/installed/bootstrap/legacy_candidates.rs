//! Journal-owned observations and exact selection, without any data effects.
use super::*;
use std::time::{Duration, Instant};

impl BootstrapAuthority {
    /// Attach only the installer-derived fixed worker before sharing authority.
    /// This neither opens a source nor certifies native service/ACL acceptance.
    pub fn with_installed_candidates(
        mut self,
        installation: p::InstallationManifest,
    ) -> BootstrapResult<Self> {
        if installation.install_id != self.config.install_ref
            || self.config.journal_path != installation.paths.host_state.join("bootstrap.journal")
            || self.config.signing_key_path != installation.paths.keys.join("bootstrap.key")
            || self.config.web_credential_path
                != installation.paths.web_auth.join("bootstrap-web.token")
            || self.config.new_brain_directory != installation.paths.brain
            || self
                .config
                .historical_brain_directory
                .as_ref()
                .is_some_and(|path| *path != installation.paths.backups.join("historical-brains"))
            || self
                .config
                .verified_backup_directory
                .as_ref()
                .is_some_and(|path| *path != installation.paths.backups.join("candidates"))
        {
            return Err(BootstrapError(Code::InvalidInput));
        }
        self.candidate_program = Some(
            super::super::maintenance_pipe::ReleaseDataProgram::new(installation)
                .map_err(|_| BootstrapError(Code::Unavailable))?,
        );
        Ok(self)
    }

    /// Allocate the epoch under the sole journal writer and persist only an
    /// actual offline-worker observation. Browser digests/flags are not proof.
    pub fn candidate_catalog_v2(
        &self,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapCandidateCatalogRequestV2,
        now: i64,
    ) -> BootstrapResult<p::BootstrapCandidateCatalogV2> {
        let started = Instant::now();
        self.check_service(auth)?;
        request
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let mut journal = self.lock_after_wait(now, started)?;
        let session = session_v1(&request.session);
        let reservation =
            self.check_session(&journal, &session, completed_at(now, started)?, false)?;
        if journal.state.has_legacy_selection() {
            // A lost selection response may be recovered by reading its exact
            // original observation. Never re-observe a selected source or turn
            // a prepared/interrupted transaction into a fresh choice.
            if journal.claim_view_v2(&session.operation_id)?.phase
                != p::BootstrapClaimPhaseV2::Selected
            {
                return Err(BootstrapError(Code::RecoveryRequired));
            }
            return journal
                .state
                .candidate_catalog(&session.operation_id)
                .cloned()
                .ok_or(BootstrapError(Code::RecoveryRequired));
        }
        if reservation.selection.is_some() {
            // The original schema-1 choice is already immutable. Refresh may
            // read its original observation, but cannot discover/choose again.
            self.config.check_empty_target()?;
            return journal
                .state
                .candidate_catalog(&session.operation_id)
                .cloned()
                .ok_or(BootstrapError(Code::Conflict));
        }
        let epoch = journal.state.next_verification_epoch()?;
        let catalog =
            if self.candidate_program.is_none() && self.config.candidate_roots().next().is_none() {
                // Legacy schema-1 installations have no historical source roots.
                // Observe only their existing checked empty target. Never use this
                // branch after an installed worker or source verification fails.
                self.new_target_catalog(epoch)?
            } else {
                self.inspect_catalog(&request, epoch)?
            };
        let completed = completed_at(now, started)?;
        self.check_session(&journal, &session, completed, false)?;
        journal.append(Fact::CandidateInventoryObserved {
            operation: session.operation_id,
            catalog: catalog.clone(),
            at: completed,
        })?;
        Ok(catalog)
    }

    fn new_target_catalog(&self, epoch: u64) -> BootstrapResult<p::BootstrapCandidateCatalogV2> {
        let empty = self.config.check_empty_target().is_ok();
        let mut candidate = p::BootstrapCandidateViewV2 {
            schema_version: p::SchemaVersion(2),
            candidate_ref: self.config.candidate_ref.clone(),
            kind: p::BootstrapCandidateKind::NewBrainTarget,
            status: if empty {
                p::BootstrapCandidateStatus::Available
            } else {
                p::BootstrapCandidateStatus::Unverifiable
            },
            candidate_digest: p::sha256_content_digest(b"pending"),
            verification_epoch: epoch,
            owner_source: p::BootstrapOwnerSource::GeneratedForNewBrain,
            owner_ref: None,
            owner_binding_digest: None,
            store_identity_digest: None,
            schema_digest: None,
            protection_state: if empty {
                p::BootstrapProtectionState::Protected
            } else {
                p::BootstrapProtectionState::Unknown
            },
            source_observation_digest: p::canonical_digest(&(
                "forme-bootstrap-empty-target-v2",
                empty,
            ))
            .map_err(|_| BootstrapError(Code::Unavailable))?,
            issue_ref: (!empty).then(|| "issue:bootstrap-source-unverified".into()),
        };
        candidate.candidate_digest = candidate
            .expected_digest(&self.config.install_ref)
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        let mut catalog = p::BootstrapCandidateCatalogV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: self.config.install_ref.clone(),
            verification_epoch: epoch,
            candidates: vec![candidate],
            catalog_digest: p::sha256_content_digest(b"pending"),
        };
        catalog.catalog_digest = catalog
            .expected_digest()
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        catalog
            .validate()
            .map_err(|_| BootstrapError(Code::RecoveryRequired))?;
        Ok(catalog)
    }

    /// Re-inspect the same source under a fresh real lock but the original
    /// epoch. Drift refuses selection; a duplicate never appends a second fact.
    pub fn select_candidate_v2(
        &self,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapCandidateSelectionV2,
        now: i64,
    ) -> BootstrapResult<p::BootstrapCandidateViewV2> {
        let started = Instant::now();
        self.check_service(auth)?;
        request
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let mut journal = self.lock_after_wait(now, started)?;
        let session = session_v1(&request.session);
        let reservation =
            self.check_session(&journal, &session, completed_at(now, started)?, false)?;
        if reservation.selection.is_some() {
            return Err(BootstrapError(Code::Conflict));
        }
        let selected = journal.claim_view_v2(&session.operation_id)?;
        if !matches!(
            selected.phase,
            p::BootstrapClaimPhaseV2::Observed | p::BootstrapClaimPhaseV2::Selected
        ) {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let candidate = journal
            .state
            .candidate_catalog(&session.operation_id)
            .and_then(|catalog| catalog.candidate(&request.candidate_ref))
            .cloned()
            .ok_or(BootstrapError(Code::Conflict))?;
        if candidate.status != p::BootstrapCandidateStatus::Available
            || candidate.kind == p::BootstrapCandidateKind::NewBrainTarget
            || candidate.candidate_digest != request.expected_candidate_digest
            || selected
                .candidate_ref
                .as_ref()
                .is_some_and(|reference| reference != &request.candidate_ref)
        {
            return Err(BootstrapError(Code::Conflict));
        }
        let current = self.inspect_catalog(
            &p::BootstrapCandidateCatalogRequestV2 {
                schema_version: p::SchemaVersion(2),
                session: request.session.clone(),
            },
            candidate.verification_epoch,
        )?;
        if current.candidate(&candidate.candidate_ref) != Some(&candidate) {
            return Err(BootstrapError(Code::Conflict));
        }
        let completed = completed_at(now, started)?;
        self.check_session(&journal, &session, completed, false)?;
        if selected.phase == p::BootstrapClaimPhaseV2::Observed {
            journal.append(Fact::CandidateSelectedV2 {
                selection: request,
                candidate: candidate.clone(),
                at: completed,
            })?;
        }
        Ok(candidate)
    }

    fn inspect_catalog(
        &self,
        request: &p::BootstrapCandidateCatalogRequestV2,
        epoch: u64,
    ) -> BootstrapResult<p::BootstrapCandidateCatalogV2> {
        self.candidate_program
            .as_ref()
            .ok_or(BootstrapError(Code::Unavailable))?
            .inspect_bootstrap_candidates(request, epoch, Instant::now() + Duration::from_secs(30))
            .map_err(|_| BootstrapError(Code::DataUnavailable))
    }

    pub(super) fn lock_after_wait(
        &self,
        now: i64,
        started: Instant,
    ) -> BootstrapResult<MutexGuard<'_, Journal>> {
        let journal = self
            .journal
            .lock()
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        journal.ensure_healthy()?;
        let observed = completed_at(now, started)?;
        if now <= 0 || observed < journal.state.last_at {
            return Err(BootstrapError(Code::InvalidInput));
        }
        Ok(journal)
    }
}

fn completed_at(now: i64, started: Instant) -> BootstrapResult<i64> {
    now.checked_add(
        i64::try_from(started.elapsed().as_millis())
            .map_err(|_| BootstrapError(Code::InvalidInput))?,
    )
    .ok_or(BootstrapError(Code::InvalidInput))
}

pub(super) fn session_v1(session: &p::BootstrapSessionBindingV2) -> p::BootstrapSessionBinding {
    p::BootstrapSessionBinding {
        schema_version: p::SchemaVersion(1),
        operation_id: session.operation_id.clone(),
        setup_digest: session.setup_digest.clone(),
    }
}
