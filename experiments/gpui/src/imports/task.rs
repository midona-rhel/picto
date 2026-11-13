//! A foreground task (an import) as the application reports it
//! (`tasks.list`), and the words and states `taskPresentation.ts` gives it.
use crate::grid::format::grouped;
use serde_json::Value;

/// `ForegroundTask` (`core/src/tasks.rs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub task_id: i64,
    pub kind: String,
    pub title: String,
    /// running, paused, reverting, finished, stopped or reverted.
    pub state: String,
    pub scan_complete: bool,
    pub total: u64,
    pub done: u64,
    pub added: u64,
    pub existing: u64,
    pub failed: u64,
    pub revertible: bool,
    pub rate_per_second: Option<f64>,
    pub eta_seconds: Option<u64>,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub error: Option<String>,
    pub blocks: Vec<String>,
}

impl Task {
    /// One task of the `tasks.list` answer.
    pub fn read(value: &Value) -> Option<Self> {
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        let count = |key: &str| value[key].as_u64().unwrap_or(0);
        Some(Self {
            task_id: value["task_id"].as_i64()?,
            kind: text("kind").unwrap_or_default(),
            title: text("title").unwrap_or_default(),
            state: text("state")?,
            scan_complete: value["scan_complete"].as_bool().unwrap_or(true),
            total: count("total"),
            done: count("done"),
            added: count("added"),
            existing: count("existing"),
            failed: count("failed"),
            revertible: value["revertible"].as_bool().unwrap_or(false),
            rate_per_second: value["rate_per_second"].as_f64(),
            eta_seconds: value["eta_seconds"].as_u64(),
            created_at_ms: value["created_at_ms"].as_i64().unwrap_or(0),
            finished_at_ms: value["finished_at_ms"].as_i64(),
            error: text("error"),
            blocks: value["blocks"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// The `tasks.list` answer (or a control's), or `None` when it is not
    /// a list of tasks.
    pub fn read_list(value: &Value) -> Option<Vec<Self>> {
        value.as_array()?.iter().map(Self::read).collect()
    }
}

/// `TaskFailure`: a file that could not be imported.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub path: String,
    pub error: String,
}

impl Failure {
    /// The `tasks.failures` answer.
    pub fn read_list(value: &Value) -> Vec<Self> {
        value
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        Some(Self {
                            path: row["path"].as_str()?.to_owned(),
                            error: row["error"].as_str().unwrap_or_default().to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Finding,
    Importing,
    Paused,
    Removing,
    Finished,
    Stopped,
    Undone,
}

/// `taskTone`: the colour of the task's glyph and progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Working,
    Paused,
    Done,
    Warning,
}

impl Task {
    /// `taskPhase`
    pub fn phase(&self) -> Phase {
        match self.state.as_str() {
            "paused" => Phase::Paused,
            "reverting" => Phase::Removing,
            "finished" => Phase::Finished,
            "stopped" => Phase::Stopped,
            "reverted" => Phase::Undone,
            _ if self.scan_complete => Phase::Importing,
            _ => Phase::Finding,
        }
    }

    /// `isTaskWorking`: progress moves, so the list is read again each second.
    pub fn working(&self) -> bool {
        matches!(self.state.as_str(), "running" | "reverting")
    }

    /// `isTaskOpen`: not yet over.
    pub fn open(&self) -> bool {
        matches!(self.state.as_str(), "running" | "paused" | "reverting")
    }

    /// `taskTitle`
    pub fn heading(&self) -> String {
        let title = &self.title;
        match self.phase() {
            Phase::Finished => format!("Imported “{title}”"),
            Phase::Stopped => format!("Stopped importing “{title}”"),
            Phase::Undone => format!("Undid importing “{title}”"),
            Phase::Removing => format!("Undoing “{title}”"),
            _ => format!("Importing “{title}”"),
        }
    }

    /// `taskSubtitle`
    pub fn subtitle(&self) -> String {
        match self.phase() {
            Phase::Finding => "Finding files…".into(),
            Phase::Importing => match self.rate_per_second {
                Some(rate) if rate >= 0.1 => {
                    let rate = if rate >= 10. {
                        format!("{}", rate.round() as i64)
                    } else {
                        format!("{rate:.1}")
                    };
                    format!("Adding to library · {rate} files/s")
                }
                _ => "Adding to library".into(),
            },
            Phase::Paused => "Paused".into(),
            Phase::Removing => "Removing the items it added…".into(),
            Phase::Finished => match self.finished_at_ms {
                Some(finished) => format!(
                    "Finished in {}",
                    duration(finished.saturating_sub(self.created_at_ms))
                ),
                None => "Finished".into(),
            },
            Phase::Stopped => self
                .error
                .clone()
                .unwrap_or_else(|| "Stopped before every file was imported".into()),
            Phase::Undone => format!("Removed the {} items it added", grouped(self.added)),
        }
    }

    /// `taskTone`
    pub fn tone(&self) -> Tone {
        match self.phase() {
            Phase::Paused => Tone::Paused,
            Phase::Finished => Tone::Done,
            Phase::Stopped | Phase::Undone if self.error.is_some() => Tone::Warning,
            Phase::Stopped | Phase::Undone => Tone::Paused,
            _ => Tone::Working,
        }
    }

    /// The task's glyph (`TaskIcon`).
    pub fn glyph(&self) -> &'static str {
        match self.phase() {
            Phase::Finding => "task-folder-search",
            Phase::Paused => "task-player-pause",
            Phase::Finished => "task-check",
            Phase::Stopped | Phase::Undone if self.error.is_some() => "task-alert-triangle",
            Phase::Stopped | Phase::Undone => "task-player-stop",
            _ => "task-photo-down",
        }
    }

    /// `Progress`: no share is known while files are still being found or
    /// removed, or while none have been found.
    pub fn indeterminate(&self) -> bool {
        match self.phase() {
            Phase::Finding | Phase::Removing => true,
            Phase::Importing => self.total == 0,
            _ => false,
        }
    }

    /// How much of the track the fill covers, 0 to 1.
    pub fn share(&self) -> f32 {
        if self.phase() == Phase::Finished {
            return 1.;
        }
        if self.total == 0 {
            return 0.;
        }
        (self.done as f32 / self.total as f32).min(1.)
    }

    /// `Counts`: the left and right of the counts line.
    pub fn counts(&self) -> (String, Option<String>) {
        match self.phase() {
            Phase::Finding => (
                format!("{} files found", grouped(self.total)),
                Some("Starting…".into()),
            ),
            Phase::Finished | Phase::Stopped | Phase::Undone => (
                format!("{} added", grouped(self.added)),
                Some(format!("{} files", grouped(self.total))),
            ),
            phase => (
                format!("{} of {} files", grouped(self.done), grouped(self.total)),
                match phase {
                    Phase::Paused => Some("Paused".into()),
                    Phase::Removing => None,
                    _ => Some(time_left(self.eta_seconds).unwrap_or_else(|| "Estimating…".into())),
                },
            ),
        }
    }

    /// The task bar's detail: "N found" while finding, else "done / total"
    /// with the short time left while importing.
    pub fn bar_detail(&self) -> String {
        let phase = self.phase();
        if phase == Phase::Finding {
            return format!("{} found", grouped(self.total));
        }
        let progress = format!("{} / {}", grouped(self.done), grouped(self.total));
        match time_left_short(self.eta_seconds).filter(|_| phase == Phase::Importing) {
            Some(left) => format!("{progress} · {left}"),
            None => progress,
        }
    }

    /// The Stop view's question and its choices (`StopView`).
    pub fn stop_choices(&self) -> Vec<(StopChoice, &'static str, String)> {
        let remaining = self.total.saturating_sub(self.done);
        let mut choices = vec![(
            StopChoice::Keep,
            "Keep what was imported",
            format!(
                "The {} new items stay. The remaining {} files are not imported.",
                grouped(self.added),
                grouped(remaining)
            ),
        )];
        if self.revertible {
            choices.push((
                StopChoice::Remove,
                "Remove what was imported",
                format!(
                    "Deletes the {} items this import added. Items that were already in your library are left alone.",
                    grouped(self.added)
                ),
            ));
        }
        choices
    }
}

/// What a stopped import does with what it already added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopChoice {
    Keep,
    Remove,
}

/// `formatTimeLeft`: "About 3 min left", or nothing while there is no
/// estimate yet.
pub fn time_left(seconds: Option<u64>) -> Option<String> {
    let seconds = seconds?;
    Some(if seconds < 60 {
        "Less than a minute left".into()
    } else if seconds < 3600 {
        format!("About {} min left", seconds.div_ceil(60))
    } else {
        let (hours, minutes) = hours_and_minutes(seconds);
        format!("About {hours} h {minutes} min left")
    })
}

/// `formatTimeLeftShort`: "3 min", "1 h 5 min".
pub fn time_left_short(seconds: Option<u64>) -> Option<String> {
    let seconds = seconds?;
    Some(if seconds < 60 {
        "<1 min".into()
    } else if seconds < 3600 {
        format!("{} min", seconds.div_ceil(60))
    } else {
        let (hours, minutes) = hours_and_minutes(seconds);
        format!("{hours} h {minutes} min")
    })
}

/// Whole hours, and the rest rounded to minutes, as `Math.round` does.
fn hours_and_minutes(seconds: u64) -> (u64, u64) {
    (
        seconds / 3600,
        ((seconds % 3600) as f64 / 60.).round() as u64,
    )
}

/// `formatDuration`: "12 s", "3 min 4 s", "1 h 2 min"; at least a second.
pub fn duration(milliseconds: i64) -> String {
    let seconds = ((milliseconds as f64 / 1000.).round() as i64).max(1);
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} min {} s", seconds % 60);
    }
    format!("{} h {} min", minutes / 60, minutes % 60)
}

/// Which task the foreground dialog shows and which run in the task bar
/// (`TaskPresentation`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Presentation {
    pub focused: Option<i64>,
    pub minimized: Vec<i64>,
}

