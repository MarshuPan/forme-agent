//! Host process composition and bounded listener lifecycle.
pub(super) mod config;
pub(super) mod http;
#[cfg(windows)]
mod service;

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use config::*;
use forme_execution::installed::{HostCredentials, ProductHostBroker};
use forme_protocol as p;
use http::handle_connection;

pub(super) fn run() -> p::Result<()> {
    #[cfg(windows)]
    {
        match service::dispatch() {
            Ok(()) => return Ok(()),
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1063) => {}
            Err(error) => {
                return Err(p::Error(format!(
                    "Windows service dispatcher failed: {error}"
                )))
            }
        }
    }
    run_host(None, true, false, || Ok(()))
}

fn run_host(
    shutdown: Option<Receiver<()>>,
    announce: bool,
    managed_credentials: bool,
    on_ready: impl FnOnce() -> p::Result<()>,
) -> p::Result<()> {
    let development_workers =
        development_process_workers(std::env::args().skip(1), managed_credentials)?;
    let journal = required_path("FORME_HOST_JOURNAL_PATH")?;
    let install_id = required("FORME_INSTALL_ID")?;
    let credentials = if managed_credentials {
        load_managed_credentials()?
    } else {
        let maintenance_token = load_or_create_token()?
            .ok_or_else(|| p::Error("host management token is unavailable".into()))?;
        HostCredentials::new(
            maintenance_token,
            std::env::var("FORME_AGENT_EXECUTION_TOKEN").ok(),
        )?
    };
    let target = target_from_environment()?;
    let active_release =
        std::env::var("FORME_ACTIVE_RELEASE").unwrap_or_else(|_| "unconfigured".into());
    let agent_digest = credentials
        .agent_execution()
        .map(|value| p::canonical_digest(&value))
        .transpose()?;
    let broker = Arc::new(ProductHostBroker::open_with_service_manager(
        &journal,
        &install_id,
        credentials,
        target,
        active_release,
        forme_execution::installed::SystemServiceManager,
    )?);
    let has_agent = agent_digest.is_some();
    let worker = load_worker_host(&journal, agent_digest, development_workers)?;
    let mut endpoints = vec![HostEndpoint::bind(
        "FORME_HOST_BIND",
        "127.0.0.1:8766",
        EndpointDomain::Maintenance,
    )?];
    if has_agent {
        endpoints.push(HostEndpoint::bind(
            "FORME_AGENT_EXECUTION_BIND",
            "127.0.0.1:8767",
            EndpointDomain::AgentExecution,
        )?);
    }
    if announce {
        for endpoint in &endpoints {
            let name = match endpoint.domain {
                EndpointDomain::Maintenance => "FORME_HOST_URL",
                EndpointDomain::AgentExecution => "FORME_AGENT_EXECUTION_URL",
            };
            println!(
                "{name}=http://{}",
                endpoint
                    .listener
                    .local_addr()
                    .map_err(|_| p::Error("host broker endpoint is unavailable".into()))?
            );
        }
        if let Some(path) = token_path() {
            println!("FORME_HOST_TOKEN_PATH={}", path.display());
        }
        let _ = std::io::stdout().flush();
    }
    on_ready()?;
    loop {
        if let Some(receiver) = shutdown.as_ref() {
            match receiver.try_recv() {
                Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
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
                        let _ = handle_connection(stream, broker, worker, domain);
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
    Ok(())
}

#[derive(Clone, Copy)]
enum EndpointDomain {
    Maintenance,
    AgentExecution,
}

struct HostEndpoint {
    listener: TcpListener,
    domain: EndpointDomain,
    connections: Arc<AtomicUsize>,
}

impl HostEndpoint {
    fn bind(key: &str, default: &str, domain: EndpointDomain) -> p::Result<Self> {
        let address: SocketAddr = std::env::var(key)
            .unwrap_or_else(|_| default.into())
            .parse()
            .map_err(|_| p::Error("host broker requires a numeric loopback address".into()))?;
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
