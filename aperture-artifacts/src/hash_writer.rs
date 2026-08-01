//! Async writer that hashes bytes inline.

use std::pin::Pin;
use std::task::{Context, Poll, ready};

use aperture_storage::{Digest, DigestAlgorithm};
use sha2::{Digest as _, Sha256};
use tokio::io::{self, AsyncWrite, AsyncWriteExt as _};

/// An [`AsyncWrite`] that hashes bytes as they pass through.
pub struct HashWriter<W> {
    inner: W,
    hasher: Sha256,
    total: u64,
}

impl<W: AsyncWrite + Unpin> HashWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            total: 0,
        }
    }

    /// Flushes `inner` and returns the computed digest and byte count.
    pub(crate) async fn finalize(mut self) -> io::Result<(Digest, u64)> {
        self.shutdown().await?;
        let hash = self.hasher.finalize();
        Ok((
            Digest::from_hash(DigestAlgorithm::Sha256, &hash),
            self.total,
        ))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for HashWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = ready!(Pin::new(&mut this.inner).poll_write(cx, buf))?;
        this.hasher.update(&buf[..written]);
        this.total += written as u64;
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
