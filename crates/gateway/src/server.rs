use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use forme_communication::AuthContext;
use forme_harness::{ReactiveHarness, SchedulerService};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use tower_http::timeout::TimeoutLayer;

use crate::LocalGateway;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");
const GOLDEN_TASKS: &str = include_str!("../../../evals/m1/golden-tasks.json");
const CSRF_HEADER: &str = "x-forme-csrf";

#[derive(Clone)]
struct LocalBearerSecret(Arc<str>);

impl LocalBearerSecret {
    fn new(value: String) -> p::Result<Self> {
        if value.len() < 32 || value.chars().any(char::is_whitespace) {
            return Err(p::Error(
                "gateway bearer secret must contain at least 32 non-whitespace characters".into(),
            ));
        }
        Ok(Self(Arc::from(value)))
    }

    fn matches(&self, candidate: &str) -> bool {
        let expected = self.0.as_bytes();
        let actual = candidate.as_bytes();
        if expected.len() != actual.len() {
            return false;
        }
        expected
            .iter()
            .zip(actual)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }
}

impl fmt::Debug for LocalBearerSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocalBearerSecret(<redacted>)")
    }
}

#[derive(Clone)]
pub struct GatewayServerConfig {
    pub schema_version: p::SchemaVersion,
    pub bind: IpAddr,
    pub port: u16,
    pub owner: p::VerifiedPrincipal,
    pub agent_profile: p::AgentProfileRef,
    pub max_body_bytes: usize,
    pub request_timeout: Duration,
    pub max_requests_per_minute: u32,
    pub scheduler_enabled: bool,
    pub scheduler: p::SchedulerConfig,
    token: LocalBearerSecret,
    token_path: Option<PathBuf>,
}

impl fmt::Debug for GatewayServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayServerConfig")
            .field("schema_version", &self.schema_version)
            .field("bind", &self.bind)
            .field("port", &self.port)
            .field("owner", &self.owner)
            .field("agent_profile", &self.agent_profile)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("request_timeout", &self.request_timeout)
            .field("max_requests_per_minute", &self.max_requests_per_minute)
            .field("scheduler_enabled", &self.scheduler_enabled)
            .field("scheduler", &self.scheduler)
            .field("token", &self.token)
            .field("token_path", &self.token_path)
            .finish()
    }
}

impl GatewayServerConfig {
    pub fn from_environment() -> p::Result<Self> {
        let bind = std::env::var("FORME_GATEWAY_BIND")
            .unwrap_or_else(|_| Ipv4Addr::LOCALHOST.to_string())
            .parse::<IpAddr>()
            .map_err(|_| p::Error("FORME_GATEWAY_BIND must be an IP address".into()))?;
        let port = env_parse("FORME_GATEWAY_PORT", 0_u16)?;
        let max_body_bytes = env_parse("FORME_GATEWAY_MAX_BODY_BYTES", 262_144_usize)?;
        let timeout_ms = env_parse("FORME_GATEWAY_TIMEOUT_MS", 30_000_u64)?;
        let max_requests_per_minute = env_parse("FORME_GATEWAY_MAX_REQUESTS_PER_MINUTE", 120_u32)?;
        let scheduler_enabled = env_bool("FORME_SCHEDULER_ENABLED", true)?;
        let scheduler = p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(env_parse("FORME_SCHEDULER_TICK_MS", 1_000_u64)?),
            lease: p::DurationMs(env_parse("FORME_SCHEDULER_LEASE_MS", 30_000_u64)?),
            max_claims_per_tick: env_parse("FORME_SCHEDULER_MAX_CLAIMS", 1_u32)?,
        };
        let (token, token_path) = load_or_create_token()?;
        let config = Self {
            schema_version: p::SchemaVersion(1),
            bind,
            port,
            owner: p::VerifiedPrincipal(
                std::env::var("FORME_OWNER_ID").unwrap_or_else(|_| "local-owner".into()),
            ),
            agent_profile: p::AgentProfileRef("agent:forme-local".into()),
            max_body_bytes,
            request_timeout: Duration::from_millis(timeout_ms),
            max_requests_per_minute,
            scheduler_enabled,
            scheduler,
            token,
            token_path,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn token_path(&self) -> Option<&std::path::Path> {
        self.token_path.as_deref()
    }

    fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || !self.bind.is_loopback()
            || self.owner.0.trim().is_empty()
            || self.agent_profile.0.trim().is_empty()
            || self.max_body_bytes == 0
            || self.request_timeout.is_zero()
            || self.max_requests_per_minute == 0
            || self.scheduler_enabled && self.scheduler.validate().is_err()
        {
            return Err(p::Error(
                "gateway configuration must be versioned, loopback-only, and bounded".into(),
            ));
        }
        Ok(())
    }
}

