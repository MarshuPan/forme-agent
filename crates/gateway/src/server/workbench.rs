//! Installed workbench uses the same runtime and owner controls as every entry.
use super::*;

pub(super) fn routes() -> Router<Arc<HttpState>> {
    Router::new()
        .route("/v1/brain/workbench/query", post(query_workbench))
        .route("/v1/brain/workbench/material", post(read_material))
        .route(
            "/v1/brain/workbench/material-lifecycle",
            post(material_lifecycle),
        )
        .route("/v1/brain/workbench/review", post(review_workbench))
        .route("/v1/brain/workbench/correction", post(correct_cognition))
        .route(
            "/v1/brain/workbench/corrections/query",
            post(query_corrections),
        )
        .route(
            "/v1/brain/workbench/proposal",
            post(resolve_workbench_proposal),
        )
}

async fn correct_cognition(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::WorkbenchCorrectionCommand>,
) -> Result<Json<p::WorkbenchCorrectionReceipt>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.correct_workbench_cognition(&owner, command))
        .await
        .map(Json)
        .map_err(classify)
}

async fn query_corrections(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(query): Json<p::WorkbenchCorrectionQuery>,
) -> Result<Json<p::WorkbenchCorrectionsView>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.workbench_corrections(&owner, query))
        .await
        .map(Json)
        .map_err(classify)
}

async fn material_lifecycle(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(query): Json<p::WorkbenchMaterialQuery>,
) -> Result<Json<p::WorkbenchMaterialLifecycleView>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.workbench_material_lifecycle_target(&owner, query))
        .await
        .map(Json)
        .map_err(classify)
}

async fn read_material(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(query): Json<p::WorkbenchMaterialQuery>,
) -> Result<Json<p::WorkbenchMaterialView>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.workbench_material(&owner, query))
        .await
        .map(Json)
        .map_err(classify)
}

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

pub(super) async fn resolve_workbench_proposal(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::WorkbenchProposalCommand>,
) -> Result<Json<p::WorkbenchCommandReceipt>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    state
        .blocking(move || runtime.resolve_workbench_proposal(&owner, command))
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
            | "proposal control has an unresolved receipt; inspect the original operation"
            | "ordinary proactive proposal was not found in the requested run"
            | "proposal already has a recorded owner outcome"
            | "workbench command identity conflict"
            | "owner correction source version is stale"
            | "owner correction source state is stale"
            | "owner correction source is unavailable"
            | "owner correction source is not active"
    ) {
        error.status = StatusCode::CONFLICT;
    }
    error
}
