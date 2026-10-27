//! Host process composition and bounded listener lifecycle.
mod bootstrap;
pub(super) mod config;
pub(super) mod http;
#[cfg(windows)]
mod service;
mod startup;
#[cfg(windows)]
mod updater_service;
pub(super) mod worker_routes;

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use config::*;
#[cfg(windows)]
use forme_execution::installed::WindowsAgentExecutionListener;
use forme_execution::installed::{ProductConfigStore, ProductHostBroker};
use forme_protocol as p;
use http::handle_connection;

pub(super) fn run() -> p::Result<()> {
    let launch = launch_from_args(std::env::args_os().skip(1))?;
    let launch = match launch {
        Launch::VerifyRelease {
            root,
            trust,
            target,
        } => {
            let config = forme_execution::installed::ReleaseStagingConfig::load(&trust)?;
            let digest = forme_execution::installed::ReleaseStager::verify_directory(
                &root, &config, target,
            )?;
            println!(
                "{}",
                serde_json::json!({ "schema_version": 1, "release_digest": digest,
                "verification": "signature_and_closed_content", "native_installation": "not_performed" })
            );
            return Ok(());
        }
        launch => launch,
    };
    #[cfg(windows)]
    {
        match launch {
            Launch::DevelopmentConfig(path) => run_foreground(Launch::DevelopmentConfig(path)),
            Launch::InstallService(path) => service::install(&path),
            Launch::UninstallService(path) => service::uninstall(&path),
            Launch::ReleaseUpdater(_) => updater_service::dispatch().map_err(|_| {
                p::Error("release updater must run under its fixed SCM service".into())
            }),
            Launch::Development { .. } | Launch::Managed(_) => match service::dispatch() {
                Ok(()) => Ok(()),
                Err(windows_service::Error::Winapi(error))
                    if error.raw_os_error() == Some(1063) =>
                {
                    run_foreground(launch)
                }
                Err(error) => Err(p::Error(format!(
                    "Windows service dispatcher failed: {error}"
                ))),
            },
            Launch::VerifyRelease { .. } => Err(p::Error(
                "offline verification cannot start a service".into(),
            )),
        }
    }
    #[cfg(not(windows))]
    match launch {
        Launch::DevelopmentConfig(path) => run_foreground(Launch::DevelopmentConfig(path)),
        Launch::Development { .. } | Launch::Managed(_) => run_foreground(launch),
        Launch::InstallService(_) | Launch::UninstallService(_) => Err(p::Error(
            "host service installation is managed by the native package manager on this platform"
                .into(),
        )),
        Launch::ReleaseUpdater(_) => Err(p::Error(
            "native independent release updater is unavailable on this platform".into(),
        )),
        Launch::VerifyRelease { .. } => Err(p::Error(
            "offline verification cannot start a service".into(),
        )),
    }
}

fn run_foreground(launch: Launch) -> p::Result<()> {
    #[cfg(windows)]
    if matches!(&launch, Launch::Managed(_)) {
        // A registered Brain SID is not proof that this process belongs to SCM.
        // Reject before loading credentials, creating journals, or binding IPC.
        return Err(p::Error(
            "managed Windows Host requires its fixed SCM service".into(),
        ));
    }
    #[cfg(not(windows))]
    let managed_service_bound = matches!(&launch, Launch::Managed(_));
    #[cfg(windows)]
    let managed_service_bound = false;
    let runtime = match launch {
        Launch::Development { process_workers } => from_environment(process_workers)?,
        Launch::DevelopmentConfig(path) => from_development_config(&path)?,
        Launch::Managed(path) => from_managed_config(&path)?,
        _ => {
            return Err(p::Error(
                "host administrative command cannot start a runtime".into(),
            ))
        }
    };
    run_host(
        foreground_shutdown()?,
        true,
        runtime,
        managed_service_bound,
        |_| {},
        || Ok(()),
        || Ok(()),
    )
}

#[cfg(unix)]
fn foreground_shutdown() -> p::Result<Option<Receiver<()>>> {
    use signal_hook::{
        consts::{SIGINT, SIGTERM},
        iterator::Signals,
    };

    let (sender, receiver) = std::sync::mpsc::channel();
    let mut signals = Signals::new([SIGINT, SIGTERM])
        .map_err(|_| p::Error("host shutdown signal registration failed".into()))?;
    std::thread::Builder::new()
        .name("forme-host-signal".into())
        .spawn(move || {
            if signals.forever().next().is_some() {
                let _ = sender.send(());
            }
        })
        .map_err(|_| p::Error("host shutdown signal worker could not start".into()))?;
    Ok(Some(receiver))
}

#[cfg(not(unix))]
fn foreground_shutdown() -> p::Result<Option<Receiver<()>>> {
    Ok(None)
}

