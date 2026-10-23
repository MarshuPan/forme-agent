//! Closed project supervision surface on the existing production runtime.
use super::*;

pub(super) fn routes() -> Router<Arc<HttpState>> {
    Router::new()
        .route("/v1/brain/projects/status", get(status))
        .route("/v1/brain/projects/supervision", post(control))
}

async fn status(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<p::WorkspaceControlSnapshot>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.workspace_control_snapshot(&owner))
        .await
        .map(Json)
        .map_err(classify)
}

async fn control(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::ProjectSupervisionCommand>,
) -> Result<Json<p::WorkbenchCommandReceipt>, ApiError> {
    state.authorize(&headers, true)?;
    if headers.get("idempotency-key").and_then(|v| v.to_str().ok())
        != Some(command.command.0.as_str())
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "project supervision command key does not match",
        ));
    }
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.control_project_supervision(&owner, command))
        .await
        .map(Json)
        .map_err(classify)
}

fn classify(mut error: ApiError) -> ApiError {
    if matches!(
        error.message.as_str(),
        "workspace control snapshot changed; reload status"
            | "project supervision expected state is stale"
            | "orchestration expected revision conflict"
            | "orchestration command identity conflict"
    ) {
        error.status = StatusCode::CONFLICT;
    }
    error
}
