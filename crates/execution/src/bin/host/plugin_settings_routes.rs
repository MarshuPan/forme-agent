//! Reauthenticated plugin configuration, without execution-domain authority.
use super::{http::Request, EndpointDomain};
use forme_execution::installed::{HostAuth, ProductHostBroker};
use forme_protocol as p;
const ROOT: &str = "/v1/host/configuration/plugin/settings";
fn operation(path: &str) -> Option<&str> {
    path.strip_prefix("/v1/host/configuration/plugin/settings/operations/")
        .filter(|id| p::validate_product_id(id).is_ok())
}
pub(super) fn is_path(path: &str) -> bool {
    path == ROOT || operation(path).is_some()
}
pub(super) fn route(
    request: &Request,
    broker: &ProductHostBroker,
    auth: &HostAuth,
    domain: EndpointDomain,
) -> Option<p::Result<Vec<u8>>> {
    if !request.path.starts_with(ROOT) {
        return None;
    }
    Some((|| {
        if !matches!(domain, EndpointDomain::Configuration)
            || request.body.len() > 128 * 1024
            || (request.method == "GET" && !request.body.is_empty())
        {
            return Err(invalid());
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", ROOT) => {
                serde_json::to_vec(&broker.plugin_settings(auth)?).map_err(|_| invalid())
            }
            ("POST", ROOT) => serde_json::to_vec(&broker.apply_plugin_settings(
                auth,
                serde_json::from_slice(&request.body).map_err(|_| invalid())?,
            )?)
            .map_err(|_| invalid()),
            ("GET", path) => serde_json::to_vec(
                &broker.plugin_settings_operation(auth, operation(path).ok_or_else(invalid)?)?,
            )
            .map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    })())
}
fn invalid() -> p::Error {
    p::Error("managed plugin settings route is invalid".into())
}
