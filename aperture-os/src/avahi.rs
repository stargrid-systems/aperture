//! Avahi mDNS service publication via D-Bus.

// The zbus proxy macro generates methods that exceed clippy's argument limit.
#![expect(clippy::too_many_arguments)]

use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

use crate::Connection;
use crate::error::OsError;

/// Avahi interface wildcard: any interface.
const IF_UNSPEC: i32 = -1;
/// Avahi protocol wildcard: any protocol (IPv4 + IPv6).
const PROTO_UNSPEC: i32 = -1;
/// No Avahi publishing flags.
const FLAGS_NONE: u32 = 0;

#[proxy(
    interface = "org.freedesktop.Avahi.Server",
    default_service = "org.freedesktop.Avahi",
    default_path = "/"
)]
trait AvahiServer {
    /// Creates a new entry group and returns its object path.
    fn entry_group_new(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.Avahi.EntryGroup",
    default_service = "org.freedesktop.Avahi"
)]
trait AvahiEntryGroup {
    /// Adds a service to the group.
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

    /// Commits pending changes so services become visible on the network.
    fn commit(&self) -> zbus::Result<()>;

    /// Removes all entries from the group without freeing it.
    fn reset(&self) -> zbus::Result<()>;

    /// Frees the entry group on the server side, withdrawing all services.
    #[zbus(name = "Free")]
    fn free_group(&self) -> zbus::Result<()>;
}

/// Describes a single mDNS service to publish.
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    /// Service type, e.g. `_https._tcp`.
    pub service_type: String,
    /// Port the service listens on.
    pub port: u16,
}

/// Publishes mDNS services via Avahi.
///
/// The entry group is held alive by the underlying D-Bus connection. Call
/// [`free`](Self::free) during shutdown to explicitly withdraw services.
pub struct ServicePublisher {
    connection: Connection,
    group_path: OwnedObjectPath,
}

impl ServicePublisher {
    /// Creates a new Avahi entry group and publishes the given services.
    ///
    /// An empty `host` field is used so services follow the system hostname
    /// automatically.
    ///
    /// # Errors
    ///
    /// Returns `OsError::Dbus` if the Avahi D-Bus call fails.
    pub async fn start(
        connection: &Connection,
        hostname: &str,
        services: &[ServiceSpec],
    ) -> Result<Self, OsError> {
        let zbus = connection.inner();
        let server = AvahiServerProxy::new(zbus).await?;
        let group_path = server.entry_group_new().await?;
        let group = AvahiEntryGroupProxy::new(zbus, group_path.clone()).await?;

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
                .await?;
        }
        group.commit().await?;

        Ok(Self {
            connection: connection.clone(),
            group_path,
        })
    }

    /// Frees the entry group, withdrawing all published services.
    ///
    /// # Errors
    ///
    /// Returns `OsError::Dbus` if the Avahi D-Bus call fails.
    pub async fn free(&self) -> Result<(), OsError> {
        let group =
            AvahiEntryGroupProxy::new(self.connection.inner(), self.group_path.clone()).await?;
        group.free_group().await?;
        Ok(())
    }
}
