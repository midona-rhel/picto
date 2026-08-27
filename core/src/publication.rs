use std::collections::HashSet;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::app::{ItemId, LibraryChanged, MutationReceipt, LIBRARY_CHANGED_EVENT};

const COALESCE_WINDOW: Duration = Duration::from_millis(8);
const MAX_EVENT_RESOURCES: usize = 128;
const MAX_EVENT_ITEM_IDS: usize = 256;

type EventSink = Arc<dyn Fn(LibraryChanged) + Send + Sync>;

#[derive(Default)]
struct PendingPublication {
    revision: u64,
    resources: HashSet<String>,
    item_ids: HashSet<ItemId>,
    resources_overflowed: bool,
    item_ids_overflowed: bool,
}

impl PendingPublication {
    fn merge(&mut self, receipt: &MutationReceipt) {
        self.revision = self.revision.max(receipt.revision);

        if !self.resources_overflowed {
            self.resources.extend(receipt.resources.iter().cloned());
            if self.resources.len() > MAX_EVENT_RESOURCES {
                self.degrade_resources_to_library();
            }
        }

        if !self.item_ids_overflowed {
            self.item_ids.extend(receipt.item_ids.iter().copied());
            if self.item_ids.len() > MAX_EVENT_ITEM_IDS {
                self.item_ids.clear();
                self.item_ids_overflowed = true;
                self.insert_library_resource();
            }
        }
    }

    fn insert_library_resource(&mut self) {
        if self.resources_overflowed || self.resources.contains("library") {
            return;
        }
        if self.resources.len() == MAX_EVENT_RESOURCES {
            self.degrade_resources_to_library();
        } else {
            self.resources.insert("library".to_string());
        }
    }

    fn degrade_resources_to_library(&mut self) {
        self.resources.clear();
        self.resources.insert("library".to_string());
        self.resources_overflowed = true;
    }

    fn is_empty(&self) -> bool {
        self.resources.is_empty() && self.item_ids.is_empty()
    }

    fn take_event(&mut self) -> Option<LibraryChanged> {
        if self.is_empty() {
            return None;
        }

        let batch = std::mem::take(self);
        let mut resources: Vec<_> = batch.resources.into_iter().collect();
        let mut item_ids: Vec<_> = batch.item_ids.into_iter().collect();
        resources.sort();
        item_ids.sort_by_key(|item_id| item_id.0);
        Some(LibraryChanged {
            revision: batch.revision,
            resources,
            item_ids,
        })
    }
}

struct SharedState {
    pending: Mutex<PendingPublication>,
    #[cfg(test)]
    worker_starts: std::sync::atomic::AtomicUsize,
}

