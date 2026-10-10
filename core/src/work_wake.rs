//! Background workers sleep until the library publishes a change.
//!
//! Every queue a worker drains (ingest jobs, derivative and AI work, FTS dirty
//! rows, subscription runs) is filled by a published write, and every published
//! write reaches the renderer through the publication coordinator. Workers
//! therefore wait for the next publication instead of opening a writer
//! transaction on a fixed short period. An idle library used to start about a
//! hundred `BEGIN IMMEDIATE` transactions per second this way; now it starts a
//! handful per minute (the fallback probes below).
//!
//! The fallback covers work that becomes ready with time alone: retry backoff,
//! scheduled subscription runs, domain rate limits.

use std::time::Duration;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;

#[derive(Clone)]
pub struct WorkWake {
    revisions: watch::Receiver<u64>,
}

impl WorkWake {
    /// Subscribe once per runtime; clone the result for each worker.
    pub fn subscribe(application: &LibraryApplication) -> Self {
        let (sender, revisions) = watch::channel(0_u64);
        application
            .library()
            .publication()
            .subscribe(std::sync::Arc::new(move |event| {
                sender.send_replace(event.revision);
            }));
        Self { revisions }
    }

    /// Mark every publication so far as seen. Call before a worker drains its
    /// queue, so writes made during the drain wake it for one more pass.
    pub fn mark_seen(&mut self) {
        self.revisions.borrow_and_update();
    }

    /// Wait at least `pace` (to coalesce bursts), then until the library
    /// publishes again or `fallback` elapses. Returns false when cancelled.
    pub async fn next(
        &mut self,
        cancel: &CancellationToken,
        pace: Duration,
        fallback: Duration,
    ) -> bool {
        tokio::select! {
            _ = cancel.cancelled() => return false,
            _ = tokio::time::sleep(pace) => {}
        }
        tokio::select! {
            _ = cancel.cancelled() => false,
            _ = self.revisions.changed() => true,
            _ = tokio::time::sleep(fallback) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn workers_wake_on_a_publication_and_otherwise_wait_for_the_fallback() {
        let (sender, revisions) = watch::channel(0_u64);
        let mut wake = WorkWake { revisions };
        let cancel = CancellationToken::new();
        wake.mark_seen();

        let started = tokio::time::Instant::now();
        assert!(wake.next(&cancel, Duration::from_millis(10), Duration::from_secs(5)).await);
        assert!(started.elapsed() >= Duration::from_secs(5), "an idle worker waits for the fallback");

        wake.mark_seen();
        sender.send_replace(7);
        let started = tokio::time::Instant::now();
        assert!(wake.next(&cancel, Duration::from_millis(10), Duration::from_secs(5)).await);
        assert!(started.elapsed() < Duration::from_secs(1), "a publication wakes it after the pace");

        cancel.cancel();
        assert!(!wake.next(&cancel, Duration::from_millis(10), Duration::from_secs(5)).await);
    }
}
