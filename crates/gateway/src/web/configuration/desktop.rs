//! Owner desktop controls cannot issue worker grants or expose helper bindings.
use super::*;
impl LocalConfigurationClient {
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
