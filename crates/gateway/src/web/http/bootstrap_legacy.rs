//! Browser schema-2 setup accepts references only; storage paths stay in Host.
use super::{auth, bootstrap, security, WebError, WebState};
use crate::web::identity::{
    AuthError, SensitiveLoginRequest, SensitivePassword, SetupClaimInput, WebRequestBinding,
};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use forme_protocol as p;
use serde::Deserialize;
use std::sync::Arc;

pub(super) fn routes() -> axum::Router<Arc<WebState>> {
    use axum::{
        extract::DefaultBodyLimit,
        routing::{get, post},
        Router,
    };
    use std::time::Duration;
    use tower_http::timeout::TimeoutLayer;
    let inspection = Router::new()
        .route("/api/v1/bootstrap/candidates", get(candidates))
        .route("/api/v1/bootstrap/select-v2", post(select))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(48),
        ));
    let adoption = Router::new()
        .route("/api/v1/bootstrap/claim-v2", post(claim))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(135),
        ));
    inspection
        .merge(adoption)
        .layer(DefaultBodyLimit::max(8192))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    schema_version: p::SchemaVersion,
    candidate_ref: String,
    expected_candidate_digest: p::SchemaDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Claim {
    schema_version: p::SchemaVersion,
    operation_id: String,
    candidate_ref: String,
    login_name: String,
    password: SensitivePassword,
}

fn session(binding: &p::BootstrapSessionBinding) -> p::BootstrapSessionBindingV2 {
    p::BootstrapSessionBindingV2 {
        schema_version: p::SchemaVersion(2),
        operation_id: binding.operation_id.clone(),
        setup_digest: binding.setup_digest.clone(),
    }
}

pub(super) async fn candidates(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let client = bootstrap::client(&state)?;
    let cookie = bootstrap::cookie(&headers)?;
    let value = state
        .blocking(false, move |identity| {
            let setup = identity.setup_access(
                &cookie,
                WebRequestBinding {
                    mutation: false,
                    csrf: None,
                },
                false,
            )?;
            if let Some(selected) = setup.legacy_candidate {
                return Ok(
                    serde_json::json!({"schema_version":2,"operation_id":setup.binding.operation_id,"catalog":null,"selected":selected}),
                );
            }
            let catalog = client
                .candidate_catalog_v2(&p::BootstrapCandidateCatalogRequestV2 {
                    schema_version: p::SchemaVersion(2),
                    session: session(&setup.binding),
                })
                .map_err(AuthError::from)?;
            if let Some(reference) = setup.candidate {
                let selected = catalog.candidate(&reference).ok_or(AuthError::Conflict)?;
                if selected.kind != p::BootstrapCandidateKind::NewBrainTarget {
                    return Err(AuthError::Conflict);
                }
                return Ok(serde_json::json!({"schema_version":2,"operation_id":setup.binding.operation_id,"catalog":null,"selected":selected}));
            }
            // Host may have persisted the selection before its response reached
            // Web. Recover only that original choice; no select/copy is sent.
            let claim = client
                .claim_state_v2(&p::BootstrapClaimQueryV2 {
                    schema_version: p::SchemaVersion(2),
                    session: session(&setup.binding),
                })
                .map_err(AuthError::from)?;
            match claim.phase {
                p::BootstrapClaimPhaseV2::Selected => {
                    let selected = catalog
                        .candidate(claim.candidate_ref.as_deref().ok_or(AuthError::Conflict)?)
                        .ok_or(AuthError::Conflict)?;
                    if claim.install_ref != catalog.install_ref
                        || claim.candidate_digest.as_ref() != Some(&selected.candidate_digest)
                        || claim.verification_epoch != Some(selected.verification_epoch)
                    {
                        return Err(AuthError::Conflict);
                    }
                    identity.record_setup_candidate_v2(&setup, selected)?;
                    return Ok(serde_json::json!({"schema_version":2,"operation_id":setup.binding.operation_id,"catalog":null,"selected":selected}));
                }
                p::BootstrapClaimPhaseV2::Observed => {
                    let original = client.candidate(&setup.binding).map_err(AuthError::from)?;
                    if original.selected {
                        let selected = catalog
                            .candidate(&original.candidate_ref)
                            .filter(|c| c.kind == p::BootstrapCandidateKind::NewBrainTarget)
                            .ok_or(AuthError::Conflict)?;
                        identity.record_setup_candidate(&setup, &original)?;
                        return Ok(serde_json::json!({"schema_version":2,"operation_id":setup.binding.operation_id,"catalog":null,"selected":selected}));
                    }
                }
                _ => return Err(AuthError::RecoveryRequired),
            }
            Ok(serde_json::json!({"schema_version":2,"operation_id":setup.binding.operation_id,"catalog":catalog,"selected":null}))
        })
        .await?;
    Ok(Json(value).into_response())
}

pub(super) async fn select(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<Selection>, JsonRejection>,
) -> Result<Response, WebError> {
    let body = auth::json(body)?;
    if body.schema_version != p::SchemaVersion(2) {
        return Err(WebError::invalid());
    }
    let client = bootstrap::client(&state)?;
    let cookie = bootstrap::cookie(&headers)?;
    let binding = auth::mutation(&headers)?;
    let operation = auth::key(&headers)?.to_owned();
    let candidate = state
        .blocking(false, move |identity| {
            let setup = identity.setup_access(&cookie, binding, false)?;
            if setup.binding.operation_id != operation {
                return Err(AuthError::Conflict);
            }
            let candidate = client
                .select_candidate_v2(&p::BootstrapCandidateSelectionV2 {
                    schema_version: p::SchemaVersion(2),
                    session: session(&setup.binding),
                    candidate_ref: body.candidate_ref,
                    expected_candidate_digest: body.expected_candidate_digest,
                })
                .map_err(AuthError::from)?;
            identity.record_setup_candidate_v2(&setup, &candidate)?;
            Ok(candidate)
        })
        .await?;
    Ok(Json(candidate).into_response())
}

pub(super) async fn claim(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<Claim>, JsonRejection>,
) -> Result<Response, WebError> {
    let body = auth::json(body)?;
    if body.schema_version != p::SchemaVersion(2) || body.operation_id != auth::key(&headers)? {
        return Err(WebError::invalid());
    }
    let client = bootstrap::client(&state)?;
    let input = SetupClaimInput {
        cookie: bootstrap::cookie(&headers)?,
        request_binding: auth::mutation(&headers)?,
        login: SensitiveLoginRequest {
            login_name: body.login_name,
            password: body.password,
        },
        candidate_ref: body.candidate_ref,
        operation: body.operation_id,
        existing_session: security::cookie(&headers, security::SESSION_COOKIE)?,
    };
    let session = state
        .blocking(true, move |identity| {
            let setup = identity.setup_access(
                &input.cookie,
                WebRequestBinding {
                    mutation: false,
                    csrf: None,
                },
                true,
            )?;
            if setup.legacy_candidate.is_none() {
                return Err(AuthError::Conflict);
            }
            identity.finish_bootstrap(&client, None, input)
        })
        .await?;
    let mut response = Json(serde_json::json!({"schema_version":1,"session":session.view,"csrf":session.csrf.expose_for_transport()})).into_response();
    security::set_cookie(
        &mut response,
        security::SESSION_COOKIE,
        session.cookie.expose_for_transport(),
        43_200,
    )?;
    security::set_cookie(&mut response, security::SETUP_COOKIE, "", 0)?;
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}
