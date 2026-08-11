//! OS worker: mDNS publishing and hostname change reactions.

use std::error::Error as StdError;

use aperture_runtime::{Stop, Worker};
use aperture_settings::Settings;
use aperture_storage::ActorId;
use aperture_tasks::Tasks;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

use crate::avahi::{ServicePublisher, ServiceSpec};
use crate::hostname::{ApplyHostnameDefinition, ApplyHostnameInput};
use crate::setting::{Hostname, HostnameSetting};

/// Events emitted by the OS worker.
#[derive(Debug, Clone)]
pub enum OsEvent {
    /// The system hostname was successfully applied via systemd-hostnamed.
    HostnameApplied(Hostname),
}

/// Background worker for mDNS publishing and hostname management.
pub struct OsWorker {
    settings: Settings,
    tasks: Tasks,
    connection: zbus::Connection,
    hostname: String,
    https_port: Option<u16>,
    plain_port: Option<u16>,
    events: broadcast::Sender<OsEvent>,
}

impl OsWorker {
    pub(crate) fn new(
        settings: Settings,
        tasks: Tasks,
        connection: zbus::Connection,
        hostname: String,
        https_port: Option<u16>,
        plain_port: Option<u16>,
    ) -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            settings,
            tasks,
            connection,
            hostname,
            https_port,
            plain_port,
            events,
        }
    }

    /// Subscribes to OS events.
    pub fn subscribe(&self) -> broadcast::Receiver<OsEvent> {
        self.events.subscribe()
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
                        Ok(change) => {
                            if let Some(setting) = change.decode::<HostnameSetting>() {
                                self.on_hostname_change(setting.hostname().clone()).await;
                            }
                        }
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
            tracing::warn!(error = &*err, "failed to free avahi entry group");
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
                tracing::warn!(error = &*err, "failed to publish mDNS services");
                None
            }
        }
    }

    async fn on_hostname_change(&self, hostname: Hostname) {
        let Ok(handle) = self
            .tasks
            .spawn::<ApplyHostnameDefinition>(
                ApplyHostnameInput {
                    hostname: hostname.clone(),
                },
                ActorId::SYSTEM,
            )
            .await
        else {
            tracing::error!("failed to spawn apply-hostname task");
            return;
        };

        if let Err(err) = handle.wait().await {
            tracing::error!(error = &err as &dyn StdError, "apply-hostname task failed");
            return;
        }

        tracing::info!(hostname = %hostname, "hostname updated");
        let _ = self.events.send(OsEvent::HostnameApplied(hostname));
    }
}
