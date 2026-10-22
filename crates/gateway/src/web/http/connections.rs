//! Bound accepted sockets, including clients that never send HTTP headers.
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::serve::Listener;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

pub(super) struct BoundedListener {
    inner: TcpListener,
    capacity: Arc<Semaphore>,
    tls: Option<TlsAcceptor>,
    pub(super) authorities: Vec<String>,
    handshakes: JoinSet<Option<(BoundedStream, std::net::SocketAddr)>>,
}

impl BoundedListener {
    pub(super) fn new(inner: TcpListener) -> Self {
        Self {
            inner,
            capacity: Arc::new(Semaphore::new(64)),
            tls: None,
            authorities: Vec::new(),
            handshakes: JoinSet::new(),
        }
    }

    pub(super) fn encrypted(
        inner: TcpListener,
        tls: TlsAcceptor,
        authorities: Vec<String>,
    ) -> Self {
        Self {
            tls: Some(tls),
            authorities,
            ..Self::new(inner)
        }
    }

    pub(super) fn secure(&self) -> bool {
        self.tls.is_some()
    }
}

impl Listener for BoundedListener {
    type Io = BoundedStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let accepted = tokio::select! {
                completed = self.handshakes.join_next(), if !self.handshakes.is_empty() => {
                    if let Some(Ok(Some(stream))) = completed {
                        return stream;
                    }
                    continue;
                }
                accepted = self.inner.accept() => accepted,
            };
            match accepted {
                Ok((inner, address)) => {
                    if let Ok(permit) = self.capacity.clone().try_acquire_owned() {
                        if let Some(tls) = self.tls.clone() {
                            if self.handshakes.len() < 16 {
                                self.handshakes.spawn(async move {
                                    let stream = tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        tls.accept(inner),
                                    )
                                    .await
                                    .ok()?
                                    .ok()?;
                                    Some((
                                        BoundedStream::new(
                                            WebStream::Tls(Box::new(stream)),
                                            permit,
                                        ),
                                        address,
                                    ))
                                });
                            }
                        } else {
                            return (BoundedStream::new(WebStream::Plain(inner), permit), address);
                        }
                        continue;
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
    inner: WebStream,
    _permit: OwnedSemaphorePermit,
    deadline: Pin<Box<tokio::time::Sleep>>,
}

impl BoundedStream {
    fn new(inner: WebStream, permit: OwnedSemaphorePermit) -> Self {
        Self {
            inner,
            _permit: permit,
            deadline: Box::pin(tokio::time::sleep(std::time::Duration::from_secs(120))),
        }
    }
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
        match &mut self.inner {
            WebStream::Plain(inner) => Pin::new(inner).poll_read(context, buffer),
            WebStream::Tls(inner) => Pin::new(inner).poll_read(context, buffer),
        }
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
        match &mut self.inner {
            WebStream::Plain(inner) => Pin::new(inner).poll_write(context, bytes),
            WebStream::Tls(inner) => Pin::new(inner).poll_write(context, bytes),
        }
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Err(error) = self.check_deadline(context) {
            return Poll::Ready(Err(error));
        }
        match &mut self.inner {
            WebStream::Plain(inner) => Pin::new(inner).poll_flush(context),
            WebStream::Tls(inner) => Pin::new(inner).poll_flush(context),
        }
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.inner {
            WebStream::Plain(inner) => Pin::new(inner).poll_shutdown(context),
            WebStream::Tls(inner) => Pin::new(inner).poll_shutdown(context),
        }
    }
}

enum WebStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}
