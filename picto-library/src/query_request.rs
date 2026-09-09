//! Bounded, per-consumer supersession of read-only grid requests. A generation
//! received before an older query starts also cancels that obsolete query.
//! Opening a library runs under the same scoped token with no deadline, so the
//! resident loads' existing check points serve open cancellation unchanged.
use crate::{LibraryError, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

// A scoped execution context lets the shared predicate/order primitives remain
// usable by mutations and sequential consumers. It never escapes the worker;
// parallel work explicitly captures the token instead of inheriting thread state.
thread_local! {
    static CURRENT: RefCell<Option<Cancellation>> = const { RefCell::new(None) };
}

pub(crate) fn current() -> Option<Cancellation> {
    CURRENT.with(|slot| slot.borrow().clone())
}

pub(crate) fn check() -> Result<()> {
    CURRENT.with(|slot| slot.borrow().as_ref().map_or(Ok(()), Cancellation::check))
}

/// Serialize under the same request deadline. Checking the writer also handles
/// unusually large strings without waiting for the whole JSON result to finish.
pub(crate) fn serialize<T: Serialize>(value: &T) -> Result<String> {
    struct CheckedWriter {
        bytes: Vec<u8>,
        until_check: usize,
    }
    impl std::io::Write for CheckedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.until_check == 0 || bytes.len() > self.until_check {
                check().map_err(std::io::Error::other)?;
                self.until_check = 64 * 1024;
            }
            self.until_check = self.until_check.saturating_sub(bytes.len());
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = CheckedWriter {
        bytes: Vec::new(),
        until_check: 0,
    };
    let result = serde_json::to_writer(&mut writer, value);
    check()?;
    result?;
    String::from_utf8(writer.bytes).map_err(|error| LibraryError::InvalidState(error.to_string()))
}

pub(crate) struct ExecutionGuard(Option<Cancellation>);
impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub channel: String,
    pub generation: u64,
}

#[derive(Default)]
pub(crate) struct WindowRequests(Mutex<Vec<(String, Arc<AtomicU64>)>>);

impl WindowRequests {
    pub(crate) fn begin(&self, request: &QueryRequest) -> Result<Cancellation> {
        if request.channel.is_empty() || request.channel.len() > 128 {
            return Err(LibraryError::InvalidInput(
                "invalid query request channel".into(),
            ));
        }
        let mut entries = self.0.lock();
        let latest =
            if let Some(index) = entries.iter().position(|(key, _)| *key == request.channel) {
                let entry = entries.remove(index);
                let latest = entry.1.clone();
                entries.push(entry);
                latest
            } else {
                if entries.len() >= 32 {
                    let index = entries
                        .iter()
                        .position(|(_, value)| Arc::strong_count(value) == 1)
                        .ok_or_else(|| {
                            LibraryError::InvalidInput("too many active query consumers".into())
                        })?;
                    entries.remove(index);
                }
                let latest = Arc::new(AtomicU64::new(request.generation));
                entries.push((request.channel.clone(), latest.clone()));
                latest
            };
        latest.fetch_max(request.generation, Ordering::Relaxed);
        Ok(Cancellation {
            latest,
            generation: request.generation,
            deadline: Some(Instant::now() + Duration::from_secs(10)),
            origin: Origin::Query,
        })
    }
}

/// A one-shot handle that cancels one library open. The shell keeps it while
/// the open runs and cancels it when the open is closed, replaced or the
/// process quits; a cancelled handle stays cancelled, so each open gets a
/// fresh one.
#[derive(Clone, Debug, Default)]
pub struct OpenCancellation(Arc<AtomicU64>);

impl OpenCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(1, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed) != 0
    }

    /// The raw flag, for SQLite progress handlers that must be `'static`.
    pub(crate) fn flag(&self) -> Arc<AtomicU64> {
        self.0.clone()
    }
}

/// What an open is doing, reported in order. `Loading` stages follow the
/// projection load: the checkpoint, the resident text, name, drawing and
/// navigation scans, validation, then only when the checkpoint is missing or
/// stale the canonical rebuild, smart-folder evaluation and partition repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenStage {
    Migrating,
    Loading(LoadStage),
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadStage {
    Checkpoint,
    Text,
    Names,
    Drawing,
    Navigation,
    Validation,
    Canonical,
    Smart,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Query,
    Open,
}

