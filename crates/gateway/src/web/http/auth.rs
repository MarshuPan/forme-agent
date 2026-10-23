//! Explicit authentication routes. No wildcard proxy or core fallback exists.
use std::sync::Arc;

use axum::extract::{rejection::JsonRejection, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use forme_protocol as p;
use serde::Deserialize;

use super::FenceSync;
use super::{security, WebError, WebState};
use crate::web::identity::{
    AuthError, LoginChallengeBinding, ReauthenticationProof, RevokeSessions, SensitiveCookie,
    SensitiveLoginRequest, SensitivePassword, WebIdentityService, WebIdentityStore,
    WebRequestBinding,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LoginBody {
    schema_version: p::SchemaVersion,
    login_name: String,
    password: SensitivePassword,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReauthBody {
    schema_version: p::SchemaVersion,
    password: SensitivePassword,
    operation_digest: p::SchemaDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevokeBody {
    schema_version: p::SchemaVersion,
    operation_digest: p::SchemaDigest,
    proof: ReauthenticationProof,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PasswordBody {
    schema_version: p::SchemaVersion,
    old_password: SensitivePassword,
    new_password: SensitivePassword,
}

pub(super) fn json<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, WebError> {
    body.map(|Json(value)| value)
        .map_err(|_| WebError::invalid())
}

pub(super) fn version(version: p::SchemaVersion) -> Result<(), WebError> {
    if version == p::SchemaVersion(1) {
        Ok(())
    } else {
        Err(WebError::invalid())
    }
}

pub(super) fn key(headers: &HeaderMap) -> Result<&str, WebError> {
    let key = security::one(headers, "idempotency-key")?.ok_or_else(WebError::invalid)?;
    p::validate_product_id(key).map_err(|_| WebError::invalid())?;
    Ok(key)
}

fn session_cookie(headers: &HeaderMap) -> Result<SensitiveCookie, WebError> {
    security::cookie(headers, security::SESSION_COOKIE)?
        .ok_or_else(|| AuthError::Unauthenticated.into())
}

pub(super) fn mutation(headers: &HeaderMap) -> Result<WebRequestBinding, WebError> {
    key(headers)?;
    let csrf = security::one(headers, "x-forme-csrf")?.ok_or(AuthError::CsrfRejected)?;
    Ok(WebRequestBinding {
        mutation: true,
        csrf: Some(SensitiveCookie::new(csrf.into())),
    })
}

pub(super) async fn bootstrap(State(state): State<Arc<WebState>>) -> Result<Response, WebError> {
    let claimed = state
        .blocking(false, |identity| identity.is_claimed())
        .await?;
    Ok(Json(serde_json::json!({
        "schema_version":1, "claimed":claimed, "setup_needed":!claimed, "protocol_version":1
    }))
    .into_response())
}

pub(super) async fn challenge(State(state): State<Arc<WebState>>) -> Result<Response, WebError> {
    let challenge = state
        .blocking(false, |identity| {
            if !identity.is_claimed()? {
                return Err(AuthError::Conflict);
            }
            identity.login_challenge()
        })
        .await?;
    let mut response = Json(challenge.view).into_response();
    security::set_cookie(
        &mut response,
        security::LOGIN_COOKIE,
        challenge.cookie.expose_for_transport(),
        300,
    )?;
    Ok(response)
}

pub(super) async fn login(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<LoginBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let body = json(body)?;
    version(body.schema_version)?;
    if body.login_name.len() > 64 {
        return Err(WebError::invalid());
    }
    let attempt = p::SchemaDigest(key(&headers)?.into());
    p::validate_product_digest(&attempt).map_err(|_| WebError::invalid())?;
    let cookie =
        security::cookie(&headers, security::LOGIN_COOKIE)?.ok_or(AuthError::CsrfRejected)?;
    let csrf = SensitiveCookie::new(
        security::one(&headers, "x-forme-csrf")?
            .ok_or(AuthError::CsrfRejected)?
            .into(),
    );
    let existing = security::cookie(&headers, security::SESSION_COOKIE)?;
    let session = state
        .blocking(true, move |identity| {
            identity.login_challenged(
                SensitiveLoginRequest {
                    login_name: body.login_name,
                    password: body.password,
                },
                LoginChallengeBinding {
                    cookie: &cookie,
                    csrf: &csrf,
                    attempt: &attempt,
                    existing_session: existing.as_ref(),
                },
            )
        })
        .await?;
    let mut response = Json(serde_json::json!({"schema_version":1, "session":session.view, "csrf":session.csrf.expose_for_transport()})).into_response();
    security::set_cookie(
        &mut response,
        security::SESSION_COOKIE,
        session.cookie.expose_for_transport(),
        43_200,
    )?;
    Ok(response)
}

pub(super) async fn session(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let cookie = session_cookie(&headers)?;
    let view = state.blocking(false, move |identity| {
        let owner = identity.authenticate(&cookie, WebRequestBinding { mutation:false, csrf:None })?;
        let csrf = identity.csrf_for_session(&cookie)?;
        let operation = WebIdentityStore::revoke_operation(&owner)?;
        Ok(serde_json::json!({"schema_version":1, "session":owner.view(), "csrf":csrf.expose_for_transport(), "revoke_operation":operation}))
    }).await?;
    Ok(Json(view).into_response())
}

pub(super) async fn logout(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, WebError> {
    let cookie = session_cookie(&headers)?;
    let binding = mutation(&headers)?;
    let owner = state
        .blocking(false, move |identity| {
            identity.authenticate(&cookie, binding)
        })
        .await?;
    let _fence_gate = state.fence_gate().await;
    state
        .blocking(false, move |identity| identity.logout(&owner))
        .await?;
    let synced = matches!(state.sync_fences_locked().await, Ok(FenceSync::Clean));
    let mut response = fence_response(synced);
    security::set_cookie(&mut response, security::SESSION_COOKIE, "", 0)?;
    security::set_cookie(&mut response, security::LOGIN_COOKIE, "", 0)?;
    Ok(response)
}

pub(super) async fn reauthenticate(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<ReauthBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let body = json(body)?;
    version(body.schema_version)?;
    let cookie = session_cookie(&headers)?;
    let binding = mutation(&headers)?;
    let proof = state
        .blocking(true, move |identity| {
            let owner = identity.authenticate(&cookie, binding)?;
            identity.reauthenticate(&owner, body.password, &body.operation_digest)
        })
        .await?;
    Ok(Json(serde_json::json!({"schema_version":1, "proof":proof.expose_for_transport(), "expires_in_seconds":300})).into_response())
}

pub(super) async fn revoke(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<RevokeBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let body = json(body)?;
    version(body.schema_version)?;
    let cookie = session_cookie(&headers)?;
    let binding = mutation(&headers)?;
    let RevokeBody {
        operation_digest,
        proof,
        ..
    } = body;
    let owner = state
        .blocking(false, move |identity| {
            identity.authenticate(&cookie, binding)
        })
        .await?;
    let command = RevokeSessions {
        operation_digest,
        proof,
    };
    let command = state
        .blocking(false, {
            let owner = owner.clone();
            move |identity| {
                identity.validate_revocation(&owner, &command)?;
                Ok(command)
            }
        })
        .await?;
    let _fence_gate = state.fence_gate().await;
    let revision = state
        .blocking(false, move |identity| identity.revoke(&owner, command))
        .await?;
    let synced = matches!(state.sync_fences_locked().await, Ok(FenceSync::Clean));
    let mut response = if synced {
        Json(revision).into_response()
    } else {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "schema_version": 1,
                "state": "local_revoked_core_sync_pending",
                "auth_epoch": revision.auth_epoch,
                "credential_revision": revision.credential_revision,
            })),
        )
            .into_response()
    };
    security::set_cookie(&mut response, security::SESSION_COOKIE, "", 0)?;
    Ok(response)
}

pub(super) async fn password(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: Result<Json<PasswordBody>, JsonRejection>,
) -> Result<Response, WebError> {
    let body = json(body)?;
    version(body.schema_version)?;
    let cookie = session_cookie(&headers)?;
    let operation_id = key(&headers)?.to_owned();
    let binding = mutation(&headers)?;
    let PasswordBody {
        old_password,
        new_password,
        ..
    } = body;
    let owner = state
        .blocking(false, move |identity| {
            identity.authenticate(&cookie, binding)
        })
        .await?;
    let prepared = state
        .blocking(true, {
            let owner = owner.clone();
            move |identity| identity.prepare_password_change(&owner, &operation_id, new_password)
        })
        .await?;
    let operation_digest = prepared.operation_digest().clone();
    let proof = state
        .blocking(true, {
            let owner = owner.clone();
            let operation_digest = operation_digest.clone();
            move |identity| identity.reauthenticate(&owner, old_password, &operation_digest)
        })
        .await?;
    let _fence_gate = state.fence_gate().await;
    let revision = state
        .blocking(false, move |identity| {
            identity.change_password(
                &owner,
                RevokeSessions {
                    operation_digest,
                    proof,
                },
                prepared,
            )
        })
        .await?;
    let synced = matches!(state.sync_fences_locked().await, Ok(FenceSync::Clean));
    let mut response = if synced {
        Json(revision).into_response()
    } else {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "schema_version": 1,
                "state": "local_revoked_core_sync_pending",
                "auth_epoch": revision.auth_epoch,
                "credential_revision": revision.credential_revision,
            })),
        )
            .into_response()
    };
    security::set_cookie(&mut response, security::SESSION_COOKIE, "", 0)?;
    Ok(response)
}

pub(super) async fn missing() -> WebError {
    WebError::new(StatusCode::NOT_FOUND, "NotFound", "此接口不可用")
}

fn fence_response(synced: bool) -> Response {
    if synced {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "schema_version": 1,
                "state": "local_revoked_core_sync_pending",
            })),
        )
            .into_response()
    }
}
