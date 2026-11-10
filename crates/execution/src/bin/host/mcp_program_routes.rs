//! Finite Configuration-domain routes. No dispatch or arbitrary program input.
use super::{http::Request, EndpointDomain};
use forme_execution::installed::{HostAuth, ProductHostBroker};
use forme_protocol as p;

const ROOT: &str = "/v1/host/configuration/mcp/programs";
pub(super) fn is_path(path: &str) -> bool {
    path == ROOT || path == format!("{ROOT}/commands") || operation(path).is_some()
}
fn operation(path: &str) -> Option<&str> {
    path.strip_prefix("/v1/host/configuration/mcp/programs/operations/")
        .filter(|id| p::validate_product_id(id).is_ok())
}
pub(super) fn route(
    request: &Request,
    broker: &ProductHostBroker,
    auth: &HostAuth,
    domain: EndpointDomain,
    now: p::Timestamp,
) -> Option<p::Result<Vec<u8>>> {
    if !request.path.starts_with(ROOT) {
        return None;
    }
    Some((|| {
        if !matches!(domain, EndpointDomain::Configuration)
            || request.body.len() > p::MANAGED_MCP_PROGRAM_REQUEST_MAX_BYTES
            || (request.method == "GET" && !request.body.is_empty())
        {
            return Err(invalid());
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", ROOT) => serde_json::to_vec(&broker.mcp_programs(auth)?).map_err(|_| invalid()),
            ("POST", "/v1/host/configuration/mcp/programs/commands") => {
                let command = serde_json::from_slice(&request.body).map_err(|_| invalid())?;
                serde_json::to_vec(&broker.apply_mcp_program(auth, command, now)?)
                    .map_err(|_| invalid())
            }
            ("GET", path) => serde_json::to_vec(
                &broker.mcp_program_operation(auth, operation(path).ok_or_else(invalid)?)?,
            )
            .map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    })())
}
fn invalid() -> p::Error {
    p::Error("managed MCP program route is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_do_not_accept_execution_private_sources_or_trailing_segments() {
        assert!(is_path(ROOT));
        assert!(is_path(&format!("{ROOT}/commands")));
        assert!(is_path(&format!("{ROOT}/operations/register:one")));
        for suffix in [
            "/execute",
            "/sources",
            "/commands/extra",
            "/operations/",
            "/operations/../file",
            "/operations/a?x=1",
        ] {
            assert!(!is_path(&format!("{ROOT}{suffix}")));
        }
    }
}
