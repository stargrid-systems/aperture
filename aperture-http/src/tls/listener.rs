//! Hot-swappable TLS listener implementing `axum::serve::Listener`.
//!
//! Wraps a `TcpListener` and performs a TLS handshake per connection. The
//! `ServerConfig` is behind an `ArcSwap` so cert swaps are atomic.

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

/// Backoff after a TCP accept error (fd exhaustion, etc.).
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(50);
/// Backoff after a TLS handshake failure.
const HANDSHAKE_ERROR_BACKOFF: Duration = Duration::from_millis(50);
/// Log every N-th consecutive failure to avoid flooding.
const ACCEPT_FAILURES_PER_LOG: u32 = 60;

pub struct TlsListener {
    inner: TcpListener,
    config: SharedConfig,
}

impl TlsListener {
    pub const fn new(inner: TcpListener, config: SharedConfig) -> Self {
        Self { inner, config }
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let mut accept_failures: u32 = 0;
        let mut handshake_failures: u32 = 0;
        loop {
            let (stream, addr) = match self.inner.accept().await {
                Ok(conn) => {
                    accept_failures = 0;
                    conn
                }
                Err(err) => {
                    accept_failures = accept_failures.wrapping_add(1);
                    if should_log(accept_failures) {
                        tracing::warn!(
                            error = &err as &dyn StdError,
                            consecutive_failures = accept_failures,
                            "tcp accept failed (logging every {ACCEPT_FAILURES_PER_LOG} failures)"
                        );
                    }
                    sleep(ACCEPT_ERROR_BACKOFF).await;
                    continue;
                }
            };
            let config = self.config.load_full();
            let acceptor = TlsAcceptor::from(config);
            match acceptor.accept(stream).await {
                Ok(tls) => return (tls, addr),
                Err(err) => {
                    handshake_failures = handshake_failures.wrapping_add(1);
                    if should_log(handshake_failures) {
                        tracing::warn!(
                            error = &err as &dyn StdError,
                            %addr,
                            consecutive_failures = handshake_failures,
                            "tls handshake failed (logging every {ACCEPT_FAILURES_PER_LOG} failures)"
                        );
                    }
                    sleep(HANDSHAKE_ERROR_BACKOFF).await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// Logs the first failure then every `interval`-th.
const fn should_log(consecutive: u32) -> bool {
    consecutive == 1 || consecutive.is_multiple_of(ACCEPT_FAILURES_PER_LOG)
}
