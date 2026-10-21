use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::future::{Future, IntoFuture};
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
use forme_harness::{
    AgentHarness, CognitiveTickOutcome, CompositionManifest, GatewayControl, ProductionRuntime,
    ProductionRuntimeBuilder, ReactiveHarness, RuntimeServiceState, RuntimeServiceStatus,
    SchedulerService,
};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use tower_http::timeout::TimeoutLayer;

use crate::lifecycle::WorkAdmission;
use crate::{GatewayLifecycleConfig, LocalGateway};

#[cfg(test)]
#[path = "server_lifecycle_tests.rs"]
mod lifecycle_tests;

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
            owner: p::VerifiedPrincipal(required_env("FORME_OWNER_ID")?),
            agent_profile: p::AgentProfileRef(
                std::env::var("FORME_AGENT_PROFILE").unwrap_or_else(|_| "agent:forme-local".into()),
            ),
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
    serve_environment_until(
        config,
        GatewayLifecycleConfig::from_environment()?,
        move |address| {
            on_bound(address);
            Ok(())
        },
        shutdown_signal(),
    )
    .await
}

/// OS service hosts supply their authenticated control signal here. No HTTP
/// route can manufacture this signal or construct a second brain runtime.
pub async fn serve_environment_until<F, S>(
    config: GatewayServerConfig,
    lifecycle: GatewayLifecycleConfig,
    on_bound: F,
    shutdown: S,
) -> p::Result<()>
where
    F: FnOnce(SocketAddr) -> p::Result<()>,
    S: Future<Output = p::Result<()>>,
{
    config.validate()?;
    lifecycle.validate()?;
    let runtime = Arc::new(ProductionRuntimeBuilder::from_environment()?.build()?);
    serve_runtime_until(config, runtime, lifecycle, on_bound, shutdown).await
}

async fn serve_runtime_until<F, S>(
    config: GatewayServerConfig,
    runtime: Arc<ProductionRuntime>,
    lifecycle: GatewayLifecycleConfig,
    on_bound: F,
    shutdown: S,
) -> p::Result<()>
where
    F: FnOnce(SocketAddr) -> p::Result<()>,
    S: Future<Output = p::Result<()>>,
{
    config.validate()?;
    lifecycle.validate()?;
    if runtime.owner().0 != config.owner.0 {
        return Err(p::Error(
            "gateway owner does not match the production runtime owner".into(),
        ));
    }
    if runtime.manifest().agent_profile != config.agent_profile {
        return Err(p::Error(
            "gateway agent profile does not match the production runtime profile".into(),
        ));
    }
    if runtime.config().scheduler_enabled != config.scheduler_enabled
        || runtime.config().scheduler != config.scheduler
    {
        return Err(p::Error(
            "gateway scheduler configuration does not match the production runtime".into(),
        ));
    }
    let harness = runtime.harness();
    if config.scheduler_enabled {
        SchedulerService::recover(harness.as_ref(), now_ms())?;
    }
    let gateway = Arc::new(LocalGateway::from_production_runtime(runtime.clone())?);
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
    let admission = Arc::new(WorkAdmission::default());
    let _close_on_drop = CloseAdmissionOnDrop(admission.clone());
    let state = Arc::new(HttpState {
        gateway,
        runtime: runtime.clone(),
        owner: config.owner.clone(),
        token: config.token.clone(),
        origin: format!("http://{address}"),
        rate: Mutex::new(RateWindow::new()),
        max_requests_per_minute: config.max_requests_per_minute,
        admission: admission.clone(),
    });
    let app = router(state, &config);
    tokio::pin!(shutdown);
    // A service stopped during startup must not announce readiness or claim a
    // scheduler window after the controller has already requested shutdown.
    tokio::select! {
        biased;
        result = &mut shutdown => return result,
        _ = std::future::ready(()) => {}
    }
    on_bound(address)?;
    // One background task drives the runtime's shared scheduler pass: due
    // intentions and the owner-brain tick both run through the same harness.
    let cognition = &runtime.config().cognition;
    let scheduler_task = if config.scheduler_enabled || cognition.tick_enabled {
        let scheduler = runtime.clone();
        let scheduler_admission = admission.clone();
        let tick = Duration::from_millis(if config.scheduler_enabled {
            config.scheduler.tick.0.min(cognition.tick_interval.0)
        } else {
            cognition.tick_interval.0
        });
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = scheduler_admission.stopped() => break,
                    _ = interval.tick() => {}
                }
                let Ok(lease) = scheduler_admission.acquire() else {
                    break;
                };
                let scheduler = scheduler.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let _lease = lease;
                    scheduler.scheduler_pass(now_ms())
                })
                .await;
            }
        }))
    } else {
        None
    };
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = stop_rx.await;
        })
        .into_future();
    tokio::pin!(server);
    let (server_finished, stop_result) = tokio::select! {
        result = &mut shutdown => (false, result),
        result = &mut server => (true, result.map_err(|error| p::Error(format!("local gateway server failed: {error}")))),
    };
    // The same lock fences both HTTP blocking work and scheduler claims. A
    // timed-out HTTP request retains its lease inside the blocking operation.
    admission.close()?;
    let _ = stop_tx.send(());
    let deadline = tokio::time::Instant::now() + lifecycle.drain_timeout;
    let settle_work = async {
        admission.idle().await?;
        if let Some(task) = scheduler_task {
            task.await
                .map_err(|_| p::Error("core scheduler did not settle".into()))?;
        }
        let drain_runtime = runtime.clone();
        let limit = deadline.into_std();
        tokio::task::spawn_blocking(move || drain_runs(drain_runtime, limit))
            .await
            .map_err(|_| p::Error("core run drain did not settle".into()))??;
        Ok::<_, p::Error>(())
    };
    let settle_http = async {
        if !server_finished {
            server
                .await
                .map_err(|error| p::Error(format!("local gateway shutdown failed: {error}")))?;
        }
        Ok::<_, p::Error>(())
    };
    // Keep polling graceful shutdown while work drains, so the listener closes
    // immediately rather than waiting behind an in-flight model or tool call.
    let drain = async {
        tokio::try_join!(settle_http, settle_work)?;
        Ok::<_, p::Error>(())
    };
    tokio::time::timeout_at(deadline, drain)
        .await
        .map_err(|_| {
            p::Error("core shutdown deadline exceeded; normal startup recovery is required".into())
        })??;
    stop_result
}

