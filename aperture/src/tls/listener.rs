//! Hot-swappable TLS listener implementing `axum::serve::Listener`.
//!
//! Wraps a `TcpListener` and performs a TLS handshake on each accepted
//! connection. The `rustls::ServerConfig` is held behind an `ArcSwap`, so
//! swapping certificates at runtime is a single atomic store with no
//! connection drop.

use std::error::Error as StdError;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::serve::Listener;
use rustls::ServerConfig;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::tls::SharedConfig;

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
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// Creates the initial shared config from a `ServerConfig`.
pub fn shared_config(config: ServerConfig) -> SharedConfig {
    Arc::new(ArcSwap::from_pointee(config))
}
