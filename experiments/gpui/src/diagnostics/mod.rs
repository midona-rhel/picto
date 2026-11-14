//! The log window's lines, as production's renderer keeps them
//! (`diagnosticsStore.ts`, `diagnosticsRuntime.ts`): the core's tracing
//! events (its `log` event, forwarded by `picto_core::events::EventEmitLayer`),
//! slow and failed commands, and panics, the last 2,000 of them. The store is
//! the process's, filled from any thread from the moment the application
//! starts, so lines logged before the window opens are kept; the panel
//! (`panel.rs`) shows it, `report.rs` writes it out, and `crash.rs` records
//! panics.
pub mod crash;
pub mod panel;
pub mod report;

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// `MAX_ENTRIES`.
pub const MAX_ENTRIES: usize = 2_000;

/// `DiagnosticLevel`, most severe first as the panel lists them.
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

    /// As production writes it, in the panel and the report.
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// `normalizeLevel`: anything not named is INFO.
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

/// `DiagnosticSource`. Production's main process and renderer are one
/// process here: `Main` holds what the app records about itself between
/// launches (a panic of the previous session), `Renderer` a panic now.
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

    /// `sourceForTarget`.
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

/// `DiagnosticEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub level: Level,
    pub source: Source,
    pub target: String,
    pub message: String,
    /// ISO 8601 in UTC with milliseconds, as `toISOString()` writes it.
    pub timestamp: String,
    pub duration_ms: Option<f64>,
    pub native_duration_ms: Option<f64>,
}

/// A line before it is numbered.
pub struct Line {
    pub level: Level,
    pub source: Source,
    pub target: String,
    pub message: String,
    pub timestamp: String,
    pub duration_ms: Option<f64>,
}

/// The lines kept, oldest first.
#[derive(Default)]
pub struct Store {
    entries: VecDeque<Entry>,
    next_id: u64,
}

impl Store {
    /// `addDiagnostic`: numbered, and the oldest let go past the limit.
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
/// Bumped on every change, so the panel redraws only when there is more.
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

/// Adds `line` unless the store is held (by the thread that panicked).
pub fn try_add(line: Line) {
    if let Ok(mut store) = STORE.try_lock() {
        store.get_or_insert_with(Store::default).add(line);
        drop(store);
        REVISION.fetch_add(1, Ordering::Release);
    }
}

/// `clearDiagnostics`.
pub fn clear() {
    with_store(Store::clear);
    REVISION.fetch_add(1, Ordering::Release);
}

pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

/// The lines kept now, oldest first.
pub fn snapshot() -> Vec<Entry> {
    with_store(|store| store.entries().cloned().collect())
}

/// Now, as `new Date().toISOString()` writes it.
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// `accept` for the core's `log` event: its level, target, message and
/// time. The core's own command trace (`picto::ipc`) is left out, as the
/// command timings below say the same with more context.
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

/// The core's `log` event, from whichever thread logged it.
pub fn accept_core(payload: &str) {
    if let Some(line) = parse_core(payload) {
        add(line);
    }
}

/// `recordIpcCall`: what a command's round trip is worth keeping. Routine,
/// quick commands are not; slow ones are DEBUG, or WARN from 100 ms;
/// failures are ERROR. The panel's own poll, a grid read superseded by a
/// newer one, and the cloud status asked before the library is open are
/// expected and kept out (the superseded read as DEBUG when slow).
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
    if command == "cloud.status.get"
        && error.is_some_and(|error| {
            // `/^No library is open\b/i`
            let lower = error.to_ascii_lowercase();
            lower.starts_with("no library is open")
                && lower[18..]
                    .chars()
                    .next()
                    .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
        })
    {
        return None;
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
            // The command ran in this process: there is no delivery time to
            // tell from the core's, so production's message without one.
            "Completed",
        )),
    }
}

/// Records a command's round trip, from any thread.
pub fn record_ipc(command: &str, duration_ms: f64, error: Option<&str>) {
    if let Some(line) = ipc_line(command, duration_ms, error) {
        add(line);
    }
}

/// The `log` crate's records: GPUI, its renderers, wgpu and the decoders
/// report through it (a skipped frame, a lost drawable, a failed upload).
/// Warnings and errors go to the log window as the renderer's, and to
/// standard error. Installed before the core's tracing, whose own bridge for
/// `log` then stays out: its filter keeps only the core's targets, which
/// do not log through `log`.
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Error => Level::Error,
            _ => Level::Warn,
        };
        let message = record.args().to_string();
        eprintln!("picto: {} {}: {message}", level.name(), record.target());
        // `try_add`: a record logged while the store is held (a panic
        // being recorded) is not worth waiting for.
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

/// Takes the `log` crate's warnings and errors from here on.
pub fn install_logger() {
    static LOGGER: Logger = Logger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
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
    fn does_not_surface_expected_cloud_status_polling_before_a_library_opens() {
        assert!(
            ipc_line(
                "cloud.status.get",
                3.,
                Some("No library is open. Call open_library() first.")
            )
            .is_none()
        );
        let failed = ipc_line("cloud.status.get", 3., Some("Cloud provider failed")).unwrap();
        assert_eq!(failed.level, Level::Error);
        assert_eq!(failed.message, "Cloud provider failed");
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
        // The core's own command trace is left to the command timings.
        assert!(parse_core(r#"{"level":"INFO","target":"picto::ipc","message":"x"}"#).is_none());
        // An unknown level is INFO; a main-process target is `main`.
        let line = parse_core(r#"{"level":"notice","target":"main.crash"}"#).unwrap();
        assert_eq!((line.level, line.source), (Level::Info, Source::Main));
    }
}
