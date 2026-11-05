//! Independent historical-review facade; never falls back to ordinary recovery.
use super::http::Request;
use forme_execution::installed::{HostAuth, IsolatedExecutorHost, ProductHostBroker};
use forme_protocol as p;

#[cfg(test)]
mod tests;

pub(super) fn route(
    request: &Request,
    broker: &ProductHostBroker,
    workers: &dyn IsolatedExecutorHost,
    auth: &HostAuth,
    now: p::Timestamp,
) -> Option<p::Result<Vec<u8>>> {
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/host/configuration/legacy-release-recovery/operations") => broker
            .owner_legacy_release_recoveries(auth)
            .and_then(encode),
        ("GET", path)
            if path.starts_with("/v1/host/configuration/legacy-release-recovery/review/") =>
        {
            path.strip_prefix("/v1/host/configuration/legacy-release-recovery/review/")
                .ok_or_else(invalid)
                .and_then(|id| broker.legacy_release_review(auth, id, workers, now))
                .and_then(encode)
        }
        ("GET", path)
            if path.starts_with("/v1/host/configuration/legacy-release-recovery/operations/") =>
        {
            path.strip_prefix("/v1/host/configuration/legacy-release-recovery/operations/")
                .ok_or_else(invalid)
                .and_then(|id| broker.owner_legacy_release_recovery_operation(auth, id))
                .and_then(encode)
        }
        ("POST", "/v1/host/configuration/legacy-release-recovery") => decode(&request.body)
            .and_then(|body| broker.owner_legacy_release_recovery(auth, body, workers, now))
            .and_then(encode),
        ("POST", "/v1/host/configuration/legacy-release-recovery/verify") => decode(&request.body)
            .and_then(|body| broker.verify_owner_legacy_release_recovery(auth, body, workers, now))
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
    p::Error("legacy review transport is invalid".into())
}
