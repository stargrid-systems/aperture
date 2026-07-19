//! Unified HTTP server: owns all listeners and the TLS reload watcher.

use std::error::Error as StdError;
use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use aperture_artifacts::Artifacts;
use axum::Router;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::tls::{TlsEndpoint, TlsListener, TlsReload, ensure_certificates, redirect_router};

/// Upper bound on graceful drain after `stop` resolves.
///
/// axum's `with_graceful_shutdown` waits forever for in-flight connections by
/// default. Without an outer timeout, a single slow client can hang the
/// shutdown indefinitely. The supervisor layers its own per-worker timeout on
/// top, but here we want connections to close on their own scale before
/// the hard kill.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);

/// A TLS listener paired with its application router and reload watcher.
struct TlsEntry {
    listener: TlsListener,
    reload: TlsReload,
    app: Router,
}

/// A plain HTTP listener paired with its application router.
struct HttpEntry {
    listener: TcpListener,
    app: Router,
}

/// Runs the gateway's HTTP stack.
///
/// Owns an optional HTTPS endpoint (listener + reload watcher), zero or more
/// plain-HTTP listeners, and drains them together when `stop` resolves.
///
/// Construct with [`HttpServer::start`] for the standard gateway setup, or
/// build piece by piece via [`HttpServer::new`] and [`HttpServer::serve_tls`]
/// / [`HttpServer::serve_http`].
pub struct HttpServer {
    tls: Option<TlsEntry>,
    http: Vec<HttpEntry>,
}

impl HttpServer {
    /// Creates an empty server with no listeners.
    pub fn new() -> Self {
        Self {
            tls: None,
            http: Vec::new(),
        }
    }

    /// Sets up the gateway HTTP stack from the two optional listener addrs.
    ///
    /// - both set: HTTPS listener plus an HTTP listener that redirects to it
    /// - https only: HTTPS listener serving the full API
    /// - http only: plain HTTP listener serving the full API (recovery mode)
    /// - neither: no listeners ([`HttpServer::run`] returns immediately)
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
            let tcp_listener = TcpListener::bind(https_addr).await?;
            // Log the actual bound address so an OS-assigned port (":0")
            // surfaces in the startup banner instead of the requested one.
            let bound = tcp_listener.local_addr().unwrap_or(https_addr);
            tracing::info!(%bound, "aperture listening (https)");
            let endpoint = TlsEndpoint::new(artifacts.clone(), tcp_listener).await?;
            server = server.serve_tls(endpoint, app.clone());
        }

        if let Some(http_addr) = http_addr {
            let http_listener = TcpListener::bind(http_addr).await?;
            let bound = http_listener.local_addr().unwrap_or(http_addr);
            let http_app = match https_addr {
                Some(https) => {
                    tracing::info!(%bound, "http redirect listening");
                    redirect_router(https.port())
                }
                None => {
                    tracing::warn!(
                        %bound,
                        "serving full API over plain HTTP (https disabled)"
                    );
                    app
                }
            };
            server = server.serve_http(http_listener, http_app);
        }

        Ok(server)
    }

    /// Serves `app` over the TLS endpoint (listener + reload watcher).
    ///
    /// The endpoint bundles both halves so you cannot accidentally run a TLS
    /// listener without its reload watcher.
    #[must_use]
    pub fn serve_tls(mut self, endpoint: TlsEndpoint, app: Router) -> Self {
        let (listener, reload) = endpoint.into_parts();
        self.tls = Some(TlsEntry {
            listener,
            reload,
            app,
        });
        self
    }

    /// Serves `app` over a plain HTTP listener.
    ///
    /// Multiple plain HTTP listeners can be attached by calling this method
    /// more than once. The production gateway only attaches one, but the
    /// builder stays additive so testing and recovery setups can run several.
    #[must_use]
    pub fn serve_http(mut self, listener: TcpListener, app: Router) -> Self {
        self.http.push(HttpEntry { listener, app });
        self
    }

    /// Runs all configured listeners and reload watchers until `stop`
    /// resolves, then drains in-flight connections.
    ///
    /// If any listener exits before `stop`, the remaining listeners are
    /// drained immediately.
    pub async fn run(self, stop: impl Future<Output = ()> + Send + 'static) {
        let token = CancellationToken::new();

        let mut handles: FuturesUnordered<JoinHandle<()>> = FuturesUnordered::new();

        if let Some(TlsEntry {
            listener,
            reload,
            app,
        }) = self.tls
        {
            let listener_token = token.clone();
            handles.push(tokio::spawn(async move {
                if let Err(err) = axum::serve(listener, app)
                    .with_graceful_shutdown(async move { listener_token.cancelled().await })
                    .await
                {
                    tracing::error!(
                        error = &err as &dyn StdError,
                        "https server exited with error"
                    );
                }
            }));
            let reload_token = token.clone();
            handles.push(tokio::spawn(reload.run(reload_token)));
        }

        for HttpEntry { listener, app } in self.http {
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
            tracing::debug!("http server has no listeners configured, returning");
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

/// Drains all remaining join handles with a hard ceiling of [`DRAIN_TIMEOUT`].
///
/// axum's `with_graceful_shutdown` waits forever for in-flight connections by
/// default. Without an outer timeout, a single slow client can hang the
/// shutdown indefinitely. Tasks that do not finish before the deadline are
/// detached (the `FuturesUnordered` is dropped mid-iteration) and left for
/// the runtime to clean up at process exit. We do not abort them, so an
/// in-flight response can still finish writing if the connection drops
/// cleanly afterwards.
async fn drain(handles: FuturesUnordered<JoinHandle<()>>) {
    match timeout(DRAIN_TIMEOUT, drain_all(handles)).await {
        Ok(()) => tracing::info!("http server drain complete"),
        Err(_) => tracing::warn!(
            "http server drain timed out after {:?}, detaching remaining tasks",
            DRAIN_TIMEOUT
        ),
    }
}

/// Inner drain loop without an outer deadline. Returns when every task has
/// completed.
async fn drain_all(mut handles: FuturesUnordered<JoinHandle<()>>) {
    while let Some(result) = handles.next().await {
        if let Err(err) = result {
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
    use tokio::time::timeout;

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

    /// Polls `addr` until it accepts a TCP connection, so the test does not
    /// race against the listener spawning.
    async fn wait_until_listening(addr: SocketAddr) {
        use tokio::net::TcpStream;
        use tokio::time::{sleep, timeout};

        let deadline = Duration::from_secs(2);
        timeout(deadline, async {
            loop {
                if TcpStream::connect(addr).await.is_ok() {
                    return;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("listener becomes ready within the deadline");
    }

    #[tokio::test]
    async fn run_drains_after_stop_signal() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bound = listener.local_addr().unwrap();
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

        wait_until_listening(bound).await;
        let _ = tx.send(());

        timeout(Duration::from_secs(5), handle)
            .await
            .expect("server did not drain within 5s")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn run_drains_two_listeners_after_stop() {
        let l1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let b1 = l1.local_addr().unwrap();
        let l2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let b2 = l2.local_addr().unwrap();

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

        wait_until_listening(b1).await;
        wait_until_listening(b2).await;
        let _ = tx.send(());

        timeout(Duration::from_secs(5), handle)
            .await
            .expect("server did not drain within 5s")
            .expect("server task panicked");
    }
}
