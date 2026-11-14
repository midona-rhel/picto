//! What a panic leaves behind. Production's main process logs a renderer
//! that went away (`render-process-gone`) and its own uncaught exceptions
//! as errors, which reach the log window while the app still runs. Here the
//! whole app is one process, so a panic is recorded with its backtrace
//! twice: as an error line in the log window at once (the app goes on when
//! the panic was on a worker thread), and as a file in the app's log folder
//! (`logs/` in its own folder for what it keeps per machine), written
//! before the process can end, which the next launch shows in the log
//! window as the previous session's panic.
use super::{Level, Line, Source};
use std::path::{Path, PathBuf};

/// A recorded panic not yet shown: `panic-<ms>-<process>.log`.
const PREFIX: &str = "panic-";
const PENDING: &str = ".log";
/// Shown at a launch, kept for a support request.
const SHOWN: &str = ".shown.log";
/// Panics kept once shown.
const KEEP_SHOWN: usize = 20;

/// The app's log folder, if it keeps one (not in tests without a state
/// folder).
pub fn folder() -> Option<PathBuf> {
    crate::state::location().map(|folder| folder.join("logs"))
}

/// The panic as a text: where, what, and the backtrace.
pub fn record_text(thread: &str, location: &str, payload: &str, backtrace: &str) -> String {
    format!("thread '{thread}' panicked at {location}:\n{payload}\nstack backtrace:\n{backtrace}")
}

/// What a panic's payload says.
fn payload(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Box<dyn Any>".to_owned())
}

/// Shows the panics of earlier sessions in the log window, then records
/// every panic from now on, before the default hook prints it.
pub fn install() {
    let folder = folder();
    if let Some(folder) = &folder {
        for line in take_previous(folder) {
            super::add(line);
        }
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let record = record_text(
            thread.name().unwrap_or("<unnamed>"),
            &info
                .location()
                .map(|location| location.to_string())
                .unwrap_or_default(),
            &payload(info),
            &std::backtrace::Backtrace::force_capture().to_string(),
        );
        let timestamp = super::now();
        if let Some(folder) = &folder {
            let _ = write(folder, &timestamp, &record);
        }
        // The store may be what panicked while it was held; then the file
        // is the only record.
        super::try_add(Line {
            level: Level::Error,
            source: Source::Renderer,
            // i18n: a log target
            target: "renderer.panic".to_owned(),
            message: record,
            timestamp,
            duration_ms: None,
        });
        previous(info);
    }));
}

/// Writes one panic's record: its time on the first line, then the text.
pub fn write(folder: &Path, timestamp: &str, record: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(format!(
        "{PREFIX}{}-{}{PENDING}",
        chrono::Utc::now().timestamp_millis(),
        std::process::id()
    ));
    std::fs::write(&path, format!("{timestamp}\n{record}"))?;
    Ok(path)
}

/// The panics recorded in `folder` and not yet shown, oldest first, as log
/// lines; each is then marked shown, and only the latest shown are kept.
pub fn take_previous(folder: &Path) -> Vec<Line> {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut pending = Vec::new();
    let mut shown = Vec::new();
    for entry in listing.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(PREFIX) {
            continue;
        }
        if name.ends_with(SHOWN) {
            shown.push(entry.path());
        } else if name.ends_with(PENDING) {
            pending.push(entry.path());
        }
    }
    // The names start with the time in milliseconds, all of one length for
    // centuries to come.
    pending.sort();
    let mut lines = Vec::new();
    for path in pending {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (timestamp, record) = text.split_once('\n').unwrap_or(("", text.as_str()));
        lines.push(Line {
            level: Level::Error,
            source: Source::Main,
            // i18n: a log target
            target: "main.crash".to_owned(),
            message: format!(
                "The previous session ended after a panic, recorded in {}\n{record}",
                path.display()
            ),
            timestamp: if chrono::DateTime::parse_from_rfc3339(timestamp).is_ok() {
                timestamp.to_owned()
            } else {
                super::now()
            },
            duration_ms: None,
        });
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let marked = path.with_file_name(format!(
            "{}{SHOWN}",
            name.strip_suffix(PENDING).unwrap_or(&name)
        ));
        if std::fs::rename(&path, &marked).is_ok() {
            shown.push(marked);
        }
    }
    shown.sort();
    if shown.len() > KEEP_SHOWN {
        for old in &shown[..shown.len() - KEEP_SHOWN] {
            let _ = std::fs::remove_file(old);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_panic_shows_once_at_the_next_launch() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("logs");
        let record = record_text(
            "main",
            "src/grid/mod.rs:10:5",
            "index out of bounds",
            "   0: picto_gpui_spike::grid::paint",
        );
        assert_eq!(
            record,
            "thread 'main' panicked at src/grid/mod.rs:10:5:\nindex out of bounds\nstack backtrace:\n   0: picto_gpui_spike::grid::paint"
        );
        let path = write(&logs, "2026-08-26T12:00:00.000Z", &record).unwrap();
        let lines = take_previous(&logs);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!((line.level, line.source), (Level::Error, Source::Main));
        assert_eq!(line.timestamp, "2026-08-26T12:00:00.000Z");
        assert!(line.message.contains(&path.display().to_string()));
        assert!(line.message.ends_with(&record));
        // Shown once: kept, but not shown again.
        assert!(!path.exists());
        assert!(take_previous(&logs).is_empty());
        assert_eq!(std::fs::read_dir(&logs).unwrap().count(), 1);
    }

    #[test]
    fn only_the_latest_shown_panics_are_kept() {
        let folder = tempfile::tempdir().unwrap();
        for index in 0..KEEP_SHOWN + 3 {
            std::fs::write(
                folder.path().join(format!(
                    "{PREFIX}{:013}-1{SHOWN}",
                    1_700_000_000_000u64 + index as u64
                )),
                "x",
            )
            .unwrap();
        }
        std::fs::write(folder.path().join("unrelated.txt"), "x").unwrap();
        assert!(take_previous(folder.path()).is_empty());
        assert_eq!(
            std::fs::read_dir(folder.path()).unwrap().count(),
            KEEP_SHOWN + 1
        );
        assert!(
            !folder
                .path()
                .join(format!("{PREFIX}{:013}-1{SHOWN}", 1_700_000_000_000u64))
                .exists()
        );
    }
}
