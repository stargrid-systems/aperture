//! `OsWorker`: background worker for mDNS publishing and hostname change reactions.

use std::error::Error as StdError;

use aperture_http::{RegenerateCertificateDefinition, RegenerateCertificateInput};
use aperture_os::{
    ApplyHostnameDefinition, ApplyHostnameInput, Connection, Hostname, HostnameDef,
    ServicePublisher, ServiceSpec,
};
use aperture_runtime::{Stop, Worker};
use aperture_settings::Settings;
use aperture_storage::ActorId;
use aperture_tasks::{TaskDefinition, Tasks};
use tokio::sync::broadcast::error::RecvError;

pub struct OsWorker {
    settings: Settings,
    tasks: Tasks,
    connection: Connection,
    hostname: String,
    https_port: Option<u16>,
    plain_port: Option<u16>,
}

impl OsWorker {
    pub(crate) const fn new(
        settings: Settings,
        tasks: Tasks,
        connection: Connection,
        hostname: String,
        https_port: Option<u16>,
        plain_port: Option<u16>,
    ) -> Self {
        Self {
            settings,
            tasks,
            connection,
            hostname,
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
                        Ok(change) => {
                            if let Ok(Some(hostname)) = change.decode_as::<HostnameDef>() {
                                self.on_hostname_change(&hostname).await;
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

    async fn on_hostname_change(&self, hostname: &Hostname) {
        let name = hostname.as_str();

        if self
            .spawn_task::<ApplyHostnameDefinition>(ApplyHostnameInput {
                hostname: name.to_owned(),
            })
            .await
            .is_none()
        {
            return;
        }

        self.spawn_task::<RegenerateCertificateDefinition>(RegenerateCertificateInput {
            hostname: Some(name.to_owned()),
        })
        .await;

        tracing::info!(hostname = %name, "hostname updated");
    }

    async fn spawn_task<T: TaskDefinition>(&self, input: T::Input) -> Option<T::Output> {
        let handle = match self.tasks.spawn::<T>(input, ActorId::SYSTEM).await {
            Ok(handle) => handle,
            Err(err) => {
                tracing::error!(
                    error = &err as &dyn StdError,
                    kind = T::KIND,
                    "failed to spawn task"
                );
                return None;
            }
        };
        match handle.wait().await {
            Ok(output) => Some(output),
            Err(err) => {
                tracing::error!(
                    error = &err as &dyn StdError,
                    kind = T::KIND,
                    "task failed"
                );
                None
            }
        }
    }
}
