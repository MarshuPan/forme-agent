//! Late receipt acceptance shares the authority lock with revocation.
use super::*;
impl DesktopAuthority {
    pub(crate) fn with_current_consumed<T>(
        &self,
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
        complete: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        binding.validate_at(now)?;
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let grant = writer
            .state
            .grants
            .values()
            .find(|grant| grant.binding.grant_ref == binding.grant_ref)
            .ok_or_else(invalid)?;
        if !grant.consumed || grant.binding != *binding {
            return Err(invalid());
        }
        let consent = writer
            .state
            .consents
            .get(&grant.consent_ref)
            .ok_or_else(invalid)?;
        if consent.revoked || consent.request.expires_at <= now {
            return Err(invalid());
        }
        self.provider.recheck(&consent.selected)?;
        // Keep the read guard through Host seal/receipt acceptance. A revoke
        // that already committed wins; a later revoke cannot rewrite history.
        complete()
    }
}
