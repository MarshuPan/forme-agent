//! Read only the original authenticated setup operation. No worker dispatch.
use super::*;

impl BootstrapAuthority {
    /// Read-only recovery of an already activated original operation. This
    /// never invokes a data worker, changes expiry, or binds a new account.
    pub fn claim_activation_v2(
        &self,
        auth: &BootstrapServiceAuth,
        query: p::BootstrapClaimQueryV2,
        now: i64,
    ) -> BootstrapResult<p::SignedBootstrapClaimActivationV2> {
        let view = self.claim_state_v2(auth, query, now)?;
        self.sign_activation(view)
    }
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
        let original = journal
            .state
            .reservation
            .as_ref()
            .ok_or(BootstrapError(Code::Unauthenticated))?;
        if original.operation != binding.operation_id || original.setup != binding.setup_digest {
            return Err(BootstrapError(Code::Conflict));
        }
        if journal.state.legacy_intent(&binding.operation_id).is_none() {
            self.check_session(&journal, &binding, now, true)?;
        }
        journal.claim_view_v2(&binding.operation_id)
    }
}
