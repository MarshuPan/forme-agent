//! HTTP runs responsibilities; no independent runtime or writer.
use super::*;

pub(super) async fn control_profile(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<p::ControlProfile>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.control_profile(p::SurfaceRef("surface:local-web".into()), auth))
        .await
        .map(Json)
}

pub(super) async fn submit_run(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<p::RunRequest>,
) -> Result<(StatusCode, Json<p::RunId>), ApiError> {
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let run = state
        .blocking(move || gateway.start_run(request, auth))
        .await?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}

pub(super) async fn list_runs(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::RunSummary>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.list_runs(auth))
        .await
        .map(Json)
}

pub(super) async fn run_summary(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::RunSummary>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.run_summary(p::RunId(run), auth))
        .await
        .map(Json)
}

pub(super) async fn run_pending(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::RunPendingView>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.run_pending_view(p::RunId(run), auth))
        .await
        .map(Json)
}

#[derive(Debug, Serialize)]
pub(super) struct RunAnswerView {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    answer: Option<String>,
}

pub(super) async fn run_answer(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RunAnswerView>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    let run = p::RunId(run);
    let answer = {
        let run = run.clone();
        state
            .blocking(move || gateway.answer_for_owner(run, auth))
            .await?
    };
    Ok(Json(RunAnswerView {
        schema_version: p::SchemaVersion(1),
        run,
        answer,
    }))
}

#[derive(Debug, Deserialize)]
pub(super) struct EventQuery {
    after: Option<u64>,
}

pub(super) async fn run_events(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let after = event_cursor(&run, query.after, &headers)?;
    let gateway = state.gateway.clone();
    let page = state
        .blocking(move || {
            gateway.event_page(
                p::EventCursor {
                    schema_version: p::SchemaVersion(1),
                    run: p::RunId(run),
                    after_stream_seq: after,
                },
                auth,
            )
        })
        .await?;
    let mut body = String::new();
    for event in &page.events {
        let json = serde_json::to_string(event)
            .map_err(|_| ApiError::internal("event stream serialization failed"))?;
        body.push_str("id: ");
        body.push_str(&page.run.0);
        body.push('|');
        body.push_str(&event.stream_seq.to_string());
        body.push_str("\nevent: forme-event\ndata: ");
        body.push_str(&json);
        body.push_str("\n\n");
    }
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

pub(super) async fn approvals(
    State(state): State<Arc<HttpState>>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::PendingApproval>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.pending_approvals(p::SessionId(session), auth))
        .await
        .map(Json)
}

pub(super) async fn control_run(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
    Json(control): Json<p::RunControl>,
) -> Result<StatusCode, ApiError> {
    let auth = state.authorize(&headers, true)?;
    control
        .validate()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid run control"))?;
    if let p::RunControl::ResolveApproval(decision) = &control {
        if decision.approver != state.owner {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "approval principal does not match the authenticated owner",
            ));
        }
    }
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.control_run(p::RunId(run), control, auth))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn trace_view(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::TraceView>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.trace_view(p::RunId(run), auth))
        .await
        .map(Json)
}

pub(super) async fn list_jobs(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::ScheduledJob>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.list_jobs(auth))
        .await
        .map(Json)
}

pub(super) async fn schedule_job(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::ScheduleCommand>,
) -> Result<(StatusCode, Json<p::IntentionId>), ApiError> {
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let intention = state
        .blocking(move || gateway.schedule_job(command, auth))
        .await?;
    Ok((StatusCode::ACCEPTED, Json(intention)))
}

pub(super) async fn cancel_job(
    State(state): State<Arc<HttpState>>,
    Path(intention): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.cancel_job(p::IntentionId(intention), auth))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
