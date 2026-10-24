//! Minimal unauthenticated listener proof used by the Host health probe.
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::{WebError, WebState};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HealthQuery {
    nonce: String,
}

pub(super) async fn health(
    State(state): State<Arc<WebState>>,
    Query(query): Query<HealthQuery>,
) -> Result<Response, WebError> {
    if query.nonce.len() != 64
        || !query
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(WebError::invalid());
    }
    let client = state.core.clone().ok_or_else(|| {
        WebError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "HealthUnavailable",
            "Web 健康证明尚未配置",
        )
    })?;
    let proof = client
        .health_proof(&state.listener_digest, query.nonce)
        .map_err(|_| {
            WebError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "HealthUnavailable",
                "Web 健康证明暂不可用",
            )
        })?;
    Ok(Json(proof).into_response())
}
