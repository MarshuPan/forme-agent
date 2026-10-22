//! Installed workbench uses the same runtime and owner controls as every entry.
use super::*;

pub(super) async fn query_workbench(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(query): Json<p::WorkbenchQuery>,
) -> Result<Json<p::WorkbenchSnapshot>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.workbench_snapshot(&owner, query))
        .await
        .map(Json)
        .map_err(classify)
}

pub(super) async fn review_workbench(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::WorkbenchReviewCommand>,
) -> Result<Json<p::WorkbenchCommandReceipt>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.review_workbench_candidate(&owner, command))
        .await
        .map(Json)
        .map_err(classify)
}

fn classify(mut error: ApiError) -> ApiError {
    if matches!(
        error.message.as_str(),
        "workbench snapshot changed; restart pagination"
            | "candidate review expected state is stale"
            | "candidate has an unresolved review; inspect the original receipt"
            | "workbench command identity conflict"
    ) {
        error.status = StatusCode::CONFLICT;
    }
    error
}
