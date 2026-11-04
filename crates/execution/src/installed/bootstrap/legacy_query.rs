//! Read only the original authenticated setup operation. No worker dispatch.
use super::*;

impl BootstrapAuthority {
    /// Reconstructs a safe view from the claim journal, without interpreting a
    /// commit as data activation. After restart a prepared, unactivated claim
    /// requires explicit recovery; this query never completes it.
    pub fn claim_state_v2(
        &self,
        auth: &BootstrapServiceAuth,
        query: p::BootstrapClaimQueryV2,
        now: i64,
    ) -> BootstrapResult<p::BootstrapClaimViewV2> {
        self.check_service(auth)?;
        query
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let binding = p::BootstrapSessionBinding {
            schema_version: p::SchemaVersion(1),
            operation_id: query.session.operation_id,
            setup_digest: query.session.setup_digest,
        };
        let journal = self.lock(now)?;
        self.check_session(&journal, &binding, now, true)?;
        journal.claim_view_v2(&binding.operation_id)
    }
}
