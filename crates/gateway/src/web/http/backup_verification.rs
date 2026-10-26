//! Explicit offline read; normal data routes never silently switch to Host.
use super::{auth, configuration, WebError, WebState};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use forme_protocol as p;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerificationBody {
    schema_version: p::SchemaVersion,
    source: String,
    config_revision: u64,
}

pub(super) async fn verify(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<VerificationBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let owner = configuration::owner(&state, &headers, true).await?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    p::validate_data_object_name(&body.source).map_err(|_| WebError::invalid())?;
    if body.config_revision == 0 {
        return Err(WebError::invalid());
    }
    let response = state
        .configuration_call(move |client| {
            let snapshot = client.snapshot()?;
            if snapshot.owner.as_ref() != Some(&owner.view().owner)
                || snapshot.active_revision != body.config_revision
            {
                return Err(p::Error(
                    "backup verification owner or revision mismatch".into(),
                ));
            }
            client.verify_backup(p::ProductBackupVerificationRequest {
                schema_version: body.schema_version,
                install_ref: snapshot.install_ref,
                owner: owner.view().owner.clone(),
                expected_config_revision: body.config_revision,
                source: body.source,
            })
        })
        .await?;
    Ok(Json(response).into_response())
}
