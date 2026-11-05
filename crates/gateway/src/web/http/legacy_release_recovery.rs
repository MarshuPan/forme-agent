//! The owner authorizes a report reference; only Host can adjudicate its evidence.
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
    review_id: String,
    review_digest: p::SchemaDigest,
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

pub(super) async fn review(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    let original = id.clone();
    let response = state
        .configuration_call(move |client| client.legacy_release_review(&id))
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.binding.owner != owner.view().owner
        || response.binding.original_operation_id != original
    {
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
    let original = id.clone();
    let response = state
        .configuration_call(move |client| client.legacy_release_recovery_operation(&id))
        .await?;
    response.validate().map_err(|_| WebError::invalid())?;
    if response.request.owner != owner.view().owner || response.request.operation_id != original {
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
        .configuration_call(|client| client.legacy_release_recoveries())
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
    if auth::key(&headers)? != body.operation_id {
        return Err(WebError::invalid());
    }
    let request = p::LegacyReleaseRecoveryRequest {
        schema_version: body.schema_version,
        operation_id: body.operation_id,
        idempotency_key: body.idempotency_key,
        expected_revision: body.expected_revision,
        owner: owner.view().owner.clone(),
        review_id: body.review_id,
        review_digest: body.review_digest,
        requested_deadline: body.requested_deadline,
    };
    let digest = request.digest().map_err(|_| WebError::invalid())?;
    let expected = digest.clone();
    let expected_owner = request.owner.clone();
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    let response = state
        .configuration_call(move |client| client.recover_legacy_release(request))
        .await?;
    response.validate().map_err(|_| unverified_outcome())?;
    if response.request_digest != expected || response.request.owner != expected_owner {
        return Err(unverified_outcome());
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
    let request = p::LegacyReleaseRecoveryVerification {
        schema_version: body.schema_version,
        operation_id: body.operation_id,
        request_digest: body.request_digest,
        expected_revision: body.expected_revision,
        owner: owner.view().owner.clone(),
        requested_deadline: body.requested_deadline,
    };
    let digest = request.digest().map_err(|_| WebError::invalid())?;
    let expected = request.request_digest.clone();
    let expected_id = request.operation_id.clone();
    let expected_owner = request.owner.clone();
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    let response = state
        .configuration_call(move |client| client.verify_legacy_release_recovery(request))
        .await?;
    response.validate().map_err(|_| unverified_outcome())?;
    if response.request_digest != expected
        || response.request.operation_id != expected_id
        || response.request.owner != expected_owner
    {
        return Err(unverified_outcome());
    }
    Ok(Json(response).into_response())
}

fn unverified_outcome() -> WebError {
    WebError::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "LegacyRecoveryUnverified",
        "结果尚未核实，请保留原操作号并查询",
    )
}
