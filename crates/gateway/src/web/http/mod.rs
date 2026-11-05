//! Independent Web ingress; no ProductionRuntime, provider or brain-store access.
mod assets;
mod auth;
mod backup_verification;
mod bootstrap;
mod configuration;
mod connections;
#[path = "core.rs"]
mod core_routes;
mod data;
mod error;
mod legacy_release_recovery;
mod maintenance;
mod network;
mod proposal_controls;
mod release_recovery;
mod security;
mod sessions;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tls_tests;

use std::future::{Future, IntoFuture};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post, put};
use axum::serve::Listener;
use axum::Router;
use forme_protocol as p;
use tokio::sync::Semaphore;
use tower_http::timeout::TimeoutLayer;

use crate::lifecycle::WorkAdmission;
use crate::web::identity::{AuthError, AuthResult, FenceOutboxRecord, WebIdentityStore};
use crate::web::WebServerConfig;
use error::WebError;

pub(super) struct WebState {
    identity: Arc<WebIdentityStore>,
    bootstrap: Option<Arc<crate::web::bootstrap::LocalBootstrapClient>>,
    configuration: Option<Arc<crate::web::configuration::LocalConfigurationClient>>,
    core: Option<Arc<crate::web::core::CoreBridgeClient>>,
    authorities: Vec<String>,
    secure: bool,
    listener_digest: p::SchemaDigest,
    rate: Mutex<security::RateWindow>,
    reads: Arc<Semaphore>,
    passwords: Arc<Semaphore>,
    admission: Arc<WorkAdmission>,
    fence_gate: Arc<tokio::sync::Mutex<()>>,
}

impl WebState {
    async fn blocking<T: Send + 'static>(
        &self,
        password_work: bool,
        work: impl FnOnce(&WebIdentityStore) -> AuthResult<T> + Send + 'static,
    ) -> Result<T, WebError> {
        let lease = self
            .admission
            .acquire()
            .map_err(|_| AuthError::Unavailable)?;
        let capacity = if password_work {
            &self.passwords
        } else {
            &self.reads
        };
        let permit = capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        let identity = self.identity.clone();
        tokio::task::spawn_blocking(move || {
            // Both guards live in the real work, including after client timeout.
            let (_lease, _permit) = (lease, permit);
            work(&identity)
        })
        .await
        .map_err(|_| AuthError::Unavailable)?
        .map_err(Into::into)
    }

    async fn fence_gate(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.fence_gate.clone().lock_owned().await
    }

    async fn core_access(&self) -> Result<tokio::sync::OwnedMutexGuard<()>, WebError> {
        let gate = self.fence_gate.clone().lock_owned().await;
        if self.sync_fences_locked().await? == FenceSync::Clean {
            Ok(gate)
        } else {
            Err(WebError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "CoreFencePending",
                "主脑授权撤销正在同步，请稍后再访问主脑",
            ))
        }
    }

    async fn sync_fences_locked(&self) -> Result<FenceSync, WebError> {
        let pending = self
            .blocking(false, |identity| identity.pending_fences())
            .await?;
        if pending.is_empty() {
            return Ok(FenceSync::Clean);
        }
        let Some(client) = self.core.clone() else {
            return Ok(FenceSync::Pending);
        };
        for fence in pending {
            if !send_fence(&client, &fence).await {
                return Ok(FenceSync::Pending);
            }
            let operation = fence.operation.clone();
            if self
                .blocking(false, move |identity| {
                    identity.acknowledge_fence(&operation)
                })
                .await
                .is_err()
            {
                return Ok(FenceSync::Pending);
            }
        }
        let remaining = self
            .blocking(false, |identity| identity.pending_fences())
            .await?;
        Ok(if remaining.is_empty() {
            FenceSync::Clean
        } else {
            FenceSync::Pending
        })
    }

    async fn configuration_call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&crate::web::configuration::LocalConfigurationClient) -> p::Result<T>
            + Send
            + 'static,
    ) -> Result<T, WebError> {
        let client = self.configuration.clone().ok_or_else(|| {
            WebError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "ConfigurationUnavailable",
                "产品配置服务不可用",
            )
        })?;
        let lease = self
            .admission
            .acquire()
            .map_err(|_| AuthError::Unavailable)?;
        let permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        tokio::task::spawn_blocking(move || {
            let (_lease, _permit) = (lease, permit);
            work(client.as_ref())
        })
        .await
        .map_err(|_| {
            WebError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "ConfigurationUnavailable",
                "产品配置服务不可用",
            )
        })?
        .map_err(|error| {
            if error.0 == "configuration patch rejected before publication" {
                return WebError::new(
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    "ConfigurationInvalid",
                    "模型配置未保存，请检查端点、模型参数及密钥",
                );
            }
            WebError::new(
                if error.0.contains("stale") || error.0.contains("reused") {
                    axum::http::StatusCode::CONFLICT
                } else {
                    axum::http::StatusCode::BAD_GATEWAY
                },
                "ConfigurationRejected",
                "产品配置没有被后台接受",
            )
        })
    }
}

