//! Reauthenticated owner selection uses opaque Host catalog references only.
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
pub(super) struct ConsentBody {
    schema_version: p::SchemaVersion,
    request: p::DesktopConsentRequest,
    password: SensitivePassword,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevokeBody {
    schema_version: p::SchemaVersion,
    request: p::DesktopRevokeRequest,
    password: SensitivePassword,
}
async fn same_owner(state: &WebState, owner: p::OwnerPrincipalRef) -> Result<(), WebError> {
    let actual = state.configuration_call(|client| client.snapshot()).await?;
    if actual.owner.as_ref() != Some(&owner) {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(())
}
pub(super) async fn sessions(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    same_owner(&state, owner.view().owner.clone()).await?;
    let catalog = state
        .configuration_call(|client| client.desktop_sessions())
        .await?;
    if catalog.owner_ref.0 != owner.view().owner.0 {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(catalog).into_response())
}
pub(super) async fn consent(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ConsentBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    body.request.validate().map_err(|_| WebError::invalid())?;
    if auth::key(&headers)? != body.request.operation_id {
        return Err(WebError::invalid());
    }
    let owner_ref = owner.view().owner.clone();
    let digest = body.request.digest().map_err(|_| WebError::invalid())?;
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    same_owner(&state, owner_ref).await?;
    let response = state
        .configuration_call(move |client| client.desktop_consent(body.request))
        .await?;
    Ok(Json(response).into_response())
}
pub(super) async fn operation(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    same_owner(&state, owner.view().owner.clone()).await?;
    let response = state
        .configuration_call(move |client| client.desktop_consent_operation(&id))
        .await?;
    Ok(Json(response).into_response())
}
pub(super) async fn revoke(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<RevokeBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    body.request.validate().map_err(|_| WebError::invalid())?;
    if auth::key(&headers)? != body.request.operation_id {
        return Err(WebError::invalid());
    }
    let owner_ref = owner.view().owner.clone();
    let digest = p::canonical_digest(&("forme.product.desktop.revoke.v1", &body.request))
        .map_err(|_| WebError::invalid())?;
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    same_owner(&state, owner_ref).await?;
    let response = state
        .configuration_call(move |client| client.revoke_desktop(body.request))
        .await?;
    Ok(Json(response).into_response())
}
