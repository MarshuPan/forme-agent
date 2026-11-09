//! Typed authenticated read-only endpoints for completed tool results.
use super::{ApiError, HttpState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use forme_protocol as p;
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<HttpState>> {
    Router::new()
        .route("/v1/artifacts/tools/query", post(query))
        .route("/v1/artifacts/tools/read", post(read))
}
async fn query(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(query): Json<p::ToolArtifactCatalogQuery>,
) -> Result<Json<p::ToolArtifactCatalogView>, ApiError> {
    state.authorize(&headers, false)?;
    let (runtime, owner) = (state.runtime.clone(), state.owner.clone());
    state
        .blocking(move || runtime.query_tool_artifacts(&owner, query))
        .await
        .map(Json)
        .map_err(|mut error| {
            if error.message == "tool artifact catalog snapshot conflict" {
                error.status = StatusCode::CONFLICT;
            }
            error
        })
}
async fn read(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<p::ToolArtifactRead>,
) -> Result<Json<p::ToolArtifactChunk>, ApiError> {
    state.authorize(&headers, false)?;
    let (runtime, owner) = (state.runtime.clone(), state.owner.clone());
    state
        .blocking(move || runtime.read_tool_artifact(&owner, request))
        .await
        .map(Json)
}
