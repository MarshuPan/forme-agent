//! Bound accepted sockets, including clients that never send HTTP headers.
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::serve::Listener;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) struct BoundedListener {
    inner: TcpListener,
    capacity: Arc<Semaphore>,
}

impl BoundedListener {
    pub(super) fn new(inner: TcpListener) -> Self {
        Self {
            inner,
            capacity: Arc::new(Semaphore::new(64)),
        }
    }
}

impl Listener for BoundedListener {
    type Io = BoundedStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.inner.accept().await {
                Ok((inner, address)) => {
                    if let Ok(permit) = self.capacity.clone().try_acquire_owned() {
                        return (
                            BoundedStream {
                                inner,
                                _permit: permit,
                                deadline: Box::pin(tokio::time::sleep(
                                    std::time::Duration::from_secs(120),
                                )),
                            },
                            address,
                        );
                    }
                    // Drop overflow sockets. No handler or authentication job
                    // is created, and no unbounded waiting task is spawned.
                    drop(inner);
                    tokio::task::yield_now().await;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

pub(super) struct BoundedStream {
    inner: TcpStream,
    _permit: OwnedSemaphorePermit,
    deadline: Pin<Box<tokio::time::Sleep>>,
}

impl BoundedStream {
    fn check_deadline(&mut self, context: &mut Context<'_>) -> std::io::Result<()> {
        if self.deadline.as_mut().poll(context).is_ready() {
            return Err(std::io::Error::from(std::io::ErrorKind::TimedOut));
        }
        Ok(())
    }
}

impl AsyncRead for BoundedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Err(error) = self.check_deadline(context) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for BoundedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Err(error) = self.check_deadline(context) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.inner).poll_write(context, bytes)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Err(error) = self.check_deadline(context) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.inner).poll_flush(context)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}
