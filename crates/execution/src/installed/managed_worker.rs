//! Protected core/service configuration for the AgentExecution domain.

use std::{collections::BTreeSet, net::SocketAddr, path::PathBuf, sync::Arc};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{WorkerBrokerClient, WorkerBrokerTransport};

/// A model, Web request or ActionIntent cannot choose the installed endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManagedWorkerEndpoint {
    LoopbackTcp { address: SocketAddr },
    WindowsNamedPipe { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedWorkerBinding {
    pub schema_version: p::SchemaVersion,
    pub endpoint: ManagedWorkerEndpoint,
    pub credential_path: PathBuf,
    pub authority_epoch: p::AuthorityEpoch,
    pub sandbox_profile_ref: String,
    /// No capability is executable until explicitly provisioned by the Host.
    #[serde(default)]
    pub backend_kinds: BTreeSet<p::BackendKind>,
}

impl ManagedWorkerBinding {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.authority_epoch.0 == 0
            || !self.credential_path.is_absolute()
        {
            return Err(p::Error("managed worker binding is incomplete".into()));
        }
        super::reject_linked_ancestors(&self.credential_path)?;
        p::validate_product_id(&self.sandbox_profile_ref)
            .map_err(|_| p::Error("managed worker sandbox profile is invalid".into()))?;
        match &self.endpoint {
            ManagedWorkerEndpoint::LoopbackTcp { address } => {
                if address.port() == 0 || !address.ip().is_loopback() {
                    return Err(p::Error(
                        "managed worker endpoint must be loopback with a port".into(),
                    ));
                }
            }
            ManagedWorkerEndpoint::WindowsNamedPipe { name } => {
                if !name.starts_with(r"\\.\pipe\forme-agent-execution-v1-")
                    || name.len() > 256
                    || name.chars().any(char::is_control)
                {
                    return Err(p::Error("managed worker named pipe is invalid".into()));
                }
            }
        }
        Ok(())
    }

    pub fn supports(&self, kind: p::BackendKind) -> bool {
        self.backend_kinds.contains(&kind)
    }

    /// Windows production cannot fall back to bearer-only TCP.
    pub fn connect(&self) -> p::Result<Arc<dyn WorkerBrokerTransport + Send + Sync>> {
        self.validate()?;
        match &self.endpoint {
            ManagedWorkerEndpoint::LoopbackTcp { address } => {
                #[cfg(windows)]
                {
                    let _ = address;
                    Err(p::Error(
                        "managed Windows worker requires the installed named pipe".into(),
                    ))
                }
                #[cfg(not(windows))]
                {
                    Ok(Arc::new(WorkerBrokerClient::connect(
                        *address,
                        &self.credential_path,
                    )?))
                }
            }
            ManagedWorkerEndpoint::WindowsNamedPipe { name } => {
                #[cfg(windows)]
                {
                    Ok(Arc::new(WorkerBrokerClient::connect_named_pipe(
                        name,
                        &self.credential_path,
                    )?))
                }
                #[cfg(not(windows))]
                {
                    let _ = name;
                    Err(p::Error(
                        "Windows worker named pipe is unavailable on this platform".into(),
                    ))
                }
            }
        }
    }
}
