//! Authentication adapter for the independent Web process.
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod session_fence;
use session_fence::SessionFence;

use axum::body::{to_bytes, Body};
use axum::extract::{Extension, Json, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use forme_protocol as p;

use super::{now_ms, ApiError, HttpState, CSRF_HEADER};

#[derive(Clone)]
pub(crate) struct LocalBearerSecret(pub(super) Arc<str>);

pub(super) struct WebBridgeState {
    pub(super) install_ref: Option<String>,
    pub(super) token: Option<LocalBearerSecret>,
    pub(super) verifying_key: Option<VerifyingKey>,
    fence: Option<SessionFence>,
}

#[derive(Clone)]
pub(super) struct WebBridgeAssertion(pub(super) p::CoreSessionAssertion);

impl WebBridgeState {
    pub(super) fn open(
        install_ref: Option<String>,
        token: Option<LocalBearerSecret>,
        verifying_key: Option<VerifyingKey>,
        fence_path: Option<&Path>,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<Self> {
        let fence = fence_path
            .map(|path| SessionFence::open(path, install_ref.as_deref().unwrap_or_default(), owner))
            .transpose()?;
        Ok(Self {
            install_ref,
            token,
            verifying_key,
            fence,
        })
    }
}

impl LocalBearerSecret {
    pub(super) fn new(value: String) -> p::Result<Self> {
        if value.len() < 32 || value.chars().any(char::is_whitespace) {
            return Err(p::Error(
                "gateway bearer secret must contain at least 32 non-whitespace characters".into(),
            ));
        }
        Ok(Self(Arc::from(value)))
    }

    pub(super) fn matches(&self, candidate: &str) -> bool {
        let expected = self.0.as_bytes();
        let actual = candidate.as_bytes();
        if expected.len() != actual.len() {
            return false;
        }
        expected
            .iter()
            .zip(actual)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }
}

impl fmt::Debug for LocalBearerSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocalBearerSecret(<redacted>)")
    }
}

const CORE_ASSERTION_HEADER: &str = "x-forme-core-assertion";
const CORE_REQUEST_DIGEST_HEADER: &str = "x-forme-request-digest";

pub(super) struct WebBridgeConfiguration {
    pub(super) install_ref: Option<String>,
    pub(super) web_bridge_token: Option<LocalBearerSecret>,
    pub(super) web_assertion_verifying_key: Option<VerifyingKey>,
    pub(super) fence_path: Option<PathBuf>,
}

pub(super) fn load_configuration() -> p::Result<WebBridgeConfiguration> {
    let install_ref = std::env::var("FORME_INSTALL_ID").ok();
    let web_bridge_token = std::env::var("FORME_WEB_BRIDGE_TOKEN")
        .ok()
        .map(LocalBearerSecret::new)
        .transpose()?;
    let web_assertion_verifying_key = std::env::var("FORME_WEB_ASSERTION_VERIFYING_KEY")
        .ok()
        .map(|value| {
            let bytes = decode_hex(&value)?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| p::Error("FORME_WEB_ASSERTION_VERIFYING_KEY is invalid".into()))?;
            VerifyingKey::from_bytes(&bytes)
                .map_err(|_| p::Error("FORME_WEB_ASSERTION_VERIFYING_KEY is invalid".into()))
        })
        .transpose()?;
    let fence_path = std::env::var_os("FORME_WEB_SESSION_FENCE_PATH").map(PathBuf::from);
    Ok(WebBridgeConfiguration {
        install_ref,
        web_bridge_token,
        web_assertion_verifying_key,
        fence_path,
    })
}

pub(super) fn validate_configuration(
    install_ref: Option<&str>,
    web_bridge_token: Option<&LocalBearerSecret>,
    web_assertion_verifying_key: Option<&VerifyingKey>,
    fence_path: Option<&Path>,
    gateway_token: &LocalBearerSecret,
) -> p::Result<()> {
    match (
        install_ref,
        web_bridge_token,
        web_assertion_verifying_key,
        fence_path,
    ) {
        (None, None, None, None) => Ok(()),
        (Some(install), Some(web_token), Some(_), Some(path)) => {
            p::validate_product_id(install)?;
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir | std::path::Component::CurDir))
            {
                return Err(p::Error("Web session fence path is invalid".into()));
            }
            if web_token.matches(&gateway_token.0) {
                return Err(p::Error(
                    "Web bridge credential must be distinct from the owner gateway credential"
                        .into(),
                ));
            }
            Ok(())
        }
        _ => Err(p::Error(
            "Web bridge configuration must provide install, credential, verification key and fence together".into(),
        )),
    }
}

