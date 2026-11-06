#![forbid(unsafe_code)]
use super::*;
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

// These use the current-user namespace, not the installed service identities.
fn fixture_listener() -> (OwnedHandle, String) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = format!(
        r"\\.\pipe\forme-agent-execution-v1-io-fixture-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    (create_pipe_instance(&name, true, None).unwrap(), name)
}

fn fixture_server(owned: OwnedHandle) -> File {
    let deadline = Instant::now() + Duration::from_secs(2);
    while matches!(poll_connection(&owned).unwrap(), PipeConnection::Listening) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    File::from(owned)
}

fn pipe_pair() -> (File, File) {
    let (owned, name) = fixture_listener();
    let client = open_nonblocking_pipe(&name).unwrap();
    (fixture_server(owned), client)
}

struct IdentityChild(std::process::Child);
impl Drop for IdentityChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn identity_child() -> IdentityChild {
    use std::os::windows::process::CommandExt;
    IdentityChild(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "installed::windows_agent_execution_native::tests::identity_query_child_fixture",
                "--ignored",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap(),
    )
}

#[test]
#[ignore = "child-only fixture for kernel object ACL tests"]
fn identity_query_child_fixture() {
    let _ = std::io::stdin().read_exact(&mut [0]);
}

fn local_service_sid() -> SidBuffer {
    SidBuffer {
        bytes: vec![1, 1, 0, 0, 0, 0, 0, 5, 19, 0, 0, 0],
    }
}

fn acl_entries(bytes: &[u8]) -> Vec<Vec<u8>> {
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut offset = 8;
    (0..count)
        .map(|_| {
            let size = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
            let entry = bytes[offset..offset + size].to_vec();
            offset += size;
            entry
        })
        .collect()
}

fn assert_exact_grant_preserving_other_aces(object: &OwnedHandle, rights: u32) {
    let sid = local_service_sid();
    assert_eq!(object_rights(object, &sid).unwrap(), 0);
    let before = acl_entries(&fixture_acl_bytes(object).unwrap());
    grant_object_access(object, &sid, rights).unwrap();
    assert_eq!(object_rights(object, &sid).unwrap(), rights);
    let once = fixture_acl_bytes(object).unwrap();
    let after = acl_entries(&once);
    let added: Vec<_> = after
        .iter()
        .filter(|entry| !before.contains(entry))
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(&added[0][8..], sid.bytes);
    assert_eq!(
        after
            .iter()
            .filter(|entry| before.contains(entry))
            .cloned()
            .collect::<Vec<_>>(),
        before
    );
    grant_object_access(object, &sid, rights).unwrap();
    assert_eq!(fixture_acl_bytes(object).unwrap(), once);
}

#[test]
fn peer_process_query_acl_is_minimal_preserves_others_and_is_idempotent() {
    let child = identity_child();
    let process = fixture_process(child.0.id()).unwrap();
    assert_exact_grant_preserving_other_aces(&process, PROCESS_QUERY_LIMITED_INFORMATION);
}

#[test]
fn peer_token_query_acl_is_minimal_preserves_others_and_is_idempotent() {
    let child = identity_child();
    let process = fixture_process(child.0.id()).unwrap();
    let token = identity_query_token(&process).unwrap();
    assert_exact_grant_preserving_other_aces(&token, TOKEN_QUERY | TOKEN_DUPLICATE);
}

#[test]
fn non_brain_cannot_prepare_identity_query_or_mutate_own_acl() {
    let process = current_process().unwrap();
    let before = fixture_acl_bytes(&process).unwrap();
    assert!(prepare_brain_identity_query().is_err());
    assert_eq!(fixture_acl_bytes(&process).unwrap(), before);
}

#[test]
fn pipe_name_is_install_bound_and_closed() {
    let first = windows_agent_execution_pipe_name("managed-process-test").unwrap();
    let second = windows_agent_execution_pipe_name("another-installation").unwrap();
    assert_ne!(first, second);
    assert!(first.starts_with(r"\\.\pipe\forme-agent-execution-v1-"));
    assert!(windows_agent_execution_pipe_name("not a valid install").is_err());
}

#[test]
fn occupied_host_pipe_rejects_before_any_credential_or_job_byte() {
    let (owned, name) = fixture_listener();
    let reader = std::thread::spawn(move || {
        let server = fixture_server(owned);
        let mut server =
            super::super::worker_pipe::DeadlinePipe::new(server, Duration::from_secs(2));
        assert_eq!(server.read(&mut [0; 1]).unwrap(), 0);
    });
    let temporary = tempfile::tempdir().unwrap();
    let credential = temporary.path().join("agent.token");
    fs::write(&credential, "fixture-agent-credential-never-sent").unwrap();
    let client = super::super::WorkerBrokerClient::connect_named_pipe(&name, &credential).unwrap();
    use super::super::WorkerBrokerTransport;
    assert!(client.probe("fixture-original-job").is_err());
    reader.join().unwrap();
}

#[test]
fn idle_native_pipe_client_expires_without_a_blocked_io_thread() {
    let (_server, client) = pipe_pair();
    let mut client =
        super::super::worker_pipe::DeadlinePipe::new(client, Duration::from_millis(25));
    let started = Instant::now();
    assert_eq!(
        client.read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn native_pipe_client_reads_delayed_bytes_and_observes_disconnect() {
    let (mut server, client) = pipe_pair();
    let (acknowledge, consumed) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(15));
        server.write_all(b"bounded fixture result").unwrap();
        consumed.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let mut client = super::super::worker_pipe::DeadlinePipe::new(client, Duration::from_secs(2));
    let mut bytes = vec![0; b"bounded fixture result".len()];
    client.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, b"bounded fixture result");
    acknowledge.send(()).unwrap();
    writer.join().unwrap();
    assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
}

#[test]
fn next_pipe_instance_coexists_with_inflight_peer_and_keeps_namespace_owned() {
    let (first, name) = fixture_listener();
    let mut first_client = open_nonblocking_pipe(&name).unwrap();
    let mut first_server = fixture_server(first);
    assert!(create_pipe_instance(&name, true, None).is_err());
    let next = create_pipe_instance(&name, false, None).unwrap();
    let mut next_client = open_nonblocking_pipe(&name).unwrap();
    let mut next_server = fixture_server(next);
    let pending = create_pipe_instance(&name, false, None).unwrap();
    first_client.write_all(b"a").unwrap();
    next_client.write_all(b"b").unwrap();
    let mut byte = [0];
    first_server.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"a");
    next_server.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"b");
    drop((first_client, first_server, next_client, next_server));
    assert!(create_pipe_instance(&name, true, None).is_err());
    drop(pending);
    assert!(create_pipe_instance(&name, true, None).is_ok());
}

#[test]
fn disconnected_unverified_peer_does_not_end_listener_namespace() {
    let (first, name) = fixture_listener();
    let client = open_nonblocking_pipe(&name).unwrap();
    drop(client);
    let mut listener = WindowsAgentExecutionListener::fixture(
        name.clone(),
        std::env::current_exe().unwrap(),
        first,
    );
    assert!(listener.try_accept().unwrap().is_none());
    assert!(listener.has_pending());
    assert!(create_pipe_instance(&name, true, None).is_err());
}
