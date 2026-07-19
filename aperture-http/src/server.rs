//! Unified HTTP server: owns all listeners and the TLS reload watcher.

use std::error::Error as StdError;
use std::future::Future;
use std::net::SocketAddr;

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

    /// Sets up the gateway HTTP stack from the two optional listener addrs.
    ///
    /// - both set: HTTPS listener plus an HTTP listener that redirects to it
    /// - https only: HTTPS listener serving the full API
    /// - http only: plain HTTP listener serving the full API (recovery mode)
    /// - neither: no listeners (the returned [`HttpServer::run`] returns
    ///   immediately)
    ///
    /// The TLS PKI is ensured and the reload watcher is attached only when
    /// HTTPS is enabled.
    pub async fn start(
        artifacts: Artifacts,
        https_addr: Option<SocketAddr>,
        http_addr: Option<SocketAddr>,
        app: Router,
    ) -> anyhow::Result<Self> {
        let mut server = HttpServer::new();

        if let Some(https_addr) = https_addr {
            ensure_certificates(&artifacts, https_addr).await?;
            let shared_config = load_shared_config(&artifacts).await?;

            let tcp_listener = TcpListener::bind(https_addr).await?;
            let tls_listener = TlsListener::new(tcp_listener, shared_config.clone());
            let tls_reload = TlsReload::new(artifacts.clone(), shared_config);
            tracing::info!(%https_addr, "aperture listening (https)");

            server = server
                .serve_tls(tls_listener, app.clone())
                .with_tls_reload(tls_reload);
        }

        if let Some(http_addr) = http_addr {
            let http_listener = TcpListener::bind(http_addr).await?;
            let http_app = match https_addr {
                Some(https) => {
                    tracing::info!(%http_addr, "http redirect listening");
                    redirect_router(https.port())
                }
                None => {
                    tracing::warn!(
                        %http_addr,
                        "serving full API over plain HTTP (https disabled)"
                    );
                    app
                }
            };
            server = server.serve_http(http_listener, http_app);
        }

        Ok(server)
    }

    /// Serves `app` over the TLS listener.
    #[must_use]
    pub fn serve_tls(mut self, listener: TlsListener, app: Router) -> Self {
        self.tls = Some(TlsEntry { listener, app });
        self
    }

    /// Attaches a TLS reload watcher.
    ///
    /// Only effective when [`serve_tls`] is also used.
    ///
    /// [`serve_tls`]: HttpServer::serve_tls
    #[must_use]
    pub fn with_tls_reload(mut self, reload: TlsReload) -> Self {
        self.reload = Some(reload);
        self
    }

    /// Serves `app` over a plain HTTP listener.
    #[must_use]
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
                    tracing::error!(
                        error = &err as &dyn StdError,
                        "https server exited with error"
                    );
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
                    tracing::error!(
                        error = &err as &dyn StdError,
                        "http server exited with error"
                    );
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
                        tracing::error!(error = &err as &dyn StdError, "http server task panicked");
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
            tracing::error!(
                error = &err as &dyn StdError,
                "http server task panicked during drain"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::routing::get;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio::time::{sleep, timeout};

    use super::*;

    #[tokio::test]
    async fn empty_server_returns_immediately() {
        let server = HttpServer::new();
        let (_tx, rx) = oneshot::channel::<()>();
        timeout(
            Duration::from_millis(100),
            server.run(async move {
                let _ = rx.await;
            }),
        )
        .await
        .expect("server with no listeners should return immediately");
    }

    #[tokio::test]
    async fn run_drains_after_stop_signal() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = HttpServer::new()
            .serve_http(listener, Router::new().route("/", get(|| async { "ok" })));

        let (tx, rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            server
                .run(async move {
                    let _ = rx.await;
                })
                .await;
        });

        sleep(Duration::from_millis(100)).await;
        let _ = tx.send(());

        timeout(Duration::from_secs(5), handle)
            .await
            .expect("server did not drain within 5s")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn run_drains_two_listeners_after_stop() {
        let l1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let l2 = TcpListener::bind("127.0.0.1:0").await.unwrap();

        let server = HttpServer::new()
            .serve_http(l1, Router::new().route("/", get(|| async { "1" })))
            .serve_http(l2, Router::new().route("/", get(|| async { "2" })));

        let (tx, rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            server
                .run(async move {
                    let _ = rx.await;
                })
                .await;
        });

        sleep(Duration::from_millis(100)).await;
        let _ = tx.send(());

        timeout(Duration::from_secs(5), handle)
            .await
            .expect("server did not drain within 5s")
            .expect("server task panicked");
    }
}
