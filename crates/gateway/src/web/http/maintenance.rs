//! Owner-only maintenance facade. Never forwards arbitrary broker commands.
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use forme_protocol as p;
use serde::Deserialize;
use std::sync::Arc;

use super::{auth, configuration, WebError, WebState};
use crate::web::identity::{AuthError, SensitivePassword};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MaintenanceBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    expected_revision: u64,
    action: p::ProductMaintenanceAction,
    requested_deadline: p::Timestamp,
    password: SensitivePassword,
}

pub(super) async fn read(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    let snapshot = state
        .configuration_call(|client| client.maintenance())
        .await?;
    if snapshot.owner != owner.view().owner {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(snapshot).into_response())
}

pub(super) async fn apply(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<MaintenanceBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if auth::key(&headers)? != body.operation_id {
        return Err(WebError::invalid());
    }
    let request = p::ProductMaintenanceRequest {
        schema_version: body.schema_version,
        operation_id: body.operation_id,
        expected_revision: body.expected_revision,
        owner: owner.view().owner.clone(),
        action: body.action,
        requested_deadline: body.requested_deadline,
    };
    let digest = request.digest().map_err(|_| WebError::invalid())?;
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    let response = state
        .configuration_call(move |client| client.maintain(request))
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
    let response = state
        .configuration_call(move |client| {
            // An authenticated session cannot query a different installed owner's history.
            if client.snapshot()?.owner.as_ref() != Some(&owner.view().owner) {
                return Err(p::Error("maintenance owner mismatch".into()));
            }
            client.maintenance_operation(&id)
        })
        .await?;
    Ok(Json(response).into_response())
}
