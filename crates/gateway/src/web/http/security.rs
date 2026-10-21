//! Host/Origin checks run before parsing bodies or invoking identity/brain code.
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{WebError, WebState};
use crate::web::identity::{AuthError, SensitiveCookie};

pub(super) const SESSION_COOKIE: &str = "forme_local_session";
pub(super) const LOGIN_COOKIE: &str = "forme_local_login";
pub(super) const SETUP_COOKIE: &str = "forme_local_setup";

pub(super) struct RateWindow {
    since: Instant,
    used: [u32; 3],
}

impl RateWindow {
    pub(super) fn new() -> Self {
        Self {
            since: Instant::now(),
            used: [0; 3],
        }
    }

    fn take(&mut self, bucket: usize) -> Result<(), WebError> {
        if self.since.elapsed().as_secs() >= 60 {
            self.since = Instant::now();
            self.used = [0; 3];
        }
        if self.used[bucket] >= [16, 60, 600][bucket] {
            return Err(AuthError::Busy.into());
        }
        self.used[bucket] += 1;
        Ok(())
    }
}

pub(super) fn one<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, WebError> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|v| v.to_str().map_err(|_| WebError::invalid()))
        .transpose()?;
    if values.next().is_some() {
        return Err(WebError::invalid());
    }
    Ok(value)
}

fn check(state: &WebState, headers: &HeaderMap, method: &Method) -> Result<(), WebError> {
    let invalid = || {
        WebError::new(
            StatusCode::FORBIDDEN,
            "RequestBindingRejected",
            "请求来源不受信任",
        )
    };
    if one(headers, "host")? != Some(state.authority.as_str()) {
        return Err(invalid());
    }
    if headers.keys().any(|name| {
        let name = name.as_str();
        name.starts_with("x-forwarded-")
            || matches!(name, "forwarded" | "x-original-url" | "x-rewrite-url")
    }) {
        return Err(invalid());
    }
    if let Some(site) = one(headers, "sec-fetch-site")? {
        if !matches!(site, "same-origin" | "none") {
            return Err(invalid());
        }
    }
    let origin = one(headers, "origin")?;
    if origin.is_some_and(|origin| origin != state.origin)
        || (*method != Method::GET && *method != Method::HEAD && origin.is_none())
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) async fn boundary(
    State(state): State<Arc<WebState>>,
    request: Request,
    next: Next,
) -> Response {
    let result = check(&state, request.headers(), request.method()).and_then(|()| {
        let path = request.uri().path();
        let bucket = if matches!(
            path,
            "/api/v1/auth/login"
                | "/api/v1/bootstrap/prove"
                | "/api/v1/bootstrap/claim"
                | "/api/v1/auth/login-challenge"
                | "/api/v1/auth/reauth"
                | "/api/v1/auth/password"
        ) {
            0
        } else if matches!(*request.method(), Method::GET | Method::HEAD) {
            2
        } else {
            1
        };
        state
            .rate
            .lock()
            .map_err(|_| WebError::from(AuthError::Unavailable))?
            .take(bucket)
    });
    let mut response = match result {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    for (name, value) in [
        ("cache-control", "no-store"),
        ("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),
        ("x-content-type-options", "nosniff"), ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"), ("cross-origin-resource-policy", "same-origin"),
        ("cross-origin-opener-policy", "same-origin"),
    ] {
        response.headers_mut().insert(name, HeaderValue::from_static(value));
    }
    response
}

pub(super) fn cookie(headers: &HeaderMap, name: &str) -> Result<Option<SensitiveCookie>, WebError> {
    let mut found = None;
    let mut total = 0;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| WebError::invalid())?;
        total += value.len();
        if total > 4096 {
            return Err(WebError::invalid());
        }
        for part in value.split(';') {
            if let Some((key, value)) = part.trim().split_once('=') {
                if key == name {
                    if found.is_some()
                        || value.len() != 64
                        || !value
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    {
                        return Err(WebError::invalid());
                    }
                    found = Some(SensitiveCookie::new(value.into()));
                }
            }
        }
    }
    Ok(found)
}

pub(super) fn set_cookie(
    response: &mut Response,
    name: &str,
    value: &str,
    age: u32,
) -> Result<(), WebError> {
    let header = HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={age}"
    ))
    .map_err(|_| WebError::from(AuthError::Unavailable))?;
    response.headers_mut().append(header::SET_COOKIE, header);
    Ok(())
}
