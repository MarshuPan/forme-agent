//! Explicit owner reauthentication binds program management to the exact command.
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
pub(super) struct CommandBody {
    schema_version: p::SchemaVersion,
    command: p::ManagedMcpProgramCommand,
    password: SensitivePassword,
}
async fn same_owner(state: &WebState, owner: p::OwnerPrincipalRef) -> Result<(), WebError> {
    let config = state.configuration_call(|client| client.snapshot()).await?;
    if config.owner.as_ref() != Some(&owner) {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(())
}
pub(super) async fn catalog(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    let principal = owner.view().owner.clone();
    same_owner(&state, principal.clone()).await?;
    let catalog = state
        .configuration_call(|client| client.mcp_programs())
        .await?;
    if catalog.owner_ref != principal {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(catalog).into_response())
}
pub(super) async fn command(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<CommandBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    body.command.validate().map_err(|_| WebError::invalid())?;
    if body.command.owner_ref != owner.view().owner
        || auth::key(&headers)? != body.command.operation_id
    {
        return Err(WebError::invalid());
    }
    let principal = owner.view().owner.clone();
    let digest = body.command.digest().map_err(|_| WebError::invalid())?;
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    same_owner(&state, principal).await?;
    let operation = state
        .configuration_call(move |client| client.apply_mcp_program(body.command))
        .await?;
    Ok(Json(operation).into_response())
}
pub(super) async fn operation(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    same_owner(&state, owner.view().owner.clone()).await?;
    let operation = state
        .configuration_call(move |client| client.mcp_program_operation(&id))
        .await?;
    Ok(Json(operation).into_response())
}