impl Presentation {
    /// `showTask`: brings the task to the foreground dialog.
    pub fn show(&mut self, task: i64) {
        self.minimized.retain(|id| *id != task);
        self.focused = Some(task);
    }

    /// `minimizeTask`: the task goes on in the task bar.
    pub fn minimize(&mut self, task: i64) {
        if !self.minimized.contains(&task) {
            self.minimized.push(task);
        }
        if self.focused == Some(task) {
            self.focused = None;
        }
    }

    /// `applyTasks`: forgets tasks that are gone, and gives the foreground
    /// to an open task that was not sent to the background, one resumed
    /// after the app was reopened included.
    pub fn apply(&mut self, tasks: &[Task]) {
        let known = |id: &i64| tasks.iter().any(|task| task.task_id == *id);
        self.minimized.retain(known);
        self.focused = self.focused.filter(known);
        if self.focused.is_none() {
            self.focused = tasks
                .iter()
                .find(|task| task.open() && !self.minimized.contains(&task.task_id))
                .map(|task| task.task_id);
        }
    }

    /// The tasks the bar lists: sent to the background and not in front.
    pub fn in_bar<'a>(&'a self, tasks: &'a [Task]) -> impl Iterator<Item = &'a Task> + 'a {
        tasks.iter().filter(move |task| {
            self.minimized.contains(&task.task_id) && Some(task.task_id) != self.focused
        })
    }
}

