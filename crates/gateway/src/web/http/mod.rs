//! Independent Web ingress; no ProductionRuntime, provider or brain-store access.
mod assets;
mod auth;
mod bootstrap;
mod connections;
#[path = "core.rs"]
mod core_routes;
mod error;
mod security;

#[cfg(test)]
mod tests;

use std::future::{Future, IntoFuture};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post, put};
use axum::Router;
use forme_protocol as p;
use tokio::sync::Semaphore;
use tower_http::timeout::TimeoutLayer;

use crate::lifecycle::WorkAdmission;
use crate::web::identity::{AuthError, AuthResult, AuthenticatedWebOwner, WebIdentityStore};
use crate::web::WebServerConfig;
use error::WebError;

pub(super) struct WebState {
    identity: Arc<WebIdentityStore>,
    bootstrap: Option<Arc<crate::web::bootstrap::LocalBootstrapClient>>,
    core: Option<Arc<crate::web::core::CoreBridgeClient>>,
    authority: String,
    origin: String,
    rate: Mutex<security::RateWindow>,
    reads: Arc<Semaphore>,
    passwords: Arc<Semaphore>,
    admission: Arc<WorkAdmission>,
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

    async fn fence_session(
        &self,
        owner: &AuthenticatedWebOwner,
        session_digest: Option<p::SchemaDigest>,
        minimum_auth_epoch: u64,
    ) -> Result<(), WebError> {
        let Some(client) = self.core.clone() else {
            // During first-run/maintenance there is no core that could accept
            // an assertion. The Web identity store remains independently
            // usable; once a bridge is configured, failures are fail-closed.
            return Ok(());
        };
        let binding = owner.core_binding();
        tokio::task::spawn_blocking(move || {
            client.fence_session(&binding, session_digest, minimum_auth_epoch)
        })
        .await
        .map_err(|_| {
            WebError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "CoreUnavailable",
                "主脑服务不可用",
            )
        })?
        .map_err(|_| {
            WebError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "CoreUnavailable",
                "主脑会话撤销无法提交",
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
    if let Some(client) = bootstrap.clone() {
        let recovery = identity.clone();
        let result = tokio::task::spawn_blocking(move || recovery.recover_bootstrap(&client)).await;
        if !matches!(result, Ok(Ok(()))) {
            eprintln!("forme-webd: an unfinished installation claim remains pending; local setup recovery may be required");
        }
    }
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| {
            p::Error("Web address is unavailable; choose a free configured port".into())
        })?;
    serve_identity_with_setup_until(listener, identity, bootstrap, core, on_bound, shutdown).await
}

#[cfg(test)]
async fn serve_identity_until<F, S>(
    listener: tokio::net::TcpListener,
    identity: Arc<WebIdentityStore>,
    on_bound: F,
    shutdown: S,
) -> p::Result<()>
where
    F: FnOnce(SocketAddr) -> p::Result<()>,
    S: Future<Output = p::Result<()>>,
{
    serve_identity_with_setup_until(listener, identity, None, None, on_bound, shutdown).await
}

async fn serve_identity_with_setup_until<F, S>(
    listener: tokio::net::TcpListener,
    identity: Arc<WebIdentityStore>,
    bootstrap: Option<Arc<crate::web::bootstrap::LocalBootstrapClient>>,
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
    if !address.ip().is_loopback() {
        return Err(p::Error(
            "plaintext Web ingress must remain loopback-only".into(),
        ));
    }
    let admission = Arc::new(WorkAdmission::default());
    let _close_on_drop = CloseWebAdmission(admission.clone());
    let state = Arc::new(WebState {
        identity,
        bootstrap,
        core,
        authority: address.to_string(),
        origin: format!("http://{address}"),
        rate: Mutex::new(security::RateWindow::new()),
        reads: Arc::new(Semaphore::new(16)),
        passwords: Arc::new(Semaphore::new(2)),
        admission: admission.clone(),
    });
    let signal = admission.clone();
    let server = axum::serve(connections::BoundedListener::new(listener), router(state))
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