/// Owns revision watermarks and event coalescing, but no library data.
pub struct PublicationCoordinator {
    shared: Arc<SharedState>,
    wake_tx: Option<SyncSender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl PublicationCoordinator {
    pub fn new() -> Self {
        Self::with_sink(Arc::new(|event| {
            crate::events::emit(LIBRARY_CHANGED_EVENT, &event);
        }))
    }

    fn with_sink(sink: EventSink) -> Self {
        let shared = Arc::new(SharedState {
            pending: Mutex::new(PendingPublication::default()),
            #[cfg(test)]
            worker_starts: std::sync::atomic::AtomicUsize::new(0),
        });
        let (wake_tx, wake_rx) = mpsc::sync_channel(1);
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("picto-publication".to_string())
            .spawn(move || {
                #[cfg(test)]
                worker_shared
                    .worker_starts
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                while wake_rx.recv().is_ok() {
                    let deadline = Instant::now() + COALESCE_WINDOW;
                    loop {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        match wake_rx.recv_timeout(remaining) {
                            Ok(()) => {}
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }

                    let event = worker_shared
                        .pending
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take_event();
                    if let Some(event) = event {
                        sink(event);
                    }
                }

                // Dropping the coordinator may disconnect while a publication is pending.
                let event = worker_shared
                    .pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take_event();
                if let Some(event) = event {
                    sink(event);
                }
            })
            .expect("publication worker should start");

        Self {
            shared,
            wake_tx: Some(wake_tx),
            worker: Some(worker),
        }
    }

    pub fn submit(&self, receipt: &MutationReceipt) {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .merge(receipt);

        // One queued wake is sufficient because pending state already contains the union.
        if let Some(wake_tx) = &self.wake_tx {
            let _ = wake_tx.try_send(());
        }
    }
}

impl Drop for PublicationCoordinator {
    fn drop(&mut self) {
        self.wake_tx.take();
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != std::thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

impl Default for PublicationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use super::{PendingPublication, PublicationCoordinator, MAX_EVENT_RESOURCES};
    use crate::app::{ItemId, LibraryChanged, MutationReceipt};

    fn receipt(revision: u64, resources: Vec<String>, item_ids: Vec<ItemId>) -> MutationReceipt {
        MutationReceipt {
            revision,
            resources,
            item_ids,
        }
    }

    fn recording_coordinator() -> (PublicationCoordinator, mpsc::Receiver<LibraryChanged>) {
        let (tx, rx) = mpsc::channel();
        let coordinator = PublicationCoordinator::with_sink(Arc::new(move |event| {
            let _ = tx.send(event);
        }));
        (coordinator, rx)
    }

    #[test]
    fn pending_batch_deduplicates_and_sorts_resources_and_items() {
        let mut pending = PendingPublication::default();
        pending.merge(&receipt(
            7,
            vec!["sidebar".into(), "library".into()],
            vec![ItemId(42), ItemId(7)],
        ));
        pending.merge(&receipt(
            9,
            vec!["sidebar".into()],
            vec![ItemId(42), ItemId(3)],
        ));

        let event = pending.take_event().expect("event");
        assert_eq!(event.revision, 9);
        assert_eq!(event.resources, vec!["library", "sidebar"]);
        assert_eq!(event.item_ids, vec![ItemId(3), ItemId(7), ItemId(42)]);
        assert!(pending.is_empty());
    }

    #[test]
    fn oversized_item_and_resource_sets_degrade_to_library_invalidation() {
        let mut pending = PendingPublication::default();
        pending.merge(&receipt(
            3,
            (0..=MAX_EVENT_RESOURCES)
                .map(|index| format!("item:{index}"))
                .collect(),
            (0..300).map(ItemId).collect(),
        ));

        let event = pending.take_event().expect("event");
        assert_eq!(event.resources, vec!["library"]);
        assert!(event.item_ids.is_empty());

        pending.merge(&receipt(4, vec!["tags".into()], vec![ItemId(8), ItemId(9)]));
        let next_event = pending.take_event().expect("next event");
        assert_eq!(next_event.resources, vec!["tags"]);
        assert_eq!(next_event.item_ids, vec![ItemId(8), ItemId(9)]);
    }

    #[test]
    fn one_worker_coalesces_each_burst_into_one_event() {
        let (coordinator, rx) = recording_coordinator();
        coordinator.submit(&receipt(1, vec!["sidebar".into()], vec![ItemId(2)]));
        coordinator.submit(&receipt(2, vec!["tags".into()], vec![ItemId(1)]));

        let first = rx
            .recv_timeout(Duration::from_millis(100))
            .expect("first coalesced event");
        assert_eq!(first.revision, 2);
        assert_eq!(first.resources, vec!["sidebar", "tags"]);
        assert_eq!(first.item_ids, vec![ItemId(1), ItemId(2)]);
        assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());

        coordinator.submit(&receipt(3, vec!["folders".into()], vec![ItemId(9)]));
        let second = rx
            .recv_timeout(Duration::from_millis(100))
            .expect("second coalesced event");
        assert_eq!(second.revision, 3);
        assert_eq!(second.resources, vec!["folders"]);
        assert_eq!(second.item_ids, vec![ItemId(9)]);
        assert_eq!(coordinator.shared.worker_starts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn drop_flushes_a_pending_publication() {
        let (coordinator, rx) = recording_coordinator();
        coordinator.submit(&receipt(11, vec!["library".into()], vec![ItemId(4)]));
        drop(coordinator);

        let event = rx
            .recv_timeout(Duration::from_millis(100))
            .expect("publication flushed during drop");
        assert_eq!(event.revision, 11);
        assert_eq!(event.resources, vec!["library"]);
        assert_eq!(event.item_ids, vec![ItemId(4)]);
    }
}