fn router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/", get(assets::index))
        .route("/favicon.svg", get(assets::file))
        .route("/LICENSES.txt", get(assets::file))
        .route("/assets/{file}", get(assets::file))
        .route("/api/v1/bootstrap/status", get(auth::bootstrap))
        .route("/api/v1/bootstrap/prove", post(bootstrap::prove))
        .route(
            "/api/v1/bootstrap/data",
            get(bootstrap::data).post(bootstrap::select),
        )
        .route("/api/v1/bootstrap/claim", post(bootstrap::claim))
        .route("/api/v1/auth/login-challenge", get(auth::challenge))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/session", get(auth::session))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/auth/reauth", post(auth::reauthenticate))
        .route("/api/v1/auth/sessions", delete(auth::revoke))
        .route("/api/v1/auth/password", put(auth::password))
        .route(
            "/api/v1/sessions",
            get(sessions::list).post(sessions::create),
        )
        .route(
            "/api/v1/workbench/proposals/pending",
            get(proposal_controls::pending),
        )
        .route("/api/v1/sessions/{session}", put(sessions::update))
        .route("/api/v1/data/query", post(data::query))
        .route("/api/v1/data/command", post(data::command))
        .route(
            "/api/v1/product/data/verify",
            post(backup_verification::verify),
        )
        .route(
            "/api/v1/product/config",
            get(configuration::read)
                .post(configuration::write)
                .layer(DefaultBodyLimit::max(256 * 1024)),
        )
        .route("/api/v1/product/status", get(configuration::status))
        .route("/api/v1/product/network/health", get(network::health))
        .route(
            "/api/v1/product/config/validate",
            post(configuration::validate),
        )
        .route(
            "/api/v1/product/config/activate",
            post(configuration::activate),
        )
        .route(
            "/api/v1/product/operations/{id}",
            get(configuration::operation),
        )
        .route(
            "/api/v1/product/maintenance",
            get(maintenance::read).post(maintenance::apply),
        )
        .route(
            "/api/v1/product/maintenance/operations/{id}",
            get(maintenance::operation),
        )
        .route(
            "/api/v1/product/release-recovery/candidates/{id}",
            get(release_recovery::candidate),
        )
        .route(
            "/api/v1/product/release-recovery",
            post(release_recovery::apply),
        )
        .route(
            "/api/v1/product/release-recovery/operations/{id}",
            get(release_recovery::operation),
        )
        .route(
            "/api/v1/product/release-recovery/operations",
            get(release_recovery::operations),
        )
        .route(
            "/api/v1/product/release-recovery/verify",
            post(release_recovery::verify),
        )
        .route(
            "/api/v1/product/legacy-release-recovery/review/{id}",
            get(legacy_release_recovery::review),
        )
        .route(
            "/api/v1/product/legacy-release-recovery",
            post(legacy_release_recovery::apply),
        )
        .route(
            "/api/v1/product/legacy-release-recovery/operations/{id}",
            get(legacy_release_recovery::operation),
        )
        .route(
            "/api/v1/product/legacy-release-recovery/operations",
            get(legacy_release_recovery::operations),
        )
        .route(
            "/api/v1/product/legacy-release-recovery/verify",
            post(legacy_release_recovery::verify),
        )
        .route(
            "/api/v1/brain/artifacts/upload",
            post(core_routes::upload).layer(DefaultBodyLimit::max(
                p::ArtifactUploadCommand::MAX_REQUEST_BYTES,
            )),
        )
        .route(
            "/api/v1/brain/{*path}",
            get(core_routes::read)
                .post(core_routes::mutate)
                .put(core_routes::replace)
                .delete(core_routes::mutate),
        )
        .fallback(auth::missing)
        .layer(DefaultBodyLimit::max(8192))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            security::boundary,
        ))
        .with_state(state)
}

