#![forbid(unsafe_code)]

use forme_protocol as p;

use super::{
    host_configuration::{configuration_unavailable, current_timestamp},
    NativeServiceManager, ProductHostBroker,
};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    /// Reconcile Brain after Host has restored product state.
    pub fn reconcile_core_startup(&self) -> p::Result<()> {
        let result = (|| {
            let store = self
                .product_config
                .as_ref()
                .ok_or_else(configuration_unavailable)?;
            let view = store
                .lock()
                .map_err(|_| configuration_unavailable())?
                .snapshot()?;
            let Some(owner) = view.owner.as_ref() else {
                return Ok(());
            };
            if !view.configured || view.active_revision == 0 {
                return Ok(());
            }
            let Some(bearer) = self.web_configuration_bearer.as_deref() else {
                return Ok(());
            };
            let auth = self.authenticate_web_configuration(bearer)?;
            self.reconcile_core(
                &auth,
                owner,
                view.active_revision,
                view.pending_restart,
                current_timestamp()?,
            )
            .map(|_| ())
        })();
        self.mark_startup_reconciliation_failed(result.is_err());
        result
    }
}
