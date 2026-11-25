//! Keeps the lines that the log window shows. The lines are the core's
//! tracing events (the core's `log` event, which
//! `picto_core::events::EventEmitLayer` sends), the commands run on the core
//! that were slow or failed, the warnings and errors of the `log` crate, and
//! panics. Only the last 2,000 lines are kept.
//!
//! The process has one store of lines. Any thread can add to it, from the
//! moment the application starts, so lines that are logged before the log
//! window opens are kept. `panel.rs` shows the lines, `report.rs` writes
//! them to a support report, and `crash.rs` records panics.
pub mod crash;
pub mod panel;
pub mod report;

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The largest number of lines that the store keeps.
pub const MAX_ENTRIES: usize = 2_000;

/// The severity of a line, with the most severe first. The log window lists
/// its level filters in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    pub const ALL: [Level; 5] = [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ];

    /// Returns the level's name as the log window and the support report
    /// write it.
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// Returns the level that `level` names, ignoring case. A missing name,
    /// and a name that is none of the five, gives `Info`.
    pub fn parse(level: Option<&str>) -> Level {
        match level.map(str::to_ascii_uppercase).as_deref() {
            Some("TRACE") => Level::Trace,
            Some("DEBUG") => Level::Debug,
            Some("WARN") => Level::Warn,
            Some("ERROR") => Level::Error,
            _ => Level::Info,
        }
    }
}

/// The part of the app that a line comes from. `Core` is the core's tracing.
/// `Ipc` is a command run on the core. `Renderer` is this process: a panic,
/// a warning or error of the `log` crate, or a support report that could not
/// be written. `Main` is a panic of an earlier run of the app, which is
/// shown at the next start. The log window and the support report of Picto
/// 0.6.19 and earlier used the same four names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Core,
    Main,
    Renderer,
    Ipc,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::Core, Source::Main, Source::Renderer, Source::Ipc];

    pub fn name(self) -> &'static str {
        match self {
            Source::Core => "core",
            Source::Main => "main",
            Source::Renderer => "renderer",
            Source::Ipc => "ipc",
        }
    }

    /// Returns the source of a `log` event of the core from the event's
    /// target: `Main` when the target starts with `electron` or `main`,
    /// `Renderer` when it starts with `renderer`, and `Core` otherwise.
    pub fn for_target(target: &str) -> Source {
        if target.starts_with("electron") || target.starts_with("main") {
            Source::Main
        } else if target.starts_with("renderer") {
            Source::Renderer
        } else {
            Source::Core
        }
    }
}

/// One kept line.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub level: Level,
    pub source: Source,
    pub target: String,
    pub message: String,
    /// The time in UTC as ISO 8601 with milliseconds, for example
    /// `2026-08-26T12:00:00.000Z`.
    pub timestamp: String,
    pub duration_ms: Option<f64>,
    pub native_duration_ms: Option<f64>,
}

/// A line that is not in the store yet. The store gives the line its `id`
/// when it adds it.
pub struct Line {
    pub level: Level,
    pub source: Source,
    pub target: String,
    pub message: String,
    pub timestamp: String,
    pub duration_ms: Option<f64>,
}

/// Holds the kept lines, oldest first.
#[derive(Default)]
pub struct Store {
    entries: VecDeque<Entry>,
    next_id: u64,
}

