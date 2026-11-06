#![forbid(unsafe_code)]
use super::*;
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

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
    let mut listener = WindowsAgentExecutionListener {
        name: name.clone(),
        expected_image: std::env::current_exe().unwrap(),
        pending: Some(first),
    };
    assert!(listener.try_accept().unwrap().is_none());
    assert!(listener.pending.is_some());
    assert!(create_pipe_instance(&name, true, None).is_err());
}