pub async fn serve_environment<F>(config: GatewayServerConfig, on_bound: F) -> p::Result<()>
where
    F: FnOnce(SocketAddr),
{
    config.validate()?;
    let harness = Arc::new(ReactiveHarness::from_environment_local()?);
    if config.scheduler_enabled {
        SchedulerService::recover(harness.as_ref(), now_ms())?;
    }
    let gateway = Arc::new(LocalGateway::new(
        harness.clone(),
        config.owner.clone(),
        config.agent_profile.clone(),
    )?);
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(config.bind, config.port))
        .await
        .map_err(|error| p::Error(format!("failed to bind local gateway: {error}")))?;
    let address = listener
        .local_addr()
        .map_err(|error| p::Error(format!("failed to read local gateway address: {error}")))?;
    if !address.ip().is_loopback() {
        return Err(p::Error(
            "local gateway resolved to a non-loopback address".into(),
        ));
    }
    let state = Arc::new(HttpState {
        gateway,
        owner: config.owner.clone(),
        token: config.token.clone(),
        origin: format!("http://{address}"),
        rate: Mutex::new(RateWindow::new()),
        max_requests_per_minute: config.max_requests_per_minute,
    });
    let app = router(state, &config);
    let scheduler_task = if config.scheduler_enabled {
        let scheduler = harness.clone();
        let tick = Duration::from_millis(config.scheduler.tick.0);
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let scheduler = scheduler.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    SchedulerService::tick(scheduler.as_ref(), now_ms())
                })
                .await;
            }
        }))
    } else {
        None
    };
    on_bound(address);
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| p::Error(format!("local gateway server failed: {error}")));
    if let Some(task) = scheduler_task {
        task.abort();
    }
    result
}

fn router(state: Arc<HttpState>, config: &GatewayServerConfig) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(styles))
        .route("/app.js", get(script))
        .route("/healthz", get(health))
        .route("/v1/profile", get(control_profile))
        .route("/v1/runs", get(list_runs).post(submit_run))
        .route("/v1/runs/{run}", get(run_summary))
        .route("/v1/runs/{run}/events", get(run_events))
        .route("/v1/sessions/{session}/approvals", get(approvals))
        .route("/v1/runs/{run}/control", post(control_run))
        .route("/v1/runs/{run}/trace", get(trace_view))
        .route("/v1/jobs", get(list_jobs).post(schedule_job))
        .route("/v1/jobs/{intention}/cancel", post(cancel_job))
        .route("/v1/candidates/{candidate}/review", post(review_candidate))
        .route("/v1/evals/cases", get(eval_cases))
        .route("/v1/evals/run", post(run_eval))
        .route("/v1/evals/{eval}", get(export_eval))
        .route("/v1/evolution/snapshot", get(evolution_snapshot))
        .route(
            "/v1/evolution/auto-activation",
            get(auto_activation_status).post(set_auto_activation_status),
        )
        .route("/v1/evolution/candidates", post(record_strategy_candidate))
        .route("/v1/evolution/evaluations", post(evaluate_strategy))
        .route("/v1/evolution/promotions", post(promote_strategy))
        .route("/v1/evolution/activations", post(activate_strategy))
        .route("/v1/evolution/rollbacks", post(rollback_strategy))
        .route("/v1/ecosystem/snapshot", get(ecosystem_snapshot))
        .route(
            "/v1/ecosystem/publishers",
            post(provision_capability_publisher),
        )
        .route("/v1/ecosystem/admissions", post(admit_capability_package))
        .route("/v1/ecosystem/plans", post(prepare_capability_change))
        .route("/v1/ecosystem/changes", post(apply_capability_change))
        .route(
            "/v1/ecosystem/distributions/prepare",
            post(prepare_capability_distribution),
        )
        .layer(DefaultBodyLimit::max(config.max_body_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            config.request_timeout,
        ))
        .with_state(state)
}

struct HttpState {
    gateway: Arc<LocalGateway<ReactiveHarness>>,
    owner: p::VerifiedPrincipal,
    token: LocalBearerSecret,
    origin: String,
    rate: Mutex<RateWindow>,
    max_requests_per_minute: u32,
}

struct RateWindow {
    started: Instant,
    count: u32,
}

