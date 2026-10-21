//! Restricted setup routes, separate from maintenance and AgentExecution grants.
use forme_execution::installed::bootstrap::{BootstrapAuthority, BootstrapError, BootstrapResult};
use forme_execution::installed::{HostManagement, ProductHostBroker};
use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};

use super::{http::Request, now_ms, EndpointDomain};
use p::BootstrapProblemCode as Code;

pub(super) fn handles(path: &str) -> bool {
    path.starts_with("/v1/setup/") || path == "/v1/host/bootstrap/proof"
}

pub(super) fn route(
    request: &Request,
    broker: &ProductHostBroker,
    authority: Option<&BootstrapAuthority>,
    domain: EndpointDomain,
) -> (u16, Vec<u8>) {
    let result = (|| {
        if !matches!(domain, EndpointDomain::Maintenance) || request.method != "POST" {
            return Err(BootstrapError(Code::Unauthenticated));
        }
        let authority = authority.ok_or(BootstrapError(Code::Unavailable))?;
        let bearer = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(BootstrapError(Code::Unauthenticated))?;
        if request.path == "/v1/host/bootstrap/proof" {
            let admin = broker
                .authenticate(bearer)
                .map_err(|_| BootstrapError(Code::Unauthenticated))?;
            let body: p::BootstrapProofRequest = parse(request)?;
            if body.schema_version != p::SchemaVersion(1)
                || request.header("idempotency-key") != Some(body.operation_id.as_str())
            {
                return Err(BootstrapError(Code::InvalidInput));
            }
            return encode(&authority.issue_proof(&admin, &body.operation_id, now_ms())?);
        }
        let auth = authority.authenticate_service(bearer)?;
        match request.path.as_str() {
            "/v1/setup/reserve" => {
                let body: p::BootstrapReserveRequest = parse(request)?;
                require_operation_key(request, &body.operation_id)?;
                encode(&authority.reserve(&auth, body, now_ms())?)
            }
            "/v1/setup/data" => {
                let body: p::BootstrapSessionBinding = parse(request)?;
                require_operation_key(request, &body.operation_id)?;
                encode(&authority.candidate(&auth, &body, now_ms())?)
            }
            "/v1/setup/select" => {
                let body: p::BootstrapDataChoice = parse(request)?;
                require_operation_key(request, &body.session.operation_id)?;
                encode(&authority.select(&auth, body, now_ms())?)
            }
            "/v1/setup/commit" => {
                let body: p::BootstrapCommitRequest = parse(request)?;
                require_operation_key(request, &body.session.operation_id)?;
                encode(&authority.commit(&auth, body, now_ms())?)
            }
            _ => Err(BootstrapError(Code::InvalidInput)),
        }
    })();
    match result {
        Ok(body) => (200, body),
        Err(BootstrapError(code)) => {
            let status = match code {
                Code::InvalidInput => 400,
                Code::Unauthenticated => 401,
                Code::Conflict => 409,
                Code::Expired => 410,
                Code::DataUnavailable | Code::Unavailable | Code::RecoveryRequired => 503,
            };
            let body = serde_json::to_vec(&p::BootstrapProblem {
                schema_version: p::SchemaVersion(1),
                code,
            })
            .unwrap_or_else(|_| b"{}".to_vec());
            (status, body)
        }
    }
}

fn parse<T: DeserializeOwned>(request: &Request) -> BootstrapResult<T> {
    if request.body.len() > 8192 {
        return Err(BootstrapError(Code::InvalidInput));
    }
    if request.header("content-type") != Some("application/json") {
        return Err(BootstrapError(Code::InvalidInput));
    }
    serde_json::from_slice(&request.body).map_err(|_| BootstrapError(Code::InvalidInput))
}

fn require_operation_key(request: &Request, operation: &str) -> BootstrapResult<()> {
    if request.header("idempotency-key") == Some(operation) {
        Ok(())
    } else {
        Err(BootstrapError(Code::InvalidInput))
    }
}

fn encode<T: Serialize>(value: &T) -> BootstrapResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| BootstrapError(Code::Unavailable))
}
