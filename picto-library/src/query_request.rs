//! Bounded, per-consumer supersession of read-only grid requests. A generation
//! received before an older query starts also cancels that obsolete query.
use crate::{LibraryError, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

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
        })
    }
}

pub(crate) struct Cancellation {
    latest: Arc<AtomicU64>,
    generation: u64,
}

impl Cancellation {
    pub(crate) fn check(&self) -> Result<()> {
        if self.latest.load(Ordering::Relaxed) != self.generation {
            Err(LibraryError::QuerySuperseded)
        } else {
            Ok(())
        }
    }

    pub(crate) fn install<'a>(
        &self,
        connection: &'a rusqlite::Connection,
    ) -> Result<ProgressGuard<'a>> {
        let latest = self.latest.clone();
        let generation = self.generation;
        connection.progress_handler(
            1000,
            Some(move || latest.load(Ordering::Relaxed) != generation),
        )?;
        Ok(ProgressGuard(connection))
    }
}

pub(crate) struct ProgressGuard<'a>(&'a rusqlite::Connection);
impl Drop for ProgressGuard<'_> {
    fn drop(&mut self) {
        self.0
            .progress_handler(0, None::<fn() -> bool>)
            .expect("progress guard retains an owned connection");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_destination_interrupts_a_query_already_executing() {
        let requests = Arc::new(WindowRequests::default());
        let worker_requests = requests.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let connection = rusqlite::Connection::open_in_memory().unwrap();
            connection
                .create_scalar_function(
                    "started",
                    0,
                    rusqlite::functions::FunctionFlags::SQLITE_UTF8,
                    move |_| {
                        started_tx.send(()).unwrap();
                        resume_rx.recv().unwrap();
                        Ok(1i64)
                    },
                )
                .unwrap();
            let token = worker_requests
                .begin(&QueryRequest {
                    channel: "grid".into(),
                    generation: 1,
                })
                .unwrap();
            let guard = token.install(&connection).unwrap();
            let result = connection.query_row(
                "WITH RECURSIVE n(x) AS (SELECT started() UNION ALL SELECT x+1 FROM n WHERE x<1000000) SELECT sum(x) FROM n",
                [], |row| row.get::<_, i64>(0),
            );
            assert!(result.is_err());
            assert!(token.check().is_err());
            drop(guard);
            assert_eq!(
                connection
                    .query_row("SELECT 42", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                42
            );
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        requests
            .begin(&QueryRequest {
                channel: "grid".into(),
                generation: 2,
            })
            .unwrap();
        resume_tx.send(()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn supersession_interrupts_sql_and_cleans_up_the_connection() {
        let requests = WindowRequests::default();
        let old = QueryRequest {
            channel: "grid".into(),
            generation: 1,
        };
        let token = requests.begin(&old).unwrap();
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        let guard = token.install(&connection).unwrap();
        let _new = requests
            .begin(&QueryRequest {
                generation: 2,
                ..old.clone()
            })
            .unwrap();
        assert!(token.check().is_err());
        // A late arrival must not replace the newer generation.
        assert!(requests.begin(&old).unwrap().check().is_err());
        assert!(connection.query_row(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000000) SELECT sum(x) FROM n",
            [], |row| row.get::<_, i64>(0),
        ).is_err());
        drop(guard);
        assert_eq!(
            connection
                .query_row("SELECT 42", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            42
        );
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
