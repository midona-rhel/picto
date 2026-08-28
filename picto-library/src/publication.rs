use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::RootId;

pub const MAX_RECEIPT_IDS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationReceipt {
    pub revision: u64,
    pub resources: Vec<String>,
    pub item_ids: Vec<RootId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryChanged {
    pub revision: u64,
    pub resources: Vec<String>,
    pub item_ids: Vec<RootId>,
}

impl From<&MutationReceipt> for LibraryChanged {
    fn from(receipt: &MutationReceipt) -> Self {
        Self {
            revision: receipt.revision,
            resources: receipt.resources.clone(),
            item_ids: receipt.item_ids.clone(),
        }
    }
}

type Subscriber = Arc<dyn Fn(LibraryChanged) + Send + Sync>;

#[derive(Default)]
struct Pending {
    revision: u64,
    resources: BTreeSet<String>,
    item_ids: BTreeSet<RootId>,
    broad: bool,
}

#[derive(Default)]
pub struct PublicationCoordinator {
    pending: Mutex<Pending>,
    subscribers: Mutex<HashMap<u64, Subscriber>>,
    next_subscriber: Mutex<u64>,
}

impl PublicationCoordinator {
    pub fn receipt(
        revision: u64,
        resources: impl IntoIterator<Item = String>,
        roots: impl IntoIterator<Item = RootId>,
    ) -> MutationReceipt {
        let mut item_ids = roots
            .into_iter()
            .take(MAX_RECEIPT_IDS + 1)
            .collect::<Vec<_>>();
        if item_ids.len() > MAX_RECEIPT_IDS {
            item_ids.clear();
        }
        MutationReceipt {
            revision,
            resources: resources.into_iter().collect(),
            item_ids,
        }
    }

    pub fn register(&self, receipt: &MutationReceipt) {
        let mut pending = self.pending.lock();
        pending.revision = pending.revision.max(receipt.revision);
        pending.resources.extend(receipt.resources.iter().cloned());
        if receipt.item_ids.is_empty() {
            pending.broad = true;
            pending.item_ids.clear();
        } else if !pending.broad {
            pending.item_ids.extend(receipt.item_ids.iter().copied());
            if pending.item_ids.len() > MAX_RECEIPT_IDS {
                pending.broad = true;
                pending.item_ids.clear();
            }
        }
    }

    pub fn flush(&self) -> Option<LibraryChanged> {
        let event = {
            let mut pending = self.pending.lock();
            if pending.revision == 0 {
                return None;
            }
            let event = LibraryChanged {
                revision: pending.revision,
                resources: std::mem::take(&mut pending.resources).into_iter().collect(),
                item_ids: if pending.broad {
                    Vec::new()
                } else {
                    std::mem::take(&mut pending.item_ids).into_iter().collect()
                },
            };
            *pending = Pending::default();
            event
        };
        for subscriber in self.subscribers.lock().values() {
            subscriber(event.clone());
        }
        Some(event)
    }

    pub fn subscribe(&self, subscriber: Subscriber) -> u64 {
        let mut next = self.next_subscriber.lock();
        *next += 1;
        self.subscribers.lock().insert(*next, subscriber);
        *next
    }

    pub fn unsubscribe(&self, id: u64) {
        self.subscribers.lock().remove(&id);
    }
}
