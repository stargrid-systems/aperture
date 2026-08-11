//! OS integration worker: mDNS publishing and hostname management.
//!
//! Active only when the `os-integration` Cargo feature is compiled in and the
//! runtime `--os-integration` flag is set.

use std::error::Error as StdError;
use std::net::SocketAddr;

use aperture_artifacts::Artifacts;
use aperture_http::regenerate_leaf_for_identity;
use aperture_os::{
    Connection, ServicePublisher, ServiceSpec, SystemDef, SystemValue, apply_hostname,
    read_hostname,
};
use aperture_runtime::{Stop, Worker};
use aperture_settings::{SettingDefinition, Settings};
use tokio::sync::broadcast::error::RecvError;

pub struct OsWorker {
    settings: Settings,
    artifacts: Artifacts,
    connection: Connection,
    hostname: String,
    tls_addr: Option<SocketAddr>,
    https_port: Option<u16>,
    plain_port: Option<u16>,
}

impl OsWorker {
    pub(crate) const fn new(
        settings: Settings,
        artifacts: Artifacts,
        connection: Connection,
        hostname: String,
        tls_addr: Option<SocketAddr>,
        https_port: Option<u16>,
        plain_port: Option<u16>,
    ) -> Self {
        Self {
            settings,
            artifacts,
            connection,
            hostname,
            tls_addr,
            https_port,
            plain_port,
        }
    }
}

impl Worker for OsWorker {
    async fn run(self, stop: Stop) {
        let publisher = self.publish_services().await;

        let mut feed = self.settings.subscribe();

        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                recv = feed.recv() => {
                    match recv {
                        Ok(change) if change.key == SystemDef::KEY => {
                            self.on_hostname_change(change.value).await;
                        }
                        Ok(_) => {}
                        Err(RecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, "settings change feed lagged");
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        }

        if let Some(publisher) = publisher
            && let Err(err) = publisher.free().await
        {
            tracing::warn!(
                error = &err as &dyn StdError,
                "failed to free avahi entry group"
            );
        }
    }
}

impl OsWorker {
    async fn publish_services(&self) -> Option<ServicePublisher> {
        let mut services = Vec::new();
        if let Some(port) = self.https_port {
            services.push(ServiceSpec {
                service_type: "_https._tcp".to_owned(),
                port,
            });
        }
        if let Some(port) = self.plain_port {
            services.push(ServiceSpec {
                service_type: "_http._tcp".to_owned(),
                port,
            });
        }

        match ServicePublisher::start(&self.connection, &self.hostname, &services).await {
            Ok(publisher) => {
                tracing::info!(hostname = %self.hostname, "mDNS services published");
                Some(publisher)
            }
            Err(err) => {
                tracing::warn!(
                    error = &err as &dyn StdError,
                    "failed to publish mDNS services"
                );
                None
            }
        }
    }

    async fn on_hostname_change(&self, value: serde_json::Value) {
        let value: SystemValue = match serde_json::from_value(value) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    error = &err as &dyn StdError,
                    "failed to decode system setting"
                );
                return;
            }
        };

        let new_hostname = match value.hostname {
            Some(name) => {
                if let Err(err) = apply_hostname(&self.connection, &name).await {
                    tracing::error!(
                        error = &err as &dyn StdError,
                        "failed to apply hostname"
                    );
                    return;
                }
                name
            }
            None => match read_hostname(&self.connection).await {
                Ok(h) => h,
                Err(err) => {
                    tracing::error!(
                        error = &err as &dyn StdError,
                        "failed to read hostname"
                    );
                    return;
                }
            },
        };

        if let Some(addr) = self.tls_addr
            && let Err(err) =
                regenerate_leaf_for_identity(&self.artifacts, addr, Some(&new_hostname)).await
        {
            tracing::error!(
                error = &err as &dyn StdError,
                "failed to regenerate TLS leaf for new hostname"
            );
        }

        tracing::info!(hostname = %new_hostname, "hostname updated");
    }
}