impl Store {
    /// Adds `line` with the next number as its `id`. When the store then
    /// holds more than `MAX_ENTRIES` lines, the oldest are removed.
    pub fn add(&mut self, line: Line) {
        self.next_id += 1;
        self.entries.push_back(Entry {
            id: self.next_id,
            level: line.level,
            source: line.source,
            target: line.target,
            message: line.message,
            timestamp: line.timestamp,
            duration_ms: line.duration_ms,
            native_duration_ms: None,
        });
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = &Entry> {
        self.entries.iter()
    }
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
/// A number that is increased every time the store changes. The panel reads
/// the lines again only when this number has changed.
static REVISION: AtomicU64 = AtomicU64::new(0);

fn with_store<R>(change: impl FnOnce(&mut Store) -> R) -> R {
    let mut store = STORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    change(store.get_or_insert_with(Store::default))
}

pub fn add(line: Line) {
    with_store(|store| store.add(line));
    REVISION.fetch_add(1, Ordering::Release);
}

/// Adds `line` if the store's lock is free. If the lock is held, the line is
/// dropped. The panic hook and the logger call this, because they can run on
/// a thread that already holds the lock, and waiting for the lock would then
/// never end.
pub fn try_add(line: Line) {
    if let Ok(mut store) = STORE.try_lock() {
        store.get_or_insert_with(Store::default).add(line);
        drop(store);
        REVISION.fetch_add(1, Ordering::Release);
    }
}

/// Removes every kept line.
pub fn clear() {
    with_store(Store::clear);
    REVISION.fetch_add(1, Ordering::Release);
}

pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

/// Returns a copy of the kept lines, oldest first.
pub fn snapshot() -> Vec<Entry> {
    with_store(|store| store.entries().cloned().collect())
}

/// Returns the current time in UTC as ISO 8601 with milliseconds, which is
/// the form of a line's `timestamp`.
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Reads one `log` event of the core and returns it as a line. The event is
/// JSON with the fields `level`, `target`, `message`, `timestamp` and
/// `source`. Returns `None` for text that is not JSON, and for an event
/// whose target is `picto::ipc`. That target is the core's own trace of its
/// commands, and [`ipc_line`] records the same commands with their duration
/// and their error.
pub fn parse_core(payload: &str) -> Option<Line> {
    let raw: Value = serde_json::from_str(payload).ok()?;
    let text = |key: &str| {
        raw.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let target = text("target").unwrap_or("core").to_owned();
    if target == "picto::ipc" {
        return None;
    }
    Some(Line {
        // i18n: a JSON field
        level: Level::parse(text("level")),
        source: if text("source") == Some("renderer") {
            Source::Renderer
        } else {
            Source::for_target(&target)
        },
        message: text("message").unwrap_or_default().to_owned(),
        timestamp: text("timestamp").map(str::to_owned).unwrap_or_else(now),
        target,
        duration_ms: None,
    })
}

/// Adds one `log` event of the core to the store. It can be called from any
/// thread.
pub fn accept_core(payload: &str) {
    if let Some(line) = parse_core(payload) {
        add(line);
    }
}

/// Returns the line to keep for one command that was run on the core, or
/// `None` when the command gets no line. A command that failed gets an ERROR
/// line with its error. A command that succeeded gets no line when it took
/// less than 16 ms, a DEBUG line when it took less than 100 ms, and a WARN
/// line otherwise.
///
/// Two cases are expected and are treated differently. The command
/// `diagnostics.snapshot`, which the log window itself runs every 750 ms,
/// never gets a line. An `items.window` that failed with `query superseded`,
/// because a newer read of the grid replaced it, gets a DEBUG line when it
/// took 16 ms or more and no line otherwise.
pub fn ipc_line(command: &str, duration_ms: f64, error: Option<&str>) -> Option<Line> {
    if command == "diagnostics.snapshot" {
        return None;
    }
    let line = |level, message: &str| Line {
        level,
        source: Source::Ipc,
        target: command.to_owned(),
        message: message.to_owned(),
        timestamp: now(),
        duration_ms: Some(duration_ms),
    };
    if command == "items.window" && error == Some("query superseded") {
        return (duration_ms >= 16.)
            .then(|| line(Level::Debug, "Superseded by a newer grid request"));
    }
    match error {
        Some(error) => Some(line(Level::Error, error)),
        None if duration_ms < 16. => None,
        None => Some(line(
            if duration_ms >= 100. {
                Level::Warn
            } else {
                Level::Debug
            },
            // The message holds no duration. The command has one duration,
            // which is in the line's `duration_ms`, and the log window shows
            // it in a column of its own.
            "Completed",
        )),
    }
}

/// Adds the line that [`ipc_line`] returns for a command, if it returns one.
/// It can be called from any thread.
pub fn record_ipc(command: &str, duration_ms: f64, error: Option<&str>) {
    if let Some(line) = ipc_line(command, duration_ms, error) {
        add(line);
    }
}

/// Receives the records of the `log` crate. GPUI, its renderers, wgpu and
/// the decoders report through that crate, for example a skipped frame, a
/// lost drawable or a failed upload. Warnings and errors are written to
/// standard error and added to the log window with the source `Renderer`.
/// Records of a lower level are ignored, with one exception: the app's own
/// timing notes, which are informational records whose target starts with
/// [`TIMING`], are added as informational lines.
///
/// It is installed before the core starts its tracing. The core's tracing
/// would otherwise install its own receiver for the `log` crate, and only
/// one receiver can be installed. Nothing is lost by this: the core's
/// receiver keeps only the records of the core's targets, and the core does
/// not log through the `log` crate.
struct Logger;

/// The start of the target of a timing note. The video player writes such
/// notes with `log::info!` and the targets `timing.ffprobe` and
/// `timing.first_frame`.
const TIMING: &str = "timing.";

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.level() == log::Level::Info && metadata.target().starts_with(TIMING))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Error => Level::Error,
            log::Level::Info => Level::Info,
            _ => Level::Warn,
        };
        let message = record.args().to_string();
        eprintln!("picto: {} {}: {message}", level.name(), record.target());
        // GPUI logs an error for every picture that it could not load, and
        // the error does not name the file. Such a picture is a thumbnail
        // that is not written yet or is missing, and the views already draw
        // the picture of a torn page in its place. These errors are not
        // added to the log window, where they would be most of the errors.
        if record.target() == "gpui::asset_cache" {
            return;
        }
        // `try_add` is used because a record can be logged while the
        // store's lock is held, for example while a panic is being
        // recorded. The record is then dropped.
        try_add(Line {
            level,
            source: Source::Renderer,
            target: record.target().to_owned(),
            message,
            timestamp: now(),
            duration_ms: None,
        });
    }

    fn flush(&self) {}
}

