use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use super::SqliteDatabase;

const SLOW_READ_WARN_MS: u64 = 100;
const SLOW_WRITE_WARN_MS: u64 = 100;
const SLOW_TX_WARN_MS: u64 = 200;
const SLOW_QUERY_LOG_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy)]
struct SlowQueryWindow {
    started_at: Instant,
    count: u64,
    max_elapsed_ms: u64,
    max_label: &'static str,
}

static SLOW_QUERY_WINDOWS: OnceLock<StdMutex<HashMap<&'static str, SlowQueryWindow>>> =
    OnceLock::new();

fn record_slow_query(kind: &'static str, label: &'static str, elapsed_ms: u64) {
    let windows = SLOW_QUERY_WINDOWS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut guard = crate::poison::mutex_or_recover(windows, "sqlite::slow_query_windows");
    let now = Instant::now();

    match guard.get_mut(kind) {
        Some(window) if now.duration_since(window.started_at) <= SLOW_QUERY_LOG_WINDOW => {
            window.count += 1;
            if elapsed_ms >= window.max_elapsed_ms {
                window.max_elapsed_ms = elapsed_ms;
                window.max_label = label;
            }
        }
        Some(window) => {
            if window.count > 1 {
                tracing::warn!(
                    kind,
                    label = window.max_label,
                    suppressed = window.count - 1,
                    max_elapsed_ms = window.max_elapsed_ms,
                    window_ms = SLOW_QUERY_LOG_WINDOW.as_millis() as u64,
                    "suppressed repeated slow sqlite queries"
                );
            }
            *window = SlowQueryWindow {
                started_at: now,
                count: 1,
                max_elapsed_ms: elapsed_ms,
                max_label: label,
            };
            tracing::warn!(kind, label, elapsed_ms, "slow sqlite query");
        }
        None => {
            guard.insert(
                kind,
                SlowQueryWindow {
                    started_at: now,
                    count: 1,
                    max_elapsed_ms: elapsed_ms,
                    max_label: label,
                },
            );
            tracing::warn!(kind, label, elapsed_ms, "slow sqlite query");
        }
    }
}

impl SqliteDatabase {
    /// Run a read-only closure on a pooled reader connection.
    /// Uses round-robin to spread reads across the pool.
    /// Reader connections are opened with SQLITE_OPEN_READ_ONLY — writes will fail.
    pub async fn with_read_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.with_read_conn_labeled("unlabeled_read", f).await
    }

    /// Run a read-only closure on a pooled reader connection with a diagnostic label.
    pub async fn with_read_conn_labeled<F, R>(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let idx = self.read_pool_idx.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
        let conn = self.read_pool[idx].clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let conn = conn.blocking_lock();
            let result = f(&conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > SLOW_READ_WARN_MS {
                record_slow_query("read", label, elapsed_ms);
            }
            result
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    /// Run a synchronous closure with the database connection.
    /// All rusqlite operations must go through this method.
    pub async fn with_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.with_conn_labeled("unlabeled_write", f).await
    }

    /// Run a synchronous closure with the database connection and a diagnostic label.
    pub async fn with_conn_labeled<F, R>(&self, label: &'static str, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let conn = conn.blocking_lock();
            let result = f(&conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > SLOW_WRITE_WARN_MS {
                record_slow_query("write", label, elapsed_ms);
            }
            result
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    /// Run a synchronous closure with a mutable reference (for transactions).
    pub async fn with_conn_mut<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.with_conn_mut_labeled("unlabeled_transaction", f).await
    }

    /// Run a synchronous transactional closure with a diagnostic label.
    pub async fn with_conn_mut_labeled<F, R>(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R, String>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let mut conn = conn.blocking_lock();
            let result = f(&mut conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > SLOW_TX_WARN_MS {
                record_slow_query("transaction", label, elapsed_ms);
            }
            result
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }
}