fn drain_runs(runtime: Arc<ProductionRuntime>, deadline: Instant) -> p::Result<()> {
    let harness = runtime.harness();
    loop {
        if Instant::now() >= deadline {
            return Err(p::Error(
                "core run drain deadline exceeded; recovery is required".into(),
            ));
        }
        let runs = GatewayControl::list_runs(harness.as_ref())?;
        if runs
            .iter()
            .any(|run| matches!(run.status, p::RunStatus::Accepted | p::RunStatus::Running))
        {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        // Waiting includes persisted approval/unknown outcomes. Do not turn
        // either into an aborted or completed run just to make service stop.
        let sessions = runs
            .into_iter()
            .map(|run| run.session)
            .collect::<std::collections::BTreeSet<_>>();
        for session in sessions {
            AgentHarness::drain(harness.as_ref(), session)?;
        }
        return Ok(());
    }
}

struct CloseAdmissionOnDrop(Arc<WorkAdmission>);

impl Drop for CloseAdmissionOnDrop {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}

fn router(state: Arc<HttpState>, config: &GatewayServerConfig) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(styles))
        .route("/app.js", get(script))
        .route("/healthz", get(health))
        .route("/v1/runtime/manifest", get(runtime_manifest))
        .route("/v1/runtime/doctor", get(runtime_doctor))
        .route("/v1/runtime/tick", post(runtime_tick))
        .route("/v1/brain/workspace", get(brain_workspace))
        .route("/v1/brain/dashboard", get(brain_dashboard))
        .route("/v1/data/backup", post(data_backup))
        .route("/v1/data/verify-backup", post(data_verify_backup))
        .route("/v1/data/restore-plan", post(data_restore_plan))
        .route("/v1/data/restore", post(data_restore))
        .route("/v1/data/lifecycle", post(data_lifecycle))
        .route(
            "/v1/brain/projects",
            get(project_ledger).post(project_command),
        )
        .route(
            "/v1/workspaces/{id}/charter",
            get(read_workspace_charter).put(write_workspace_charter),
        )
        .route("/v1/brain/learning/goals", post(register_learning_goal))
        .route("/v1/brain/learning/answer", post(answer_learning))
        .route("/v1/brain/learning/resolve", post(resolve_learning))
        .route("/v1/profile", get(control_profile))
        .route("/v1/runs", get(list_runs).post(submit_run))
        .route("/v1/runs/{run}", get(run_summary))
        .route("/v1/runs/{run}/events", get(run_events))
        .route("/v1/runs/{run}/answer", get(run_answer))
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
        .route("/v1/evolution/seeds", post(initialize_strategy_seed))
        .route("/v1/evolution/specs", post(register_strategy_spec))
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
    runtime: Arc<ProductionRuntime>,
    owner: p::VerifiedPrincipal,
    token: LocalBearerSecret,
    origin: String,
    rate: Mutex<RateWindow>,
    max_requests_per_minute: u32,
    admission: Arc<WorkAdmission>,
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

