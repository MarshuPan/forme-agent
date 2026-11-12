mod channels;
mod maintenance_admission;
mod managed_config;
mod managed_discovery;
mod release_admission;
mod release_health;
mod runtime;
pub use managed_config::CoreServiceConfig;
#[cfg(test)]
use runtime::serve_runtime_until;
pub use runtime::{serve_environment, serve_environment_until, serve_managed_until};
mod observability;
use observability::*;
mod artifacts;
mod data;
mod data_workbench;
mod tool_artifacts;
use data::*;
mod project_controls;
mod projects;
use projects::*;
mod runs;
use runs::*;
mod evaluation;
use evaluation::*;
mod evolution;
mod workbench;
use evolution::*;
mod secrets;
pub(crate) use secrets::default_token_path;
use secrets::*;
mod web_bridge;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::future::{Future, IntoFuture};
use std::io::{ErrorKind, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
pub(crate) use web_bridge::apply_offline_session_fence;
use web_bridge::LocalBearerSecret;
use web_bridge::{
    authenticate_web_bridge, load_configuration as load_web_bridge_configuration,
    validate_configuration as validate_web_bridge_configuration, WebBridgeState,
};

use axum::extract::{DefaultBodyLimit, Extension, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ed25519_dalek::VerifyingKey;
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
#[cfg(test)]
#[path = "server/tests.rs"]
mod tests;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");
const GOLDEN_TASKS: &str = include_str!("../../../evals/m1/golden-tasks.json");
const CSRF_HEADER: &str = "x-forme-csrf";

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
    pub install_ref: Option<String>,
    web_bridge_token: Option<LocalBearerSecret>,
    web_assertion_verifying_key: Option<VerifyingKey>,
    web_session_fence_path: Option<PathBuf>,
    token: LocalBearerSecret,
    token_path: Option<PathBuf>,
    channel_ingress: Option<p::ProductChannelIngress>,
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
            .field("install_ref", &self.install_ref)
            .field("web_bridge_token", &self.web_bridge_token)
            .field(
                "web_assertion_verifying_key_configured",
                &self.web_assertion_verifying_key.is_some(),
            )
            .field(
                "web_session_fence_configured",
                &self.web_session_fence_path.is_some(),
            )
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
        let bridge = load_web_bridge_configuration()?;
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
            install_ref: bridge.install_ref,
            web_bridge_token: bridge.web_bridge_token,
            web_assertion_verifying_key: bridge.web_assertion_verifying_key,
            web_session_fence_path: bridge.fence_path,
            token,
            token_path,
            channel_ingress: None,
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
        validate_web_bridge_configuration(
            self.install_ref.as_deref(),
            self.web_bridge_token.as_ref(),
            self.web_assertion_verifying_key.as_ref(),
            self.web_session_fence_path.as_deref(),
            &self.token,
        )?;
        Ok(())
    }
}

fn router(state: Arc<HttpState>, config: &GatewayServerConfig) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(styles))
        .route("/app.js", get(script))
        .route("/healthz", get(health))
        .route(
            "/v1/product/release/readiness",
            post(release_health::probe).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/v1/runtime/manifest", get(runtime_manifest))
        .route(
            "/v1/web/session-fence",
            post(web_bridge::apply_session_fence),
        )
        .route("/v1/runtime/doctor", get(runtime_doctor))
        .route("/v1/runtime/tick", post(runtime_tick))
        .route("/v1/brain/workspace", get(brain_workspace))
        .route("/v1/brain/dashboard", get(brain_dashboard))
        .merge(workbench::routes())
        .route("/v1/data/backup", post(data_backup))
        .route("/v1/data/verify-backup", post(data_verify_backup))
        .route("/v1/data/restore-plan", post(data_restore_plan))
        .route("/v1/data/restore", post(data_restore))
        .route("/v1/data/lifecycle", post(data_lifecycle))
        .merge(data_workbench::routes())
        .merge(project_controls::routes())
        .merge(managed_discovery::routes())
        .route(
            "/v1/channels/{channel}/messages",
            post(channels::receive).layer(DefaultBodyLimit::max(20 * 1024)),
        )
        .route(
            "/v1/brain/projects",
            get(project_ledger).post(project_command),
        )
        .route(
            "/v1/workspaces/{id}/charter",
            get(read_workspace_charter)
                .post(prepare_workspace_charter)
                .put(write_workspace_charter),
        )
        .route("/v1/brain/learning/goals", post(register_learning_goal))
        .route("/v1/brain/learning/answer", post(answer_learning))
        .route("/v1/brain/learning/resolve", post(resolve_learning))
        .merge(artifacts::routes())
        .merge(tool_artifacts::routes())
        .route("/v1/profile", get(control_profile))
        .route("/v1/workbench/runs", post(submit_workbench_run))
        .route("/v1/runs", get(list_runs).post(submit_run))
        .route("/v1/runs/{run}", get(run_summary))
        .route("/v1/runs/{run}/pending", get(run_pending))
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
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_web_bridge,
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
    max_body_bytes: usize,
    admission: Arc<WorkAdmission>,
    web_bridge: WebBridgeState,
    release_health: Option<release_health::ReleaseHealth>,
    channel_ingress: Option<p::ProductChannelIngress>,
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
