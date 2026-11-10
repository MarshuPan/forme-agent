//! Web session to core gateway adapter.
use serde::Deserialize;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use forme_protocol as p;

use super::{auth, security, WebError, WebState};
use crate::web::identity::WebIdentityService;

#[derive(Deserialize)]
struct RuntimeWorkspaceSnapshot {
    schema_version: p::SchemaVersion,
    workspace: p::WorkspaceRef,
}

#[path = "core/commands.rs"]
mod commands;
#[path = "core/ecosystem.rs"]
mod ecosystem;

pub(super) async fn upload(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WebError> {
    forward(
        state,
        Method::POST,
        "artifacts/upload".into(),
        uri.query(),
        headers,
        body,
        true,
    )
    .await
}

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
    let requested_core_path = allowlisted_core_path(method.as_str(), &path, query)?;
    let mut workbench_receipt = None;
    let mut proposal_recovery = None;
    let (core_path, body) = if method == Method::POST && path == "runs" {
        let bound = commands::bind_body(
            method.as_str(),
            &path,
            &body,
            owner.view(),
            command_key.as_deref(),
        )?;
        let request: p::RunRequest =
            serde_json::from_slice(&bound).map_err(|_| WebError::invalid())?;
        let client_for_default = client.clone();
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
        let manifest = tokio::task::spawn_blocking(move || {
            let (_lease, _permit) = (lease, permit);
            client_for_default.request(&owner_binding, "GET", "/v1/runtime/manifest", &[])
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
        if manifest.status != StatusCode::OK.as_u16() {
            return Err(WebError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "CoreUnavailable",
                "无法确认主脑当前工作空间",
            ));
        }
        let runtime: RuntimeWorkspaceSnapshot =
            serde_json::from_slice(&manifest.body).map_err(|_| WebError::invalid())?;
        if runtime.schema_version != p::SchemaVersion(1) || runtime.workspace.0.trim().is_empty() {
            return Err(WebError::invalid());
        }
        let owner_for_binding = owner.clone();
        let request_for_binding = request.clone();
        let default_workspace = runtime.workspace;
        let stored = state
            .blocking(false, move |identity| {
                identity.bind_workbench_run(
                    &owner_for_binding,
                    &request_for_binding,
                    &default_workspace,
                )
            })
            .await?;
        let submission = p::WorkbenchRunSubmission {
            schema_version: p::SchemaVersion(1),
            session: p::SessionId(request.session.0.clone()),
            workspace: stored.workspace,
            request: request.clone(),
        };
        submission.validate().map_err(|_| WebError::invalid())?;
        workbench_receipt = Some((owner.clone(), request, stored.run));
        (
            "/v1/workbench/runs".to_owned(),
            serde_json::to_vec(&submission).map_err(|_| WebError::invalid())?,
        )
    } else {
        let bound = commands::bind_body(
            method.as_str(),
            &path,
            &body,
            owner.view(),
            command_key.as_deref(),
        )?;
        if method == Method::POST && path == "workbench/proposal" {
            let command: p::WorkbenchProposalCommand =
                serde_json::from_slice(&bound).map_err(|_| WebError::invalid())?;
            let record_owner = owner.clone();
            let record_command = command.clone();
            state
                .blocking(false, move |identity| {
                    identity.prepare_workbench_proposal(&record_owner, &record_command)
                })
                .await?;
            proposal_recovery = Some((owner.clone(), command));
        }
        (requested_core_path, bound)
    };
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
        if status == StatusCode::CONFLICT {
            if let Some((recovery_owner, command)) = proposal_recovery.take() {
                state
                    .blocking(false, move |identity| {
                        identity.reject_workbench_proposal_conflict(&recovery_owner, &command)
                    })
                    .await?;
            }
        }
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
    if let Some((receipt_owner, receipt_request, expected_run)) = workbench_receipt {
        let run: p::RunId = serde_json::from_slice(&response.body).map_err(|_| {
            WebError::new(
                StatusCode::BAD_GATEWAY,
                "CoreProtocolError",
                "主脑运行回执无效",
            )
        })?;
        if expected_run
            .as_ref()
            .is_some_and(|expected| expected != &run)
        {
            return Err(WebError::new(
                StatusCode::BAD_GATEWAY,
                "CoreProtocolError",
                "主脑返回的原运行回执不一致",
            ));
        }
        state
            .blocking(false, move |identity| {
                identity.record_workbench_run(&receipt_owner, &receipt_request, &run)
            })
            .await?;
    }
    if let Some((recovery_owner, command)) = proposal_recovery {
        let receipt: p::WorkbenchCommandReceipt =
            serde_json::from_slice(&response.body).map_err(|_| {
                WebError::new(
                    StatusCode::BAD_GATEWAY,
                    "CoreProtocolError",
                    "主脑提议处置回执无效",
                )
            })?;
        let binding = p::canonical_digest(&(
            p::VerifiedPrincipal(recovery_owner.view().owner.0.clone()),
            &command,
        ))
        .map_err(|_| {
            WebError::new(
                StatusCode::BAD_GATEWAY,
                "CoreProtocolError",
                "主脑提议处置回执无效",
            )
        })?;
        if receipt.schema_version != p::SchemaVersion(1)
            || receipt.command != command.command
            || receipt.binding != binding
        {
            return Err(WebError::new(
                StatusCode::BAD_GATEWAY,
                "CoreProtocolError",
                "主脑提议处置回执与原操作不匹配",
            ));
        }
        if receipt.status != p::WorkbenchCommandStatus::Unknown {
            state
                .blocking(false, move |identity| {
                    identity.finish_workbench_proposal(&recovery_owner, &command, &receipt)
                })
                .await?;
        }
    }
    let mut output = (status, response.body).into_response();
    if let Some(content_type) = response.content_type {
        if let Ok(value) = HeaderValue::from_str(&content_type) {
            output.headers_mut().insert("content-type", value);
        }
    }
    if let Some(content_disposition) = response.content_disposition {
        if content_disposition != "attachment" {
            return Err(WebError::new(
                StatusCode::BAD_GATEWAY,
                "CoreProtocolError",
                "主脑附件响应处置头无效",
            ));
        }
        output.headers_mut().insert(
            "content-disposition",
            HeaderValue::from_static("attachment"),
        );
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
    if path.starts_with("ecosystem/") {
        return ecosystem::path(method, path, query);
    }
    let segments: Vec<&str> = path.split('/').collect();
    let (core_path, methods) = match segments.as_slice() {
        ["profile"] => ("/v1/profile".to_owned(), RouteMethods::Get),
        ["runtime", "manifest"] => ("/v1/runtime/manifest".to_owned(), RouteMethods::Get),
        ["runtime", "doctor"] => ("/v1/runtime/doctor".to_owned(), RouteMethods::Get),
        ["runs"] => ("/v1/runs".to_owned(), RouteMethods::GetPost),
        ["runs", run] => (format!("/v1/runs/{run}"), RouteMethods::Get),
        ["runs", run, "events"] => (format!("/v1/runs/{run}/events"), RouteMethods::Get),
        ["runs", run, "answer"] => (format!("/v1/runs/{run}/answer"), RouteMethods::Get),
        ["runs", run, "pending"] => (format!("/v1/runs/{run}/pending"), RouteMethods::Get),
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
        ["workbench", "material"] => (
            "/v1/brain/workbench/material".to_owned(),
            RouteMethods::Post,
        ),
        ["workbench", "material-lifecycle"] => (
            "/v1/brain/workbench/material-lifecycle".to_owned(),
            RouteMethods::Post,
        ),
        ["workbench", "review"] => ("/v1/brain/workbench/review".to_owned(), RouteMethods::Post),
        ["workbench", "correction"] => (
            "/v1/brain/workbench/correction".to_owned(),
            RouteMethods::Post,
        ),
        ["workbench", "corrections", "query"] => (
            "/v1/brain/workbench/corrections/query".to_owned(),
            RouteMethods::Post,
        ),
        ["workbench", "proposal"] => (
            "/v1/brain/workbench/proposal".to_owned(),
            RouteMethods::Post,
        ),
        ["projects"] => ("/v1/brain/projects".to_owned(), RouteMethods::GetPost),
        ["projects", "status"] => ("/v1/brain/projects/status".to_owned(), RouteMethods::Get),
        ["mcp", "discovery"] => ("/v1/managed/mcp/discovery".to_owned(), RouteMethods::Post),
        ["mcp", "discovery", run, "publish"] => (
            format!("/v1/managed/mcp/discovery/{run}/publish"),
            RouteMethods::Post,
        ),
        ["projects", "supervision"] => (
            "/v1/brain/projects/supervision".to_owned(),
            RouteMethods::Post,
        ),
        ["learning", "goals"] => ("/v1/brain/learning/goals".to_owned(), RouteMethods::Post),
        ["learning", "answer"] => ("/v1/brain/learning/answer".to_owned(), RouteMethods::Post),
        ["learning", "resolve"] => ("/v1/brain/learning/resolve".to_owned(), RouteMethods::Post),
        ["artifacts", "upload"] => ("/v1/artifacts/upload".to_owned(), RouteMethods::Post),
        ["artifacts", "query"] => ("/v1/artifacts/query".to_owned(), RouteMethods::Post),
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
                | "doctor"
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
        Some(value) if method == "GET" && matches!(segments.as_slice(), ["artifacts", _]) => {
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
#[path = "core/discovery_tests.rs"]
mod discovery_tests;
#[cfg(test)]
#[path = "core/tests.rs"]
mod tests;