impl HttpState {
    async fn blocking<T, F>(&self, operation: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce() -> p::Result<T> + Send + 'static,
    {
        let lease = self.admission.acquire().map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "core is draining; new work is unavailable",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _lease = lease;
            operation()
        })
        .await
        .map_err(|_| ApiError::internal("gateway worker did not complete"))?
        .map_err(ApiError::from)
    }
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

#[derive(Debug, Serialize)]
struct RuntimeServiceStateView {
    schema_version: p::SchemaVersion,
    name: String,
    status: String,
    reason: Option<p::ReasonRef>,
}

#[derive(Debug, Serialize)]
struct RuntimeManifestView {
    schema_version: p::SchemaVersion,
    owner_digest: p::SchemaDigest,
    store_identity: p::SchemaDigest,
    aggregate_refs: BTreeMap<String, p::RunId>,
    workspace: p::WorkspaceRef,
    agent_profile: p::AgentProfileRef,
    policy_profile: p::PolicyProfileRef,
    toolset_ref: p::ToolsetRef,
    model_profiles: Vec<p::ModelProfileRef>,
    capability_snapshot: p::SchemaDigest,
    service_states: Vec<RuntimeServiceStateView>,
    digest: p::SchemaDigest,
}

#[derive(Debug, Serialize)]
struct RuntimeDoctorView {
    schema_version: p::SchemaVersion,
    checks: Vec<RuntimeServiceStateView>,
    blocking: bool,
    digest: p::SchemaDigest,
}

/// One owner-brain tick signal: whether a window ran, and per phase what it
/// appended. The report digest is the one the durable tick ledger records.
#[derive(Debug, Serialize)]
struct RuntimeTickView {
    schema_version: p::SchemaVersion,
    ran: bool,
    skipped_reason: Option<String>,
    window: Option<p::Timestamp>,
    attempt: Option<u32>,
    run: Option<p::RunId>,
    phases: Vec<RuntimeTickPhaseView>,
    model_calls: u32,
    report_digest: Option<p::SchemaDigest>,
}

#[derive(Debug, Serialize)]
struct RuntimeTickPhaseView {
    phase: &'static str,
    status: &'static str,
    event_refs: Vec<p::EventId>,
    model_calls: u32,
    deferred_reason: Option<p::ReasonRef>,
    failure: Option<p::ReasonRef>,
}

fn runtime_tick_view(outcome: CognitiveTickOutcome) -> RuntimeTickView {
    match outcome {
        CognitiveTickOutcome::Ran(report) => {
            let report = *report;
            RuntimeTickView {
                schema_version: report.schema_version,
                ran: true,
                skipped_reason: None,
                window: Some(report.claim.window),
                attempt: Some(report.claim.attempt),
                model_calls: report.model_calls(),
                report_digest: Some(report.digest.clone()),
                run: Some(report.run),
                phases: report
                    .phases
                    .into_iter()
                    .map(|phase| RuntimeTickPhaseView {
                        phase: phase.phase.as_str(),
                        status: phase.status.as_str(),
                        event_refs: phase.event_refs,
                        model_calls: phase.model_calls,
                        deferred_reason: phase.deferred_reason,
                        failure: phase.failure,
                    })
                    .collect(),
            }
        }
        CognitiveTickOutcome::NotClaimed {
            reason,
            latest_window,
        } => RuntimeTickView {
            schema_version: p::SchemaVersion(1),
            ran: false,
            skipped_reason: Some(reason),
            window: latest_window,
            attempt: None,
            run: None,
            phases: Vec::new(),
            model_calls: 0,
            report_digest: None,
        },
    }
}

