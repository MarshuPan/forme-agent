//! Owner-scoped artifact transport for the installed workbench.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use forme_protocol as p;
use serde::Deserialize;

use super::{ApiError, HttpState};

pub(super) async fn upload(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::ArtifactUploadCommand>,
) -> Result<Json<p::ArtifactUploadReceipt>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.upload_workbench_artifact(&owner, command))
        .await
        .map(Json)
}

pub(super) async fn read(
    State(state): State<Arc<HttpState>>,
    Path(id): Path<String>,
    Query(query): Query<ArtifactQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    state.authorize(&headers, false)?;
    if !id.starts_with("content:") || id.len() > 256 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "artifact is unavailable",
        ));
    }
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    let session = p::SessionId(query.session);
    let bytes = state
        .blocking(move || runtime.read_workbench_artifact(&owner, &p::ContentRef(id), &session))
        .await?;
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[derive(Debug, Deserialize)]
pub(super) struct ArtifactQuery {
    session: String,
}