#[derive(Clone)]
pub(crate) struct Cancellation {
    latest: Arc<AtomicU64>,
    generation: u64,
    deadline: Option<Instant>,
    origin: Origin,
}

impl Cancellation {
    pub(crate) fn deadline() -> Self {
        Self {
            latest: Arc::new(AtomicU64::new(0)),
            generation: 0,
            deadline: Some(Instant::now() + Duration::from_secs(10)),
            origin: Origin::Query,
        }
    }

    /// A token for opening a library: no deadline, cancelled only by its handle.
    pub(crate) fn for_open(handle: &OpenCancellation) -> Self {
        Self {
            latest: handle.0.clone(),
            generation: 0,
            deadline: None,
            origin: Origin::Open,
        }
    }

    pub(crate) fn enter(&self) -> ExecutionGuard {
        ExecutionGuard(CURRENT.with(|slot| slot.replace(Some(self.clone()))))
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.latest.load(Ordering::Relaxed) != self.generation {
            Err(match self.origin {
                Origin::Query => LibraryError::QuerySuperseded,
                Origin::Open => LibraryError::OpenCancelled,
            })
        } else if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            Err(LibraryError::QueryDeadline)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_and_scoped_cancellation_do_not_leak_into_the_next_operation() {
        assert!(check().is_ok());
        let token = Cancellation {
            deadline: Some(Instant::now()),
            ..Cancellation::deadline()
        };
        {
            let _scope = token.enter();
            assert!(matches!(check(), Err(LibraryError::QueryDeadline)));
            assert!(std::thread::spawn(check).join().unwrap().is_ok());
            let fresh = Cancellation::deadline();
            {
                let _nested = fresh.enter();
                assert!(check().is_ok());
            }
            assert!(matches!(check(), Err(LibraryError::QueryDeadline)));
        }
        assert!(check().is_ok());
    }

    #[test]
    fn open_cancellation_has_no_deadline_and_reports_open_cancelled() {
        let handle = OpenCancellation::new();
        let token = Cancellation::for_open(&handle);
        assert!(token.check().is_ok());
        {
            let _scope = token.enter();
            assert!(check().is_ok());
            handle.cancel();
            assert!(matches!(check(), Err(LibraryError::OpenCancelled)));
            assert!(handle.is_cancelled());
            // Other threads and nested query scopes are unaffected.
            assert!(std::thread::spawn(check).join().unwrap().is_ok());
            let query = Cancellation::deadline();
            {
                let _nested = query.enter();
                assert!(check().is_ok());
            }
            assert!(matches!(check(), Err(LibraryError::OpenCancelled)));
        }
        assert!(check().is_ok());
        // A cancelled handle stays cancelled: a retry needs a fresh one.
        assert!(matches!(
            Cancellation::for_open(&handle).check(),
            Err(LibraryError::OpenCancelled)
        ));
        assert!(Cancellation::for_open(&OpenCancellation::new())
            .check()
            .is_ok());
    }

    #[test]
    fn supersession_interrupts_rust_work_without_any_sql_execution() {
        let requests = WindowRequests::default();
        let token = requests
            .begin(&QueryRequest {
                channel: "grid".into(),
                generation: 1,
            })
            .unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _scope = token.enter();
            started_tx.send(()).unwrap();
            loop {
                if let Err(error) = check() {
                    done_tx.send(error).unwrap();
                    break;
                }
                std::thread::yield_now();
            }
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        requests
            .begin(&QueryRequest {
                channel: "grid".into(),
                generation: 2,
            })
            .unwrap();
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            LibraryError::QuerySuperseded
        ));
        worker.join().unwrap();
    }

    #[test]
    fn a_late_arrival_cannot_replace_the_newer_generation_and_channels_are_independent() {
        let requests = WindowRequests::default();
        let old = QueryRequest {
            channel: "grid".into(),
            generation: 1,
        };
        let token = requests.begin(&old).unwrap();
        let _new = requests
            .begin(&QueryRequest {
                generation: 2,
                ..old.clone()
            })
            .unwrap();
        assert!(token.check().is_err());
        assert!(requests.begin(&old).unwrap().check().is_err());
        assert!(requests
            .begin(&QueryRequest {
                channel: "inspector".into(),
                generation: 1
            })
            .unwrap()
            .check()
            .is_ok());
    }
}
