//! OS worker: mDNS publishing and hostname change reactions.

use std::error::Error as StdError;
use std::time::{Duration, Instant};

use aperture_events::{EventBus, TypedEventStream};
use aperture_runtime::{Stop, Worker};
use aperture_settings::SettingChange;
use aperture_storage::ActorId;
use aperture_tasks::Tasks;
use tokio::time::sleep;

use crate::avahi::{RecreateReason, ServicePublisher, ServiceSpec, host_fqdn};
use crate::event::HostnameApplied;
use crate::hostname::{ApplyHostnameDefinition, ApplyHostnameInput};
use crate::setting::{Hostname, HostnameSetting};

/// Delay before the first mDNS publish retry.
const PUBLISH_RETRY_DELAY: Duration = Duration::from_secs(1);
/// Upper bound for the mDNS publish retry backoff.
const PUBLISH_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

/// Backoff before the first recreate-driven re-publication.
const RECREATE_FIRST_DELAY: Duration = Duration::from_secs(1);
/// Upper bound for the recreate-driven re-publication backoff.
const RECREATE_MAX_DELAY: Duration = Duration::from_secs(30);
/// Recreate triggers farther apart than this count as unrelated and reset
/// the backoff.
const RECREATE_QUIET_PERIOD: Duration = Duration::from_secs(60);

/// Byte limit for a DNS label: the service instance name must fit.
const INSTANCE_LABEL_LIMIT: usize = 63;
/// Suffix digits always kept in reserve when truncating the instance base,
/// so single- to multi-digit bumps rarely re-truncate.
const INSTANCE_SUFFIX_RESERVE: usize = 4;

/// Background worker for mDNS publishing and hostname management.
pub struct OsWorker {
    tasks: Tasks,
    connection: zbus::Connection,
    hostname: String,
    /// Service instance name currently published: the hostname plus a
    /// numeric suffix after collisions.
    instance: String,
    https_port: Option<u16>,
    plain_port: Option<u16>,
    tls_enabled: bool,
    event_bus: EventBus,
    setting_changes: TypedEventStream<SettingChange>,
}

