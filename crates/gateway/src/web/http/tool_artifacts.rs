//! Fixed v2 browser routes. Original material authorization remains in core.
use super::{security, WebError, WebState};
use crate::web::identity::{AuthError, WebIdentityService, WebRequestBinding};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use forme_protocol as p;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<WebState>> {
    Router::new()
        .route(
            "/api/v2/sessions/{session}/runs/{run}/artifacts",
            get(catalog),
        )
        .route(
            "/api/v2/sessions/{session}/runs/{run}/artifacts/{content}",
            get(read),
        )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    offset: usize,
    limit: usize,
    snapshot: Option<p::SchemaDigest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Range {
    content_digest: p::SchemaDigest,
    offset: u64,
    max_bytes: u32,
}
async fn catalog(
    State(state): State<Arc<WebState>>,
    Path((session, run)): Path<(String, String)>,
    Query(page): Query<Page>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let query = p::ToolArtifactCatalogQuery {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: p::SessionId(session),
        run: p::RunId(run),
        page: p::DataPageQuery {
            schema_version: p::SchemaVersion(1),
            offset: page.offset,
            limit: page.limit,
            snapshot: page.snapshot,
        },
    };
    query.validate().map_err(|_| WebError::invalid())?;
    let result: p::ToolArtifactCatalogView = forward(state, headers, "query", &query).await?;
    result.validate_for(&query).map_err(|_| protocol_error())?;
    let mut response = Json(result).into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    Ok(response)
}
async fn read(
    State(state): State<Arc<WebState>>,
    Path((session, run, content)): Path<(String, String, String)>,
    Query(range): Query<Range>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let request = p::ToolArtifactRead {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: p::SessionId(session),
        run: p::RunId(run),
        content_ref: p::ContentRef(content),
        content_digest: range.content_digest,
        offset: range.offset,
        max_bytes: range.max_bytes,
    };
    request.validate().map_err(|_| WebError::invalid())?;
    let chunk: p::ToolArtifactChunk = forward(state, headers, "read", &request).await?;
    chunk.validate_for(&request).map_err(|_| protocol_error())?;
    let range = format!(
        "bytes {}-{}/{}",
        chunk.offset,
        chunk.next_offset - 1,
        chunk.total_bytes
    );
    let mut response = (StatusCode::PARTIAL_CONTENT, chunk.bytes).into_response();
    for (name, value) in [
        ("content-type", "image/png"),
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("accept-ranges", "bytes"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    for (name, value) in [
        ("content-range", range),
        ("x-forme-content-digest", chunk.content_digest.0),
        ("x-forme-total-bytes", chunk.total_bytes.to_string()),
        ("x-forme-next-offset", chunk.next_offset.to_string()),
    ] {
        response.headers_mut().insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| protocol_error())?,
        );
    }
    Ok(response)
}
async fn forward<T: Serialize, R: DeserializeOwned>(
    state: Arc<WebState>,
    headers: HeaderMap,
    route: &str,
    value: &T,
) -> Result<R, WebError> {
    if !matches!(route, "query" | "read") {
        return Err(WebError::invalid());
    }
    let cookie =
        security::cookie(&headers, security::SESSION_COOKIE)?.ok_or(AuthError::Unauthenticated)?;
    let cookie = Arc::new(cookie);
    let original_cookie = cookie.clone();
    let owner = state
        .blocking(false, move |identity| {
            identity.authenticate(
                &original_cookie,
                WebRequestBinding {
                    mutation: false,
                    csrf: None,
                },
            )
        })
        .await?;
    let _fence = state.core_access().await?;
    let client = state.core.clone().ok_or(AuthError::Unavailable)?;
    let path = format!("/v1/artifacts/tools/{route}");
    let bytes = serde_json::to_vec(value).map_err(|_| WebError::invalid())?;
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
    .map_err(|_| AuthError::Unavailable)?;
    // Recheck expiry or out-of-band revocation before publishing the body.
    // Web logout itself serializes behind this read through the core fence.
    state
        .blocking(false, move |identity| {
            identity.authenticate(
                &cookie,
                WebRequestBinding {
                    mutation: false,
                    csrf: None,
                },
            )
        })
        .await?;
    let status = StatusCode::from_u16(reply.status).map_err(|_| protocol_error())?;
    if !status.is_success() {
        return Err(WebError::new(
            status,
            "ToolArtifactUnavailable",
            "此任务产物暂不可读取，请刷新任务或检查是否已删除。",
        ));
    }
    // Core responses contain only one bounded chunk or one page of metadata.
    if reply.body.len() > 2 * 1024 * 1024 {
        return Err(protocol_error());
    }
    serde_json::from_slice(&reply.body).map_err(|_| protocol_error())
}
fn protocol_error() -> WebError {
    WebError::new(
        StatusCode::BAD_GATEWAY,
        "ToolArtifactProtocol",
        "任务产物响应不完整或绑定不一致。",
    )
}
