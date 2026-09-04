//! Avahi mDNS service publication via D-Bus.

// The zbus proxy macro generates methods that exceed clippy's argument limit.
#![expect(clippy::too_many_arguments)]

use std::error::Error as StdError;
use std::future::{pending, poll_fn};
use std::pin::Pin;
use std::task::{Poll, Waker};
use std::time::Duration;

use anyhow::Context;
use futures_util::Stream;
use tokio::time::timeout;
use zbus::zvariant::OwnedObjectPath;

use self::group::AvahiEntryGroupProxy;
use self::server::AvahiServerProxy;

const IF_UNSPEC: i32 = -1;
const PROTO_UNSPEC: i32 = -1;
const FLAGS_NONE: u32 = 0;

/// Upper bound for freeing the entry group so a dead D-Bus object cannot
/// stall shutdown or re-publication.
const FREE_TIMEOUT: Duration = Duration::from_secs(5);

/// Avahi server states that invalidate a published entry group: a fresh
/// daemon instance has no groups, and collision/failure stop resolution.
mod server_state {
    pub const RUNNING: i32 = 2;
    pub const COLLISION: i32 = 3;
    pub const FAILURE: i32 = 4;
}

/// Avahi entry group states that stop a group from resolving.
mod group_state {
    pub const COLLISION: i32 = 3;
    pub const FAILURE: i32 = 4;
}

mod server {
    use zbus::proxy;
    use zbus::zvariant::OwnedObjectPath;

    #[proxy(
        interface = "org.freedesktop.Avahi.Server",
        default_service = "org.freedesktop.Avahi",
        default_path = "/"
    )]
    pub(super) trait AvahiServer {
        fn entry_group_new(&self) -> zbus::Result<OwnedObjectPath>;

        #[zbus(signal)]
        fn state_changed(&self, state: i32, error: String) -> zbus::Result<()>;
    }
}

mod group {
    use zbus::proxy;

    #[proxy(
        interface = "org.freedesktop.Avahi.EntryGroup",
        default_service = "org.freedesktop.Avahi"
    )]
    pub(super) trait AvahiEntryGroup {
        fn add_service(
            &self,
            interface: i32,
            protocol: i32,
            flags: u32,
            name: &str,
            type_: &str,
            domain: &str,
            host: &str,
            port: u16,
            txt: Vec<Vec<u8>>,
        ) -> zbus::Result<()>;

        fn commit(&self) -> zbus::Result<()>;

        #[zbus(name = "Free")]
        fn free_group(&self) -> zbus::Result<()>;

        #[zbus(signal)]
        fn state_changed(&self, state: i32, error: String) -> zbus::Result<()>;
    }
}

#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub service_type: String,
    pub port: u16,
}

/// Reason the current advertisement must be torn down and re-created.
#[derive(Debug)]
pub enum RecreateReason {
    /// The daemon restarted, or reported a hostname collision or failure.
    Server,
    /// The entry group collided: another host owns the service instance
    /// name.
    GroupCollision,
    /// The entry group failed.
    GroupFailure,
}

/// Publishes services on the local network via a single Avahi entry group.
///
/// Monitors the daemon and the group so the advertisement can be re-created
/// after a daemon restart or a name collision. Created with
/// [`ServicePublisher::start`], freed with [`ServicePublisher::free`].
pub struct ServicePublisher {
    connection: zbus::Connection,
    group_path: Option<OwnedObjectPath>,
    server_states: Option<server::StateChangedStream>,
    group_states: Option<group::StateChangedStream>,
}

impl ServicePublisher {
    /// Creates an entry group and subscribes to the daemon and group state
    /// signals.
    ///
    /// The group advertises each service under the service instance name
    /// `instance` and targets the mDNS host `host` (e.g. `aperture.local`),
    /// which should match the certificate SANs. The daemon state stream is
    /// subscribed before the group is created so a daemon restart during
    /// publication is still observed; the group stream can only be
    /// subscribed after the group exists.
    ///
    /// With an empty `services` slice nothing is published and
    /// [`Self::next_recreate`] parks forever.
    ///
    /// # Errors
    ///
    /// Returns an error if any Avahi D-Bus call fails.
    pub async fn start(
        connection: &zbus::Connection,
        instance: &str,
        host: &str,
        services: &[ServiceSpec],
    ) -> anyhow::Result<Self> {
        if services.is_empty() {
            return Ok(Self {
                connection: connection.clone(),
                group_path: None,
                server_states: None,
                group_states: None,
            });
        }

        let server = AvahiServerProxy::new(connection)
            .await
            .context("failed to create avahi server proxy")?;
        let server_states = server
            .receive_state_changed()
            .await
            .context("failed to subscribe to avahi server state changes")?;

        let group_path = create_group(connection, instance, host, services).await?;
        let group = AvahiEntryGroupProxy::new(connection, group_path.clone())
            .await
            .context("failed to create entry group proxy")?;
        let group_states = group
            .receive_state_changed()
            .await
            .context("failed to subscribe to entry group state changes")?;

        Ok(Self {
            connection: connection.clone(),
            group_path: Some(group_path),
            server_states: Some(server_states),
            group_states: Some(group_states),
        })
    }

    /// Resolves when the advertisement must be torn down and re-created:
    /// the daemon restarted or reported a collision/failure, or the entry
    /// group collided or failed. Parks forever when nothing is advertised.
    pub async fn next_recreate(&mut self) -> RecreateReason {
        loop {
            let reason = match (&mut self.server_states, self.group_states.as_mut()) {
                (Some(server_states), Some(group_states)) => {
                    tokio::select! {
                        signal = next_signal(server_states) => server_recreate_reason(&signal),
                        signal = next_signal(group_states) => group_recreate_reason(&signal),
                    }
                }
                (Some(server_states), None) => {
                    server_recreate_reason(&next_signal(server_states).await)
                }
                (None, _) => pending().await,
            };
            if let Some(reason) = reason {
                return reason;
            }
        }
    }

