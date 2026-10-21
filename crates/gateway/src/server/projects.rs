//! HTTP projects responsibilities; no independent runtime or writer.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LearningGoalRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    goal: p::GoalFrameRef,
    knowledge: p::GoalKnowledge,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LearningAnswerRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    reply: p::LearningReplyRecord,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LearningResolveRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    proposal: p::ProposalRef,
    outcome: p::ProposalOutcome,
    defer_until: Option<p::Timestamp>,
}

pub(super) fn learning_schema(version: p::SchemaVersion) -> Result<(), ApiError> {
    if version != p::SchemaVersion(1) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported learning control schema",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CharterRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    change: p::WorkspaceCharterChangedPayload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrepareCharterRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    record: p::WorkspaceCharterRecord,
    expected_version: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectRequest {
    schema_version: p::SchemaVersion,
    request: ProjectCommand,
}
#[derive(serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ProjectCommand {
    Register {
        command: p::IdempotencyKey,
        project: p::WorkspaceProject,
    },
    Revise {
        command: p::IdempotencyKey,
        project: p::WorkspaceProject,
        expected_digest: p::SchemaDigest,
    },
    Plan {
        command: p::IdempotencyKey,
        project: String,
        signature: p::TaskSignature,
        input_digest: p::SchemaDigest,
    },
    Execute {
        route: p::ExecutionRouteRef,
    },
    Cancel {
        command: p::IdempotencyKey,
        route: p::ExecutionRouteRef,
    },
}
pub(super) async fn project_ledger(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<p::OrchestrationLedger>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    Ok(Json(
        state
            .blocking(move || gateway.orchestration_state(auth))
            .await?,
    ))
}
pub(super) async fn project_command(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ProjectRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    learning_schema(request.schema_version)?;
    let gateway = state.gateway.clone();
    let value = state
        .blocking(move || match request.request {
            ProjectCommand::Register { command, project } => gateway
                .register_project(command, project, auth)
                .map(|event| serde_json::json!({"event":event})),
            ProjectCommand::Revise {
                command,
                project,
                expected_digest,
            } => gateway
                .revise_project(command, project, expected_digest, auth)
                .map(|event| serde_json::json!({"event":event})),
            ProjectCommand::Plan {
                command,
                project,
                signature,
                input_digest,
            } => gateway
                .plan_project_route(command, project, signature, input_digest, auth)
                .and_then(|route| {
                    serde_json::to_value(route)
                        .map_err(|_| p::Error("route response encoding failed".into()))
                }),
            ProjectCommand::Execute { route } => gateway
                .execute_project_route(route, auth)
                .and_then(|outcome| {
                    serde_json::to_value(outcome)
                        .map_err(|_| p::Error("route response encoding failed".into()))
                }),
            ProjectCommand::Cancel { command, route } => gateway
                .cancel_project_route(command, route, auth)
                .map(|_| serde_json::json!({"cancelled":true})),
        })
        .await?;
    Ok(Json(value))
}

pub(super) async fn read_workspace_charter(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Option<p::WorkspaceCharterRecord>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    Ok(Json(
        state
            .blocking(move || gateway.workspace_charter(p::WorkspaceRef(id), auth))
            .await?,
    ))
}

pub(super) async fn write_workspace_charter(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CharterRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    learning_schema(request.schema_version)?;
    if request.change.charter.workspace.0 != id {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "charter path and workspace disagree",
        ));
    }
    let gateway = state.gateway.clone();
    let event = state
        .blocking(move || gateway.apply_workspace_charter(request.command, request.change, auth))
        .await?;
    Ok(Json(serde_json::json!({"schema_version":1,"event":event})))
}

pub(super) async fn prepare_workspace_charter(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<PrepareCharterRequest>,
) -> Result<Json<p::WorkspaceCharterChangedPayload>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    learning_schema(request.schema_version)?;
    if request.command.0.trim().is_empty() || request.record.workspace.0 != id {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "charter path and preparation do not match",
        ));
    }
    let gateway = state.gateway.clone();
    let change = state
        .blocking(move || {
            gateway.prepare_workspace_charter(request.record, request.expected_version, auth)
        })
        .await?;
    Ok(Json(change))
}

pub(super) async fn register_learning_goal(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<LearningGoalRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    learning_schema(request.schema_version)?;
    let gateway = state.gateway.clone();
    let event = state
        .blocking(move || {
            gateway.register_learning_goal(request.command, request.goal, request.knowledge, auth)
        })
        .await?;
    Ok(Json(serde_json::json!({"schema_version":1,"event":event})))
}
pub(super) async fn answer_learning(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<LearningAnswerRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    learning_schema(request.schema_version)?;
    let gateway = state.gateway.clone();
    let candidate = state
        .blocking(move || gateway.answer_learning(request.command, request.reply, auth))
        .await?;
    Ok(Json(
        serde_json::json!({"schema_version":1,"candidate":candidate}),
    ))
}
pub(super) async fn resolve_learning(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<LearningResolveRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    learning_schema(request.schema_version)?;
    let gateway = state.gateway.clone();
    let event = state
        .blocking(move || {
            gateway.resolve_learning(
                request.command,
                request.proposal,
                request.outcome,
                request.defer_until,
                auth,
            )
        })
        .await?;
    Ok(Json(serde_json::json!({"schema_version":1,"event":event})))
}
