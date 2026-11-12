//! Explicit Windows process evidence for the finite channel worker.
//!
//! This test is ignored by default because it launches the installed worker
//! image supplied through `FORME_CHANNEL_WORKER_EXE`. It is a component check,
//! not a substitute for the full SCM/AppContainer installed acceptance.
use forme_protocol as p;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn worker_exe() -> std::path::PathBuf {
    std::env::var_os("FORME_CHANNEL_WORKER_EXE")
        .map(std::path::PathBuf::from)
        .expect("FORME_CHANNEL_WORKER_EXE must point at the built forme-file-worker.exe")
}

fn request() -> p::ManagedChannelWorkerRequestV1 {
    p::ManagedChannelWorkerRequestV1 {
        schema_version: p::SchemaVersion(1),
        grant_ref: "channel-delivery-grant:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
    }
}

#[test]
#[ignore = "explicit native worker image; set FORME_CHANNEL_WORKER_EXE"]
fn native_channel_worker_echoes_only_the_bound_grant_and_rejects_invalid_input() {
    let exe = worker_exe();
    let mut child = Command::new(&exe)
        .arg("--installed-channel-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("worker image should start");
    let body = serde_json::to_vec(&request()).unwrap();
    child.stdin.take().unwrap().write_all(&body).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let echoed: p::ManagedChannelWorkerRequestV1 = serde_json::from_slice(&output.stdout).unwrap();
    assert!(echoed == request());

    let mut child = Command::new(&exe)
        .arg("--installed-channel-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("worker image should start");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"grant_ref\":\"forged\"}")
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let status = child.wait().unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    stdout.read_to_end(&mut out).unwrap();
    stderr.read_to_end(&mut err).unwrap();
    assert!(!status.success());
    assert!(out.is_empty());
    assert!(!err.is_empty());
}
