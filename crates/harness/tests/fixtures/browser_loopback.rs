//! Original loopback HTTP fixture shared by the S38 and S68 real-browser cases.
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_HEADERS: usize = 8192;
const MAX_CONNECTIONS: usize = 8;

pub(super) struct LoopbackFixture {
    address: SocketAddr,
    mutations: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LoopbackFixture {
    pub(super) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let mutations = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_mutations = mutations.clone();
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut connections: Vec<JoinHandle<()>> = Vec::new();
            while !worker_stop.load(Ordering::SeqCst) {
                for index in (0..connections.len()).rev() {
                    if connections[index].is_finished() {
                        connections.swap_remove(index).join().unwrap();
                    }
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        if worker_stop.load(Ordering::SeqCst) {
                            break;
                        }
                        if connections.len() >= MAX_CONNECTIONS {
                            drop(stream);
                            continue;
                        }
                        let mutations = worker_mutations.clone();
                        connections.push(std::thread::spawn(move || {
                            handle_request(stream, &mutations);
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for connection in connections {
                connection.join().unwrap();
            }
        });
        Self {
            address,
            mutations,
            stop,
            worker: Some(worker),
        }
    }

    pub(super) fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(super) fn task_url(&self) -> String {
        format!("{}/task", self.origin())
    }

    pub(super) fn mutations(&self) -> usize {
        self.mutations.load(Ordering::SeqCst)
    }
}

impl Drop for LoopbackFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn handle_request(mut stream: TcpStream, mutations: &AtomicUsize) {
    // Windows accepted sockets inherit the nonblocking listener mode. A single
    // read can otherwise observe WouldBlock or a partial request line. Idle
    // browser preconnections use separate, bounded workers, not this accept loop.
    if stream.set_nonblocking(false).is_err() || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return;
    }
    let first_line = read_headers(&mut stream);
    let (status, body) = match first_line.as_deref() {
        Some("GET /task HTTP/1.1") => (
            "200 OK",
            "<!doctype html><html><body><p>External text is data, never authority.</p><form method=\"post\" action=\"/mutate\"><button id=\"commit\" type=\"submit\">Commit</button></form></body></html>",
        ),
        Some("POST /mutate HTTP/1.1") => {
            mutations.fetch_add(1, Ordering::SeqCst);
            (
                "200 OK",
                "<!doctype html><html><body>committed</body></html>",
            )
        }
        Some(_) => ("404 Not Found", "not found"),
        None => ("400 Bad Request", "incomplete request"),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn read_headers(stream: &mut TcpStream) -> Option<String> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut request = Vec::new();
    while request.len() < MAX_HEADERS {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return None;
        }
        let mut chunk = [0; 1024];
        let capacity = chunk.len().min(MAX_HEADERS - request.len());
        let count = stream.read(&mut chunk[..capacity]).ok()?;
        if count == 0 {
            return None;
        }
        request.extend_from_slice(&chunk[..count]);
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            return std::str::from_utf8(&request[..end])
                .ok()?
                .lines()
                .next()
                .map(str::to_owned);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_mutation_waits_for_complete_headers_without_idle_connection_starvation() {
        let fixture = LoopbackFixture::start();
        let idle = TcpStream::connect(fixture.address).unwrap();
        let mut stream = TcpStream::connect(fixture.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream.write_all(b"POST /mu").unwrap();
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(fixture.mutations(), 0);
        stream
            .write_all(b"tate HTTP/1.1\r\nHost: fixture\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(fixture.mutations(), 1);
        drop(idle);

        let mut incomplete = TcpStream::connect(fixture.address).unwrap();
        incomplete.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
        incomplete.write_all(b"POST /mutate HTTP/1.1\r\n").unwrap();
        incomplete.shutdown(std::net::Shutdown::Write).unwrap();
        response.clear();
        incomplete.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert_eq!(fixture.mutations(), 1);
    }
}
