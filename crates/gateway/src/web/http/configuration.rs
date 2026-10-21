//! Authenticated product configuration routes.
use std::sync::Arc;

use axum::extract::{rejection::JsonRejection, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use forme_protocol as p;
use serde::Deserialize;

use super::{auth, security, WebError, WebState};
use crate::web::identity::{
    AuthError, AuthenticatedWebOwner, WebIdentityService, WebRequestBinding,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProductConfigBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    expected_revision: u64,
    models: Vec<p::ProductModelProfilePatch>,
}

fn client(state: &WebState) -> Result<(), WebError> {
    if state.configuration.is_some() {
        Ok(())
    } else {
        Err(WebError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "ConfigurationUnavailable",
            "产品配置服务尚未配置",
        ))
    }
}

async fn owner(
    state: &WebState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<AuthenticatedWebOwner, WebError> {
    let cookie =
        security::cookie(headers, security::SESSION_COOKIE)?.ok_or(AuthError::Unauthenticated)?;
    let binding = if mutation {
        auth::mutation(headers)?
    } else {
        WebRequestBinding {
            mutation: false,
            csrf: None,
        }
    };
    state
        .blocking(false, move |identity| {
            identity.authenticate(&cookie, binding)
        })
        .await
}

pub(super) async fn read(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    client(&state)?;
    let _owner = owner(&state, &headers, false).await?;
    let view = state
        .configuration_call(|configuration| configuration.snapshot())
        .await?;
    Ok(Json(view).into_response())
}

pub(super) async fn write(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ProductConfigBody>, JsonRejection>,
) -> Result<Response, WebError> {
    client(&state)?;
    let operation_id = auth::key(&headers)?.to_owned();
    let owner = owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if body.operation_id != operation_id {
        return Err(WebError::new(
            axum::http::StatusCode::CONFLICT,
            "Conflict",
            "配置操作标识与请求绑定不一致",
        ));
    }
    let patch = p::ProductConfigPatch {
        schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
        operation_id,
        expected_revision: body.expected_revision,
        owner: owner.view().owner.clone(),
        models: body.models,
    };
    let response = state
        .configuration_call(move |configuration| configuration.apply(patch))
        .await?;
    Ok(Json(response).into_response())
}
