//! Web session to core gateway adapter.
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use super::{auth, security, WebError, WebState};
use crate::web::identity::WebIdentityService;

#[path = "core/commands.rs"]
mod commands;

pub(super) async fn read(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WebError> {
    forward(state, Method::GET, path, uri.query(), headers, body, false).await
}

pub(super) async fn mutate(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WebError> {
    forward(state, Method::POST, path, uri.query(), headers, body, true).await
}

pub(super) async fn replace(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WebError> {
    forward(state, Method::PUT, path, uri.query(), headers, body, true).await
}

async fn forward(
    state: Arc<WebState>,
    method: Method,
    path: String,
    query: Option<&str>,
    headers: HeaderMap,
    body: Bytes,
    mutation: bool,
) -> Result<Response, WebError> {
    let client = state.core.clone().ok_or_else(|| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "CoreIntegrationPending",
            "主脑服务尚未配置，请使用本机维护工具",
        )
    })?;
    let command_key = if mutation {
        Some(auth::key(&headers)?.to_owned())
    } else {
        None
    };
    let binding = if mutation {
        auth::mutation(&headers)?
    } else {
        super::super::identity::WebRequestBinding {
            mutation: false,
            csrf: None,
        }
    };
    let cookie = security::cookie(&headers, security::SESSION_COOKIE)?
        .ok_or(crate::web::identity::AuthError::Unauthenticated)?;
    let owner = state
        .blocking(false, move |identity| {
            identity.authenticate(&cookie, binding)
        })
        .await?;
    let _fence_gate = state.core_access().await?;
    let core_path = allowlisted_core_path(method.as_str(), &path, query)?;
    let body = commands::bind_body(
        method.as_str(),
        &path,
        &body,
        owner.view(),
        command_key.as_deref(),
    )?;
    let method_name = method.as_str().to_owned();
    let owner_binding = owner.core_binding();
    let lease = state
        .admission
        .acquire()
        .map_err(|_| crate::web::identity::AuthError::Unavailable)?;
    let permit = state
        .reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| crate::web::identity::AuthError::Busy)?;
    let response = tokio::task::spawn_blocking(move || {
        let (_lease, _permit) = (lease, permit);
        client.request(&owner_binding, &method_name, &core_path, &body)
    })
    .await
    .map_err(|_| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "CoreUnavailable",
            "主脑服务不可用",
        )
    })?
    .map_err(|_| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "CoreUnavailable",
            "主脑服务不可用",
        )
    })?;
    let status = StatusCode::from_u16(response.status).map_err(|_| {
        WebError::new(
            StatusCode::BAD_GATEWAY,
            "CoreProtocolError",
            "主脑响应状态无效",
        )
    })?;
    if !status.is_success() {
        return Err(WebError::new(
            status,
            if status == StatusCode::CONFLICT {
                "Conflict"
            } else {
                "CoreCommandRejected"
            },
            "主脑未接受该请求，请刷新当前状态并检查权限或操作绑定",
        ));
    }
    let mut output = (status, response.body).into_response();
    if let Some(content_type) = response.content_type {
        if let Ok(value) = HeaderValue::from_str(&content_type) {
            output.headers_mut().insert("content-type", value);
        }
    }
    output
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    Ok(output)
}

#[derive(Clone, Copy)]
enum RouteMethods {
    Get,
    Post,
    GetPost,
    GetPostPut,
}

impl RouteMethods {
    fn accepts(self, method: &str) -> bool {
        match self {
            Self::Get => method == "GET",
            Self::Post => method == "POST",
            Self::GetPost => matches!(method, "GET" | "POST"),
            Self::GetPostPut => matches!(method, "GET" | "POST" | "PUT"),
        }
    }
}

