//! A generic batching worker driver.
//!
//! [`run_batched`] drains a channel into batches and hands each batch to a
//! [`BatchSink`]: when the batch reaches its size limit, when the flush
//! interval elapses, or when the worker is asked to stop. On stop it first
//! closes the channel, then drains whatever is still queued, flushes, and
//! runs the sink's shutdown hook.
//!
//! Both the log worker and the event recorder are built on this driver.

use std::future::Future;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};

use crate::Stop;

/// Consumes batches of items drained from a channel.
///
/// A sink owns the destination (a repository, a writer, ...) and decides
/// what a flush means. Items are handed over by draining `batch`; the buffer
/// is reused across flushes.
pub trait BatchSink<T>: Send + 'static {
    /// Disposes of one batch.
    fn flush(&mut self, batch: &mut Vec<T>) -> impl Future<Output = ()> + Send;

    /// Runs once after the final flush, on shutdown or channel close.
    fn shutdown(&mut self) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// Drains `rx` through `sink` until `stop` fires or the channel closes.
///
/// Flushes when a batch reaches `flush_batch` items, when `flush_interval`
/// elapses with a pending batch, and once more after the queue is fully
/// drained on exit. The queue is not buffered anywhere else.
/// The driver itself never discards items: every drained item is handed to
/// the sink, and the sink owns what a failed flush means. On stop the
/// channel is closed before the final drain, so a send that races the
/// shutdown fails instead of buffering an item nobody reads.
pub async fn run_batched<T, S>(
    mut rx: mpsc::Receiver<T>,
    stop: Stop,
    flush_interval: Duration,
    flush_batch: usize,
    mut sink: S,
) -> S
where
    T: Send + 'static,
    S: BatchSink<T>,
{
    let mut batch: Vec<T> = Vec::with_capacity(flush_batch);
    let mut interval = interval(flush_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => {
                // Close before draining: a send that lands in the buffer
                // after the drain loop exits would be lost when the
                // receiver drops. A late sender gets a SendError instead.
                rx.close();
                while let Ok(item) = rx.try_recv() {
                    batch.push(item);
                }
                sink.flush(&mut batch).await;
                sink.shutdown().await;
                return sink;
            }
            maybe_item = rx.recv() => {
                if let Some(item) = maybe_item {
                    batch.push(item);
                } else {
                    sink.flush(&mut batch).await;
                    sink.shutdown().await;
                    return sink;
                }
                if batch.len() >= flush_batch {
                    sink.flush(&mut batch).await;
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    sink.flush(&mut batch).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::mem::take;
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};
    use tokio::time::sleep;
    use tokio_util::sync::CancellationToken;

    use super::*;

    /// Records every flushed batch and every shutdown call.
    struct Collector {
        batches: Vec<Vec<u32>>,
        shutdowns: usize,
    }

    impl BatchSink<u32> for Collector {
        async fn flush(&mut self, batch: &mut Vec<u32>) {
            if batch.is_empty() {
                return;
            }
            self.batches.push(take(batch));
        }

        async fn shutdown(&mut self) {
            self.shutdowns += 1;
        }
    }

    fn driver(rx: mpsc::Receiver<u32>, stop: Stop) -> impl Future<Output = Collector> + Send {
        run_batched(
            rx,
            stop,
            Duration::from_secs(3600),
            3,
            Collector {
                batches: Vec::new(),
                shutdowns: 0,
            },
        )
    }

    /// A sink whose first flush signals a oneshot and then parks until
    /// released, so a test can observe and hold the drain flush.
    struct Gated {
        batches: Vec<Vec<u32>>,
        flush_started: Option<oneshot::Sender<()>>,
        release: Option<oneshot::Receiver<()>>,
    }

    impl BatchSink<u32> for Gated {
        async fn flush(&mut self, batch: &mut Vec<u32>) {
            if batch.is_empty() {
                return;
            }
            self.batches.push(take(batch));
            if let Some(started) = self.flush_started.take() {
                let _ = started.send(());
                if let Some(release) = self.release.take() {
                    let _ = release.await;
                }
            }
        }
    }

    #[tokio::test]
    async fn flushes_when_batch_is_full() {
        let (tx, rx) = mpsc::channel(16);
        let stop = CancellationToken::new();
        let task = tokio::spawn(driver(rx, stop.clone()));

        for i in 0..3 {
            tx.send(i).await.unwrap();
        }
        sleep(Duration::from_millis(50)).await;

        stop.cancel();
        let sink = task.await.unwrap();
        assert_eq!(sink.batches, vec![vec![0, 1, 2]]);
        assert_eq!(sink.shutdowns, 1);
    }

    #[tokio::test]
    async fn stop_drains_queue_and_flushes() {
        let (tx, rx) = mpsc::channel(16);
        let stop = CancellationToken::new();
        let task = tokio::spawn(driver(rx, stop.clone()));

        tx.send(7).await.unwrap();
        tx.send(8).await.unwrap();
        sleep(Duration::from_millis(50)).await;

        stop.cancel();
        let sink = task.await.unwrap();
        // The two queued items never reached the batch size and no interval
        // tick fired: only the stop drain flushed them.
        assert_eq!(sink.batches, vec![vec![7, 8]]);
        assert_eq!(sink.shutdowns, 1);
    }

    #[tokio::test]
    async fn stop_closes_the_channel_before_the_drain_flush() {
        let (tx, rx) = mpsc::channel(8);
        let stop = CancellationToken::new();
        let (flush_started, started) = oneshot::channel();
        let (release, gate) = oneshot::channel();

        tx.send(0).await.unwrap();
        tx.send(1).await.unwrap();

        let task = tokio::spawn(run_batched(
            rx,
            stop.clone(),
            Duration::from_secs(3600),
            3,
            Gated {
                batches: Vec::new(),
                flush_started: Some(flush_started),
                release: Some(gate),
            },
        ));
        // No await between spawn and cancel: on the current-thread runtime
        // the worker cannot poll before stop is cancelled, so its first
        // (biased) select poll takes the stop branch and drains both
        // buffered items in one batch.
        stop.cancel();

        // The drain flush began, so close() has already happened.
        started.await.unwrap();
        let err = tx.send(2).await.unwrap_err();
        assert_eq!(err.0, 2);

        release.send(()).unwrap();
        let sink = task.await.unwrap();
        assert_eq!(sink.batches, vec![vec![0, 1]]);
    }

    #[tokio::test]
    async fn channel_close_flushes_and_shuts_down() {
        let (tx, rx) = mpsc::channel(16);
        let stop = CancellationToken::new();
        let task = tokio::spawn(driver(rx, stop.clone()));

        tx.send(1).await.unwrap();
        drop(tx);

        let sink = task.await.unwrap();
        assert_eq!(sink.batches, vec![vec![1]]);
        assert_eq!(sink.shutdowns, 1);
        assert!(!stop.is_cancelled());
    }

    #[tokio::test]
    async fn interval_flushes_pending_batch() {
        let (tx, rx) = mpsc::channel(16);
        let stop = CancellationToken::new();
        let task = tokio::spawn(run_batched(
            rx,
            stop.clone(),
            Duration::from_millis(20),
            100,
            Collector {
                batches: Vec::new(),
                shutdowns: 0,
            },
        ));

        tx.send(5).await.unwrap();
        sleep(Duration::from_millis(100)).await;
        stop.cancel();
        let sink = task.await.unwrap();
        assert_eq!(sink.batches, vec![vec![5]]);
    }
}
