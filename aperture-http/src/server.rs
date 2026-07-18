//! Unified HTTP server: owns all listeners and the TLS reload watcher.

use std::future::Future;

use axum::Router;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::tls::{TlsListener, TlsReload};

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

/// Runs the gateway's HTTP stack: an optional HTTPS listener, an optional
/// plain-HTTP listener, and an optional TLS certificate reload watcher.
///
/// Construct with [`HttpServer::new`], then add listeners via the builder
/// methods. All configured listeners and the reload watcher run concurrently
/// inside [`HttpServer::run`] and drain together when the `stop` future
/// resolves.
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

    /// Serves `app` over the TLS listener.
    pub fn serve_tls(mut self, listener: TlsListener, app: Router) -> Self {
        self.tls = Some(TlsEntry { listener, app });
        self
    }

    /// Attaches a TLS reload watcher. Only effective when [`serve_tls`] is
    /// also used.
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

/// Drains all remaining join handles with a timeout per handle.
async fn drain(mut handles: FuturesUnordered<JoinHandle<()>>) {
    while let Some(handle) = handles.next().await {
        if let Err(err) = handle {
            tracing::error!(error = %err, "http server task panicked during drain");
        }
    }
}
