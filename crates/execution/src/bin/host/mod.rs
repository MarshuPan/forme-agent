//! Host process composition and bounded listener lifecycle.
mod bootstrap;
pub(super) mod config;
pub(super) mod http;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod updater_service;

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use config::*;
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
    let runtime = match launch {
        Launch::Development { process_workers } => from_environment(process_workers)?,
        Launch::Managed(path) => from_managed_config(&path)?,
        _ => {
            return Err(p::Error(
                "host administrative command cannot start a runtime".into(),
            ))
        }
    };
    run_host(foreground_shutdown()?, true, runtime, || Ok(()), || Ok(()))
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
    on_ready: impl FnOnce() -> p::Result<()>,
    on_stopping: impl FnOnce() -> p::Result<()>,
) -> p::Result<()> {
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
    let product_config = runtime
        .product_config_template_path
        .as_deref()
        .map(ProductConfigStore::open)
        .transpose()?
        .map(Arc::new);
    let has_configuration = credentials.web_configuration().is_some() && product_config.is_some();
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
    let worker = load_worker_host(
        &journal,
        agent_digest,
        runtime.worker_config_path.as_deref(),
        runtime.development_workers,
        &runtime.worker_protected_paths,
    )?;
    let mut endpoints = vec![HostEndpoint::bind(
        runtime.host_bind,
        EndpointDomain::Maintenance,
    )?];
    if has_agent {
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
        if let Some(path) = token_path().filter(|_| !runtime.managed) {
            println!("FORME_HOST_TOKEN_PATH={}", path.display());
        }
        let _ = std::io::stdout().flush();
    }
    on_ready()?;
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
        if !accepted {
            // A single supervisor owns both listeners and shutdown. Domain
            // connection budgets remain independent.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    // Stop admitting connections before draining operations that already own
    // journal/worker locks. A callback returning is not proof they finished.
    let connections: Vec<_> = endpoints.iter().map(|e| e.connections.clone()).collect();
    drop(endpoints);
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
