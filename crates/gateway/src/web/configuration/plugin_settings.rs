//! Finite configuration routes, separate from discovery and worker execution.
use super::*;
const ROOT: &str = "/v1/host/configuration/plugin/settings";
impl LocalConfigurationClient {
    pub(in crate::web) fn plugin_settings(&self) -> p::Result<p::ProductPluginSettingsView> {
        self.request("GET", ROOT, None::<&()>)
    }
    pub(in crate::web) fn plugin_settings_operation(
        &self,
        operation: &str,
    ) -> p::Result<p::ProductPluginSettingsOperation> {
        p::validate_product_id(operation)?;
        self.request(
            "GET",
            &format!("{ROOT}/operations/{operation}"),
            None::<&()>,
        )
    }
    pub(in crate::web) fn apply_plugin_settings(
        &self,
        patch: p::ProductPluginSettingsPatch,
    ) -> p::Result<p::ProductPluginSettingsResponse> {
        patch.validate()?;
        self.request("POST", ROOT, Some(&patch))
    }
}
pub(super) fn allows_route(method: &str, path: &str) -> bool {
    matches!((method, path), ("GET" | "POST", ROOT))
        || (method == "GET"
            && path
                .strip_prefix("/v1/host/configuration/plugin/settings/operations/")
                .is_some_and(|id| p::validate_product_id(id).is_ok()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_routes_are_closed_and_do_not_grant_execution() {
        assert!(allows_route("POST", ROOT));
        assert!(allows_route("GET", &format!("{ROOT}/operations/save:one")));
        for path in [
            format!("{ROOT}/execute"),
            format!("{ROOT}/operations/"),
            format!("{ROOT}/operations/a/b"),
            format!("{ROOT}?owner=other"),
            "/v1/agent/workers/dispatch".into(),
        ] {
            assert!(!allows_route("GET", &path));
            assert!(!allows_route("POST", &path));
        }
        assert!(!allows_route(
            "POST",
            &format!("{ROOT}/operations/save:one")
        ));
    }
}
