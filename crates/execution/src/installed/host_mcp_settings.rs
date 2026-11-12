//! Authenticated owner edits preserve the Host lifecycle exclusion.
use super::*;
impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn mcp_settings(&self, auth: &HostAuth) -> p::Result<p::ProductMcpSettingsView> {
        self.verify_web_configuration_auth(auth)?;
        self.product_config
            .as_ref()
            .ok_or_else(unavailable)?
            .lock()
            .map_err(|_| unavailable())?
            .mcp_settings()
    }
    pub fn mcp_settings_operation(
        &self,
        auth: &HostAuth,
        operation: &str,
    ) -> p::Result<p::ProductMcpSettingsOperation> {
        self.verify_web_configuration_auth(auth)?;
        self.product_config
            .as_ref()
            .ok_or_else(unavailable)?
            .lock()
            .map_err(|_| unavailable())?
            .mcp_settings_operation(operation)
    }
    pub fn apply_mcp_settings(
        &self,
        auth: &HostAuth,
        patch: p::ProductMcpSettingsPatch,
    ) -> p::Result<p::ProductMcpSettingsResponse> {
        self.verify_web_configuration_auth(auth)?;
        let mut store = self
            .product_config
            .as_ref()
            .ok_or_else(unavailable)?
            .lock()
            .map_err(|_| unavailable())?;
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
        store.apply_mcp_settings(patch)
    }
}
fn unavailable() -> p::Error {
    p::Error("managed product configuration is unavailable".into())
}
