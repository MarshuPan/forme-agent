//! Absolute request deadlines for the development/POSIX loopback transport.
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};

pub(super) struct DeadlineSocket {
    stream: TcpStream,
    deadline: Instant,
}

impl DeadlineSocket {
    pub(super) fn connect(address: &SocketAddr, timeout: Duration) -> io::Result<Self> {
        let deadline = Instant::now() + timeout;
        let stream = TcpStream::connect_timeout(address, timeout)?;
        Ok(Self { stream, deadline })
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "worker request deadline exceeded")
            })
    }
}

impl Read for DeadlineSocket {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        let count = self.stream.read(buffer)?;
        self.remaining()?;
        Ok(count)
    }
}

impl Write for DeadlineSocket {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        let count = self.stream.write(buffer)?;
        self.remaining()?;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.remaining().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, sync::mpsc, thread};

    #[test]
    fn fragmented_tcp_bytes_cannot_extend_the_absolute_request_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (observed, ready) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection.set_nodelay(true).unwrap();
            connection.write_all(b"x").unwrap();
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            for _ in 0..200 {
                thread::sleep(Duration::from_millis(5));
                if connection.write_all(b"x").is_err() {
                    break;
                }
            }
        });
        let mut client = DeadlineSocket::connect(&address, Duration::from_secs(2)).unwrap();
        client.read_exact(&mut [0; 1]).unwrap();
        client.deadline = Instant::now() + Duration::from_millis(40);
        observed.send(()).unwrap();
        let started = Instant::now();
        let mut bytes = Vec::new();
        assert!(client.read_to_end(&mut bytes).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(bytes.len() < 200);
        drop(client);
        server.join().unwrap();
    }
}
