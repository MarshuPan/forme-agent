//! Response ordering remains independent from maintenance dispatch semantics.
use super::*;
use std::net::{Shutdown, TcpListener};
use std::sync::mpsc;

#[test]
fn response_eof_releases_the_web_client_before_service_dispatch_finishes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (released, receiver) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        respond_before_dispatch(&mut stream, b"{\"stage\":\"planned\"}", || {
            // A Web stop drains its current call only after response EOF.
            receiver.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(())
        })
        .unwrap();
    });
    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("{\"stage\":\"planned\"}"));
    released.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn failed_response_never_dispatches_the_side_effect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    let mut dispatched = false;
    assert!(respond_before_dispatch(&mut stream, b"{}", || {
        dispatched = true;
        Ok(())
    })
    .is_err());
    assert!(!dispatched);
}
