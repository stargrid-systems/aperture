//! The bus link seam: an async byte stream plus a way to open it.
//!
//! The worker only talks to the [`BusLink`] abstraction and only obtains
//! links through a [`LinkFactory`]. Production wires a serial port in,
//! tests wire in-memory duplex streams, so the driver logic is tested
//! against real encoded frames either way.

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_serial::SerialPortBuilderExt;

/// An async byte stream to the bus.
///
/// This is the transport seam of the driver. Anything readable and
/// writable qualifies: the trait exists to name the requirement, every
/// conforming type implements it automatically.
pub trait BusLink: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> BusLink for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

/// Opens [`BusLink`] streams on demand.
///
/// The worker calls [`LinkFactory::open`] whenever it needs a link and
/// treats a failure as "the device is not there yet", retrying with
/// backoff.
pub trait LinkFactory: Send + Sync + 'static {
    /// The link type this factory produces.
    type Link: BusLink;

    /// Opens a link, or fails while the device is absent.
    fn open(&self) -> impl Future<Output = io::Result<Self::Link>> + Send;
}

/// The production [`LinkFactory`]: opens the serial port at `path`.
#[derive(Debug, Clone)]
pub struct SerialLinkFactory {
    path: PathBuf,
    baud: u32,
}

impl SerialLinkFactory {
    /// Builds a factory for the serial device at `path`.
    #[must_use]
    pub const fn new(path: PathBuf, baud: u32) -> Self {
        Self { path, baud }
    }
}

impl LinkFactory for SerialLinkFactory {
    type Link = tokio_serial::SerialStream;

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "opening the port is a fast blocking syscall, not worth spawn_blocking"
    )]
    async fn open(&self) -> io::Result<Self::Link> {
        open_serial(&self.path, self.baud)
    }
}

/// Opens the serial port at `path` as 8N1 with the default settings.
fn open_serial(path: &Path, baud: u32) -> io::Result<tokio_serial::SerialStream> {
    let name = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "serial device path is not valid UTF-8",
        )
    })?;
    tokio_serial::new(name, baud)
        .open_native_async()
        .map_err(io::Error::other)
}