impl RateWindow {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            count: 0,
        }
    }

    fn admit(&mut self, limit: u32) -> bool {
        if self.started.elapsed() >= Duration::from_secs(60) {
            self.started = Instant::now();
            self.count = 0;
        }
        if self.count >= limit {
            return false;
        }
        self.count += 1;
        true
    }
}

impl HttpState {
    fn authorize(&self, headers: &HeaderMap, mutation: bool) -> Result<AuthContext, ApiError> {
        if !self
            .rate
            .lock()
            .map_err(|_| ApiError::internal("gateway rate state is unavailable"))?
            .admit(self.max_requests_per_minute)
        {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "gateway request rate exceeded",
            ));
        }
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "bearer token is required"))?;
        if !self.token.matches(bearer) {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "bearer token is invalid",
            ));
        }
        if mutation {
            let origin = headers
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| ApiError::new(StatusCode::FORBIDDEN, "origin is required"))?;
            let csrf = headers
                .get(CSRF_HEADER)
                .and_then(|value| value.to_str().ok());
            if origin != self.origin || csrf != Some("1") {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "same-origin CSRF proof is invalid",
                ));
            }
        }
        Ok(AuthContext::local_owner(
            self.owner.clone(),
            now_ms().saturating_add(60_000),
        ))
    }
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    schema_version: p::SchemaVersion,
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<p::Error> for ApiError {
    fn from(error: p::Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                schema_version: p::SchemaVersion(1),
                error: self.message,
            }),
        )
            .into_response()
    }
}

async fn blocking<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> p::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| ApiError::internal("gateway worker did not complete"))?
        .map_err(ApiError::from)
}

async fn index() -> Response {
    secure_static(Html(INDEX_HTML)).into_response()
}

async fn styles() -> Response {
    static_response("text/css; charset=utf-8", APP_CSS)
}

async fn script() -> Response {
    static_response("text/javascript; charset=utf-8", APP_JS)
}

async fn health() -> &'static str {
    "ok"
}

async fn control_profile(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<p::ControlProfile>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.control_profile(p::SurfaceRef("surface:local-web".into()), auth))
        .await
        .map(Json)
}

async fn submit_run(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<p::RunRequest>,
) -> Result<(StatusCode, Json<p::RunId>), ApiError> {
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let run = blocking(move || gateway.start_run(request, auth)).await?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}

async fn list_runs(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::RunSummary>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.list_runs(auth)).await.map(Json)
}

async fn run_summary(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::RunSummary>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.run_summary(p::RunId(run), auth))
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
struct EventQuery {
    after: Option<u64>,
}

