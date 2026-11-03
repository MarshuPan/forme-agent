//! Configuration credentials permit only this closed recovery facade.
use super::http::Request;
use forme_execution::installed::{HostAuth, IsolatedExecutorHost, ProductHostBroker};
use forme_protocol as p;

pub(super) fn route(
    request: &Request,
    broker: &ProductHostBroker,
    workers: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    now: p::Timestamp,
) -> Option<p::Result<Vec<u8>>> {
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/host/configuration/release-recovery/operations") => {
            broker.owner_release_recoveries(auth).and_then(encode)
        }
        ("GET", path)
            if path.starts_with("/v1/host/configuration/release-recovery/candidates/") =>
        {
            path.strip_prefix("/v1/host/configuration/release-recovery/candidates/")
                .ok_or_else(invalid)
                .and_then(|id| broker.release_recovery_candidate(auth, id, workers, now))
                .and_then(encode)
        }
        ("GET", path)
            if path.starts_with("/v1/host/configuration/release-recovery/operations/") =>
        {
            path.strip_prefix("/v1/host/configuration/release-recovery/operations/")
                .ok_or_else(invalid)
                .and_then(|id| broker.owner_release_recovery_operation(auth, id))
                .and_then(encode)
        }
        ("POST", "/v1/host/configuration/release-recovery") => decode(&request.body)
            .and_then(|body| broker.owner_release_recovery(auth, body, workers, now))
            .and_then(encode),
        ("POST", "/v1/host/configuration/release-recovery/verify") => decode(&request.body)
            .and_then(|body| broker.verify_owner_release_recovery(auth, body, workers, now))
            .and_then(encode),
        _ => return None,
    };
    Some(result)
}
fn encode(value: impl serde::Serialize) -> p::Result<Vec<u8>> {
    serde_json::to_vec(&value).map_err(|_| invalid())
}
fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> p::Result<T> {
    serde_json::from_slice(body).map_err(|_| invalid())
}
fn invalid() -> p::Error {
    p::Error("release recovery transport is invalid".into())
}
