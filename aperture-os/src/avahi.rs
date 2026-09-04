//! Avahi mDNS service publication via D-Bus.

// The zbus proxy macro generates methods that exceed clippy's argument limit.
#![expect(clippy::too_many_arguments)]

use anyhow::Context;
use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

const IF_UNSPEC: i32 = -1;
const PROTO_UNSPEC: i32 = -1;
const FLAGS_NONE: u32 = 0;

#[proxy(
    interface = "org.freedesktop.Avahi.Server",
    default_service = "org.freedesktop.Avahi",
    default_path = "/"
)]
trait AvahiServer {
    fn entry_group_new(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.Avahi.EntryGroup",
    default_service = "org.freedesktop.Avahi"
)]
trait AvahiEntryGroup {
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

    fn reset(&self) -> zbus::Result<()>;

    #[zbus(name = "Free")]
    fn free_group(&self) -> zbus::Result<()>;
}

#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub service_type: String,
    pub port: u16,
}

pub struct ServicePublisher {
    connection: zbus::Connection,
    group_path: OwnedObjectPath,
}

impl ServicePublisher {
    pub async fn start(
        connection: &zbus::Connection,
        hostname: &str,
        services: &[ServiceSpec],
    ) -> anyhow::Result<Self> {
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

        Ok(Self {
            connection: connection.clone(),
            group_path,
        })
    }

    pub async fn free(&self) -> anyhow::Result<()> {
        let group = AvahiEntryGroupProxy::new(&self.connection, self.group_path.clone())
            .await
            .context("failed to create entry group proxy")?;
        group
            .free_group()
            .await
            .context("failed to free entry group")?;
        Ok(())
    }
}
