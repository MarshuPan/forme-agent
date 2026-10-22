//! Durable conversation metadata in the independent Web identity store.
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use forme_protocol as p;

use super::{auth, security, WebError, WebState};
use crate::web::identity::{WebIdentityService, WebRequestBinding};

fn cookie(headers: &HeaderMap) -> Result<crate::web::identity::SensitiveCookie, WebError> {
    security::cookie(headers, security::SESSION_COOKIE)?
        .ok_or_else(|| crate::web::identity::AuthError::Unauthenticated.into())
}

async fn owner(
    state: &Arc<WebState>,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<crate::web::identity::AuthenticatedWebOwner, WebError> {
    let cookie = cookie(headers)?;
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

pub(super) async fn list(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Json<p::WorkbenchSessionList>, WebError> {
    let authenticated = owner(&state, &headers, false).await?;
    let value = state
        .blocking(false, move |identity| {
            identity.list_workbench_sessions(&authenticated)
        })
        .await?;
    Ok(Json(value))
}

pub(super) async fn create(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(command): Json<p::WorkbenchSessionCreate>,
) -> Result<Json<p::WorkbenchSessionRecord>, WebError> {
    let authenticated = owner(&state, &headers, true).await?;
    let value = state
        .blocking(false, move |identity| {
            identity.create_workbench_session(&authenticated, command)
        })
        .await?;
    Ok(Json(value))
}

pub(super) async fn update(
    State(state): State<Arc<WebState>>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(command): Json<p::WorkbenchSessionUpdate>,
) -> Result<Json<p::WorkbenchSessionRecord>, WebError> {
    if command.session.0 != session {
        return Err(WebError::invalid());
    }
    let authenticated = owner(&state, &headers, true).await?;
    let value = state
        .blocking(false, move |identity| {
            identity.update_workbench_session(&authenticated, command)
        })
        .await?;
    Ok(Json(value))
}

pub(super) fn _response_marker(_: Response) {}