fn runtime_service_state_view(state: &RuntimeServiceState) -> RuntimeServiceStateView {
    RuntimeServiceStateView {
        schema_version: state.schema_version,
        name: state.name.clone(),
        status: match state.status {
            RuntimeServiceStatus::Ready => "ready",
            RuntimeServiceStatus::Disabled => "disabled",
            RuntimeServiceStatus::Unsupported => "unsupported",
            RuntimeServiceStatus::Blocking => "blocking",
        }
        .into(),
        reason: state.reason.clone(),
    }
}

fn runtime_manifest_view(manifest: &CompositionManifest) -> RuntimeManifestView {
    RuntimeManifestView {
        schema_version: manifest.schema_version,
        owner_digest: manifest.owner_digest.clone(),
        store_identity: manifest.store_identity.clone(),
        aggregate_refs: manifest
            .aggregate_refs
            .as_map()
            .into_iter()
            .map(|(kind, run)| (kind.as_str().to_owned(), run))
            .collect(),
        workspace: manifest.workspace.clone(),
        agent_profile: manifest.agent_profile.clone(),
        policy_profile: manifest.policy_profile.clone(),
        toolset_ref: manifest.toolset_ref.clone(),
        model_profiles: manifest.model_profiles.clone(),
        capability_snapshot: manifest.capability_snapshot.clone(),
        service_states: manifest
            .service_states
            .iter()
            .map(runtime_service_state_view)
            .collect(),
        digest: manifest.digest.clone(),
    }
}

fn runtime_doctor_view(doctor: &forme_harness::ConfigDoctorSnapshot) -> RuntimeDoctorView {
    RuntimeDoctorView {
        schema_version: doctor.schema_version,
        checks: doctor
            .checks
            .iter()
            .map(runtime_service_state_view)
            .collect(),
        blocking: doctor.blocking,
        digest: doctor.digest.clone(),
    }
}