async fn run_events(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let after = event_cursor(&run, query.after, &headers)?;
    let gateway = state.gateway.clone();
    let page = blocking(move || {
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

async fn approvals(
    State(state): State<Arc<HttpState>>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::PendingApproval>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.pending_approvals(p::SessionId(session), auth))
        .await
        .map(Json)
}

async fn control_run(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
    Json(control): Json<p::RunControl>,
) -> Result<StatusCode, ApiError> {
    let auth = state.authorize(&headers, true)?;
    if let p::RunControl::ResolveApproval(decision) = &control {
        if decision.approver != state.owner {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "approval principal does not match the authenticated owner",
            ));
        }
    }
    let gateway = state.gateway.clone();
    blocking(move || gateway.control_run(p::RunId(run), control, auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn trace_view(
    State(state): State<Arc<HttpState>>,
    Path(run): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::TraceView>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.trace_view(p::RunId(run), auth))
        .await
        .map(Json)
}

async fn list_jobs(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<p::ScheduledJob>>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.list_jobs(auth)).await.map(Json)
}

async fn schedule_job(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<p::ScheduleCommand>,
) -> Result<(StatusCode, Json<p::IntentionId>), ApiError> {
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let intention = blocking(move || gateway.schedule_job(command, auth)).await?;
    Ok((StatusCode::ACCEPTED, Json(intention)))
}

async fn cancel_job(
    State(state): State<Arc<HttpState>>,
    Path(intention): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.cancel_job(p::IntentionId(intention), auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn review_candidate(
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
    blocking(move || gateway.review_candidate(command, auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn eval_cases(
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

async fn run_eval(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<p::ManualEvalRequest>,
) -> Result<Json<p::ManualEvalReport>, ApiError> {
    let auth = state.authorize(&headers, true)?;
    request.validate().map_err(ApiError::from)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.run_eval(request.case, request.profile, auth))
        .await
        .map(Json)
}

async fn export_eval(
    State(state): State<Arc<HttpState>>,
    Path(eval): Path<String>,
    headers: HeaderMap,
) -> Result<Json<p::ManualEvalReport>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    blocking(move || gateway.export_eval(p::EvalRef(eval), auth))
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
struct EvolutionSnapshotQuery {
    scope: String,
}

#[derive(Debug, Deserialize)]
struct EcosystemSnapshotQuery {
    scope: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionCapabilityPublisherRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    grant: p::CapabilityPublisherGrant,
    #[serde(default)]
    previous: Option<p::CapabilityPublisherGrantRef>,
    expected: p::EcosystemAggregateVersion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmitCapabilityPackageRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    catalog_run: p::RunId,
    package: p::SignedCapabilityPackage,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareCapabilityChangeRequest {
    schema_version: p::SchemaVersion,
    operation: p::CapabilityPackageOperation,
    package: p::CapabilityPackageRef,
    release: p::CapabilityReleaseRef,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyCapabilityChangeRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    plan: p::CapabilityInstallPlan,
    approval: p::CapabilityPackageApproval,
    reason: p::ReasonRef,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareCapabilityDistributionRequest {
    schema_version: p::SchemaVersion,
    plan: p::CapabilityInstallPlan,
    target_peer: p::FederatedPeerRef,
    authority_epoch: p::AuthorityEpoch,
}

#[derive(Debug, Serialize)]
struct AutoActivationStatus {
    schema_version: p::SchemaVersion,
    paused: bool,
}

#[derive(Debug, Deserialize)]
struct SetAutoActivationStatusRequest {
    schema_version: p::SchemaVersion,
    paused: bool,
}

#[derive(Debug, Deserialize)]
struct RecordStrategyCandidateRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    candidate: p::StrategyCandidate,
}

#[derive(Debug, Deserialize)]
struct EvaluateStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    candidate: p::StrategyCandidate,
    comparison: p::EvolutionComparison,
}

#[derive(Debug, Serialize)]
struct EvaluateStrategyResponse {
    schema_version: p::SchemaVersion,
    evaluation: p::EvolutionEvaluation,
    event_id: p::EventId,
}

#[derive(Debug, Deserialize)]
struct PromoteStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    candidate: p::StrategyCandidate,
    evaluation: p::EvolutionEvaluation,
}

#[derive(Debug, Deserialize)]
struct ActivateStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    aggregate: p::EvolutionAggregateRef,
    candidate: p::StrategyCandidate,
    evaluation: p::EvolutionEvaluation,
    promotion: p::EventId,
}

#[derive(Debug, Deserialize)]
struct RollbackStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    aggregate: p::EvolutionAggregateRef,
    domain: p::StrategyDomain,
    scope: p::Scope,
    restored: p::StrategyVersionRef,
    triggers: Vec<p::EvidenceRef>,
    in_flight: p::InFlightDisposition,
}

#[derive(Debug, Serialize)]
struct EvolutionActivationResponse {
    schema_version: p::SchemaVersion,
    append: p::ExpectedAppend,
    snapshot: p::EvolutionSnapshot,
}

async fn evolution_snapshot(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<EvolutionSnapshotQuery>,
    headers: HeaderMap,
) -> Result<Json<p::EvolutionSnapshot>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    if query.scope.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "evolution snapshot scope is required",
        ));
    }
    let gateway = state.gateway.clone();
    blocking(move || gateway.evolution_snapshot(p::Scope(query.scope), auth))
        .await
        .map(Json)
}

async fn ecosystem_snapshot(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<EcosystemSnapshotQuery>,
    headers: HeaderMap,
) -> Result<Json<p::CapabilityEcosystemSnapshot>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    if query.scope.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "ecosystem snapshot scope is required",
        ));
    }
    let gateway = state.gateway.clone();
    blocking(move || gateway.ecosystem_snapshot(p::Scope(query.scope), auth))
        .await
        .map(Json)
}

async fn provision_capability_publisher(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ProvisionCapabilityPublisherRequest>,
) -> Result<(StatusCode, Json<p::ExpectedAppend>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = blocking(move || {
        gateway.provision_capability_publisher(
            request.run,
            request.grant,
            request.previous,
            request.expected,
            auth,
        )
    })
    .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn admit_capability_package(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<AdmitCapabilityPackageRequest>,
) -> Result<(StatusCode, Json<p::CapabilityPackageAdmission>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let admission = blocking(move || {
        gateway.admit_capability_package(request.run, request.catalog_run, request.package, auth)
    })
    .await?;
    Ok((StatusCode::CREATED, Json(admission)))
}

async fn prepare_capability_change(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<PrepareCapabilityChangeRequest>,
) -> Result<Json<p::CapabilityInstallPlan>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    blocking(move || {
        gateway.prepare_capability_change(request.operation, request.package, request.release, auth)
    })
    .await
    .map(Json)
}

