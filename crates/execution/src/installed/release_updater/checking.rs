use super::*;
use crate::installed::ProductConfigStore;

impl InstalledReleaseUpdater {
    pub(crate) fn reconcile(
        &self,
        journal: &ProductJournal,
        config: &mut ProductConfigStore,
    ) -> p::Result<()> {
        for operation in journal.operations(0)? {
            let Some(checking) = journal.release_checking(&operation.operation_id)? else {
                continue;
            };
            if operation.stage == p::MaintenanceStage::RecoveryRequired {
                continue;
            }
            if operation.stage == p::MaintenanceStage::Committed {
                if config.release_binding() == Some(&checking.binding) {
                    if config.release_is_open(&checking.binding).unwrap_or(false) {
                        continue;
                    }
                    // A lost post-commit release can only be repaired by new
                    // live proof of this same committed binding; no OS effects.
                    if self.verify(journal, config, &checking, None).is_ok() {
                        config.release_work_admission(&checking.binding, now()?)?;
                    }
                }
                continue;
            }
            let result = deadline(checking.takeover.deadline)
                .and_then(|limit| self.verify(journal, config, &checking, Some(limit)));
            match result {
                Ok(proof) => {
                    if operation.stage == p::MaintenanceStage::Switching
                        && operation.pending_stage == Some(p::MaintenanceStage::Checking)
                    {
                        journal.finish_stage(
                            &operation.operation_id,
                            journal.revision()?,
                            p::MaintenanceStage::Checking,
                            vec![p::canonical_digest(&proof)?],
                            now()?,
                        )?;
                    } else if operation.stage != p::MaintenanceStage::Checking
                        || !matches!(
                            operation.pending_stage,
                            None | Some(p::MaintenanceStage::Committed)
                        )
                    {
                        return Err(invalid());
                    }
                    if now()? >= checking.takeover.deadline {
                        return Err(invalid());
                    }
                    if journal
                        .operation(&operation.operation_id)?
                        .pending_stage
                        .is_none()
                    {
                        journal.begin_stage(
                            &operation.operation_id,
                            journal.revision()?,
                            p::MaintenanceStage::Committed,
                            p::MaintenanceActorClass::OwnerReauthenticated,
                            now()?,
                        )?;
                    }
                    journal.finish_stage(
                        &operation.operation_id,
                        journal.revision()?,
                        p::MaintenanceStage::Committed,
                        vec![p::canonical_digest(&("release-verified-commit-v1", proof))?],
                        now()?,
                    )?;
                    config.release_work_admission(&checking.binding, now()?)?;
                }
                Err(error) if now()? >= checking.takeover.deadline => {
                    journal.require_recovery(
                        &operation.operation_id,
                        journal.revision()?,
                        p::canonical_digest(&error.to_string())?,
                        now()?,
                    )?;
                }
                Err(_) => {} // Booting dependencies are not readiness. Wait, without any repeated action.
            }
        }
        Ok(())
    }
    fn verify(
        &self,
        journal: &ProductJournal,
        config: &mut ProductConfigStore,
        checking: &p::ProductReleaseChecking,
        limit: Option<Instant>,
    ) -> p::Result<p::ProductReleaseReadiness> {
        checking.validate()?;
        let takeover = &checking.takeover;
        if takeover.installation_digest != self.installation.digest()?
            || journal
                .release_takeover(&takeover.release.operation_id)?
                .as_ref()
                != Some(takeover)
            || journal.active_release()?
                != Some((
                    takeover.release.target_release_ref.clone(),
                    takeover.release.target_release_digest.clone(),
                ))
            || config.release_binding() != Some(&checking.binding)
        {
            return Err(invalid());
        }
        let target = self.target(journal, &takeover.release)?;
        let root = target.content_root();
        let host = root.join(if cfg!(windows) {
            "bin/forme-hostd.exe"
        } else {
            "bin/forme-hostd"
        });
        self.platform.verify_host_image(
            &host,
            &checking
                .service_receipt
                .images
                .iter()
                .find(|r| r.service == p::BuiltinServiceId::Host)
                .ok_or_else(invalid)?
                .target_image_digest,
        )?;
        for id in services() {
            self.platform.observe(
                &self.installation,
                root,
                id,
                Some(p::NativeServiceState::Running),
            )?;
        }
        // An uncommitted operation retains its original absolute deadline.
        // After commit, the fresh network proof gets its own bounded window;
        // read-only release/image checks must not consume that window first.
        let probe_deadline = limit.unwrap_or_else(|| Instant::now() + Duration::from_secs(2));
        let ready = config.live_release_readiness(&checking.binding, probe_deadline)?;
        if ready.data_identity.owner_digest != checking.data_identity.owner_digest
            || ready.data_identity.store_identity != checking.data_identity.store_identity
        {
            return Err(invalid());
        }
        target.recheck()?;
        Ok(ready)
    }
}
