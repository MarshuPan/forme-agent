//! Separate v2 pipe exchange. The v1 helper mode and messages stay unchanged.
#![cfg(windows)]
use super::desktop_output_sink::DesktopOutputSink;
use super::windows_desktop_helper_peer_native::{
    connect_output_server, create_pipe, desktop_pipe_name, now_ms, open_output_pipe,
    prepare_output_peer, read_frame, read_output_bytes, verify_interactive_desktop,
    write_output_bytes, write_output_frame,
};
use super::windows_desktop_session_native::{
    WindowsDesktopSessionHost, WindowsDesktopSessionSnapshot,
};
use crate::{
    ArtifactRecord, ArtifactStore, ComputerDriver, NativeComputerDriver, RejectingContentResolver,
    RejectingSecretResolver,
};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::os::windows::io::OwnedHandle;
use std::{
    fs::File,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum HostMessage {
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
        transfer: Box<p::DesktopOutputTransfer>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum HelperMessage {
    Hello { schema_version: p::SchemaVersion },
    Pong { schema_version: p::SchemaVersion },
    Header { header: p::DesktopOutputHeader },
    Chunk { header: p::DesktopOutputChunkHeader },
    Finish { finish: p::DesktopOutputFinish },
    Rejected { schema_version: p::SchemaVersion },
}

pub(crate) struct WindowsDesktopOutputPeer {
    stream: File,
    _process: OwnedHandle,
    _job: OwnedHandle,
    grant: String,
}
impl WindowsDesktopOutputPeer {
    pub(crate) fn connect(
        host: &WindowsDesktopSessionHost,
        binding: &p::DesktopSessionBinding,
        snapshot: &WindowsDesktopSessionSnapshot,
    ) -> p::Result<Self> {
        let pipe_name = desktop_pipe_name(binding)?;
        let (server, _security) = create_pipe(&pipe_name, &snapshot.user_sid)?;
        let spawned = host.spawn_output_helper(snapshot, &pipe_name)?;
        let deadline = output_deadline(binding.expires_at)?.min(Instant::now() + HELLO_TIMEOUT);
        let mut stream =
            connect_output_server(server, spawned.pid, &spawned, snapshot, host, deadline)?;
        let hello: HelperMessage = read_frame(&mut stream, deadline)?;
        if !matches!(hello,HelperMessage::Hello {schema_version} if schema_version==p::DESKTOP_OUTPUT_SCHEMA_VERSION)
        {
            return Err(invalid());
        }
        write_output_frame(
            &mut stream,
            &HostMessage::Ready {
                schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            },
            deadline,
        )?;
        Ok(Self {
            stream,
            _process: spawned.process,
            _job: spawned.job,
            grant: binding.grant_ref.clone(),
        })
    }
    pub(crate) fn binding_key(&self) -> &str {
        &self.grant
    }
    pub(crate) fn ping(&mut self, deadline: Instant) -> p::Result<()> {
        write_output_frame(
            &mut self.stream,
            &HostMessage::Ping {
                schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            },
            deadline,
        )?;
        match read_frame(&mut self.stream, deadline)? {
            HelperMessage::Pong { schema_version }
                if schema_version == p::DESKTOP_OUTPUT_SCHEMA_VERSION =>
            {
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
    pub(crate) fn execute(
        mut self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
        sink: &mut dyn DesktopOutputSink,
    ) -> p::Result<p::DesktopActionReceipt> {
        sink.transfer().binding.validate_request(request)?;
        sink.transfer().validate_intent(intent)?;
        let deadline = output_deadline(request.deadline)?;
        write_output_frame(
            &mut self.stream,
            &HostMessage::Execute {
                schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
                request: Box::new(request.clone()),
                intent: Box::new(intent.clone()),
                transfer: Box::new(sink.transfer().clone()),
            },
            deadline,
        )?;
        let HelperMessage::Header { header } = read_frame(&mut self.stream, deadline)? else {
            return Err(invalid());
        };
        header.validate_for(sink.transfer())?;
        sink.begin(header)?;
        for _ in 0..p::DESKTOP_OUTPUT_MAX_CHUNKS {
            let HelperMessage::Chunk { header } = read_frame(&mut self.stream, deadline)? else {
                return Err(invalid());
            };
            let bytes = read_output_bytes(&mut self.stream, header.bytes, deadline)?;
            let finished = header.finished;
            sink.write_chunk(header, &bytes)?;
            if finished {
                let HelperMessage::Finish { finish } = read_frame(&mut self.stream, deadline)?
                else {
                    return Err(invalid());
                };
                let receipt = finish.receipt.clone();
                sink.finish(finish)?;
                return Ok(receipt);
            }
        }
        Err(invalid())
    }
}

pub fn run_desktop_output_helper() -> p::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || args[0] != "--desktop-session-helper-output-v2" || args[1] != "--pipe" {
        return Err(invalid());
    }
    if !args[2].starts_with(r"\\.\pipe\forme-desktop-helper-v1-")
        || args[2].len() > 256
        || args[2].chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let deadline = Instant::now() + HELLO_TIMEOUT;
    let mut stream = open_output_pipe(&args[2], deadline)?;
    prepare_output_peer(&stream)?;
    verify_interactive_desktop()?;
    write_output_frame(
        &mut stream,
        &HelperMessage::Hello {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        },
        deadline,
    )?;
    let ready: HostMessage = read_frame(&mut stream, deadline)?;
    if !matches!(ready,HostMessage::Ready {schema_version} if schema_version==p::DESKTOP_OUTPUT_SCHEMA_VERSION)
    {
        return Err(invalid());
    }
    loop {
        let deadline = Instant::now() + HELLO_TIMEOUT;
        match read_frame(&mut stream, deadline)? {
            HostMessage::Ping { schema_version }
                if schema_version == p::DESKTOP_OUTPUT_SCHEMA_VERSION =>
            {
                verify_interactive_desktop()?;
                write_output_frame(
                    &mut stream,
                    &HelperMessage::Pong { schema_version },
                    deadline,
                )?;
            }
            HostMessage::Execute {
                schema_version,
                request,
                intent,
                transfer,
            } if schema_version == p::DESKTOP_OUTPUT_SCHEMA_VERSION => {
                // Consume exactly one action. A failure never re-enters the loop.
                return capture_and_send(&mut stream, &request, &intent, *transfer);
            }
            _ => return Err(invalid()),
        }
    }
}
fn capture_and_send(
    stream: &mut File,
    request: &p::DesktopActionRequest,
    intent: &p::ActionIntent,
    transfer: p::DesktopOutputTransfer,
) -> p::Result<()> {
    let deadline = output_deadline(request.deadline)?;
    transfer.binding.validate_request(request)?;
    transfer.validate_intent(intent)?;
    request.binding.validate_at(now_ms()?)?;
    if now_ms()? >= request.deadline {
        return Err(invalid());
    }
    verify_interactive_desktop()?;
    let p::ActionParameters::Computer(spec) = &intent.parameters else {
        return Err(invalid());
    };
    let capture = Arc::new(Capture {
        transfer: transfer.clone(),
        scope: spec.artifact_scope.clone(),
        bytes: Mutex::new(None),
    });
    let driver = NativeComputerDriver::new(Arc::new(RejectingContentResolver), capture.clone())?;
    let receipt = driver.perform(spec, &RejectingSecretResolver)?;
    if receipt.effect != p::EffectStatus::Observed
        || receipt.content_ref.as_ref().map(|r| r.0.as_str())
            != Some(transfer.transfer_ref.as_str())
    {
        return Err(invalid());
    }
    let bytes = capture
        .bytes
        .lock()
        .map_err(|_| invalid())?
        .take()
        .ok_or_else(invalid)?;
    let observed_at = now_ms()?;
    verify_interactive_desktop()?;
    if observed_at > request.deadline {
        return Err(invalid());
    }
    let header = p::DesktopOutputHeader {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding: transfer.binding.clone(),
        transfer_ref: transfer.transfer_ref.clone(),
        media_type: "image/png".into(),
        bytes: bytes.len() as u64,
        content_digest: p::sha256_content_digest(&bytes),
        width: transfer.width,
        height: transfer.height,
    };
    header.validate_for(&transfer)?;
    write_output_frame(
        stream,
        &HelperMessage::Header {
            header: header.clone(),
        },
        deadline,
    )?;
    let mut offset = 0;
    for chunk in bytes.chunks(p::DESKTOP_OUTPUT_CHUNK_BYTES as usize) {
        let next_offset = offset + chunk.len() as u64;
        let frame = p::DesktopOutputChunkHeader {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            binding_digest: transfer.binding.digest()?,
            transfer_ref: transfer.transfer_ref.clone(),
            offset,
            bytes: chunk.len() as u32,
            chunk_digest: p::sha256_content_digest(chunk),
            next_offset,
            finished: next_offset == header.bytes,
        };
        write_output_frame(stream, &HelperMessage::Chunk { header: frame }, deadline)?;
        write_output_bytes(stream, chunk, deadline)?;
        offset = next_offset;
    }
    let receipt = p::DesktopActionReceipt {
        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
        job_id: request.job_id.clone(),
        permit_ref: request.permit_ref.clone(),
        action_ref: request.binding.action_ref.clone(),
        plan_digest: request.binding.plan_digest.clone(),
        nonce: request.binding.nonce.clone(),
        normalized_action_digest: request.normalized_action_digest.clone(),
        outcome: p::DesktopActionOutcome::Committed,
        result_ref: Some(transfer.transfer_ref.clone()),
        observed_at,
    };
    let finish = p::DesktopOutputFinish {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding_digest: transfer.binding.digest()?,
        transfer_ref: transfer.transfer_ref,
        header_digest: header.digest()?,
        bytes: header.bytes,
        content_digest: header.content_digest.clone(),
        receipt,
    };
    finish.validate_for(&header, request)?;
    if now_ms()? > request.deadline {
        return Err(invalid());
    }
    write_output_frame(stream, &HelperMessage::Finish { finish }, deadline)
}

// The driver's output is retained only inside this one explicit call. There is
// no process-global result cache, filesystem path or second capture operation.
struct Capture {
    transfer: p::DesktopOutputTransfer,
    scope: p::Scope,
    bytes: Mutex<Option<Vec<u8>>>,
}
impl ArtifactStore for Capture {
    fn write(&self, scope: &p::Scope, extension: &str, bytes: &[u8]) -> p::Result<ArtifactRecord> {
        if scope != &self.scope
            || extension != "png"
            || bytes.is_empty()
            || bytes.len() as u64 > self.transfer.output_budget_bytes
        {
            return Err(invalid());
        }
        let mut slot = self.bytes.lock().map_err(|_| invalid())?;
        if slot.is_some() {
            return Err(invalid());
        }
        *slot = Some(bytes.to_vec());
        Ok(ArtifactRecord {
            schema_version: p::SchemaVersion(1),
            content_ref: p::ContentRef(self.transfer.transfer_ref.clone()),
            digest: p::sha256_content_digest(bytes),
            bytes: bytes.len() as u64,
        })
    }
}
fn invalid() -> p::Error {
    p::Error("desktop output peer exchange failed".into())
}

fn output_deadline(at: p::Timestamp) -> p::Result<Instant> {
    let remaining = at
        .checked_sub(now_ms()?)
        .filter(|value| *value > 0 && *value <= 600_000)
        .ok_or_else(invalid)?;
    Instant::now()
        .checked_add(Duration::from_millis(remaining as u64))
        .ok_or_else(invalid)
}
