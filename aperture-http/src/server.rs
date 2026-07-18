//! Unified HTTP server: owns all listeners and the TLS reload watcher.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use aperture_artifacts::Artifacts;
use axum::Router;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::tls::{
    TlsListener, TlsReload, ensure_certificates, load_shared_config, redirect_router,
};

/// A TLS listener paired with its application router.
struct TlsEntry {
    listener: TlsListener,
    app: Router,
}

/// A plain HTTP listener paired with its application router.
struct HttpEntry {
    listener: TcpListener,
    app: Router,
}

/// Runs the gateway's HTTP stack.
///
/// Owns an optional HTTPS listener, an optional plain-HTTP listener, and an
/// optional TLS certificate reload watcher. All configured listeners and the
/// reload watcher run concurrently inside [`HttpServer::run`] and drain
/// together when the `stop` future resolves.
///
/// Construct with [`HttpServer::start`] for the standard gateway setup, or
/// build piece by piece via [`HttpServer::new`] and the builder methods.
pub struct HttpServer {
    tls: Option<TlsEntry>,
    reload: Option<TlsReload>,
    http: Option<HttpEntry>,
}

impl HttpServer {
    /// Creates an empty server with no listeners.
    pub fn new() -> Self {
        Self {
            tls: None,
            reload: None,
            http: None,
        }
    }

    /// Sets up the standard gateway HTTP stack.
    ///
    /// Ensures TLS certificates exist, binds the HTTPS listener, attaches the
    /// certificate reload watcher, and optionally binds a second HTTP listener
    /// that either redirects to HTTPS or serves the full API in plain HTTP
    /// (recovery mode).
    pub async fn start(
        artifacts: Arc<Artifacts>,
        https_addr: SocketAddr,
        http_addr: Option<SocketAddr>,
        insecure_http: bool,
        app: Router,
    ) -> anyhow::Result<Self> {
        ensure_certificates(&artifacts, https_addr).await?;
        let shared_config = load_shared_config(&artifacts).await?;

        let tcp_listener = TcpListener::bind(https_addr).await?;
        let tls_listener = TlsListener::new(tcp_listener, shared_config.clone());
        let tls_reload = TlsReload::new(Arc::clone(&artifacts), shared_config);
        tracing::info!(%https_addr, "aperture listening (https)");

        let mut server = HttpServer::new()
            .serve_tls(tls_listener, app.clone())
            .with_tls_reload(tls_reload);

        if let Some(http_addr) = http_addr {
            let http_listener = TcpListener::bind(http_addr).await?;
            if insecure_http {
                tracing::warn!(%http_addr, "serving full API over plain HTTP (insecure mode)");
                server = server.serve_http(http_listener, app);
            } else {
                tracing::info!(%http_addr, "http redirect listening");
                let redirect = redirect_router(https_addr.port());
                server = server.serve_http(http_listener, redirect);
            }
        }

        Ok(server)
    }

    /// Serves `app` over the TLS listener.
    pub fn serve_tls(mut self, listener: TlsListener, app: Router) -> Self {
        self.tls = Some(TlsEntry { listener, app });
        self
    }

    /// Attaches a TLS reload watcher.
    ///
    /// Only effective when [`serve_tls`] is also used.
    ///
    /// [`serve_tls`]: HttpServer::serve_tls
    pub fn with_tls_reload(mut self, reload: TlsReload) -> Self {
        self.reload = Some(reload);
        self
    }

    /// Serves `app` over a plain HTTP listener.
    pub fn serve_http(mut self, listener: TcpListener, app: Router) -> Self {
        self.http = Some(HttpEntry { listener, app });
        self
    }

    /// Runs all configured listeners and the reload watcher until `stop`
    /// resolves, then drains in-flight connections.
    ///
    /// If any listener exits before `stop`, the remaining listeners are
    /// drained immediately.
    pub async fn run(self, stop: impl Future<Output = ()> + Send + 'static) {
        let token = CancellationToken::new();

        let mut handles: FuturesUnordered<JoinHandle<()>> = FuturesUnordered::new();

        if let Some(TlsEntry { listener, app }) = self.tls {
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                if let Err(err) = axum::serve(listener, app)
                    .with_graceful_shutdown(async move { token.cancelled().await })
                    .await
                {
                    tracing::error!(error = %err, "https server exited with error");
                }
            }));
        }

        if let Some(reload) = self.reload {
            let token = token.clone();
            handles.push(tokio::spawn(reload.run(token)));
        }

        if let Some(HttpEntry { listener, app }) = self.http {
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                if let Err(err) = axum::serve(listener, app)
                    .with_graceful_shutdown(async move { token.cancelled().await })
                    .await
                {
                    tracing::error!(error = %err, "http server exited with error");
                }
            }));
        }

        if handles.is_empty() {
            return;
        }

        tokio::pin!(stop);

        loop {
            tokio::select! {
                biased;
                _ = &mut stop => {
                    token.cancel();
                    drain(handles).await;
                    return;
                }
                result = handles.next() => match result {
                    None => return,
                    Some(Err(err)) => {
                        tracing::error!(error = %err, "http server task panicked");
                        token.cancel();
                    }
                    Some(Ok(())) => {
                        token.cancel();
                    }
                },
            }
        }
    }
}

impl Default for HttpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Drains all remaining join handles.
async fn drain(mut handles: FuturesUnordered<JoinHandle<()>>) {
    while let Some(handle) = handles.next().await {
        if let Err(err) = handle {
            tracing::error!(error = %err, "http server task panicked during drain");
        }
    }
}
