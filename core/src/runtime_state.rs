//! Centralized runtime state registry.
//!
//! Owns the global monotonic sequence counter (shared with `events.rs`) and the
//! in-memory task registry. All task mutations emit events so the frontend can
//! stay in sync.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::runtime_contract::snapshot::RuntimeSnapshot;
use crate::runtime_contract::task::RuntimeTask;

// ---------------------------------------------------------------------------
// Sequence counter — single source of truth for the whole process
// ---------------------------------------------------------------------------

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Atomically increment and return the next sequence number.
pub fn next_seq() -> u64 {
    SEQ.fetch_add(1, Ordering::Relaxed) + 1
}

/// Return the current sequence number without incrementing.
pub fn current_seq() -> u64 {
    SEQ.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Task registry
// ---------------------------------------------------------------------------

struct RuntimeStateInner {
    tasks: HashMap<String, RuntimeTask>,
}

static RUNTIME: OnceLock<Mutex<RuntimeStateInner>> = OnceLock::new();

fn runtime() -> &'static Mutex<RuntimeStateInner> {
    RUNTIME.get_or_init(|| {
        Mutex::new(RuntimeStateInner {
            tasks: HashMap::new(),
        })
    })
}

/// Insert or update a task, then emit `runtime/task_upserted`.
pub fn upsert_task(task: RuntimeTask) {
    {
        let mut guard = crate::poison::mutex_or_recover(runtime(), "runtime_state::upsert_task");
        guard.tasks.insert(task.task_id.clone(), task.clone());
    }
    crate::events::emit(crate::events::event_names::RUNTIME_TASK_UPSERTED, &task);
}

/// Remove a task by ID, then emit `runtime/task_removed`.
pub fn remove_task(task_id: &str) {
    {
        let mut guard = crate::poison::mutex_or_recover(runtime(), "runtime_state::remove_task");
        guard.tasks.remove(task_id);
    }
    crate::events::emit(
        crate::events::event_names::RUNTIME_TASK_REMOVED,
        &serde_json::json!({ "task_id": task_id }),
    );
}

/// Get a single task by ID (clone).
pub fn get_task(task_id: &str) -> Option<RuntimeTask> {
    let guard = crate::poison::mutex_or_recover(runtime(), "runtime_state::get_task");
    guard.tasks.get(task_id).cloned()
}

/// List all current tasks (cloned).
pub fn list_tasks() -> Vec<RuntimeTask> {
    let guard = crate::poison::mutex_or_recover(runtime(), "runtime_state::list_tasks");
    guard.tasks.values().cloned().collect()
}

/// Build a full runtime snapshot for frontend initialization / recovery.
pub fn get_runtime_snapshot() -> RuntimeSnapshot {
    let guard = crate::poison::mutex_or_recover(runtime(), "runtime_state::get_runtime_snapshot");
    RuntimeSnapshot {
        seq: current_seq(),
        ts: chrono::Utc::now().to_rfc3339(),
        tasks: guard.tasks.values().cloned().collect(),
    }
}

/// Clear all tasks. Called on library close.
pub fn reset() {
    let mut guard = crate::poison::mutex_or_recover(runtime(), "runtime_state::reset");
    guard.tasks.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_contract::task::{TaskKind, TaskStatus};
    use std::sync::Mutex as StdMutex;

    // Tests share global static state — serialize them.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn make_task(id: &str) -> RuntimeTask {
        RuntimeTask {
            task_id: id.to_string(),
            kind: TaskKind::Subscription,
            status: TaskStatus::Running,
            label: format!("Test task {id}"),
            parent_task_id: None,
            progress: None,
            detail: None,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn seq_increments() {
        let a = next_seq();
        let b = next_seq();
        assert!(b > a, "seq should monotonically increase");
    }

    #[test]
    fn task_upsert_get_remove() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        upsert_task(make_task("test-1"));
        assert_eq!(list_tasks().len(), 1);
        assert!(get_task("test-1").is_some());

        upsert_task(make_task("test-2"));
        assert_eq!(list_tasks().len(), 2);

        remove_task("test-1");
        assert_eq!(list_tasks().len(), 1);
        assert!(get_task("test-1").is_none());
        assert!(get_task("test-2").is_some());

        reset();
    }

    #[test]
    fn snapshot_contains_tasks() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        upsert_task(make_task("snap-1"));
        upsert_task(make_task("snap-2"));

        let snap = get_runtime_snapshot();
        assert_eq!(snap.tasks.len(), 2);
        assert!(snap.seq > 0);

        reset();
    }

    #[test]
    fn reset_clears_all_tasks() {
        let _lock = TEST_LOCK.lock().unwrap();

        upsert_task(make_task("reset-test-1"));
        reset();
        assert!(list_tasks().is_empty());
    }
}
