//! Safe views over the shared runtime's owner data controls.
use super::*;

pub(super) fn routes() -> Router<Arc<HttpState>> {
    Router::new()
        .route("/v1/data/workbench/query", post(query))
        .route("/v1/data/workbench/command", post(command))
}
async fn query(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(query): Json<p::DataWorkbenchQuery>,
) -> Result<Json<p::DataWorkbenchView>, ApiError> {
    state.authorize(&headers, false)?;
    let (runtime, owner) = (state.runtime.clone(), state.owner.clone());
    state
        .blocking(move || runtime.query_data_workbench(&owner, query))
        .await
        .map(Json)
        .map_err(classify)
}
async fn command(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::DataWorkbenchCommand>,
) -> Result<Json<p::DataCommandReceipt>, ApiError> {
    state.authorize(&headers, true)?;
    let (runtime, owner) = (state.runtime.clone(), state.owner.clone());
    state
        .blocking(move || runtime.execute_data_workbench(&owner, command))
        .await
        .map(Json)
        .map_err(classify)
}
fn classify(mut error: ApiError) -> ApiError {
    if matches!(
        error.message.as_str(),
        "data command identity conflict"
            | "data snapshot changed; restart pagination"
            | "data plan changed or expired"
            | "data plan already has an application; inspect its receipt"
    ) {
        error.status = StatusCode::CONFLICT;
    }
    error
}
