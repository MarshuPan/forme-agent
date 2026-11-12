//! A narrow reauthenticated configuration edit, never an execution grant.
use super::{auth, configuration, WebError, WebState};
use crate::web::identity::{AuthError, SensitivePassword};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::HeaderMap,
    Json,
};
use forme_protocol as p;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SettingsBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    expected_revision: u64,
    plugins: Vec<p::ProductPluginConfig>,
    password: SensitivePassword,
}

pub(super) async fn read(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Json<p::ProductPluginSettingsView>, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    let view = state
        .configuration_call(|client| client.plugin_settings())
        .await?;
    if view.owner.as_ref() != Some(&owner.view().owner) {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(view))
}

pub(super) async fn operation(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<p::ProductPluginSettingsOperation>, WebError> {
    let owner = configuration::owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    let view = state
        .configuration_call(move |client| client.plugin_settings_operation(&id))
        .await?;
    if view.owner != owner.view().owner {
        return Err(AuthError::Unauthenticated.into());
    }
    Ok(Json(view))
}

pub(super) async fn write(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<SettingsBody>, JsonRejection>,
) -> Result<Json<p::ProductPluginSettingsResponse>, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if body.operation_id != auth::key(&headers)? {
        return Err(WebError::invalid());
    }
    let patch = p::ProductPluginSettingsPatch {
        schema_version: body.schema_version,
        operation_id: body.operation_id,
        expected_revision: body.expected_revision,
        owner: owner.view().owner.clone(),
        plugins: body.plugins,
    };
    let digest = patch.digest().map_err(|_| WebError::invalid())?;
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&owner, body.password, &digest)
        })
        .await?;
    let before = state
        .configuration_call(|client| client.plugin_settings())
        .await?;
    if before.owner.as_ref() != Some(&patch.owner) {
        return Err(AuthError::Unauthenticated.into());
    }
    let response = state
        .configuration_call(move |client| client.apply_plugin_settings(patch))
        .await?;
    Ok(Json(response))
}
