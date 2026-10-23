//! Read-only recovery access to the owner's unresolved proposal commands.
use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use forme_protocol as p;
use serde::Serialize;

use super::{security, WebError, WebState};
use crate::web::identity::{WebIdentityService, WebRequestBinding};

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingWorkbenchProposals {
    schema_version: p::SchemaVersion,
    commands: Vec<p::WorkbenchProposalCommand>,
}

pub(super) async fn pending(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Json<PendingWorkbenchProposals>, WebError> {
    let cookie = security::cookie(&headers, security::SESSION_COOKIE)?
        .ok_or(crate::web::identity::AuthError::Unauthenticated)?;
    let owner = state
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
    let commands = state
        .blocking(false, move |identity| {
            identity.pending_workbench_proposals(&owner)
        })
        .await?;
    Ok(Json(PendingWorkbenchProposals {
        schema_version: p::SchemaVersion(1),
        commands,
    }))
}