    /// Drops recreate reasons already buffered by the state streams.
    ///
    /// Bounded: each stream is polled once without waiting. Used after a
    /// deliberate re-publication to discard triggers that predate the fresh
    /// advertisement.
    pub fn drain_stale(&mut self) {
        if let Some(states) = self.server_states.as_mut() {
            while let Some(signal) = try_next_signal(states) {
                if server_recreate_reason(&signal).is_some() {
                    tracing::debug!("dropped stale avahi server state signal");
                }
            }
        }
        if let Some(states) = self.group_states.as_mut() {
            while let Some(signal) = try_next_signal(states) {
                if group_recreate_reason(&signal).is_some() {
                    tracing::debug!("dropped stale avahi group state signal");
                }
            }
        }
    }

    /// Frees the entry group. A no-op when nothing was published.
    ///
    /// Bounded by a short timeout and tolerant of stale targets: a daemon
    /// that restarted since publication has no group left to free.
    ///
    /// # Errors
    ///
    /// Returns an error if the Avahi D-Bus call fails.
    pub async fn free(&mut self) -> anyhow::Result<()> {
        let Some(path) = self.group_path.take() else {
            return Ok(());
        };
        let group = match AvahiEntryGroupProxy::new(&self.connection, path).await {
            Ok(group) => group,
            Err(err) => {
                tracing::debug!(error = &err as &dyn StdError, "entry group already gone");
                return Ok(());
            }
        };
        match timeout(FREE_TIMEOUT, group.free_group()).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) if is_stale_group(&err) => {
                tracing::debug!(error = &err as &dyn StdError, "entry group already gone");
                Ok(())
            }
            Ok(Err(err)) => Err(err).context("failed to free entry group"),
            Err(_) => {
                tracing::debug!(timeout = ?FREE_TIMEOUT, "timed out freeing entry group");
                Ok(())
            }
        }
    }
}

/// Creates, fills, and commits a new entry group.
async fn create_group(
    connection: &zbus::Connection,
    instance: &str,
    host: &str,
    services: &[ServiceSpec],
) -> anyhow::Result<OwnedObjectPath> {
    let server = AvahiServerProxy::new(connection)
        .await
        .context("failed to create avahi server proxy")?;
    let group_path = server
        .entry_group_new()
        .await
        .context("failed to create entry group")?;
    let group = AvahiEntryGroupProxy::new(connection, group_path.clone())
        .await
        .context("failed to create entry group proxy")?;

    for spec in services {
        group
            .add_service(
                IF_UNSPEC,
                PROTO_UNSPEC,
                FLAGS_NONE,
                instance,
                &spec.service_type,
                "",
                host,
                spec.port,
                vec![],
            )
            .await
            .context("failed to add service")?;
    }
    group
        .commit()
        .await
        .context("failed to commit entry group")?;

    Ok(group_path)
}

/// Maps a daemon state change to a re-publish trigger. `None` for states
/// that leave the advertisement intact.
fn server_recreate_reason(signal: &server::StateChanged) -> Option<RecreateReason> {
    let Ok(args) = signal.args() else {
        return None;
    };
    if matches!(
        args.state,
        server_state::RUNNING | server_state::COLLISION | server_state::FAILURE
    ) {
        tracing::debug!(state = args.state, error = %args.error, "avahi server state changed");
        Some(RecreateReason::Server)
    } else {
        None
    }
}

/// Maps an entry group state change to a re-publish trigger. `None` for
/// states that leave the advertisement intact.
fn group_recreate_reason(signal: &group::StateChanged) -> Option<RecreateReason> {
    let Ok(args) = signal.args() else {
        return None;
    };
    let reason = match args.state {
        group_state::COLLISION => RecreateReason::GroupCollision,
        group_state::FAILURE => RecreateReason::GroupFailure,
        _ => return None,
    };
    tracing::debug!(state = args.state, error = %args.error, "avahi entry group state changed");
    Some(reason)
}

/// True for errors meaning the entry group no longer exists: a daemon
/// restart dropped it, so there is nothing left to free.
fn is_stale_group(err: &zbus::Error) -> bool {
    const INVALID_OBJECT: &str = "org.freedesktop.Avahi.InvalidObjectError";
    const UNKNOWN_OBJECT: &str = "org.freedesktop.DBus.Error.UnknownObject";
    const SERVICE_UNKNOWN: &str = "org.freedesktop.DBus.Error.ServiceUnknown";

    match err {
        zbus::Error::MethodError(name, ..) => matches!(
            name.as_str(),
            INVALID_OBJECT | UNKNOWN_OBJECT | SERVICE_UNKNOWN
        ),
        _ => false,
    }
}

/// Awaits the next signal from `stream`.
///
/// Parks forever once the stream terminates: with the D-Bus connection gone
/// nothing more can arrive, and shutdown handles cleanup.
async fn next_signal<S>(stream: &mut S) -> S::Item
where
    S: Stream + Unpin,
{
    poll_fn(|cx| match Pin::new(&mut *stream).poll_next(cx) {
        Poll::Ready(Some(item)) => Poll::Ready(item),
        Poll::Ready(None) | Poll::Pending => Poll::Pending,
    })
    .await
}

/// Non-blocking sibling of [`next_signal`]: returns only buffered items.
fn try_next_signal<S>(stream: &mut S) -> Option<S::Item>
where
    S: Stream + Unpin,
{
    match Pin::new(&mut *stream).poll_next(&mut std::task::Context::from_waker(Waker::noop())) {
        Poll::Ready(item) => item,
        Poll::Pending => None,
    }
}