fn allowlisted_core_path(
    method: &str,
    path: &str,
    query: Option<&str>,
) -> Result<String, WebError> {
    let segments: Vec<&str> = path.split('/').collect();
    let (core_path, methods) = match segments.as_slice() {
        ["profile"] => ("/v1/profile".to_owned(), RouteMethods::Get),
        ["runtime", "manifest"] => ("/v1/runtime/manifest".to_owned(), RouteMethods::Get),
        ["runs"] => ("/v1/runs".to_owned(), RouteMethods::GetPost),
        ["runs", run] => (format!("/v1/runs/{run}"), RouteMethods::Get),
        ["runs", run, "events"] => (format!("/v1/runs/{run}/events"), RouteMethods::Get),
        ["runs", run, "answer"] => (format!("/v1/runs/{run}/answer"), RouteMethods::Get),
        ["runs", run, "control"] => (format!("/v1/runs/{run}/control"), RouteMethods::Post),
        ["runs", run, "trace"] => (format!("/v1/runs/{run}/trace"), RouteMethods::Get),
        ["sessions", session, "approvals"] => (
            format!("/v1/sessions/{session}/approvals"),
            RouteMethods::Get,
        ),
        ["jobs"] => ("/v1/jobs".to_owned(), RouteMethods::GetPost),
        ["jobs", intention, "cancel"] => {
            (format!("/v1/jobs/{intention}/cancel"), RouteMethods::Post)
        }
        ["workspace"] => ("/v1/brain/workspace".to_owned(), RouteMethods::Get),
        ["dashboard"] => ("/v1/brain/dashboard".to_owned(), RouteMethods::Get),
        ["workbench", "query"] => ("/v1/brain/workbench/query".to_owned(), RouteMethods::Post),
        ["workbench", "review"] => ("/v1/brain/workbench/review".to_owned(), RouteMethods::Post),
        ["workbench", "proposal"] => (
            "/v1/brain/workbench/proposal".to_owned(),
            RouteMethods::Post,
        ),
        ["projects"] => ("/v1/brain/projects".to_owned(), RouteMethods::GetPost),
        ["learning", "goals"] => ("/v1/brain/learning/goals".to_owned(), RouteMethods::Post),
        ["learning", "answer"] => ("/v1/brain/learning/answer".to_owned(), RouteMethods::Post),
        ["learning", "resolve"] => ("/v1/brain/learning/resolve".to_owned(), RouteMethods::Post),
        ["artifacts", "upload"] => ("/v1/artifacts/upload".to_owned(), RouteMethods::Post),
        ["artifacts", artifact] => (format!("/v1/artifacts/{artifact}"), RouteMethods::Get),
        ["workspaces", workspace, "charter"] => (
            format!("/v1/workspaces/{workspace}/charter"),
            RouteMethods::GetPostPut,
        ),
        ["evals", "cases"] => ("/v1/evals/cases".to_owned(), RouteMethods::Get),
        ["evals", "run"] => ("/v1/evals/run".to_owned(), RouteMethods::Post),
        ["evals", eval] => (format!("/v1/evals/{eval}"), RouteMethods::Get),
        _ => {
            return Err(WebError::new(
                StatusCode::NOT_FOUND,
                "NotFound",
                "请求路径不存在",
            ))
        }
    };

    if !methods.accepts(method) {
        return Err(WebError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "MethodNotAllowed",
            "请求方法不适用于该路径",
        ));
    }

    for segment in segments.iter().filter(|segment| {
        !matches!(
            **segment,
            "profile"
                | "runtime"
                | "manifest"
                | "runs"
                | "events"
                | "answer"
                | "control"
                | "trace"
                | "sessions"
                | "approvals"
                | "jobs"
                | "cancel"
                | "workspace"
                | "dashboard"
                | "projects"
                | "learning"
                | "goals"
                | "resolve"
                | "workspaces"
                | "charter"
                | "evals"
                | "cases"
                | "run"
                | "candidates"
                | "review"
                | "workbench"
                | "query"
                | "proposal"
                | "artifacts"
                | "upload"
        )
    }) {
        forme_protocol::validate_product_id(segment).map_err(|_| WebError::invalid())?;
    }

    match query {
        None => Ok(core_path),
        Some(value) if matches!(segments.as_slice(), ["runs", _, "events"]) => {
            let after = value
                .strip_prefix("after=")
                .filter(|after| {
                    !after.is_empty() && after.bytes().all(|byte| byte.is_ascii_digit())
                })
                .ok_or_else(WebError::invalid)?;
            let after = after.parse::<u64>().map_err(|_| WebError::invalid())?;
            Ok(format!("{core_path}?after={after}"))
        }
        Some(value) if matches!(segments.as_slice(), ["artifacts", _]) => {
            let session = value
                .strip_prefix("session=")
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 256
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'%' | b':' | b'_' | b'-')
                        })
                })
                .ok_or_else(WebError::invalid)?;
            Ok(format!("{core_path}?session={session}"))
        }
        Some(_) => Err(WebError::invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::allowlisted_core_path;

    #[test]
    fn core_routes_are_explicit_and_preserve_event_cursor() {
        assert_eq!(
            allowlisted_core_path("GET", "runs/run:one/events", Some("after=7")).unwrap(),
            "/v1/runs/run:one/events?after=7"
        );
        assert!(allowlisted_core_path("GET", "runtime/doctor", None).is_err());
        assert!(allowlisted_core_path("GET", "runs/run:one/events", Some("before=7")).is_err());
        assert!(allowlisted_core_path("GET", "runs/../events", None).is_err());
    }

    #[test]
    fn product_routes_are_mapped_without_exposing_a_wildcard_proxy() {
        assert_eq!(
            allowlisted_core_path("GET", "workspace", None).unwrap(),
            "/v1/brain/workspace"
        );
        assert_eq!(
            allowlisted_core_path("GET", "workspaces/workspace:demo/charter", None).unwrap(),
            "/v1/workspaces/workspace:demo/charter"
        );
        assert_eq!(
            allowlisted_core_path("GET", "runs/run:one", None).unwrap(),
            "/v1/runs/run:one"
        );
        assert_eq!(
            allowlisted_core_path("PUT", "workspaces/workspace:demo/charter", None).unwrap(),
            "/v1/workspaces/workspace:demo/charter"
        );
        assert_eq!(
            allowlisted_core_path("POST", "workspaces/workspace:demo/charter", None).unwrap(),
            "/v1/workspaces/workspace:demo/charter"
        );
        assert_eq!(
            allowlisted_core_path(
                "GET",
                "artifacts/content:fixture",
                Some("session=session%3Aone")
            )
            .unwrap(),
            "/v1/artifacts/content:fixture?session=session%3Aone"
        );
        assert!(
            allowlisted_core_path("GET", "artifacts/content:fixture", Some("scope=other")).is_err()
        );
        assert!(allowlisted_core_path("GET", "runtime/doctor", None).is_err());
        assert!(allowlisted_core_path("GET", "unknown", None).is_err());
        assert!(allowlisted_core_path("POST", "candidates/candidate:one/review", None).is_err());
        assert!(allowlisted_core_path("POST", "profile", None).is_err());
        assert!(allowlisted_core_path("DELETE", "runs/run:one/control", None).is_err());
    }
}
