//! Journal-backed active release identity, independent from lifecycle execution.
use super::{ActiveReleaseState, ProductHostBroker};
use crate::installed::NativeServiceManager;
use forme_protocol as p;

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn record_installation_verification(
        &self,
        manifest_digest: p::SchemaDigest,
        release_digest: p::SchemaDigest,
    ) -> p::Result<u64> {
        if let Some(worker) = &self.restore_worker {
            if worker.installation().digest()? != manifest_digest {
                return Err(p::Error(
                    "restore worker has a different installation manifest".into(),
                ));
            }
        }
        if let Some(stager) = &self.release_stager {
            if stager.installation_digest()? != manifest_digest {
                return Err(p::Error(
                    "release worker has a different installation manifest".into(),
                ));
            }
        }
        if let Some(worker) = &self.release_worker {
            if worker.installation().digest()? != manifest_digest {
                return Err(p::Error(
                    "updater has a different installation manifest".into(),
                ));
            }
        }
        let mut active = self
            .active_release
            .lock()
            .map_err(|_| p::Error("active release state is unavailable".into()))?;
        let active_digest = active
            .digest
            .clone()
            .unwrap_or_else(|| release_digest.clone());
        let revision = self
            .journal
            .record_installation_verification_with_active_release(
                manifest_digest,
                release_digest,
                Some((&active.id, &active_digest)),
            )?;
        active.digest = Some(active_digest);
        Ok(revision)
    }

    pub(super) fn active_release_state(&self) -> p::Result<ActiveReleaseState> {
        self.active_release
            .lock()
            .map(|state| state.clone())
            .map_err(|_| p::Error("active release state is unavailable".into()))
    }

    pub(super) fn active_release_id(&self) -> p::Result<String> {
        Ok(self.active_release_state()?.id)
    }

    /// Publish the active release only after the journal has accepted the
    /// Switching-stage transition. The in-memory projection is never the
    /// source of truth and is rebuilt from the journal on restart.
    pub(super) fn set_active_release(
        &self,
        operation_id: &str,
        from_release: &str,
        from_digest: &p::SchemaDigest,
        to_release: &str,
        to_digest: &p::SchemaDigest,
    ) -> p::Result<()> {
        let mut active = self
            .active_release
            .lock()
            .map_err(|_| p::Error("active release state is unavailable".into()))?;
        if active.id != from_release || active.digest.as_ref() != Some(from_digest) {
            return Err(p::Error(
                "active release changed before the switch was committed".into(),
            ));
        }
        let expected = self.journal.revision()?;
        self.journal.change_active_release(
            operation_id,
            expected,
            from_release,
            from_digest,
            to_release,
            to_digest,
        )?;
        active.id = to_release.into();
        active.digest = Some(to_digest.clone());
        Ok(())
    }
}
