//! Foreground tasks as the renderer shows them: progress, rate and estimate.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A foreground task as the renderer shows it: what it is doing, how far it
/// got, and an estimate of the time left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ForegroundTask {
    #[ts(type = "number")]
    pub task_id: i64,
    pub kind: String,
    pub title: String,
    /// running, paused, reverting, finished, stopped or reverted.
    pub state: String,
    /// False while files are still being found; `total` grows meanwhile.
    pub scan_complete: bool,
    #[ts(type = "number")]
    pub total: u64,
    /// Files handled so far: added, already in the library, or failed.
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub added: u64,
    #[ts(type = "number")]
    pub existing: u64,
    #[ts(type = "number")]
    pub failed: u64,
    /// Whether "stop and remove what was imported" is offered.
    pub revertible: bool,
    /// Files per second over the last stretch of work, once known.
    pub rate_per_second: Option<f64>,
    #[ts(type = "number | null")]
    pub eta_seconds: Option<u64>,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    #[ts(type = "number | null")]
    pub finished_at_ms: Option<i64>,
    pub error: Option<String>,
    /// Actions that cannot start while this task is open (`blocked_actions`).
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TaskFailure {
    pub path: String,
    pub error: String,
}

/// Starting an import.
pub const IMPORT_ACTION: &str = "import";

/// What an open (running, paused or undoing) task of `kind` keeps from
/// starting. This is the one table of which work excludes which: the backend
/// refuses a blocked start (`ensure_can_start`) and the renderer disables what
/// a task's `blocks` names. A new kind of heavy task (an export, a Picto Pack,
/// a duplicate scan) adds its row here, and its start calls
/// `ensure_can_start` with its own action.
pub fn blocked_actions(kind: &str) -> &'static [&'static str] {
    match kind {
        crate::import_tasks::IMPORT_TASK_KIND => &[IMPORT_ACTION],
        _ => &[],
    }
}

/// Running, paused, or undoing what it imported: the task still holds what it
/// blocks.
fn holds_blocks(state: picto_library::TaskState) -> bool {
    use picto_library::TaskState::{Paused, Reverting, Running};
    matches!(state, Running | Paused | Reverting)
}

/// Refuse to start `action` while an open task blocks it.
pub fn ensure_can_start(
    application: &crate::library_application::LibraryApplication,
    action: &str,
) -> Result<(), String> {
    let tasks = application
        .library()
        .tasks(false)
        .map_err(|error| error.to_string())?;
    match tasks
        .iter()
        .find(|task| holds_blocks(task.state) && blocked_actions(&task.kind).contains(&action))
    {
        Some(task) => Err(format!(
            "“{}” is still running. Let it finish or stop it before starting another.",
            task.title
        )),
        None => Ok(()),
    }
}

/// Recent progress samples per task, for the rate and estimate. Kept in
/// memory: after a restart the estimate simply starts over.
type ProgressKey = (std::path::PathBuf, i64);
type ProgressSamples = Vec<(std::time::Instant, u64)>;
static PROGRESS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<ProgressKey, ProgressSamples>>,
> = std::sync::LazyLock::new(Default::default);

/// The rate is measured over this much recent work.
const RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(20);
/// No estimate until progress has been watched for this long.
const RATE_MINIMUM: std::time::Duration = std::time::Duration::from_secs(3);

fn progress_rate(key: ProgressKey, done: u64, measuring: bool) -> Option<f64> {
    let mut progress = PROGRESS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !measuring {
        // Paused or ended: time spent here is not import time.
        progress.remove(&key);
        return None;
    }
    let now = std::time::Instant::now();
    let samples = progress.entry(key).or_default();
    if samples.last().is_none_or(|(_, last)| *last != done) || samples.len() < 2 {
        samples.push((now, done));
    }
    samples.retain(|(at, _)| now.duration_since(*at) <= RATE_WINDOW);
    let (first_at, first_done) = *samples.first()?;
    let elapsed = now.duration_since(first_at);
    if elapsed < RATE_MINIMUM || done <= first_done {
        return None;
    }
    Some((done - first_done) as f64 / elapsed.as_secs_f64())
}

pub fn foreground_tasks(
    application: &crate::library_application::LibraryApplication,
) -> Result<Vec<ForegroundTask>, String> {
    let tasks = application
        .library()
        .tasks(false)
        .map_err(|error| error.to_string())?;
    Ok(tasks
        .into_iter()
        .map(|task| {
            let done = task.done_units();
            let measuring = task.state == picto_library::TaskState::Running;
            let rate = progress_rate(
                (application.root().to_path_buf(), task.task_id.0),
                done,
                measuring,
            );
            let eta_seconds = rate
                .filter(|_| task.scan_complete && rate.is_some_and(|rate| rate > 0.0))
                .map(|rate| (task.total_units.saturating_sub(done) as f64 / rate).ceil() as u64);
            let blocks = if holds_blocks(task.state) {
                blocked_actions(&task.kind).iter().map(|action| (*action).to_owned()).collect()
            } else {
                Vec::new()
            };
            ForegroundTask {
                task_id: task.task_id.0,
                kind: task.kind,
                title: task.title,
                state: task.state.as_str().to_owned(),
                scan_complete: task.scan_complete,
                total: task.total_units,
                done,
                added: task.added_units,
                existing: task.existing_units,
                failed: task.failed_units,
                revertible: task.revertible,
                rate_per_second: rate,
                eta_seconds,
                created_at_ms: task.created_at_ms,
                finished_at_ms: task.finished_at_ms,
                blocks,
                error: task.error,
            }
        })
        .collect())
}

pub fn task_failures(
    application: &crate::library_application::LibraryApplication,
    task_id: i64,
) -> Result<Vec<TaskFailure>, String> {
    Ok(application
        .library()
        .task_failures(picto_library::TaskId(task_id), 500)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|(path, error)| TaskFailure { path, error })
        .collect())
}
