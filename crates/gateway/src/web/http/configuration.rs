//! Authenticated product configuration routes.
use std::sync::Arc;

use axum::extract::{rejection::JsonRejection, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use forme_protocol as p;
use serde::Deserialize;

use super::{auth, security, WebError, WebState};
use crate::web::identity::{
    AuthError, AuthenticatedWebOwner, SensitivePassword, WebIdentityService, WebRequestBinding,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProductConfigBody {
    schema_version: p::SchemaVersion,
    #[serde(rename = "operation_id")]
    _operation_id: String,
    expected_revision: u64,
    models: Vec<p::ProductModelProfilePatch>,
    #[serde(default)]
    brain: Option<p::ProductBrainPreferences>,
    #[serde(default)]
    capabilities: Option<p::ProductCapabilitySettings>,
    #[serde(default)]
    network: Option<p::ProductNetworkConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProductConfigBodyV2 {
    schema_version: p::SchemaVersion,
    #[serde(rename = "operation_id")]
    _operation_id: String,
    expected_revision: u64,
    models: Vec<p::ProductModelProfilePatch>,
    #[serde(default)]
    brain: Option<p::ProductBrainPreferences>,
    #[serde(default)]
    capabilities: Option<p::ProductCapabilitySettings>,
    #[serde(default)]
    network: Option<p::ProductNetworkSettingsPatch>,
    #[serde(default)]
    proxy: Option<p::ProductProxyConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SettingsQuery {
    #[serde(default = "legacy_settings_version")]
    schema_version: p::SchemaVersion,
}

fn legacy_settings_version() -> p::SchemaVersion {
    p::PRODUCT_CONFIG_SCHEMA_VERSION
}

fn settings_version(version: p::SchemaVersion) -> Result<(), WebError> {
    p::validate_product_settings_schema(version).map_err(|_| {
        WebError::new(
            axum::http::StatusCode::UPGRADE_REQUIRED,
            "UnsupportedVersion",
            "设置协议版本不受支持，请更新页面或服务",
        )
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ValidationBody {
    schema_version: p::SchemaVersion,
    revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActivationBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    expected_revision: u64,
    action: p::ProductCoreAction,
    requested_deadline: p::Timestamp,
    password: SensitivePassword,
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

pub(super) async fn owner(
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
    Query(query): Query<SettingsQuery>,
) -> Result<Response, WebError> {
    client(&state)?;
    let owner = owner(&state, &headers, false).await?;
    settings_version(query.schema_version)?;
    if query.schema_version == p::PRODUCT_SETTINGS_SCHEMA_VERSION {
        let view = state
            .configuration_call(|configuration| configuration.snapshot_v2())
            .await?;
        if view
            .owner
            .as_ref()
            .is_some_and(|bound| bound != &owner.view().owner)
        {
            return Err(AuthError::Unauthenticated.into());
        }
        Ok(Json(view).into_response())
    } else {
        let view = state
            .configuration_call(|configuration| configuration.snapshot())
            .await?;
        if view
            .owner
            .as_ref()
            .is_some_and(|bound| bound != &owner.view().owner)
        {
            return Err(AuthError::Unauthenticated.into());
        }
        Ok(Json(view).into_response())
    }
}

pub(super) async fn status(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    client(&state)?;
    let _owner = owner(&state, &headers, false).await?;
    let status = state
        .configuration_call(|configuration| configuration.status())
        .await?;
    Ok(Json(status).into_response())
}

pub(super) async fn write(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Response, WebError> {
    client(&state)?;
    let operation_id = auth::key(&headers)?.to_owned();
    let owner = owner(&state, &headers, true).await?;
    let value = auth::json(body)?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .map(p::SchemaVersion)
        .ok_or_else(WebError::invalid)?;
    settings_version(schema_version)?;
    if value
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        != Some(operation_id.as_str())
    {
        return Err(WebError::new(
            axum::http::StatusCode::CONFLICT,
            "Conflict",
            "配置操作标识与请求绑定不一致",
        ));
    }
    if schema_version == p::PRODUCT_SETTINGS_SCHEMA_VERSION {
        let body: ProductConfigBodyV2 =
            serde_json::from_value(value).map_err(|_| WebError::invalid())?;
        let patch = p::ProductConfigPatchV2 {
            schema_version: body.schema_version,
            operation_id,
            expected_revision: body.expected_revision,
            owner: owner.view().owner.clone(),
            models: body.models,
            brain: body.brain,
            capabilities: body.capabilities,
            network: body.network,
            proxy: body.proxy,
        };
        let response = state
            .configuration_call(move |configuration| configuration.apply_v2(patch))
            .await?;
        Ok(Json(response).into_response())
    } else {
        let body: ProductConfigBody =
            serde_json::from_value(value).map_err(|_| WebError::invalid())?;
        let patch = p::ProductConfigPatch {
            schema_version: body.schema_version,
            operation_id,
            expected_revision: body.expected_revision,
            owner: owner.view().owner.clone(),
            models: body.models,
            brain: body.brain,
            capabilities: body.capabilities,
            network: body.network,
        };
        let response = state
            .configuration_call(move |configuration| configuration.apply(patch))
            .await?;
        Ok(Json(response).into_response())
    }
}

pub(super) async fn validate(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ValidationBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    let request = p::ProductConfigValidationRequest {
        schema_version: body.schema_version,
        revision: body.revision,
        owner: owner.view().owner.clone(),
    };
    let report = state
        .configuration_call(move |configuration| configuration.validate(request))
        .await?;
    Ok(Json(report).into_response())
}

pub(super) async fn activate(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ActivationBody>, JsonRejection>,
) -> Result<Response, WebError> {
    client(&state)?;
    let owner = owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if auth::key(&headers)? != body.operation_id {
        return Err(WebError::invalid());
    }
    let request = p::ProductCoreActivation {
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
        .configuration_call(move |configuration| configuration.activate(request))
        .await?;
    Ok(Json(response).into_response())
}

pub(super) async fn operation(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, WebError> {
    let _owner = owner(&state, &headers, false).await?;
    p::validate_product_id(&id).map_err(|_| WebError::invalid())?;
    let operation = state
        .configuration_call(move |configuration| configuration.operation(&id))
        .await?;
    Ok(Json(operation).into_response())
}
