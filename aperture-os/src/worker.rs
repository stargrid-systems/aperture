//! OS worker: mDNS publishing and hostname change reactions.

use std::error::Error as StdError;

use aperture_events::{Delivery, EventBus, TypedEventStream};
use aperture_runtime::{Stop, Worker};
use aperture_settings::SettingChange;
use aperture_storage::ActorId;
use aperture_tasks::Tasks;

use crate::avahi::{ServicePublisher, ServiceSpec};
use crate::event::HostnameApplied;
use crate::hostname::{ApplyHostnameDefinition, ApplyHostnameInput};
use crate::setting::{Hostname, HostnameSetting};

/// Background worker for mDNS publishing and hostname management.
pub struct OsWorker {
    tasks: Tasks,
    connection: zbus::Connection,
    hostname: String,
    https_port: Option<u16>,
    plain_port: Option<u16>,
    event_bus: EventBus,
    setting_changes: TypedEventStream<SettingChange>,
}

impl OsWorker {
    pub(crate) const fn new(
        tasks: Tasks,
        connection: zbus::Connection,
        hostname: String,
        https_port: Option<u16>,
        plain_port: Option<u16>,
        event_bus: EventBus,
        setting_changes: TypedEventStream<SettingChange>,
    ) -> Self {
        Self {
            tasks,
            connection,
            hostname,
            https_port,
            plain_port,
            event_bus,
            setting_changes,
        }
    }
}

impl Worker for OsWorker {
    async fn run(mut self, stop: Stop) {
        let publisher = self.publish_services().await;

        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                delivery = self.setting_changes.recv() => {
                    match delivery {
                        Some(Delivery::Event(event)) => {
                            if let Some(setting) = event.payload.decode::<HostnameSetting>() {
                                self.on_hostname_change(setting.hostname().clone()).await;
                            }
                        }
                        Some(Delivery::Lagged(dropped)) => {
                            tracing::warn!(
                                dropped,
                                "setting change stream lagged, hostname changes may be missed"
                            );
                        }
                        None => break,
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
        if let Err(err) = self
            .event_bus
            .emit(
                HostnameApplied {
                    hostname: hostname.as_str().to_owned(),
                },
                ActorId::SYSTEM,
            )
            .await
        {
            tracing::warn!(
                error = &err as &dyn StdError,
                "failed to emit hostname applied event"
            );
        }
    }
}