/// Installs `Logger` as the logger of the `log` crate, and sets
/// informational records as the lowest level that the crate passes on, so
/// that the timing notes reach `Logger`, which ignores every other
/// informational record. It does nothing if a logger is already installed.
pub fn install_logger() {
    static LOGGER: Logger = Logger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(lines: impl IntoIterator<Item = Line>) -> Vec<Entry> {
        let mut store = Store::default();
        for line in lines {
            store.add(line);
        }
        store.entries().cloned().collect()
    }

    #[test]
    fn records_only_slow_and_failed_round_trips_with_useful_severity() {
        let kept = store_with(
            [
                ipc_line("items.page", 4.25, None),
                ipc_line("subscriptions.run", 125., None),
                ipc_line("files.export", 8., Some("disk full")),
            ]
            .into_iter()
            .flatten(),
        );
        assert_eq!(kept.len(), 2);
        assert_eq!(
            (kept[0].level, kept[0].source, kept[0].target.as_str()),
            (Level::Warn, Source::Ipc, "subscriptions.run")
        );
        assert_eq!(kept[0].duration_ms, Some(125.));
        assert_eq!(
            (
                kept[1].level,
                kept[1].target.as_str(),
                kept[1].message.as_str()
            ),
            (Level::Error, "files.export", "disk full")
        );
    }

    #[test]
    fn does_not_log_the_diagnostics_observer_itself() {
        assert!(ipc_line("diagnostics.snapshot", 200., None).is_none());
        assert!(ipc_line("diagnostics.snapshot", 1., Some("unavailable")).is_none());
    }

    #[test]
    fn distinguishes_normal_grid_supersession_from_read_failures() {
        let kept: Vec<Line> = [
            ipc_line("items.window", 5., Some("query superseded")),
            ipc_line("items.window", 120., Some("query superseded")),
            ipc_line("items.window", 20., Some("database error")),
        ]
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].level, Level::Debug);
        assert_eq!(kept[0].message, "Superseded by a newer grid request");
        assert_eq!(kept[0].duration_ms, Some(120.));
        assert_eq!(kept[1].message, "database error");
    }

    #[test]
    fn keeps_the_last_two_thousand_and_clears() {
        let mut store = Store::default();
        for index in 0..MAX_ENTRIES + 5 {
            store.add(Line {
                level: Level::Info,
                source: Source::Core,
                target: "test".into(),
                message: index.to_string(),
                timestamp: now(),
                duration_ms: None,
            });
        }
        assert_eq!(store.entries().len(), MAX_ENTRIES);
        assert_eq!(store.entries().next().unwrap().message, "5");
        store.clear();
        assert_eq!(store.entries().len(), 0);
    }

    #[test]
    fn core_lines_take_their_level_target_and_source() {
        let line = parse_core(
            r#"{"level":"WARN","target":"picto_core::library_media_runtime","message":"failed","timestamp":"2026-08-26T12:00:00.000Z"}"#,
        )
        .unwrap();
        assert_eq!(line.level, Level::Warn);
        assert_eq!(line.source, Source::Core);
        assert_eq!(line.timestamp, "2026-08-26T12:00:00.000Z");
        // An event of the core's own trace of its commands gives no line.
        assert!(parse_core(r#"{"level":"INFO","target":"picto::ipc","message":"x"}"#).is_none());
        // An unknown level becomes INFO, and a target that starts with
        // `main` has the source `Main`.
        let line = parse_core(r#"{"level":"notice","target":"main.crash"}"#).unwrap();
        assert_eq!((line.level, line.source), (Level::Info, Source::Main));
    }
}