async fn apply_capability_change(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ApplyCapabilityChangeRequest>,
) -> Result<Json<p::CapabilityPackageState>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    blocking(move || {
        gateway.apply_capability_change(
            request.run,
            request.plan,
            request.approval,
            request.reason,
            auth,
        )
    })
    .await
    .map(Json)
}

async fn prepare_capability_distribution(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<PrepareCapabilityDistributionRequest>,
) -> Result<Json<p::CapabilityPackageDistributionEnvelope>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    blocking(move || {
        gateway.prepare_capability_distribution(
            request.plan,
            request.target_peer,
            request.authority_epoch,
            auth,
        )
    })
    .await
    .map(Json)
}

async fn auto_activation_status(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<AutoActivationStatus>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    let paused = blocking(move || gateway.auto_activation_paused(auth)).await?;
    Ok(Json(AutoActivationStatus {
        schema_version: p::SchemaVersion(1),
        paused,
    }))
}

async fn set_auto_activation_status(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<SetAutoActivationStatusRequest>,
) -> Result<Json<AutoActivationStatus>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let paused = request.paused;
    blocking(move || gateway.set_auto_activation_paused(paused, auth)).await?;
    Ok(Json(AutoActivationStatus {
        schema_version: p::SchemaVersion(1),
        paused,
    }))
}

async fn record_strategy_candidate(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(mut request): Json<RecordStrategyCandidateRequest>,
) -> Result<(StatusCode, Json<p::EventId>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    stamp_owner_candidate(&mut request.candidate);
    let gateway = state.gateway.clone();
    let event =
        blocking(move || gateway.record_strategy_candidate(request.run, request.candidate, auth))
            .await?;
    Ok((StatusCode::CREATED, Json(event)))
}

async fn evaluate_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<EvaluateStrategyRequest>,
) -> Result<Json<EvaluateStrategyResponse>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = blocking(move || {
        gateway.evaluate_strategy(request.run, &request.candidate, request.comparison, auth)
    })
    .await?;
    Ok(Json(EvaluateStrategyResponse {
        schema_version: p::SchemaVersion(1),
        evaluation: result.evaluation,
        event_id: result.event_id,
    }))
}

async fn promote_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<PromoteStrategyRequest>,
) -> Result<(StatusCode, Json<p::EventId>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let event = blocking(move || {
        gateway.promote_strategy(request.run, &request.candidate, &request.evaluation, auth)
    })
    .await?;
    Ok((StatusCode::CREATED, Json(event)))
}

async fn activate_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ActivateStrategyRequest>,
) -> Result<Json<EvolutionActivationResponse>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = blocking(move || {
        gateway.activate_strategy(
            request.run,
            request.aggregate,
            &request.candidate,
            &request.evaluation,
            request.promotion,
            auth,
        )
    })
    .await?;
    Ok(Json(EvolutionActivationResponse {
        schema_version: p::SchemaVersion(1),
        append: result.append,
        snapshot: result.snapshot,
    }))
}

async fn rollback_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<RollbackStrategyRequest>,
) -> Result<Json<EvolutionActivationResponse>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = blocking(move || {
        gateway.rollback_strategy(
            request.run,
            request.aggregate,
            request.domain,
            request.scope,
            request.restored,
            request.triggers,
            request.in_flight,
            auth,
        )
    })
    .await?;
    Ok(Json(EvolutionActivationResponse {
        schema_version: p::SchemaVersion(1),
        append: result.append,
        snapshot: result.snapshot,
    }))
}

fn require_schema(schema_version: p::SchemaVersion) -> Result<(), ApiError> {
    if schema_version.0 == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "evolution control request is not versioned",
        ));
    }
    Ok(())
}

fn stamp_owner_candidate(candidate: &mut p::StrategyCandidate) {
    candidate.provenance = p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    };
}

