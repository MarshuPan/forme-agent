//! Authenticated settings v3 uses the same configuration writer and activation.
use super::{
    HostAuth, NativeServiceManager, ProductHostBroker, ISSUE_CORE_RECONCILIATION_UNAVAILABLE,
};
use forme_protocol as p;

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn configuration_view_v3(&self, auth: &HostAuth) -> p::Result<p::ProductConfigViewV3> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?
            .snapshot_settings_v3()
    }

    pub fn apply_configuration_v3(
        &self,
        auth: &HostAuth,
        patch: p::ProductConfigPatchV3,
    ) -> p::Result<p::ProductConfigResponseV3> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let response = {
            let mut store = store
                .lock()
                .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
            if self.journal.operations(0)?.iter().any(|operation| {
                !operation.stage.is_terminal()
                    && self
                        .journal
                        .command_kind(&operation.operation_id)
                        .map_or(true, |kind| kind.is_lifecycle_transaction())
            }) {
                return Err(p::Error(
                    "configuration cannot change during an unresolved lifecycle operation".into(),
                ));
            }
            store.apply_settings_v3(patch)?
        };
        let mut response = response;
        match self.reconcile_core_after_configuration_v3(
            auth,
            response.view.owner.as_ref(),
            response.view.active_revision,
            response.view.configured,
            response.view.pending_restart,
        ) {
            Ok(activation) => response.core_activation = activation,
            Err(_) => response
                .view
                .issue_refs
                .push(ISSUE_CORE_RECONCILIATION_UNAVAILABLE.into()),
        }
        Ok(response)
    }

    fn reconcile_core_after_configuration_v3(
        &self,
        auth: &HostAuth,
        owner: Option<&p::OwnerPrincipalRef>,
        revision: u64,
        configured: bool,
        pending_restart: bool,
    ) -> p::Result<Option<p::ProductCoreActivationResponseV3>> {
        let Some(activation) = self.reconcile_core_after_configuration(
            auth,
            owner,
            revision,
            configured,
            pending_restart,
        )?
        else {
            return Ok(None);
        };
        let store = self.product_config.as_ref().ok_or_else(unavailable)?;
        let configuration = store
            .lock()
            .map_err(|_| unavailable())?
            .snapshot_settings_v3()?;
        Ok(Some(p::ProductCoreActivationResponseV3 {
            schema_version: p::SchemaVersion(1),
            operation: activation.operation,
            configuration,
            service: activation.service,
        }))
    }
}
fn unavailable() -> p::Error {
    p::Error("managed product configuration is unavailable".into())
}
