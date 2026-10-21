use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use forme_protocol::SchemaVersion;
use serde::Serialize;

use super::super::identity::AuthError;

#[derive(Debug, Serialize)]
pub(super) struct ApiProblem {
    schema_version: SchemaVersion,
    code: &'static str,
    safe_message: &'static str,
    retryable: bool,
}

#[derive(Debug)]
pub(super) struct WebError {
    status: StatusCode,
    problem: ApiProblem,
}

impl WebError {
    pub(super) fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            problem: ApiProblem {
                schema_version: SchemaVersion(1),
                code,
                safe_message: message,
                retryable: matches!(status.as_u16(), 429 | 503),
            },
        }
    }

    pub(super) fn invalid() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "InvalidInput",
            "请求格式或版本无效",
        )
    }
}

impl From<AuthError> for WebError {
    fn from(error: AuthError) -> Self {
        let (status, code, message) = match error {
            AuthError::InvalidInput => (400, "InvalidInput", "请求格式或版本无效"),
            AuthError::Unauthenticated => (401, "Unauthenticated", "请登录，或核对账号与密码"),
            AuthError::CsrfRejected => (403, "CsrfRejected", "请求来源或表单绑定无效，请刷新页面"),
            AuthError::Conflict => (409, "Conflict", "状态已改变，请刷新后重新操作"),
            AuthError::Expired => (410, "Expired", "验证已过期，请重新操作"),
            AuthError::Busy => (429, "RateLimited", "请求过于频繁，请稍后重试"),
            AuthError::Unavailable => (503, "ServiceUnavailable", "认证服务暂时不可用"),
            AuthError::RecoveryRequired => (503, "RecoveryRequired", "认证状态需要本机维护"),
        };
        Self::new(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            code,
            message,
        )
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.problem)).into_response();
        if self.status == StatusCode::TOO_MANY_REQUESTS {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("60"),
            );
        }
        response
    }
}