/// Start the independent ingress from installer-owned configuration. Startup
/// opens only its own authentication store, not the core or personal brain.
pub async fn serve_until<F, S>(config: WebServerConfig, on_bound: F, shutdown: S) -> p::Result<()>
where
    F: FnOnce(SocketAddr) -> p::Result<()>,
    S: Future<Output = p::Result<()>>,
{
    config.validate()?;
    // Fail before opening the authentication writer if TLS material is invalid.
    let tls = config
        .tls
        .as_ref()
        .map(|tls| {
            tls.load()
                .map(|acceptor| (acceptor, tls.allowed_hosts.clone()))
        })
        .transpose()?;
    let open = config.clone();
    let identity = tokio::task::spawn_blocking(move || {
        let key = ed25519_dalek::VerifyingKey::from_bytes(&open.claim_verifying_key)
            .map_err(|_| AuthError::InvalidInput)?;
        WebIdentityStore::open(&open.identity_directory, &open.install_ref, key)
    })
    .await
    .map_err(|_| p::Error("Web authentication initialization unavailable".into()))?
    .map_err(|_| p::Error("Web authentication storage requires local maintenance".into()))?;
    let identity = Arc::new(identity);
    let bootstrap = config
        .bootstrap_bridge
        .as_ref()
        .map(|bridge| {
            crate::web::bootstrap::LocalBootstrapClient::connect(
                bridge.address,
                &bridge.credential_path,
            )
            .map(Arc::new)
        })
        .transpose()
        .map_err(|_| {
            p::Error("Web setup bridge configuration requires local maintenance".into())
        })?;
    let core = config
        .core_bridge
        .as_ref()
        .map(|bridge| {
            crate::web::core::CoreBridgeClient::connect(bridge, &config.install_ref).map(Arc::new)
        })
        .transpose()?;
    let configuration = config
        .configuration_bridge
        .as_ref()
        .map(|bridge| {
            crate::web::configuration::LocalConfigurationClient::connect(
                bridge.address,
                &bridge.credential_path,
            )
            .map(Arc::new)
        })
        .transpose()?;
    if let Some(client) = bootstrap.clone() {
        let recovery = identity.clone();
        let configuration = configuration.clone();
        let result = tokio::task::spawn_blocking(move || {
            recovery.recover_bootstrap(&client, configuration.as_deref())
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            eprintln!("forme-webd: an unfinished installation claim remains pending; local setup recovery may be required");
        }
    }
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| {
            p::Error("Web address is unavailable; choose a free configured port".into())
        })?;
    let listener = if let Some((acceptor, hosts)) = tls {
        connections::BoundedListener::encrypted(listener, acceptor, hosts)
    } else {
        connections::BoundedListener::new(listener)
    };
    serve_identity_with_setup_until(
        listener,
        identity,
        bootstrap,
        configuration,
        core,
        on_bound,
        shutdown,
    )
    .await
}

async fn serve_identity_with_setup_until<F, S>(
    listener: connections::BoundedListener,
    identity: Arc<WebIdentityStore>,
    bootstrap: Option<Arc<crate::web::bootstrap::LocalBootstrapClient>>,
    configuration: Option<Arc<crate::web::configuration::LocalConfigurationClient>>,
    core: Option<Arc<crate::web::core::CoreBridgeClient>>,
    on_bound: F,
    shutdown: S,
) -> p::Result<()>
where
    F: FnOnce(SocketAddr) -> p::Result<()>,
    S: Future<Output = p::Result<()>>,
{
    let address = listener
        .local_addr()
        .map_err(|_| p::Error("Web listener unavailable".into()))?;
    let secure = listener.secure();
    if !secure && !address.ip().is_loopback() {
        return Err(p::Error(
            "plaintext Web ingress must remain loopback-only".into(),
        ));
    }
    let authorities = if secure {
        listener.authorities.clone()
    } else {
        vec![address.to_string()]
    };
    let listener_digest = p::product_network_listener_digest(address, &authorities, secure)?;
    let admission = Arc::new(WorkAdmission::default());
    let _close_on_drop = CloseWebAdmission(admission.clone());
    let state = Arc::new(WebState {
        identity,
        bootstrap,
        configuration,
        core,
        authorities,
        secure,
        listener_digest,
        rate: Mutex::new(security::RateWindow::new()),
        reads: Arc::new(Semaphore::new(16)),
        passwords: Arc::new(Semaphore::new(2)),
        admission: admission.clone(),
        fence_gate: Arc::new(tokio::sync::Mutex::new(())),
    });
    let signal = admission.clone();
    let server = axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            signal.stopped().await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::pin!(shutdown);
    tokio::select! { biased; result = &mut shutdown => return result, _ = std::future::ready(()) => {} }
    on_bound(address)?;
    let (result, finished) = tokio::select! {
        result = &mut shutdown => (result, false),
        result = &mut server => (result.map_err(|_| p::Error("Web listener failed".into())), true),
    };
    admission.close()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    if !finished {
        tokio::time::timeout_at(deadline, &mut server)
            .await
            .map_err(|_| p::Error("Web connection drain timed out".into()))?
            .map_err(|_| p::Error("Web listener failed during drain".into()))?;
    }
    tokio::time::timeout_at(deadline, admission.idle())
        .await
        .map_err(|_| p::Error("Web authentication drain timed out".into()))??;
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FenceSync {
    Clean,
    Pending,
}

async fn send_fence(
    client: &Arc<crate::web::core::CoreBridgeClient>,
    fence: &FenceOutboxRecord,
) -> bool {
    let client = client.clone();
    let binding = fence.binding();
    let target = fence.target_session.clone();
    let minimum = fence.minimum_auth_epoch;
    tokio::task::spawn_blocking(move || client.fence_session(&binding, target, minimum))
        .await
        .is_ok_and(|result| result.is_ok())
}

pub async fn shutdown_signal() -> p::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|_| p::Error("Web termination signal unavailable".into()))?;
        tokio::select! {
            result=tokio::signal::ctrl_c()=>result.map_err(|_|p::Error("Web interrupt signal unavailable".into())),
            _=terminate.recv()=>Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .map_err(|_| p::Error("Web interrupt signal unavailable".into()))
}

struct CloseWebAdmission(Arc<WorkAdmission>);
impl Drop for CloseWebAdmission {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}