impl OsWorker {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn new(
        tasks: Tasks,
        connection: zbus::Connection,
        hostname: String,
        https_port: Option<u16>,
        plain_port: Option<u16>,
        tls_enabled: bool,
        event_bus: EventBus,
        setting_changes: TypedEventStream<SettingChange>,
    ) -> Self {
        let instance = hostname.clone();
        Self {
            tasks,
            connection,
            hostname,
            instance,
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
        let mut debounce = RecreateDebounce::default();
        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                reason = publisher.next_recreate() => {
                    self.instance = next_instance(&self.hostname, &self.instance, &reason);
                    let delay = debounce.next_delay(Instant::now());
                    tracing::info!(
                        ?reason,
                        instance = %self.instance,
                        delay = ?delay,
                        "avahi state changed, re-publishing services"
                    );
                    let superseded = tokio::select! {
                        biased;
                        () = stop.cancelled() => break,
                        () = sleep(delay) => false,
                        event = self.setting_changes.recv() => match event {
                            Some(event) => {
                                self.on_setting_change(
                                    event.payload,
                                    &mut publisher,
                                    &stop,
                                    &mut debounce,
                                )
                                .await
                            }
                            None => break,
                        },
                    };
                    if !superseded && !self.republish(&mut publisher, &stop).await {
                        break;
                    }
                }
                event = self.setting_changes.recv() => {
                    match event {
                        Some(event) => {
                            self.on_setting_change(
                                event.payload,
                                &mut publisher,
                                &stop,
                                &mut debounce,
                            )
                            .await;
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
    /// The service instance name is [`Self::instance`]; the advertised host
    /// is the daemon's own FQDN, queried fresh at every publish so a
    /// renamed peer is never targeted. When the FQDN differs from
    /// `<hostname>.local`, TLS via the mDNS name fails until the hostname
    /// setting is unique on the LAN; the mismatch is logged on each
    /// publish. Retries with exponential backoff until publishing
    /// succeeds or `stop` resolves. Returns `None` when stopped.
    async fn publish(&self, stop: &Stop) -> Option<ServicePublisher> {
        let services = services_to_publish(
            &self.hostname,
            self.https_port,
            self.plain_port,
            self.tls_enabled,
        );
        let mut delay = PUBLISH_RETRY_DELAY;
        loop {
            let (host, mismatch) = match host_fqdn(&self.connection).await {
                Ok(fqdn) => srv_host(&fqdn, &self.hostname),
                Err(err) => {
                    tracing::warn!(
                        error = &*err,
                        "failed to query the avahi host FQDN, retrying"
                    );
                    if !wait_retry(stop, &mut delay).await {
                        return None;
                    }
                    continue;
                }
            };
            if mismatch {
                tracing::warn!(
                    host = %host,
                    setting = %self.hostname,
                    "avahi reports a host differing from the os.hostname setting; \
                     TLS via the mDNS name fails until the setting hostname is \
                     unique on the LAN"
                );
            }
            match ServicePublisher::start(&self.connection, &self.instance, &host, &services).await
            {
                Ok(publisher) => {
                    tracing::info!(
                        host = %host,
                        instance = %self.instance,
                        "mDNS services published"
                    );
                    return Some(publisher);
                }
                Err(err) => {
                    tracing::warn!(error = &*err, "failed to publish mDNS services, retrying");
                }
            }
            if !wait_retry(stop, &mut delay).await {
                return None;
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
    /// under it. Returns `true` when a fresh advertisement is in place or
    /// `stop` resolved, so a pending recreate republish is superseded.
    ///
    /// Skipped entirely when the new hostname equals the current one.
    /// Returns early when `stop` resolves while the apply task runs, so a
    /// hung D-Bus call cannot stall shutdown.
    async fn on_hostname_change(
        &mut self,
        hostname: Hostname,
        publisher: &mut ServicePublisher,
        stop: &Stop,
        debounce: &mut RecreateDebounce,
    ) -> bool {
        if hostname.as_str() == self.hostname {
            tracing::debug!(hostname = %hostname, "hostname unchanged, skipping apply");
            return false;
        }

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
            return false;
        };

        if let Err(err) = tokio::select! {
            biased;
            () = stop.cancelled() => {
                tracing::info!("shutdown requested during hostname apply");
                return true;
            }
            result = handle.wait() => result,
        } {
            tracing::error!(error = &err as &dyn StdError, "apply-hostname task failed");
            return false;
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
        hostname.as_str().clone_into(&mut self.instance);
        if !self.republish(publisher, stop).await {
            return true;
        }
        // Best-effort: buffered signals predate the fresh advertisement,
        // but a post-apply RUNNING signal can still arrive after the drain
        // and cause one redundant republish, which is harmless.
        publisher.drain_stale();
        // The old backoff describes the pre-rename storm; the next
        // collision should start over.
        debounce.reset();
        true
    }

    /// Handles one settings-stream event. Returns
    /// [`Self::on_hostname_change`]'s flag; non-hostname settings never
    /// republish and are always `false`.
    async fn on_setting_change(
        &mut self,
        event: SettingChange,
        publisher: &mut ServicePublisher,
        stop: &Stop,
        debounce: &mut RecreateDebounce,
    ) -> bool {
        let Some(setting) = event.decode::<HostnameSetting>() else {
            return false;
        };
        self.on_hostname_change(setting.hostname().clone(), publisher, stop, debounce)
            .await
    }
}

/// Service instance name for the next publish attempt.
///
/// A group collision means the name is taken by another host: bump the
/// numeric suffix, `aperture` -> `aperture-2` -> `aperture-3`. The result
/// always fits the 63-byte DNS label limit: the base is truncated to make
/// room for the separator and the suffix digits. Any other trigger resets
/// to the plain hostname: a daemon restart freed the old names, and a
/// failure does not imply the name is taken.
fn next_instance(base: &str, current: &str, reason: &RecreateReason) -> String {
    match reason {
        RecreateReason::GroupCollision => current
            .rsplit_once('-')
            .and_then(|(prefix, suffix)| suffix.parse::<u32>().ok().map(|n| (prefix, n)))
            .map_or_else(
                || suffixed_instance(base, 2),
                |(prefix, n)| suffixed_instance(prefix, n.saturating_add(1)),
            ),
        RecreateReason::Server | RecreateReason::GroupFailure => base.to_owned(),
    }
}

/// Appends `-<suffix>` to `base`, truncating the base so the whole label
/// stays within the 63-byte DNS label limit.
fn suffixed_instance(base: &str, suffix: u32) -> String {
    let digits = digit_count(suffix).max(INSTANCE_SUFFIX_RESERVE);
    let budget = INSTANCE_LABEL_LIMIT.saturating_sub(1 + digits);
    let base: String = base.chars().take(budget).collect();
    format!("{base}-{suffix}")
}

/// Number of decimal digits in `n`.
const fn digit_count(n: u32) -> usize {
    let mut n = n;
    let mut count = 1;
    while n >= 10 {
        n /= 10;
        count += 1;
    }
    count
}

/// SRV target host for the advertisement, from the daemon's own FQDN.
///
/// The FQDN is used verbatim: after a collision rename, or when the
/// hostname apply failed, `<hostname>.local` can name a different machine,
/// and advertising it would silently misroute connections to that peer.
/// The bool reports whether the FQDN differs from the certificate SAN host
/// (`<hostname>.local`); when it does, TLS via the mDNS name fails against
/// this machine until the hostname setting is unique on the LAN.
fn srv_host(fqdn: &str, hostname: &str) -> (String, bool) {
    (fqdn.to_owned(), fqdn != format!("{hostname}.local"))
}

/// Sleeps out the retry delay and doubles it for the next round. Returns
/// `false` when `stop` resolved first.
async fn wait_retry(stop: &Stop, delay: &mut Duration) -> bool {
    tokio::select! {
        biased;
        () = stop.cancelled() => false,
        () = sleep(*delay) => {
            *delay = (*delay * 2).min(PUBLISH_RETRY_MAX_DELAY);
            true
        }
    }
}

/// Rate-limits recreate-driven re-publication so a signal storm cannot spin
/// the worker: triggers inside the quiet period double the delay up to a
/// cap, anything else starts over.
#[derive(Debug, Default)]
struct RecreateDebounce {
    last_trigger: Option<Instant>,
    delay: Duration,
}

impl RecreateDebounce {
    /// Backoff to wait before the next recreate-driven republish.
    fn next_delay(&mut self, now: Instant) -> Duration {
        self.delay = match self.last_trigger {
            Some(last) if now.duration_since(last) < RECREATE_QUIET_PERIOD => {
                (self.delay * 2).min(RECREATE_MAX_DELAY)
            }
            _ => RECREATE_FIRST_DELAY,
        };
        self.last_trigger = Some(now);
        self.delay
    }

    /// Clears the backoff so the next trigger starts from the first delay.
    fn reset(&mut self) {
        *self = Self::default();
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

    #[test]
    fn collision_appends_a_suffix() {
        assert_eq!(
            next_instance("aperture", "aperture", &RecreateReason::GroupCollision),
            "aperture-2"
        );
    }

    #[test]
    fn collision_bumps_the_taken_suffix() {
        assert_eq!(
            next_instance("aperture", "aperture-2", &RecreateReason::GroupCollision),
            "aperture-3"
        );
    }

    #[test]
    fn repeated_collisions_keep_bumping() {
        let mut instance = "aperture".to_owned();
        for expected in ["aperture-2", "aperture-3", "aperture-4", "aperture-5"] {
            instance = next_instance("aperture", &instance, &RecreateReason::GroupCollision);
            assert_eq!(instance, expected);
        }
    }

    #[test]
    fn collision_bumps_a_base_that_ends_in_a_number() {
        assert_eq!(
            next_instance("aperture-2", "aperture-2", &RecreateReason::GroupCollision),
            "aperture-3"
        );
    }

    #[test]
    fn collision_with_non_numeric_tail_falls_back_to_suffix_two() {
        assert_eq!(
            next_instance("my-host", "my-host", &RecreateReason::GroupCollision),
            "my-host-2"
        );
    }

    #[test]
    fn collision_truncates_a_max_length_base() {
        let base = "a".repeat(INSTANCE_LABEL_LIMIT);
        let instance = next_instance(&base, &base, &RecreateReason::GroupCollision);
        assert_eq!(instance, format!("{}-2", "a".repeat(58)));
        assert!(instance.len() <= INSTANCE_LABEL_LIMIT);
    }

    #[test]
    fn collision_bumping_past_the_budget_retruncates_the_prefix() {
        let prefix = "a".repeat(60);
        let instance = next_instance(
            "aperture",
            &format!("{prefix}-99"),
            &RecreateReason::GroupCollision,
        );
        assert_eq!(instance, format!("{}-100", "a".repeat(58)));
    }

    #[test]
    fn huge_suffix_rollover_stays_within_the_limit() {
        let prefix = "a".repeat(58);
        let instance = next_instance(
            "aperture",
            &format!("{prefix}-9999"),
            &RecreateReason::GroupCollision,
        );
        assert_eq!(instance.len(), INSTANCE_LABEL_LIMIT);
        assert_eq!(instance, format!("{}-10000", "a".repeat(57)));
    }

    #[test]
    fn repeated_collisions_on_a_max_base_stay_within_the_limit() {
        let base = "a".repeat(INSTANCE_LABEL_LIMIT);
        let mut instance = base.clone();
        for n in 2..=20_000u32 {
            instance = next_instance(&base, &instance, &RecreateReason::GroupCollision);
            assert!(instance.len() <= INSTANCE_LABEL_LIMIT, "n = {n}");
            assert!(instance.ends_with(&n.to_string()), "n = {n}");
        }
    }

    #[test]
    fn srv_host_uses_the_queried_fqdn() {
        let (host, mismatch) = srv_host("aperture.local", "aperture");
        assert_eq!(host, "aperture.local");
        assert!(!mismatch);
    }

    #[test]
    fn srv_host_flags_a_renamed_fqdn() {
        let (host, mismatch) = srv_host("aperture-2.local", "aperture");
        assert_eq!(host, "aperture-2.local");
        assert!(mismatch);
    }

    #[test]
    fn srv_host_flags_a_non_local_fqdn() {
        assert!(srv_host("aperture.example.com", "aperture").1);
    }

    #[test]
    fn server_restart_resets_to_the_hostname() {
        assert_eq!(
            next_instance("aperture", "aperture-7", &RecreateReason::Server),
            "aperture"
        );
    }

    #[test]
    fn group_failure_resets_to_the_hostname() {
        assert_eq!(
            next_instance("aperture", "aperture-3", &RecreateReason::GroupFailure),
            "aperture"
        );
    }

    #[test]
    fn debounce_doubles_under_back_to_back_triggers() {
        let start = Instant::now();
        let mut debounce = RecreateDebounce::default();
        assert_eq!(debounce.next_delay(start), RECREATE_FIRST_DELAY);
        assert_eq!(
            debounce.next_delay(start + Duration::from_millis(10)),
            RECREATE_FIRST_DELAY * 2
        );
        assert_eq!(
            debounce.next_delay(start + Duration::from_millis(20)),
            RECREATE_FIRST_DELAY * 4
        );
    }

    #[test]
    fn debounce_caps_at_the_max_delay() {
        let start = Instant::now();
        let mut debounce = RecreateDebounce::default();
        debounce.next_delay(start);
        let mut last = RECREATE_FIRST_DELAY;
        for n in 1..=10 {
            last = debounce.next_delay(start + Duration::from_secs(n));
        }
        assert_eq!(last, RECREATE_MAX_DELAY);
    }

    #[test]
    fn debounce_resets_after_a_quiet_period() {
        let start = Instant::now();
        let mut debounce = RecreateDebounce::default();
        debounce.next_delay(start);
        debounce.next_delay(start + Duration::from_millis(5));
        assert_eq!(
            debounce.next_delay(start + RECREATE_QUIET_PERIOD + Duration::from_secs(1)),
            RECREATE_FIRST_DELAY
        );
    }

    #[test]
    fn debounce_reset_restores_the_first_delay() {
        let start = Instant::now();
        let mut debounce = RecreateDebounce::default();
        debounce.next_delay(start);
        debounce.next_delay(start + Duration::from_millis(10));
        debounce.reset();
        assert_eq!(
            debounce.next_delay(start + Duration::from_millis(20)),
            RECREATE_FIRST_DELAY
        );
    }
}
