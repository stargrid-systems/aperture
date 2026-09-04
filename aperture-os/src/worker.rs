//! OS worker: mDNS publishing and hostname change reactions.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_events::{Delivery, EventBus, TypedEventStream};
use aperture_runtime::{Stop, Worker};
use aperture_settings::SettingChange;
use aperture_storage::ActorId;
use aperture_tasks::Tasks;
use tokio::time::sleep;

use crate::avahi::{ServicePublisher, ServiceSpec};
use crate::event::HostnameApplied;
use crate::hostname::{ApplyHostnameDefinition, ApplyHostnameInput};
use crate::setting::{Hostname, HostnameSetting};

/// Delay before the first mDNS publish retry.
const PUBLISH_RETRY_DELAY: Duration = Duration::from_secs(1);
/// Upper bound for the mDNS publish retry backoff.
const PUBLISH_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

/// Background worker for mDNS publishing and hostname management.
pub struct OsWorker {
    tasks: Tasks,
    connection: zbus::Connection,
    hostname: String,
    https_port: Option<u16>,
    plain_port: Option<u16>,
    tls_enabled: bool,
    event_bus: EventBus,
    setting_changes: TypedEventStream<SettingChange>,
}

impl OsWorker {
    #[expect(clippy::too_many_arguments)]
    pub(crate) const fn new(
        tasks: Tasks,
        connection: zbus::Connection,
        hostname: String,
        https_port: Option<u16>,
        plain_port: Option<u16>,
        tls_enabled: bool,
        event_bus: EventBus,
        setting_changes: TypedEventStream<SettingChange>,
    ) -> Self {
        Self {
            tasks,
            connection,
            hostname,
            https_port,
            plain_port,
            tls_enabled,
            event_bus,
            setting_changes,
        }
    }
}

impl Worker for OsWorker {
    async fn run(mut self, stop: Stop) {
        let Some(mut publisher) = self.publish(&stop).await else {
            return;
        };
        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                reason = publisher.next_recreate() => {
                    tracing::info!(?reason, "avahi state changed, re-publishing services");
                    if !self.republish(&mut publisher, &stop).await {
                        break;
                    }
                }
                delivery = self.setting_changes.recv() => {
                    match delivery {
                        Some(Delivery::Event(event)) => {
                            if let Some(setting) = event.payload.decode::<HostnameSetting>() {
                                self.on_hostname_change(
                                    setting.hostname().clone(),
                                    &mut publisher,
                                    &stop,
                                )
                                .await;
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

        if let Err(err) = publisher.free().await {
            tracing::warn!(error = &*err, "failed to free avahi entry group");
        }
    }
}

impl OsWorker {
    /// Publishes the mDNS services for the current hostname.
    ///
    /// Retries with exponential backoff until publishing succeeds or `stop`
    /// resolves. Returns `None` when stopped.
    async fn publish(&self, stop: &Stop) -> Option<ServicePublisher> {
        let services = services_to_publish(
            &self.hostname,
            self.https_port,
            self.plain_port,
            self.tls_enabled,
        );
        let mut delay = PUBLISH_RETRY_DELAY;
        loop {
            match ServicePublisher::start(&self.connection, &self.hostname, &services).await {
                Ok(publisher) => {
                    tracing::info!(hostname = %self.hostname, "mDNS services published");
                    return Some(publisher);
                }
                Err(err) => {
                    tracing::warn!(error = &*err, "failed to publish mDNS services, retrying");
                    tokio::select! {
                        () = stop.cancelled() => return None,
                        () = sleep(delay) => {}
                    }
                    delay = (delay * 2).min(PUBLISH_RETRY_MAX_DELAY);
                }
            }
        }
    }

    /// Frees the current advertisement and publishes it again under the
    /// current hostname. Returns `false` when stopped before success.
    async fn republish(&self, publisher: &mut ServicePublisher, stop: &Stop) -> bool {
        if let Err(err) = publisher.free().await {
            tracing::warn!(error = &*err, "failed to free avahi entry group");
        }
        let Some(new) = self.publish(stop).await else {
            return false;
        };
        *publisher = new;
        true
    }

    /// Applies the hostname and, on success, re-publishes the services
    /// under it.
    ///
    /// Returns early when `stop` resolves while the apply task runs, so a
    /// hung D-Bus call cannot stall shutdown.
    async fn on_hostname_change(
        &mut self,
        hostname: Hostname,
        publisher: &mut ServicePublisher,
        stop: &Stop,
    ) {
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

        if let Err(err) = tokio::select! {
            biased;
            () = stop.cancelled() => {
                tracing::info!("shutdown requested during hostname apply");
                return;
            }
            result = handle.wait() => result,
        } {
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

        hostname.as_str().clone_into(&mut self.hostname);
        self.republish(publisher, stop).await;
    }
}

/// Decides which services to advertise for the given listener layout.
///
/// The HTTPS listener is always advertised as `_https._tcp`. The plain
/// listener is advertised as `_http._tcp` only when it serves the API
/// itself (no TLS configured); with TLS enabled it merely redirects.
/// Bind-any-port configurations (`:0`) are never advertised because the
/// real port is unknown, and neither is an empty hostname.
fn services_to_publish(
    hostname: &str,
    https_port: Option<u16>,
    plain_port: Option<u16>,
    tls_enabled: bool,
) -> Vec<ServiceSpec> {
    if hostname.is_empty() {
        return Vec::new();
    }
    let mut services = Vec::new();
    if let Some(port) = https_port.filter(|port| *port != 0) {
        services.push(ServiceSpec {
            service_type: "_https._tcp".to_owned(),
            port,
        });
    }
    if !tls_enabled && let Some(port) = plain_port.filter(|port| *port != 0) {
        services.push(ServiceSpec {
            service_type: "_http._tcp".to_owned(),
            port,
        });
    }
    services
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_types(services: &[ServiceSpec]) -> Vec<(&str, u16)> {
        services
            .iter()
            .map(|spec| (spec.service_type.as_str(), spec.port))
            .collect()
    }

    #[test]
    fn https_and_plain_are_advertised_without_tls() {
        let services = services_to_publish("aperture", Some(8443), Some(8080), false);
        assert_eq!(
            service_types(&services),
            [("_https._tcp", 8443), ("_http._tcp", 8080)]
        );
    }

    #[test]
    fn plain_redirect_listener_is_not_advertised_with_tls() {
        let services = services_to_publish("aperture", Some(8443), Some(8080), true);
        assert_eq!(service_types(&services), [("_https._tcp", 8443)]);
    }

    #[test]
    fn bind_any_ports_are_not_advertised() {
        let services = services_to_publish("aperture", Some(0), Some(0), false);
        assert!(services.is_empty());
    }

    #[test]
    fn without_tls_only_the_plain_listener_is_advertised() {
        let services = services_to_publish("aperture", None, Some(8080), false);
        assert_eq!(service_types(&services), [("_http._tcp", 8080)]);
    }

    #[test]
    fn empty_hostname_is_not_advertised() {
        let services = services_to_publish("", Some(8443), Some(8080), false);
        assert!(services.is_empty());
    }
}
