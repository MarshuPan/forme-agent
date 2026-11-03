//! Authentication terminates here; Host independently verifies source evidence.
use super::{auth, configuration, WebError, WebState};
use crate::web::identity::{AuthError, SensitivePassword};
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use forme_protocol as p;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    idempotency_key: String,
    expected_revision: u64,
    decision: p::ProductReleaseRecoveryDecision,
    candidate: p::ProductReleaseRecoveryCandidate,
    requested_deadline: p::Timestamp,
    password: SensitivePassword,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerificationBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    request_digest: p::SchemaDigest,
    expected_revision: u64,
    requested_deadline: p::Timestamp,
    password: SensitivePassword,
}

pub(super) async fn candidate(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    let expected_id = id.clone();
    let response = state
        .configuration_call(move |client| client.release_recovery_candidate(&id))
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.owner != owner.view().owner || response.original_operation_id != expected_id {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(response).into_response())
}
pub(super) async fn operation(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    let expected_id = id.clone();
    let response = state
        .configuration_call(move |client| client.release_recovery_operation(&id))
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.request.owner != owner.view().owner || response.request.operation_id != expected_id
    {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(response).into_response())
}
pub(super) async fn operations(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    let response = state
        .configuration_call(|client| client.release_recoveries())
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.owner != owner.view().owner {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(response).into_response())
}
pub(super) async fn apply(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<RecoveryBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if auth::key(&headers)? != body.operation_id || body.candidate.owner != owner.view().owner {
        return Err(WebError::invalid());
    }
    let request = p::ProductReleaseRecoveryRequest {
        schema_version: body.schema_version,
        operation_id: body.operation_id,
        idempotency_key: body.idempotency_key,
        expected_revision: body.expected_revision,
        owner: owner.view().owner.clone(),
        decision: body.decision,
        candidate: body.candidate,
        requested_deadline: body.requested_deadline,
    };
    let digest = request.digest().map_err(|_| WebError::invalid())?;
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    let expected = request.digest().map_err(|_| WebError::invalid())?;
    let response = state
        .configuration_call(move |client| client.recover_release(request))
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.request_digest != expected {
        return Err(WebError::invalid());
    }
    Ok(Json(response).into_response())
}
pub(super) async fn verify(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<VerificationBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if auth::key(&headers)? != body.operation_id {
        return Err(WebError::invalid());
    }
    let request = p::ProductReleaseRecoveryVerification {
        schema_version: body.schema_version,
        operation_id: body.operation_id,
        expected_revision: body.expected_revision,
        request_digest: body.request_digest,
        owner: owner.view().owner.clone(),
        requested_deadline: body.requested_deadline,
    };
    let digest = request.digest().map_err(|_| WebError::invalid())?;
    let expected = request.request_digest.clone();
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    let response = state
        .configuration_call(move |client| client.verify_release_recovery(request))
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.request_digest != expected {
        return Err(WebError::invalid());
    }
    Ok(Json(response).into_response())
}
