//! Finite setup adapter. Owner identity only comes from the signed broker claim.
use std::sync::Arc;

use axum::extract::{rejection::JsonRejection, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use forme_protocol as p;
use serde::Deserialize;

use super::{auth, security, WebError, WebState};
use crate::web::bootstrap::LocalBootstrapClient;
use crate::web::identity::{
    AuthError, SensitiveCookie, SensitiveLoginRequest, SensitivePassword, SetupClaimInput,
    WebRequestBinding,
};
use p::BootstrapProblemCode as Code;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProveBody {
    schema_version: p::SchemaVersion,
    #[serde(deserialize_with = "proof_input")]
    proof: SensitiveCookie,
}

fn proof_input<'de, D>(input: D) -> Result<SensitiveCookie, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(input).map(SensitiveCookie::new)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataBody {
    schema_version: p::SchemaVersion,
    candidate_ref: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClaimBody {
    schema_version: p::SchemaVersion,
    operation_id: String,
    candidate_ref: String,
    login_name: String,
    password: SensitivePassword,
}

fn client(state: &WebState) -> Result<Arc<LocalBootstrapClient>, WebError> {
    state.bootstrap.clone().ok_or_else(|| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "ProductIntegrationPending",
            "初始化服务尚未配置，请使用本机维护工具",
        )
    })
}

fn cookie(headers: &HeaderMap) -> Result<SensitiveCookie, WebError> {
    security::cookie(headers, security::SETUP_COOKIE)?
        .ok_or_else(|| AuthError::Unauthenticated.into())
}

pub(super) async fn prove(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ProveBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let client = client(&state)?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    let operation = auth::key(&headers)?.to_owned();
    let existing = security::cookie(&headers, security::SETUP_COOKIE)?;
    let (setup, view) = state
        .blocking(false, move |identity| {
            let setup = identity.prepare_setup(&operation, existing)?;
            let view = match client.reserve(&p::BootstrapReserveRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: operation,
                setup_digest: setup.binding.setup_digest.clone(),
                proof: body.proof.expose_for_transport().into(),
            }) {
                Ok(view) => view,
                Err(error) => {
                    if !matches!(error.0, Code::Unavailable) {
                        identity.discard_unactivated_setup(&setup)?;
                    }
                    return Err(AuthError::from(error));
                }
            };
            identity.activate_setup(&setup, &view)?;
            Ok((setup, view))
        })
        .await?;
    let mut response = Json(serde_json::json!({"schema_version":1,"reservation":view,
        "csrf":setup.csrf.expose_for_transport()}))
    .into_response();
    security::set_cookie(
        &mut response,
        security::SETUP_COOKIE,
        setup.cookie.expose_for_transport(),
        900,
    )?;
    Ok(response)
}

pub(super) async fn data(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let client = client(&state)?;
    let cookie = cookie(&headers)?;
    let candidate = state
        .blocking(false, move |identity| {
            let setup = identity.setup_access(
                &cookie,
                WebRequestBinding {
                    mutation: false,
                    csrf: None,
                },
                false,
            )?;
            let candidate = client.candidate(&setup.binding).map_err(AuthError::from)?;
            if candidate.selected && candidate.available {
                identity.record_setup_candidate(&setup, &candidate)?;
            }
            Ok(candidate)
        })
        .await?;
    Ok(Json(candidate).into_response())
}

pub(super) async fn select(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<DataBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let client = client(&state)?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    let cookie = cookie(&headers)?;
    let binding = auth::mutation(&headers)?;
    let operation = auth::key(&headers)?.to_owned();
    let candidate = state
        .blocking(false, move |identity| {
            let setup = identity.setup_access(&cookie, binding, false)?;
            if setup.binding.operation_id != operation {
                return Err(AuthError::Conflict);
            }
            let candidate = client
                .select(&p::BootstrapDataChoice {
                    schema_version: p::SchemaVersion(1),
                    session: setup.binding.clone(),
                    candidate_ref: body.candidate_ref,
                })
                .map_err(AuthError::from)?;
            identity.record_setup_candidate(&setup, &candidate)?;
            Ok(candidate)
        })
        .await?;
    Ok(Json(candidate).into_response())
}

pub(super) async fn claim(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ClaimBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let client = client(&state)?;
    let body = auth::json(body)?;
    auth::version(body.schema_version)?;
    if body.operation_id != auth::key(&headers)? {
        return Err(WebError::invalid());
    }
    let input = SetupClaimInput {
        cookie: cookie(&headers)?,
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
            identity.finish_bootstrap(&client, input)
        })
        .await?;
    let mut response = Json(
        serde_json::json!({"schema_version":1,"session":session.view,
        "csrf":session.csrf.expose_for_transport()}),
    )
    .into_response();
    security::set_cookie(
        &mut response,
        security::SESSION_COOKIE,
        session.cookie.expose_for_transport(),
        43_200,
    )?;
    // Keeping finite setup scope until its original expiry supports only an
    // exact lost-response retry, never another account or general maintenance.
    Ok(response)
}
