//! Avahi mDNS service publication via D-Bus.

// The zbus proxy macro generates methods that exceed clippy's argument limit.
#![expect(clippy::too_many_arguments)]

use std::future::{pending, poll_fn};
use std::pin::Pin;
use std::task::Poll;

use anyhow::Context;
use futures_util::Stream;
use zbus::zvariant::OwnedObjectPath;

use self::group::AvahiEntryGroupProxy;
use self::server::AvahiServerProxy;

const IF_UNSPEC: i32 = -1;
const PROTO_UNSPEC: i32 = -1;
const FLAGS_NONE: u32 = 0;

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
    /// The entry group collided or failed.
    Group,
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
    /// Creates an entry group, advertises `services` under `hostname`, and
    /// subscribes to the daemon and group state signals.
    ///
    /// With an empty `services` slice nothing is published and
    /// [`Self::next_recreate`] parks forever.
    ///
    /// # Errors
    ///
    /// Returns an error if any Avahi D-Bus call fails.
    pub async fn start(
        connection: &zbus::Connection,
        hostname: &str,
        services: &[ServiceSpec],
    ) -> anyhow::Result<Self> {
        let group_path = if services.is_empty() {
            None
        } else {
            Some(create_group(connection, hostname, services).await?)
        };
        let (server_states, group_states) = match &group_path {
            Some(path) => {
                let server = AvahiServerProxy::new(connection)
                    .await
                    .context("failed to create avahi server proxy")?;
                let server_states = server
                    .receive_state_changed()
                    .await
                    .context("failed to subscribe to avahi server state changes")?;
                let group = AvahiEntryGroupProxy::new(connection, path.clone())
                    .await
                    .context("failed to create entry group proxy")?;
                let group_states = group
                    .receive_state_changed()
                    .await
                    .context("failed to subscribe to entry group state changes")?;
                (Some(server_states), Some(group_states))
            }
            None => (None, None),
        };

        Ok(Self {
            connection: connection.clone(),
            group_path,
            server_states,
            group_states,
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

    /// Frees the entry group. A no-op when nothing was published.
    ///
    /// # Errors
    ///
    /// Returns an error if the Avahi D-Bus call fails.
    pub async fn free(&mut self) -> anyhow::Result<()> {
        let Some(path) = self.group_path.take() else {
            return Ok(());
        };
        let group = AvahiEntryGroupProxy::new(&self.connection, path)
            .await
            .context("failed to create entry group proxy")?;
        group
            .free_group()
            .await
            .context("failed to free entry group")?;
        Ok(())
    }
}

/// Creates, fills, and commits a new entry group.
async fn create_group(
    connection: &zbus::Connection,
    hostname: &str,
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
                hostname,
                &spec.service_type,
                "",
                "",
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
    if matches!(args.state, group_state::COLLISION | group_state::FAILURE) {
        tracing::debug!(state = args.state, error = %args.error, "avahi entry group state changed");
        Some(RecreateReason::Group)
    } else {
        None
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
