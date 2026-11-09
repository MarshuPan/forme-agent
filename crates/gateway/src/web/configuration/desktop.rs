//! Owner desktop controls cannot issue worker grants or expose helper bindings.
use super::*;
impl LocalConfigurationClient {
    pub(in crate::web) fn computer_settings(&self) -> p::Result<p::ProductComputerSettingsView> {
        self.request(
            "GET",
            "/v1/host/configuration/desktop/settings",
            None::<&()>,
        )
    }
    pub(in crate::web) fn computer_settings_operation(
        &self,
        operation: &str,
    ) -> p::Result<p::ProductComputerSettingsOperation> {
        p::validate_product_id(operation)?;
        self.request(
            "GET",
            &format!("/v1/host/configuration/desktop/settings/operations/{operation}"),
            None::<&()>,
        )
    }
    pub(in crate::web) fn apply_computer_settings(
        &self,
        patch: p::ProductComputerSettingsPatch,
    ) -> p::Result<p::ProductComputerSettingsResponse> {
        patch.validate()?;
        self.request(
            "POST",
            "/v1/host/configuration/desktop/settings",
            Some(&patch),
        )
    }

    pub(in crate::web) fn desktop_sessions(&self) -> p::Result<p::DesktopSessionCatalog> {
        self.request(
            "GET",
            "/v1/host/configuration/desktop/sessions",
            None::<&()>,
        )
    }
    pub(in crate::web) fn desktop_consent(
        &self,
        request: p::DesktopConsentRequest,
    ) -> p::Result<p::DesktopConsentView> {
        request.validate()?;
        self.request(
            "POST",
            "/v1/host/configuration/desktop/consents",
            Some(&request),
        )
    }
    pub(in crate::web) fn desktop_consent_operation(
        &self,
        id: &str,
    ) -> p::Result<p::DesktopConsentView> {
        p::validate_product_id(id)?;
        self.request(
            "GET",
            &format!("/v1/host/configuration/desktop/consents/{id}"),
            None::<&()>,
        )
    }
    pub(in crate::web) fn revoke_desktop(
        &self,
        request: p::DesktopRevokeRequest,
    ) -> p::Result<p::DesktopConsentView> {
        request.validate()?;
        self.request(
            "POST",
            "/v1/host/configuration/desktop/revocations",
            Some(&request),
        )
    }
}

/// Explicit Configuration routes only; never forwards AgentExecution authority.
pub(super) fn allows_route(method: &str, path: &str) -> bool {
    matches!(
        (method, path),
        ("GET" | "POST", "/v1/host/configuration/desktop/settings")
            | ("GET", "/v1/host/configuration/desktop/sessions")
            | ("POST", "/v1/host/configuration/desktop/consents")
            | ("POST", "/v1/host/configuration/desktop/revocations")
    ) || (method == "GET"
        && path
            .strip_prefix("/v1/host/configuration/desktop/consents/")
            .or_else(|| path.strip_prefix("/v1/host/configuration/desktop/settings/operations/"))
            .is_some_and(|id| p::validate_product_id(id).is_ok()))
}

#[cfg(test)]
mod tests {
    use super::allows_route;
    #[test]
    fn desktop_allowlist_preserves_methods_domains_and_closed_operation_paths() {
        for path in [
            "/v1/host/configuration/desktop/settings",
            "/v1/host/configuration/desktop/sessions",
            "/v1/host/configuration/desktop/consents/consent:one",
            "/v1/host/configuration/desktop/settings/operations/save:one",
        ] {
            assert!(allows_route("GET", path));
            assert!(!allows_route("DELETE", path));
        }
        for path in [
            "/v1/host/configuration/desktop/execute",
            "/v1/host/configuration/desktop/grants",
            "/v1/agent/workers/desktop/grants",
            "/v1/host/configuration/desktop/consents/a/b",
            "/v1/host/configuration/desktop/settings/operations/..",
            "/v1/host/configuration/desktop/settings/operations/",
            "/v1/host/configuration/desktop/settings?owner=other",
        ] {
            assert!(!allows_route("GET", path));
            assert!(!allows_route("POST", path));
        }
        assert!(!allows_route(
            "POST",
            "/v1/host/configuration/desktop/sessions"
        ));
        assert!(!allows_route(
            "POST",
            "/v1/host/configuration/desktop/consents/original"
        ));
    }
}
