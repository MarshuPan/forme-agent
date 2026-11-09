//! Finite desktop control routes remain in their existing credential domains.
use super::{http::Request, EndpointDomain};
use forme_execution::installed::{
    HostAuth, HostManagement, IsolatedExecutorHost, ProductHostBroker,
};
use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};

pub(super) fn route(
    request: &Request,
    broker: &ProductHostBroker,
    worker: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    domain: EndpointDomain,
    now: p::Timestamp,
) -> Option<p::Result<Vec<u8>>> {
    let path = request.path.as_str();
    if !path.starts_with("/v1/host/configuration/desktop/")
        && !path.starts_with("/v1/agent/workers/desktop/")
    {
        return None;
    }
    Some(dispatch(request, broker, worker, auth, domain, now))
}
fn dispatch(
    request: &Request,
    broker: &ProductHostBroker,
    worker: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    domain: EndpointDomain,
    now: p::Timestamp,
) -> p::Result<Vec<u8>> {
    if request.body.len() > p::DESKTOP_AUTHORITY_REQUEST_MAX_BYTES
        || (request.method == "GET" && !request.body.is_empty())
    {
        return Err(invalid());
    }
    let path = request.path.as_str();
    match (domain, request.method.as_str(), path) {
        (EndpointDomain::Configuration, "GET", "/v1/host/configuration/desktop/sessions") => {
            encode(&broker.desktop_sessions(auth, now)?)
        }
        (EndpointDomain::Configuration, "POST", "/v1/host/configuration/desktop/consents") => {
            encode(&broker.consent_desktop(auth, decode(&request.body)?, now)?)
        }
        (EndpointDomain::Configuration, "POST", "/v1/host/configuration/desktop/revocations") => {
            encode(&broker.revoke_desktop(auth, decode(&request.body)?, now)?)
        }
        (EndpointDomain::Configuration, "GET", _) => encode(&broker.desktop_consent(
            auth,
            id(path, "/v1/host/configuration/desktop/consents/")?,
            now,
        )?),
        (EndpointDomain::AgentExecution, "POST", "/v1/agent/workers/desktop/pending") => {
            encode(&worker.register_desktop_pending(auth, decode(&request.body)?, now)?)
        }
        (EndpointDomain::AgentExecution, "POST", "/v1/agent/workers/desktop/grants") => {
            encode(&worker.acquire_desktop_grant(auth, decode(&request.body)?, now)?)
        }
        (EndpointDomain::AgentExecution, "GET", _)
            if path.starts_with("/v1/agent/workers/desktop/consents/") =>
        {
            encode(&worker.desktop_pending_consent(
                auth,
                id(path, "/v1/agent/workers/desktop/consents/")?,
                now,
            )?)
        }
        (EndpointDomain::AgentExecution, "GET", _) => encode(&worker.desktop_grant(
            auth,
            id(path, "/v1/agent/workers/desktop/grants/")?,
            now,
        )?),
        _ => Err(invalid()),
    }
}
fn id<'a>(path: &'a str, prefix: &str) -> p::Result<&'a str> {
    let id = path.strip_prefix(prefix).ok_or_else(invalid)?;
    p::validate_product_id(id)?;
    Ok(id)
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> p::Result<T> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}
fn encode(value: &impl Serialize) -> p::Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| invalid())
}
fn invalid() -> p::Error {
    p::Error("desktop control request is invalid".into())
}