fn event_cursor(run: &str, query: Option<u64>, headers: &HeaderMap) -> Result<u64, ApiError> {
    let header_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let (header_run, sequence) = value.rsplit_once('|').ok_or_else(|| {
                ApiError::new(StatusCode::BAD_REQUEST, "Last-Event-ID is invalid")
            })?;
            if header_run != run {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "Last-Event-ID belongs to another run",
                ));
            }
            sequence
                .parse::<u64>()
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "Last-Event-ID is invalid"))
        })
        .transpose()?;
    if query.is_some() && header_cursor.is_some() && query != header_cursor {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "query cursor and Last-Event-ID disagree",
        ));
    }
    Ok(query.or(header_cursor).unwrap_or(0))
}

fn secure_static<T>(response: T) -> impl IntoResponse
where
    T: IntoResponse,
{
    let mut response = response.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn static_response(content_type: &'static str, body: &'static str) -> Response {
    let mut response = secure_static(body).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn load_or_create_token() -> p::Result<(LocalBearerSecret, Option<PathBuf>)> {
    if let Ok(token) = std::env::var("FORME_GATEWAY_TOKEN") {
        return Ok((LocalBearerSecret::new(token)?, None));
    }
    let path = std::env::var_os("FORME_GATEWAY_TOKEN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_token_path);
    if let Ok(token) = fs::read_to_string(&path) {
        return Ok((LocalBearerSecret::new(token.trim().to_owned())?, Some(path)));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            p::Error(format!("failed to create gateway state directory: {error}"))
        })?;
    }
    let token = generate_token()?;
    match create_secret_file(&path, token.as_bytes()) {
        Ok(()) => Ok((LocalBearerSecret::new(token)?, Some(path))),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::read_to_string(&path).map_err(|read_error| {
                p::Error(format!("failed to read gateway token: {read_error}"))
            })?;
            Ok((
                LocalBearerSecret::new(existing.trim().to_owned())?,
                Some(path),
            ))
        }
        Err(error) => Err(p::Error(format!("failed to store gateway token: {error}"))),
    }
}

fn create_secret_file(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn generate_token() -> p::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| p::Error(format!("failed to obtain gateway token entropy: {error}")))?;
    let mut token = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(token)
}

fn default_token_path() -> PathBuf {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("forme").join("gateway.token");
    }
    #[cfg(not(windows))]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("forme").join("gateway.token");
    }
    std::env::temp_dir().join("forme").join("gateway.token")
}

fn env_parse<T>(key: &str, fallback: T) -> p::Result<T>
where
    T: std::str::FromStr,
{
    match std::env::var(key) {
        Ok(value) => value
            .parse()
            .map_err(|_| p::Error(format!("{key} has an invalid value"))),
        Err(_) => Ok(fallback),
    }
}

fn env_bool(key: &str, fallback: bool) -> p::Result<bool> {
    match std::env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(p::Error(format!("{key} must be a boolean"))),
        },
        Err(std::env::VarError::NotPresent) => Ok(fallback),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(p::Error(format!("{key} is not valid UTF-8")))
        }
    }
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(bind: IpAddr) -> GatewayServerConfig {
        GatewayServerConfig {
            schema_version: p::SchemaVersion(1),
            bind,
            port: 0,
            owner: p::VerifiedPrincipal("owner:test".into()),
            agent_profile: p::AgentProfileRef("agent:test".into()),
            max_body_bytes: 1024,
            request_timeout: Duration::from_secs(1),
            max_requests_per_minute: 10,
            scheduler_enabled: true,
            scheduler: p::SchedulerConfig {
                schema_version: p::SchemaVersion(1),
                tick: p::DurationMs(1_000),
                lease: p::DurationMs(30_000),
                max_claims_per_tick: 1,
            },
            token: LocalBearerSecret::new(
                "unit-test-token-0123456789abcdef0123456789abcdef".into(),
            )
            .unwrap(),
            token_path: None,
        }
    }

    #[test]
    fn gateway_config_is_loopback_only_and_debug_redacts_the_secret() {
        let local = config(IpAddr::V4(Ipv4Addr::LOCALHOST));
        local.validate().unwrap();
        let debug = format!("{local:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("unit-test-token"));

        let public = config(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert!(public.validate().is_err());
    }

    #[test]
    fn last_event_id_is_run_bound_and_cannot_disagree_with_the_query() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("run:test|7"));
        assert_eq!(event_cursor("run:test", None, &headers).unwrap(), 7);
        assert!(event_cursor("run:other", None, &headers).is_err());
        assert!(event_cursor("run:test", Some(6), &headers).is_err());
    }

    #[test]
    fn generated_bearer_has_full_os_entropy_width() {
        let token = generate_token().unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