async fn runtime_manifest(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<RuntimeManifestView>, ApiError> {
    state.authorize(&headers, false)?;
    Ok(Json(runtime_manifest_view(state.runtime.manifest())))
}

async fn runtime_doctor(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<RuntimeDoctorView>, ApiError> {
    state.authorize(&headers, false)?;
    Ok(Json(runtime_doctor_view(&state.runtime.doctor())))
}

async fn runtime_tick(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<RuntimeTickView>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let outcome = state
        .blocking(move || runtime.signal_cognitive_tick())
        .await?;
    Ok(Json(runtime_tick_view(outcome)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrainWorkspaceQuery {
    scope: Option<String>,
    capacity: Option<usize>,
    at: Option<p::Timestamp>,
}

async fn brain_workspace(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<BrainWorkspaceQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    let projection = state
        .blocking(move || {
            gateway.agent_workspace(
                forme_harness::AgentWorkspaceQuery {
                    schema_version: p::SchemaVersion(1),
                    scope: query.scope.map(p::Scope),
                    capacity: query.capacity.unwrap_or(10),
                    now: query.at.unwrap_or_else(now_ms),
                    config_version: p::Version(1),
                },
                auth,
            )
        })
        .await?;
    Ok(Json(
        serde_json::json!({"schema_version":1,"snapshot_ref":projection.snapshot_ref,"capacity":projection.capacity,
        "items":projection.items.iter().map(|item|serde_json::json!({"schema_version":1,"id":item.id,"kind":format!("{:?}",item.kind),
            "scope":item.scope,"value":item.value,"urgency":item.urgency,"event_ref":item.event_ref,"run":item.run,"stream_seq":item.stream_seq})).collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrainDashboardQuery {
    workspace: Option<String>,
    from: Option<p::Timestamp>,
    to: Option<p::Timestamp>,
}

async fn brain_dashboard(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<BrainDashboardQuery>,
    headers: HeaderMap,
) -> Result<Json<forme_harness::CognitiveDashboardSnapshot>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let snapshot = state
        .blocking(move || {
            runtime.dashboard(query.workspace.map(p::WorkspaceRef), query.from, query.to)
        })
        .await?;
    Ok(Json(snapshot))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataTargetRequest {
    schema_version: p::SchemaVersion,
    target: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataSourceRequest {
    schema_version: p::SchemaVersion,
    source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataRestorePlanRequest {
    schema_version: p::SchemaVersion,
    source: String,
    target: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataApplyRequest {
    schema_version: p::SchemaVersion,
    plan: forme_harness::DataOperationPlan,
    confirmation: forme_harness::DataOperationConfirmation,
}
#[derive(Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
enum DataLifecycleCommand {
    Prepare {
        schema_version: p::SchemaVersion,
        scope: p::Scope,
        subject_digest: p::SchemaDigest,
        operation: p::DataLifecycleOperation,
        expected_version: u64,
    },
    Apply {
        schema_version: p::SchemaVersion,
        plan: Box<forme_harness::DataOperationPlan>,
        confirmation: forme_harness::DataOperationConfirmation,
    },
}
fn data_schema(version: p::SchemaVersion) -> Result<(), ApiError> {
    if version.0 != 1 {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported data control schema",
        ))
    } else {
        Ok(())
    }
}
async fn data_backup(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataTargetRequest>,
) -> Result<Json<forme_harness::BackupManifest>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.create_data_backup(&owner, &r.target))
            .await?,
    ))
}
async fn data_verify_backup(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataSourceRequest>,
) -> Result<Json<forme_harness::BackupVerification>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.verify_data_backup(&owner, &r.source))
            .await?,
    ))
}
async fn data_restore_plan(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataRestorePlanRequest>,
) -> Result<Json<forme_harness::DataOperationPlan>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.prepare_data_restore(&owner, r.source, r.target))
            .await?,
    ))
}
async fn data_restore(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataApplyRequest>,
) -> Result<Json<forme_harness::DataOperationResult>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    if !matches!(
        r.plan.operation,
        forme_harness::DataPlannedOperation::Restore { .. }
    ) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "restore route requires a restore plan",
        ));
    }
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.apply_data_operation(&owner, r.plan, r.confirmation))
            .await?,
    ))
}
async fn data_lifecycle(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<DataLifecycleCommand>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    let value = state
        .blocking(move || match command {
            DataLifecycleCommand::Prepare {
                schema_version,
                scope,
                subject_digest,
                operation,
                expected_version,
            } => {
                if schema_version.0 != 1 {
                    return Err(p::Error("unsupported lifecycle schema".into()));
                }
                serde_json::to_value(runtime.prepare_data_lifecycle(
                    &owner,
                    scope,
                    subject_digest,
                    operation,
                    expected_version,
                )?)
                .map_err(|_| p::Error("lifecycle plan response failed".into()))
            }
            DataLifecycleCommand::Apply {
                schema_version,
                plan,
                confirmation,
            } => {
                if schema_version.0 != 1
                    || !matches!(
                        plan.operation,
                        forme_harness::DataPlannedOperation::Lifecycle { .. }
                    )
                {
                    return Err(p::Error("lifecycle route requires a lifecycle plan".into()));
                }
                serde_json::to_value(runtime.apply_data_operation(&owner, *plan, confirmation)?)
                    .map_err(|_| p::Error("lifecycle response failed".into()))
            }
        })
        .await?;
    Ok(Json(value))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LearningGoalRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    goal: p::GoalFrameRef,
    knowledge: p::GoalKnowledge,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LearningAnswerRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    reply: p::LearningReplyRecord,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LearningResolveRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    proposal: p::ProposalRef,
    outcome: p::ProposalOutcome,
    defer_until: Option<p::Timestamp>,
}

