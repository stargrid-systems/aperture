//! Unified HTTP server: owns all listeners and the TLS reload watcher.

use std::error::Error as StdError;
use std::fmt::Debug;
use std::future::IntoFuture as _;
use std::net::SocketAddr;
use std::time::Duration;

use aperture_artifacts::Artifacts;
use aperture_events::EventBus;
use aperture_runtime::{Stop, Worker, WorkerSet};
use axum::Router;
use axum::serve::Listener;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::tls::{TlsEndpoint, TlsListener, TlsReload, ensure_certificates, redirect_router};

/// Upper bound on graceful drain after `stop` resolves.
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
/// Owns an optional HTTPS endpoint (listener + reload watcher) and zero or
/// more plain-HTTP listeners. Build with [`HttpServer::start`] for the
/// standard gateway setup, or via [`HttpServer::new`] +
/// [`HttpServer::serve_tls`] / [`HttpServer::serve_http`].
pub struct HttpServer {
    tls: Option<TlsEntry>,
    http: Vec<HttpEntry>,
}

impl HttpServer {
    /// Creates an empty server with no listeners.
    pub const fn new() -> Self {
        Self {
            tls: None,
            http: Vec::new(),
        }
    }

    /// Sets up the gateway HTTP stack from two optional listener addrs.
    /// Both set: HTTPS + HTTP redirect. HTTPS only: full API over TLS.
    /// HTTP only: full API (recovery mode). Neither: no listeners.
    ///
    /// `hostname` is the advertised mDNS hostname. When set, it is baked into
    /// the leaf cert as a `<hostname>.local` SAN. Pass `None` when OS
    /// integration is disabled.
    ///
    /// # Errors
    ///
    /// Returns an error if TLS setup, certificate provisioning, or listener
    /// binding fails.
    pub async fn start(
        artifacts: Artifacts,
        tls_addr: Option<SocketAddr>,
        plain_addr: Option<SocketAddr>,
        hostname: Option<&str>,
        app: Router,
        event_bus: &EventBus,
    ) -> anyhow::Result<Self> {
        let mut server = Self::new();
        // The bound HTTPS port, so redirects target the real listener. With an
        // OS-assigned port (":0") the requested port would redirect to port 0.
        let mut https_port: Option<u16> = None;

        if let Some(tls_addr) = tls_addr {
            ensure_certificates(&artifacts, tls_addr, hostname).await?;
            let tcp_listener = TcpListener::bind(tls_addr).await?;
            // Log the actual bound address so an OS-assigned port (":0")
            // surfaces in the startup banner instead of the requested one.
            let bound = tcp_listener.local_addr().unwrap_or(tls_addr);
            https_port = Some(bound.port());
            tracing::info!(%bound, "aperture listening (https)");
            let endpoint = TlsEndpoint::new(artifacts.clone(), tcp_listener, event_bus).await?;
            server = server.serve_tls(endpoint, app.clone());
        }

        if let Some(plain_addr) = plain_addr {
            let http_listener = TcpListener::bind(plain_addr).await?;
            let bound = http_listener.local_addr().unwrap_or(plain_addr);
            let http_app = https_port.map_or_else(
                || {
                    tracing::warn!(
                        %bound,
                        "serving full API over plain HTTP (https disabled)"
                    );
                    app
                },
                |port| {
                    tracing::info!(%bound, "http redirect listening");
                    redirect_router(port)
                },
            );
            server = server.serve_http(http_listener, http_app);
        }

        Ok(server)
    }

    /// Serves `app` over the TLS endpoint (listener + reload watcher).
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

    /// Serves `app` over a plain HTTP listener. Can be called multiple times.
    #[must_use]
    pub fn serve_http(mut self, listener: TcpListener, app: Router) -> Self {
        self.http.push(HttpEntry { listener, app });
        self
    }

    /// The port the TLS listener is actually bound to. `None` when TLS is
    /// not configured or the OS-assigned port cannot be determined.
    ///
    /// Unlike the configured port this is never `0`, so it is safe to
    /// advertise.
    #[must_use]
    pub fn tls_port(&self) -> Option<u16> {
        let entry = self.tls.as_ref()?;
        entry.listener.local_addr().ok().map(|addr| addr.port())
    }

    /// The port the plain HTTP listener is actually bound to. `None` when
    /// there is no plain listener or the OS-assigned port cannot be
    /// determined.
    #[must_use]
    pub fn plain_port(&self) -> Option<u16> {
        let entry = self.http.first()?;
        entry.listener.local_addr().ok().map(|addr| addr.port())
    }

    /// Runs all listeners and reload watchers until `stop` is cancelled, then
    /// drains in-flight connections.
    pub async fn run(self, stop: CancellationToken) {
        let mut workers = WorkerSet::new();

        if let Some(TlsEntry {
            listener,
            reload,
            app,
        }) = self.tls
        {
            let listener_token = stop.clone();
            workers.spawn("https", async move {
                serve_until_cancelled(listener, app, listener_token).await;
            });
            let reload_token = stop.clone();
            workers.spawn("tls-reload", reload.run(reload_token));
        }

        for HttpEntry { listener, app } in self.http {
            let listener_token = stop.clone();
            workers.spawn("http", async move {
                serve_until_cancelled(listener, app, listener_token).await;
            });
        }
        if workers.is_empty() {
            tracing::debug!("http server has no listeners configured, returning");
            return;
        }

        // Wait for either the stop signal or any worker to exit on its own.
        // Either branch cancels the token, which the listeners and reload
        // watcher observe as their graceful-shutdown trigger.
        tokio::select! {
            biased;
            () = stop.cancelled() => {
                workers.drain(DRAIN_TIMEOUT).await;
            }
            name = workers.wait_for_any_exit() => {
                if let Some(name) = name {
                    tracing::info!(
                        worker = name,
                        "http worker exited early, draining remaining workers"
                    );
                }
                stop.cancel();
                workers.drain(DRAIN_TIMEOUT).await;
            }
        }
    }
}

/// Serve `app` from `listener` until `token` is cancelled.
async fn serve_until_cancelled<L>(listener: L, app: Router, token: CancellationToken)
where
    L: Listener + Send + 'static,
    L::Io: Send,
    L::Addr: Debug + Send,
{
    let serve =
        axum::serve(listener, app).with_graceful_shutdown(async move { token.cancelled().await });
    if let Err(err) = serve.into_future().await {
        tracing::error!(
            error = &err as &dyn StdError,
            "http server exited with error"
        );
    }
}

impl Default for HttpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapter for the `aperture-runtime` supervisor.
impl Worker for HttpServer {
    async fn run(self, stop: Stop) {
        Self::run(self, stop).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::routing::get;
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn empty_server_returns_immediately() {
        let server = HttpServer::new();
        let token = CancellationToken::new();
        // Drop the only clone so the watcher arm in `run` would fire, but the
        // server has no listeners to wait on anyway.
        drop(token);
        timeout(
            Duration::from_millis(100),
            server.run(CancellationToken::new()),
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

        let token = CancellationToken::new();
        let cancel = token.clone();
        let handle = tokio::spawn(async move {
            server.run(token).await;
        });

        wait_until_listening(bound).await;
        cancel.cancel();

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

        let token = CancellationToken::new();
        let cancel = token.clone();
        let handle = tokio::spawn(async move {
            server.run(token).await;
        });

        wait_until_listening(b1).await;
        wait_until_listening(b2).await;
        cancel.cancel();

        timeout(Duration::from_secs(5), handle)
            .await
            .expect("server did not drain within 5s")
            .expect("server task panicked");
    }
}