pub(super) async fn authenticate_web_bridge(
    State(state): State<Arc<HttpState>>,
    request: Request,
    next: Next,
) -> Response {
    let bridge_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let is_bridge = bridge_token.is_some_and(|token| {
        state
            .web_bridge
            .token
            .as_ref()
            .is_some_and(|expected| expected.matches(token))
    });
    if !is_bridge {
        return next.run(request).await;
    }

    match authenticate_web_request(&state, request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

async fn authenticate_web_request(
    state: &HttpState,
    request: Request,
) -> Result<Request, ApiError> {
    let (mut parts, body) = request.into_parts();
    let body = to_bytes(body, state.max_body_bytes)
        .await
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "core bridge body is invalid"))?;
    let request_digest = p::canonical_digest(&(
        p::SchemaVersion(1),
        "forme-web-core-request-v1",
        parts.method.as_str(),
        parts
            .uri
            .path_and_query()
            .map(|value| value.as_str())
            .unwrap_or(parts.uri.path()),
        body.as_ref(),
    ))
    .map_err(ApiError::from)?;
    let supplied_digest = parts
        .headers
        .get(CORE_REQUEST_DIGEST_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| p::SchemaDigest(value.to_owned()))
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "core request proof is missing"))?;
    if supplied_digest != request_digest {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "core request proof does not match the request",
        ));
    }
    let assertion_hex = parts
        .headers
        .get(CORE_ASSERTION_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "core session proof is missing"))?;
    let assertion_bytes = decode_hex(assertion_hex).map_err(ApiError::from)?;
    let assertion: p::CoreSessionAssertion = serde_json::from_slice(&assertion_bytes)
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "core session proof is invalid"))?;
    assertion
        .validate()
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "core session proof is invalid"))?;
    if state.web_bridge.install_ref.as_deref() != Some(assertion.install_ref.as_str())
        || state.owner.0 != assertion.owner.0
        || assertion.request_digest != request_digest
        || assertion.expires_at <= now_ms()
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "core session proof is not current",
        ));
    }
    let key = state.web_bridge.verifying_key.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Web bridge verification is not configured",
        )
    })?;
    let signature = Signature::from_slice(&assertion.signature)
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "core session proof is invalid"))?;
    let digest = assertion.signing_digest().map_err(ApiError::from)?;
    key.verify(digest.0.as_bytes(), &signature)
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "core session proof is invalid"))?;

    state
        .web_bridge
        .fence
        .as_ref()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "core session fence is unavailable",
            )
        })?
        // Fence re-delivery is the sole recovery operation allowed after its
        // previous acknowledgement was lost. It cannot run a brain command.
        .admit(
            &assertion,
            now_ms(),
            parts.uri.path() == "/v1/web/session-fence",
        )
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "core session proof was rejected"))?;

    parts
        .extensions
        .insert(WebBridgeAssertion(assertion.clone()));

    let owner_token = HeaderValue::from_str(&format!("Bearer {}", state.token.0))
        .map_err(|_| ApiError::internal("gateway authorization could not be prepared"))?;
    parts.headers.insert(header::AUTHORIZATION, owner_token);
    if parts.method != axum::http::Method::GET && parts.method != axum::http::Method::HEAD {
        let origin = HeaderValue::from_str(&state.origin)
            .map_err(|_| ApiError::internal("gateway origin could not be prepared"))?;
        parts.headers.insert(header::ORIGIN, origin);
        parts
            .headers
            .insert(CSRF_HEADER, HeaderValue::from_static("1"));
    }
    Ok(Request::from_parts(parts, Body::from(body)))
}

pub(super) async fn apply_session_fence(
    State(state): State<Arc<HttpState>>,
    assertion: Option<Extension<WebBridgeAssertion>>,
    Json(update): Json<p::WebSessionFenceUpdate>,
) -> Result<StatusCode, ApiError> {
    let Some(Extension(WebBridgeAssertion(assertion))) = assertion else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "Web session fence requires a Web bridge assertion",
        ));
    };
    update
        .validate()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "Web session fence is invalid"))?;
    if update.install_ref != assertion.install_ref
        || update.owner != assertion.owner
        || update.minimum_auth_epoch <= assertion.auth_epoch
        || update
            .session_digest
            .as_ref()
            .is_some_and(|session| session != &assertion.session_digest)
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "Web session fence is not bound to the assertion",
        ));
    }
    let fence = state.web_bridge.fence.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Web session fence is not configured",
        )
    })?;
    fence.apply(&update).map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Web session fence unavailable",
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

fn decode_hex(value: &str) -> p::Result<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(p::Error("hex configuration value is invalid".into()));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| p::Error("hex configuration value is invalid".into()))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| p::Error("hex configuration value is invalid".into()))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}
