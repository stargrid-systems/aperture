//! HTTP server: owns the listeners and drains them on shutdown.

use std::fmt::Debug;
use std::net::SocketAddr;
use std::time::Duration;

use aperture_runtime::{Stop, Worker, WorkerSet};
use axum::Router;
use axum::serve::Listener;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Upper bound on graceful drain after `stop` resolves.
///
/// axum's `with_graceful_shutdown` waits forever for in-flight connections by
/// default. Without an outer timeout, a single slow client can hang the
/// shutdown indefinitely. The outer supervisor layers its own timeout on top,
/// but here we want connections to close on their own scale before the hard
/// kill.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);

/// A plain HTTP listener paired with its application router.
struct HttpEntry {
    listener: TcpListener,
    app: Router,
}

/// Runs the gateway's HTTP stack.
///
/// Owns zero or more plain-HTTP listeners and drains them together when
/// `stop` resolves.
///
/// Construct with [`HttpServer::start`] for the standard gateway setup, or
/// build piece by piece via [`HttpServer::new`] and [`HttpServer::serve_http`].
pub struct HttpServer {
    http: Vec<HttpEntry>,
}

impl HttpServer {
    /// Creates an empty server with no listeners.
    pub fn new() -> Self {
        Self { http: Vec::new() }
    }

    /// Binds `addr` and serves `app` over a plain HTTP listener.
    pub async fn start(addr: SocketAddr, app: Router) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        // Log the actual bound address so an OS-assigned port (":0")
        // surfaces in the startup banner instead of the requested one.
        let bound = listener.local_addr().unwrap_or(addr);
        tracing::info!(%bound, "aperture listening");
        Ok(Self::new().serve_http(listener, app))
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

    /// Runs all configured listeners until `stop` is cancelled, then drains
    /// in-flight connections.
    ///
    /// If any listener exits before `stop`, the remaining listeners are
    /// drained immediately.
    pub async fn run(self, stop: CancellationToken) {
        let mut workers = WorkerSet::new();

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
        // Either branch cancels the token, which the listeners observe as
        // their graceful-shutdown trigger.
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
    use std::error::Error as StdError;
    use std::future::IntoFuture as _;

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

/// Adapter that lets [`HttpServer`] be driven by a `aperture-runtime`
/// supervisor. The supervisor hands us a stop token.
impl Worker for HttpServer {
    async fn run(self, stop: Stop) {
        HttpServer::run(self, stop).await;
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