/// `taskGate('import')`: the open task that keeps an import from starting,
/// and why, for a tooltip.
pub fn gate<'a>(tasks: &'a [Task], action: &str) -> Option<(&'a Task, String)> {
    let task = tasks
        .iter()
        .find(|task| task.blocks.iter().any(|blocked| blocked == action))?;
    Some((task, format!("Available when “{}” finishes", task.title)))
}

#[cfg(test)]
mod tests {
    use super::{Phase, Presentation, StopChoice, Task, Tone, duration, gate, time_left};

    fn task() -> Task {
        Task {
            task_id: 7,
            kind: "import".into(),
            title: "Commissions 2024".into(),
            state: "running".into(),
            scan_complete: true,
            total: 12408,
            done: 4962,
            added: 4651,
            existing: 311,
            failed: 0,
            revertible: true,
            rate_per_second: Some(42.),
            eta_seconds: Some(180),
            created_at_ms: 1_000,
            finished_at_ms: None,
            error: None,
            blocks: vec!["import".into()],
        }
    }

    #[test]
    fn a_working_import_reads_as_production_words_it() {
        let task = task();
        assert_eq!(task.heading(), "Importing “Commissions 2024”");
        assert_eq!(task.subtitle(), "Adding to library · 42 files/s");
        assert_eq!(
            task.counts(),
            (
                "4,962 of 12,408 files".into(),
                Some("About 3 min left".into())
            )
        );
        assert_eq!(task.bar_detail(), "4,962 / 12,408 · 3 min");
        assert_eq!(task.tone(), Tone::Working);
        assert!(!task.indeterminate());
    }