fn learning_schema(version: p::SchemaVersion) -> Result<(), ApiError> {
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
struct CharterRequest {
    schema_version: p::SchemaVersion,
    command: p::IdempotencyKey,
    change: p::WorkspaceCharterChangedPayload,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRequest {
    schema_version: p::SchemaVersion,
    request: ProjectCommand,
}
#[derive(serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum ProjectCommand {
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
async fn project_ledger(
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
async fn project_command(
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

async fn read_workspace_charter(
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

async fn write_workspace_charter(
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

async fn register_learning_goal(
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
async fn answer_learning(
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
async fn resolve_learning(
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

async fn control_profile(
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

async fn submit_run(
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

async fn list_runs(
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

async fn run_summary(
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

#[derive(Debug, Serialize)]
struct RunAnswerView {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    answer: Option<String>,
}

async fn run_answer(
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

async fn approvals(
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
    state
        .blocking(move || gateway.control_run(p::RunId(run), control, auth))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn trace_view(
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

async fn list_jobs(
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

async fn schedule_job(
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

async fn cancel_job(
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
    state
        .blocking(move || gateway.review_candidate(command, auth))
        .await?;
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
    state
        .blocking(move || gateway.run_eval(request.case, request.profile, auth))
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
    state
        .blocking(move || gateway.export_eval(p::EvalRef(eval), auth))
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
    state
        .blocking(move || gateway.evolution_snapshot(p::Scope(query.scope), auth))
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
    state
        .blocking(move || gateway.ecosystem_snapshot(p::Scope(query.scope), auth))
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
    let result = state
        .blocking(move || {
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
    let admission = state
        .blocking(move || {
            gateway.admit_capability_package(
                request.run,
                request.catalog_run,
                request.package,
                auth,
            )
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
    state
        .blocking(move || {
            gateway.prepare_capability_change(
                request.operation,
                request.package,
                request.release,
                auth,
            )
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
    state
        .blocking(move || {
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
    state
        .blocking(move || {
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
    let paused = state
        .blocking(move || gateway.auto_activation_paused(auth))
        .await?;
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
    state
        .blocking(move || gateway.set_auto_activation_paused(paused, auth))
        .await?;
    Ok(Json(AutoActivationStatus {
        schema_version: p::SchemaVersion(1),
        paused,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitializeStrategySeedRequest {
    schema_version: p::SchemaVersion,
    domain: p::StrategyDomain,
    workspace: p::WorkspaceRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterStrategySpecRequest {
    schema_version: p::SchemaVersion,
    spec: forme_harness::ProductionStrategySpec,
}

async fn initialize_strategy_seed(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<InitializeStrategySeedRequest>,
) -> Result<Json<forme_harness::ProductionStrategySpec>, ApiError> {
    require_schema(request.schema_version)?;
    state.authorize(&headers, true)?;
    let owner = state.owner.clone();
    let runtime = state.runtime.clone();
    let body = state
        .blocking(move || {
            runtime.initialize_strategy_seed(&owner, request.domain, &request.workspace)
        })
        .await?;
    Ok(Json(body))
}

async fn register_strategy_spec(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<RegisterStrategySpecRequest>,
) -> Result<Json<forme_harness::ProductionStrategySpec>, ApiError> {
    require_schema(request.schema_version)?;
    state.authorize(&headers, true)?;
    let owner = state.owner.clone();
    let runtime = state.runtime.clone();
    let body = state
        .blocking(move || runtime.register_strategy_spec(&owner, request.spec))
        .await?;
    Ok(Json(body))
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
    let event = state
        .blocking(move || gateway.record_strategy_candidate(request.run, request.candidate, auth))
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
    let result = state
        .blocking(move || {
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
    let event = state
        .blocking(move || {
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
    let result = state
        .blocking(move || {
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
    let result = state
        .blocking(move || {
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

pub(crate) fn default_token_path() -> PathBuf {
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

fn required_env(key: &str) -> p::Result<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| p::Error(format!("required configuration {key} is missing")))
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

async fn shutdown_signal() -> p::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|_| p::Error("core termination signal is unavailable".into()))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(|_| p::Error("core interrupt signal is unavailable".into())),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .map_err(|_| p::Error("core interrupt signal is unavailable".into()))
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
