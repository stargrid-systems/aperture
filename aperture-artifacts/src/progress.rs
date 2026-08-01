//! Bridging byte transfer into a task's progress reporter.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use aperture_tasks::ProgressHandle;
use tokio::io::AsyncWrite;

/// An [`AsyncWrite`] that forwards to `inner` and counts bytes into `progress`.
pub struct ProgressWriter<W> {
    inner: W,
    progress: ProgressHandle,
}

impl<W> ProgressWriter<W> {
    pub(crate) const fn new(inner: W, progress: ProgressHandle) -> Self {
        Self { inner, progress }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ProgressWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = ready!(Pin::new(&mut this.inner).poll_write(cx, buf))?;
        this.progress.add(written as u64);
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