    #[test]
    fn phases_follow_the_state_and_the_scan() {
        let mut task = task();
        task.scan_complete = false;
        assert_eq!(task.phase(), Phase::Finding);
        assert_eq!(task.subtitle(), "Finding files…");
        assert!(task.indeterminate());
        assert_eq!(task.bar_detail(), "12,408 found");
        task.state = "finished".into();
        task.finished_at_ms = Some(1_000 + 125_000);
        assert_eq!(task.heading(), "Imported “Commissions 2024”");
        assert_eq!(task.subtitle(), "Finished in 2 min 5 s");
        assert_eq!(task.counts().0, "4,651 added");
        assert_eq!(task.share(), 1.);
        assert_eq!(task.tone(), Tone::Done);
        task.state = "stopped".into();
        task.error = Some("Disk full".into());
        assert_eq!(task.tone(), Tone::Warning);
        assert_eq!(task.glyph(), "task-alert-triangle");
        assert_eq!(task.subtitle(), "Disk full");
    }

    #[test]
    fn slow_rates_show_a_decimal_and_tiny_ones_none() {
        let mut task = task();
        task.rate_per_second = Some(2.46);
        assert_eq!(task.subtitle(), "Adding to library · 2.5 files/s");
        task.rate_per_second = Some(0.05);
        assert_eq!(task.subtitle(), "Adding to library");
    }

    #[test]
    fn times_are_worded_as_format_time_left_words_them() {
        assert_eq!(time_left(None), None);
        assert_eq!(time_left(Some(30)).unwrap(), "Less than a minute left");
        assert_eq!(time_left(Some(61)).unwrap(), "About 2 min left");
        assert_eq!(
            time_left(Some(3600 + 5 * 60)).unwrap(),
            "About 1 h 5 min left"
        );
        assert_eq!(duration(400), "1 s");
        assert_eq!(duration(3_723_000), "1 h 2 min");
    }

    #[test]
    fn stopping_offers_removal_only_when_the_import_can_be_undone() {
        let mut task = task();
        assert_eq!(task.stop_choices().len(), 2);
        task.revertible = false;
        let choices = task.stop_choices();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].0, StopChoice::Keep);
        assert_eq!(
            choices[0].2,
            "The 4,651 new items stay. The remaining 7,446 files are not imported."
        );
    }

    #[test]
    fn an_open_task_takes_the_foreground_unless_minimized() {
        let mut presentation = Presentation::default();
        let tasks = vec![task()];
        presentation.apply(&tasks);
        assert_eq!(presentation.focused, Some(7));
        presentation.minimize(7);
        assert_eq!(presentation.focused, None);
        presentation.apply(&tasks);
        assert_eq!(presentation.focused, None);
        assert_eq!(presentation.in_bar(&tasks).count(), 1);
        presentation.show(7);
        assert_eq!(presentation.focused, Some(7));
        assert_eq!(presentation.in_bar(&tasks).count(), 0);
        // A dismissed task is forgotten.
        presentation.apply(&[]);
        assert_eq!(presentation, Presentation::default());
    }

    #[test]
    fn the_task_list_is_read_as_the_application_writes_it() {
        let list = serde_json::json!([{
            "task_id": 7, "kind": "import", "title": "Commissions 2024",
            "state": "running", "scan_complete": true, "total": 12408,
            "done": 4962, "added": 4651, "existing": 311, "failed": 0,
            "revertible": true, "rate_per_second": 42.0, "eta_seconds": 180,
            "created_at_ms": 1000, "finished_at_ms": null, "error": null,
            "blocks": ["import"]
        }]);
        assert_eq!(Task::read_list(&list), Some(vec![task()]));
        assert_eq!(Task::read_list(&serde_json::json!({"error": 1})), None);
    }

    #[test]
    fn an_open_import_blocks_another() {
        let tasks = vec![task()];
        let (blocking, reason) = gate(&tasks, "import").unwrap();
        assert_eq!(blocking.task_id, 7);
        assert_eq!(reason, "Available when “Commissions 2024” finishes");
        assert!(gate(&[], "import").is_none());
    }
}
