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

/// How many consecutive failures to swallow before logging again.
///
/// Keeps a broken listener visible in the log without flooding it. At the
/// 50 ms backoff this fires roughly every 3 seconds under sustained failure.
const ACCEPT_FAILURES_PER_LOG: u32 = 60;

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
        let mut accept_failures: u32 = 0;
        let mut handshake_failures: u32 = 0;
        loop {
            let (stream, addr) = match self.inner.accept().await {
                Ok(conn) => {
                    // Reset the accept-failure counter so the next accept
                    // error starts fresh.
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

/// Returns true the first time and then every `interval`-th time. Keeps the
/// log clear under sustained failure while still surfacing the problem.
fn should_log(consecutive: u32) -> bool {
    consecutive == 1 || consecutive.is_multiple_of(ACCEPT_FAILURES_PER_LOG)
}
