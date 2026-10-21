//! HTTP evaluation responsibilities; no independent runtime or writer.
use super::*;

pub(super) async fn review_candidate(
    State(state): State<Arc<HttpState>>,
    Path(candidate): Path<String>,
    headers: HeaderMap,
    Json(command): Json<p::CandidateReviewCommand>,
) -> Result<StatusCode, ApiError> {
    let auth = state.authorize(&headers, true)?;
    if command.candidate.0 != candidate {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "candidate path does not match the review command",
        ));
    }
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.review_candidate(command, auth))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn eval_cases(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::ManualEvalCase>>, ApiError> {
    state.authorize(&headers, false)?;
    let cases = serde_json::from_str::<Vec<p::ManualEvalCase>>(GOLDEN_TASKS)
        .map_err(|_| ApiError::internal("golden task manifest is invalid"))?;
    for case in &cases {
        case.validate().map_err(ApiError::from)?;
    }
    Ok(Json(cases))
}

pub(super) async fn run_eval(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<p::ManualEvalRequest>,
) -> Result<Json<p::ManualEvalReport>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    request.validate().map_err(ApiError::from)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.run_eval(request.case, request.profile, auth))
        .await
        .map(Json)
}

pub(super) async fn export_eval(
    State(state): State<Arc<HttpState>>,
    Path(eval): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::ManualEvalReport>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.export_eval(p::EvalRef(eval), auth))
        .await
        .map(Json)
}
