//! HTTP runtime responsibilities; no independent runtime or writer.
use super::*;

pub(super) async fn shutdown_signal() -> p::Result<()> {
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

pub(super) async fn serve_runtime_until<F, S>(
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

pub(super) fn drain_runs(runtime: Arc<ProductionRuntime>, deadline: Instant) -> p::Result<()> {
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

pub(super) struct CloseAdmissionOnDrop(Arc<WorkAdmission>);

impl Drop for CloseAdmissionOnDrop {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}
