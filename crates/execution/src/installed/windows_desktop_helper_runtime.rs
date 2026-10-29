#![cfg(windows)]

use std::fs::File;
use std::sync::Arc;
use std::time::{Duration, Instant};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use crate::{
    computer::computer_operation_within_bounds, ArtifactRecord, ArtifactStore, ComputerDriver,
    ContentResolver, NativeComputerDriver, RejectingContentResolver, RejectingSecretResolver,
    SecretResolver,
};

use super::windows_desktop_helper_peer_native::{
    connect_server, create_pipe, desktop_pipe_name, helper_pipe_argument, now_ms, open_helper_pipe,
    read_frame, timestamp_deadline, verify_interactive_desktop, write_frame,
};
use super::windows_desktop_session_native::{
    WindowsDesktopSessionHost, WindowsDesktopSessionSnapshot,
};
use std::os::windows::io::OwnedHandle;

const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(crate) enum HostMessage {
    Ready {
        schema_version: p::SchemaVersion,
    },
    Ping {
        schema_version: p::SchemaVersion,
    },
    Execute {
        schema_version: p::SchemaVersion,
        request: Box<p::DesktopActionRequest>,
        intent: Box<p::ActionIntent>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(crate) enum HelperMessage {
    Hello {
        schema_version: p::SchemaVersion,
    },
    Pong {
        schema_version: p::SchemaVersion,
    },
    Receipt {
        schema_version: p::SchemaVersion,
        receipt: p::DesktopActionReceipt,
    },
    Rejected {
        schema_version: p::SchemaVersion,
        reason: String,
    },
}

pub(crate) struct WindowsDesktopHelperPeer {
    stream: File,
    _process: OwnedHandle,
    _job: OwnedHandle,
    binding_key: String,
}

impl WindowsDesktopHelperPeer {
    pub(crate) fn connect(
        host: &WindowsDesktopSessionHost,
        binding: &p::DesktopSessionBinding,
        snapshot: &WindowsDesktopSessionSnapshot,
    ) -> p::Result<Self> {
        let pipe_name = desktop_pipe_name(binding)?;
        let (server, _security) = create_pipe(&pipe_name, &snapshot.user_sid)?;
        let spawned = host.spawn_helper(snapshot, &pipe_name)?;
        let mut stream = connect_server(server, spawned.pid, &spawned, snapshot, host)?;
        let hello: HelperMessage = read_frame(&mut stream, Instant::now() + HELLO_TIMEOUT)?;
        if !matches!(hello, HelperMessage::Hello { schema_version } if schema_version == p::DESKTOP_SESSION_SCHEMA_VERSION)
        {
            return Err(p::Error("desktop helper hello is invalid".into()));
        }
        write_frame(
            &mut stream,
            &HostMessage::Ready {
                schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            },
        )?;
        Ok(Self {
            stream,
            _process: spawned.process,
            _job: spawned.job,
            binding_key: binding.grant_ref.clone(),
        })
    }

    pub(crate) fn binding_key(&self) -> &str {
        &self.binding_key
    }

    pub(crate) fn ping(&mut self, deadline: Instant) -> p::Result<()> {
        write_frame(
            &mut self.stream,
            &HostMessage::Ping {
                schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            },
        )?;
        match read_frame(&mut self.stream, deadline)? {
            HelperMessage::Pong { schema_version }
                if schema_version == p::DESKTOP_SESSION_SCHEMA_VERSION =>
            {
                Ok(())
            }
            _ => Err(p::Error(
                "desktop helper peer did not answer the ping".into(),
            )),
        }
    }

    pub(crate) fn execute(
        mut self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        write_frame(
            &mut self.stream,
            &HostMessage::Execute {
                schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
                request: Box::new(request.clone()),
                intent: Box::new(intent.clone()),
            },
        )?;
        match read_frame(&mut self.stream, timestamp_deadline(request.deadline))? {
            HelperMessage::Receipt {
                schema_version,
                receipt,
            } if schema_version == p::DESKTOP_SESSION_SCHEMA_VERSION => Ok(receipt),
            HelperMessage::Rejected { reason, .. } => Err(p::Error(reason)),
            _ => Err(p::Error(
                "desktop helper returned an invalid response".into(),
            )),
        }
    }
}

/// Entry point for the fixed `forme-executord --desktop-session-helper` mode.
/// It has no brain, policy, key or installation-management access.
pub fn run_desktop_session_helper() -> p::Result<()> {
    let pipe = helper_pipe_argument()?;
    let mut stream = open_helper_pipe(&pipe)?;
    verify_interactive_desktop()?;
    write_frame(
        &mut stream,
        &HelperMessage::Hello {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
        },
    )?;
    let ready: HostMessage = read_frame(&mut stream, Instant::now() + HELLO_TIMEOUT)?;
    if !matches!(ready, HostMessage::Ready { schema_version } if schema_version == p::DESKTOP_SESSION_SCHEMA_VERSION)
    {
        return Err(p::Error("desktop helper host handshake is invalid".into()));
    }
    let contents: Arc<dyn ContentResolver> = Arc::new(RejectingContentResolver);
    let artifacts: Arc<dyn ArtifactStore> = Arc::new(RejectingArtifactStore);
    let driver = NativeComputerDriver::new(contents, artifacts)?;
    let secrets = RejectingSecretResolver;
    loop {
        let message: HostMessage = read_frame(&mut stream, Instant::now() + HELLO_TIMEOUT)?;
        match message {
            HostMessage::Ping { schema_version }
                if schema_version == p::DESKTOP_SESSION_SCHEMA_VERSION =>
            {
                write_frame(
                    &mut stream,
                    &HelperMessage::Pong {
                        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
                    },
                )?;
            }
            HostMessage::Execute {
                schema_version,
                request,
                intent,
            } if schema_version == p::DESKTOP_SESSION_SCHEMA_VERSION => {
                let response = match execute_action(&driver, &secrets, &request, &intent) {
                    Ok(receipt) => HelperMessage::Receipt {
                        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
                        receipt,
                    },
                    Err(error) => HelperMessage::Rejected {
                        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
                        reason: error.0,
                    },
                };
                write_frame(&mut stream, &response)?;
            }
            _ => {
                return Err(p::Error(
                    "desktop helper received an invalid command".into(),
                ))
            }
        }
    }
}

fn execute_action(
    driver: &NativeComputerDriver,
    secrets: &dyn SecretResolver,
    request: &p::DesktopActionRequest,
    intent: &p::ActionIntent,
) -> p::Result<p::DesktopActionReceipt> {
    let now = now_ms()?;
    request.binding.validate_at(now)?;
    request.validate_at(
        &p::DesktopSessionObservation {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: request.binding.helper_ref.clone(),
            grant_ref: Some(request.binding.grant_ref.clone()),
            state: p::DesktopSessionState::Ready,
            liveness: p::DesktopSessionLiveness::Live,
            os_user_ref: Some(request.binding.os_user_ref.clone()),
            os_session_ref: Some(request.binding.os_session_ref.clone()),
            session_generation: Some(request.binding.session_generation),
            platform: request.binding.platform,
            desktop_ref: Some(request.binding.desktop_ref.clone()),
            peer_ref: Some("peer:desktop-helper".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: now,
        },
        now,
    )?;
    if p::normalized_action_digest(intent)? != request.normalized_action_digest
        || intent.intent_id != request.binding.action_ref
        || intent.backend_hint != p::BackendKind::Computer
    {
        return Err(p::Error("desktop helper action digest is not bound".into()));
    }
    let p::ActionParameters::Computer(spec) = &intent.parameters else {
        return Err(p::Error(
            "desktop helper received a non-computer action".into(),
        ));
    };
    spec.bounds.validate()?;
    if !computer_operation_within_bounds(spec)
        || matches!(spec.operation, p::ComputerOperation::Screenshot)
    {
        return Err(p::Error(
            "desktop helper action is outside the implemented result boundary".into(),
        ));
    }
    if matches!(
        spec.operation,
        p::ComputerOperation::Type {
            input: p::ExternalInput::Content(_) | p::ExternalInput::Secret(_),
        }
    ) {
        return Err(p::Error(
            "desktop helper does not resolve content or secret input".into(),
        ));
    }
    let receipt = driver.perform(spec, secrets)?;
    receipt.validate()?;
    if receipt.effect != p::EffectStatus::Committed {
        return Err(p::Error(
            "desktop helper action has no committed effect".into(),
        ));
    }
    let observed_at = now_ms()?;
    if observed_at > request.deadline {
        return Err(p::Error("desktop helper action deadline expired".into()));
    }
    Ok(p::DesktopActionReceipt {
        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
        job_id: request.job_id.clone(),
        permit_ref: request.permit_ref.clone(),
        action_ref: request.binding.action_ref.clone(),
        plan_digest: request.binding.plan_digest.clone(),
        nonce: request.binding.nonce.clone(),
        normalized_action_digest: request.normalized_action_digest.clone(),
        outcome: p::DesktopActionOutcome::Committed,
        result_ref: receipt.content_ref.map(|value| value.0),
        observed_at,
    })
}

struct RejectingArtifactStore;

impl ArtifactStore for RejectingArtifactStore {
    fn write(
        &self,
        _scope: &p::Scope,
        _extension: &str,
        _bytes: &[u8],
    ) -> p::Result<ArtifactRecord> {
        Err(p::Error(
            "desktop helper artifact output is unavailable".into(),
        ))
    }
}
