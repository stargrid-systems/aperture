//! Hot-swappable TLS listener implementing `axum::serve::Listener`.
//!
//! Wraps a `TcpListener` and performs a TLS handshake on each accepted
//! connection. The `rustls::ServerConfig` is held behind an `ArcSwap`, so
//! swapping certificates at runtime is a single atomic store with no
//! connection drop.

use std::error::Error as StdError;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use axum::serve::Listener;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use super::SharedConfig;

/// Backoff applied after a `TcpListener::accept` error to avoid busy-looping
/// under persistent failures such as fd exhaustion.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// Backoff applied after a TLS handshake failure. A flood of probes that open
/// a TCP connection and immediately close it (or send garbage) would otherwise
/// spin the accept loop without any yield.
const HANDSHAKE_ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// A `TcpListener` that performs TLS on every accepted connection.
pub struct TlsListener {
    inner: TcpListener,
    config: SharedConfig,
}

impl TlsListener {
    /// Creates a TLS listener wrapping `inner` with the given shared config.
    pub fn new(inner: TcpListener, config: SharedConfig) -> Self {
        Self { inner, config }
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, addr) = match self.inner.accept().await {
                Ok(conn) => conn,
                Err(err) => {
                    tracing::error!(error = &err as &dyn StdError, "tcp accept failed");
                    sleep(ACCEPT_ERROR_BACKOFF).await;
                    continue;
                }
            };
            let config = self.config.load_full();
            let acceptor = TlsAcceptor::from(config);
            match acceptor.accept(stream).await {
                Ok(tls) => return (tls, addr),
                Err(err) => {
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        %addr,
                        "tls handshake failed"
                    );
                    sleep(HANDSHAKE_ERROR_BACKOFF).await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}
