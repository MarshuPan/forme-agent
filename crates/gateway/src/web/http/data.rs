//! Explicit, recently authenticated data maintenance; never a wildcard proxy.
use super::{auth, security, WebError, WebState};
use crate::web::identity::{AuthError, SensitivePassword, WebIdentityService, WebRequestBinding};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use forme_protocol as p;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommandBody {
    command: p::DataWorkbenchCommand,
    password: SensitivePassword,
}
pub(super) async fn query(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<p::DataWorkbenchQuery>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = owner(&state, &headers).await?;
    let query = auth::json(body)?;
    query.validate().map_err(|_| WebError::invalid())?;
    forward(state, owner, "query", &query).await
}
pub(super) async fn command(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<CommandBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = owner(&state, &headers).await?;
    let body = auth::json(body)?;
    body.command.validate().map_err(|_| WebError::invalid())?;
    if auth::key(&headers)? != body.command.command.0 {
        return Err(WebError::invalid());
    }
    let digest = p::canonical_digest(&body.command).map_err(|_| WebError::invalid())?;
    let confirmation_owner = owner.clone();
    state
        .blocking(true, move |identity| {
            identity.authorize_product_operation(&confirmation_owner, body.password, &digest)
        })
        .await?;
    forward(state, owner, "command", &body.command).await
}
async fn owner(
    state: &WebState,
    headers: &HeaderMap,
) -> Result<crate::web::identity::AuthenticatedWebOwner, WebError> {
    // Queries use POST for closed request bodies, so they also require CSRF.
    let binding: WebRequestBinding = auth::mutation(headers)?;
    let cookie =
        security::cookie(headers, security::SESSION_COOKIE)?.ok_or(AuthError::Unauthenticated)?;
    state
        .blocking(false, move |identity| {
            identity.authenticate(&cookie, binding)
        })
        .await
}
async fn forward<T: serde::Serialize>(
    state: Arc<WebState>,
    owner: crate::web::identity::AuthenticatedWebOwner,
    route: &str,
    body: &T,
) -> Result<Response, WebError> {
    let client = state.core.clone().ok_or_else(|| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "CoreUnavailable",
            "主脑服务不可用，数据操作未发送",
        )
    })?;
    let _fence_gate = state.core_access().await?;
    let bytes = serde_json::to_vec(body).map_err(|_| WebError::invalid())?;
    let path = format!("/v1/data/workbench/{route}");
    let binding = owner.core_binding();
    let lease = state
        .admission
        .acquire()
        .map_err(|_| AuthError::Unavailable)?;
    let permit = state
        .reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| AuthError::Busy)?;
    let reply = tokio::task::spawn_blocking(move || {
        let (_lease, _permit) = (lease, permit);
        client.request(&binding, "POST", &path, &bytes)
    })
    .await
    .map_err(|_| AuthError::Unavailable)?
    .map_err(|_| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "CoreUnavailable",
            "连接中断，请查询原数据操作，勿重复执行",
        )
    })?;
    let status = StatusCode::from_u16(reply.status).map_err(|_| WebError::invalid())?;
    if !status.is_success() {
        return Err(WebError::new(
            status,
            "DataCommandRejected",
            "主脑未接受数据请求，请核实范围、版本及原操作回执",
        ));
    }
    let mut response = (status, reply.body).into_response();
    response.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