fn run_host(
    shutdown: Option<Receiver<()>>,
    announce: bool,
    runtime: HostRuntimeConfig,
    managed_service_bound: bool,
    on_stage: impl Fn(startup::Stage),
    on_ready: impl FnOnce() -> p::Result<()>,
    on_stopping: impl FnOnce() -> p::Result<()>,
) -> p::Result<()> {
    on_stage(startup::Stage::SetupAuthority);
    let journal = runtime.journal_path.clone();
    let install_id = runtime.install_id.clone();
    let credentials = runtime.credentials.clone();
    let bootstrap = load_bootstrap_authority(
        &journal,
        &install_id,
        &credentials,
        runtime.bootstrap_config_path.as_deref(),
        &runtime.protected_paths,
    )?;
    let agent_digest = credentials
        .agent_execution()
        .map(|value| p::canonical_digest(&value))
        .transpose()?;
    on_stage(startup::Stage::ProductConfiguration);
    let product_config = runtime
        .product_config_template_path
        .as_deref()
        .map(|path| {
            ProductConfigStore::open_with_installed_file(path, runtime.installed_file.clone())
        })
        .transpose()?
        .map(Arc::new);
    let has_configuration = credentials.web_configuration().is_some() && product_config.is_some();
    on_stage(startup::Stage::HostJournal);
    let mut broker = ProductHostBroker::open_with_service_manager_and_config(
        &journal,
        &install_id,
        credentials,
        runtime.target,
        runtime.active_release,
        product_config,
        forme_execution::installed::SystemServiceManager,
    )?;
    if let Some(stager) = runtime.release_stager {
        broker = broker.with_release_stager(stager)?;
    }
    if let Some(worker) = runtime.restore_worker {
        broker = broker.with_restore_worker(worker)?;
    }
    if let Some(updater) = runtime.release_updater {
        broker = broker.with_release_updater(updater)?;
    }
    let broker = Arc::new(broker);
    if runtime.managed {
        on_stage(startup::Stage::InstallationVerification);
        broker.record_installation_verification(
            runtime
                .installation_manifest_digest
                .clone()
                .ok_or_else(|| p::Error("managed host manifest digest is unavailable".into()))?,
            runtime
                .release_digest
                .clone()
                .ok_or_else(|| p::Error("managed host release digest is unavailable".into()))?,
        )?;
    }
    let has_agent = runtime.has_agent_endpoint && agent_digest.is_some();
    let active_release = if runtime.managed {
        broker.verified_active_release()?
    } else {
        None
    };
    on_stage(startup::Stage::Worker);
    let worker = load_worker_host(
        &journal,
        agent_digest,
        runtime.worker_config_path.as_deref(),
        runtime.development_workers,
        WorkerHostContext {
            managed_service_bound,
            protected_paths: &runtime.worker_protected_paths,
            active_release: active_release.as_ref(),
        },
    )?;
    on_stage(startup::Stage::AgentEndpoint);
    #[cfg(windows)]
    let core_image_path = active_release
        .as_ref()
        .map(|release| release.content_root().join("bin/forme-gatewayd.exe"))
        .or_else(|| runtime.core_image_path.clone());
    #[cfg(windows)]
    let mut agent_pipe = if has_agent && runtime.managed {
        Some(WindowsAgentExecutionListener::bind(
            &runtime.install_id,
            core_image_path
                .as_deref()
                .ok_or_else(|| p::Error("managed Brain image binding is unavailable".into()))?,
        )?)
    } else {
        None
    };
    #[cfg(windows)]
    let agent_pipe_connections = Arc::new(AtomicUsize::new(0));
    on_stage(startup::Stage::ManagementEndpoints);
    let mut endpoints = vec![HostEndpoint::bind(
        runtime.host_bind,
        EndpointDomain::Maintenance,
    )?];
    #[cfg(not(windows))]
    if has_agent {
        endpoints.push(HostEndpoint::bind(
            runtime.agent_execution_bind,
            EndpointDomain::AgentExecution,
        )?);
    }
    #[cfg(windows)]
    if has_agent && !runtime.managed {
        endpoints.push(HostEndpoint::bind(
            runtime.agent_execution_bind,
            EndpointDomain::AgentExecution,
        )?);
    }
    if has_configuration {
        endpoints.push(HostEndpoint::bind(
            runtime.configuration_bind,
            EndpointDomain::Configuration,
        )?);
    }
    if has_configuration {
        // Configuration and AgentExecution listeners are ready before the
        // Host asks the native manager to restore the Brain service.
        broker.reconcile_core_startup()?;
    }
    if announce {
        for endpoint in &endpoints {
            let name = match endpoint.domain {
                EndpointDomain::Maintenance => "FORME_HOST_URL",
                EndpointDomain::AgentExecution => "FORME_AGENT_EXECUTION_URL",
                EndpointDomain::Configuration => "FORME_CONFIGURATION_URL",
            };
            println!(
                "{name}=http://{}",
                endpoint
                    .listener
                    .local_addr()
                    .map_err(|_| p::Error("host broker endpoint is unavailable".into()))?
            );
        }
        #[cfg(windows)]
        if let Some(pipe) = agent_pipe.as_ref() {
            println!("FORME_AGENT_EXECUTION_PIPE={}", pipe.name());
        }
        if let Some(path) = token_path().filter(|_| !runtime.managed) {
            println!("FORME_HOST_TOKEN_PATH={}", path.display());
        }
        let _ = std::io::stdout().flush();
    }
    on_stage(startup::Stage::Readiness);
    on_ready()?;
    on_stage(startup::Stage::Serving);
    let mut checked_release = std::time::Instant::now() - Duration::from_secs(1);
    loop {
        if let Some(receiver) = shutdown.as_ref() {
            match receiver.try_recv() {
                Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if checked_release.elapsed() >= Duration::from_secs(1) {
            broker.reconcile_release_checking()?;
            checked_release = std::time::Instant::now();
        }
        let mut accepted = false;
        for endpoint in &endpoints {
            match endpoint.listener.accept() {
                Ok((stream, _)) => {
                    accepted = true;
                    if endpoint.connections.fetch_add(1, Ordering::AcqRel) >= 16 {
                        endpoint.connections.fetch_sub(1, Ordering::AcqRel);
                        drop(stream);
                        continue;
                    }
                    let broker = broker.clone();
                    let bootstrap = bootstrap.clone();
                    let worker = worker.clone();
                    let connections = endpoint.connections.clone();
                    let domain = endpoint.domain;
                    std::thread::spawn(move || {
                        struct ConnectionGuard(Arc<AtomicUsize>);
                        impl Drop for ConnectionGuard {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Ordering::AcqRel);
                            }
                        }
                        let _guard = ConnectionGuard(connections);
                        let _ = handle_connection(stream, broker, worker, bootstrap, domain);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(p::Error("host broker accept failed".into())),
            }
        }
        #[cfg(windows)]
        if let Some(pipe) = agent_pipe.as_mut() {
            match pipe.try_accept() {
                Ok(Some(stream)) => {
                    accepted = true;
                    if agent_pipe_connections.fetch_add(1, Ordering::AcqRel) >= 16 {
                        agent_pipe_connections.fetch_sub(1, Ordering::AcqRel);
                        drop(stream);
                    } else {
                        let broker = broker.clone();
                        let bootstrap = bootstrap.clone();
                        let worker = worker.clone();
                        let connections = agent_pipe_connections.clone();
                        std::thread::spawn(move || {
                            struct ConnectionGuard(Arc<AtomicUsize>);
                            impl Drop for ConnectionGuard {
                                fn drop(&mut self) {
                                    self.0.fetch_sub(1, Ordering::AcqRel);
                                }
                            }
                            let _guard = ConnectionGuard(connections);
                            let _ = handle_connection(
                                stream,
                                broker,
                                worker,
                                bootstrap,
                                EndpointDomain::AgentExecution,
                            );
                        });
                    }
                }
                Ok(None) => {}
                Err(_) => return Err(p::Error("AgentExecution named pipe accept failed".into())),
            }
        }
        if !accepted {
            // A single supervisor owns both listeners and shutdown. Domain
            // connection budgets remain independent.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    // Stop admitting connections before draining operations that already own
    // journal/worker locks. A callback returning is not proof they finished.
    let mut connections: Vec<_> = endpoints.iter().map(|e| e.connections.clone()).collect();
    #[cfg(windows)]
    connections.push(agent_pipe_connections);
    drop(endpoints);
    on_stage(startup::Stage::Draining);
    on_stopping()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while connections.iter().any(|n| n.load(Ordering::Acquire) != 0) {
        if std::time::Instant::now() >= deadline {
            return Err(p::Error(
                "host shutdown could not verify all accepted operations".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum EndpointDomain {
    Maintenance,
    AgentExecution,
    Configuration,
}

struct HostEndpoint {
    listener: TcpListener,
    domain: EndpointDomain,
    connections: Arc<AtomicUsize>,
}

impl HostEndpoint {
    fn bind(address: SocketAddr, domain: EndpointDomain) -> p::Result<Self> {
        if !address.ip().is_loopback() {
            return Err(p::Error("host broker must remain loopback-only".into()));
        }
        let listener = TcpListener::bind(address)
            .map_err(|_| p::Error("host broker could not bind its private endpoint".into()))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| p::Error("host broker nonblocking mode is unavailable".into()))?;
        Ok(Self {
            listener,
            domain,
            connections: Arc::new(AtomicUsize::new(0)),
        })
    }
}

fn now_ms() -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
