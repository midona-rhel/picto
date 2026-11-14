//! The text the log window copies and the support report it saves
//! (`supportReport.ts`, and `picto:diagnostics:save` in the main process's
//! `registerHandlers.mjs`).
use super::Entry;
use serde_json::Value;

/// `WorkerDiagnostic`, as `diagnostics.snapshot` answers it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worker {
    pub id: String,
    pub label: String,
    pub state: String,
    pub detail: String,
    pub active: i64,
    pub queued: i64,
    pub attention: i64,
}

impl Worker {
    /// The workers of a `diagnostics.snapshot` answer.
    pub fn list(snapshot: &Value) -> Vec<Worker> {
        let text = |worker: &Value, key: &str| {
            worker
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let number =
            |worker: &Value, key: &str| worker.get(key).and_then(Value::as_i64).unwrap_or(0);
        snapshot
            .get("workers")
            .and_then(Value::as_array)
            .map(|workers| {
                workers
                    .iter()
                    .map(|worker| Worker {
                        id: text(worker, "id"),
                        label: text(worker, "label"),
                        state: text(worker, "state"),
                        detail: text(worker, "detail"),
                        active: number(worker, "active"),
                        queued: number(worker, "queued"),
                        attention: number(worker, "attention"),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `formatDiagnosticEntry`: every field, tab-delimited, untruncated.
pub fn entry_line(entry: &Entry) -> String {
    let duration = match (entry.duration_ms, entry.native_duration_ms) {
        (None, _) => String::new(),
        (Some(total), None) => format!("{total:.1} ms"),
        (Some(total), Some(native)) => format!("{native:.1} ms native / {total:.1} ms total"),
    };
    [
        entry.timestamp.as_str(),
        entry.level.name(),
        entry.source.name(),
        entry.target.as_str(),
        duration.as_str(),
        entry.message.as_str(),
    ]
    .join("\t")
}

/// `DISPLAY_COLUMN_WIDTHS`.
const DISPLAY_COLUMNS: [usize; 5] = [14, 8, 10, 34, 38];

/// `formatDiagnosticDisplayEntry`: one line with stable character columns
/// and no truncation, with `time` in the first column.
pub fn display_line(entry: &Entry, time: &str) -> String {
    let line = entry_line(entry);
    let mut fields: Vec<&str> = line.split('\t').collect();
    fields[0] = time;
    let last = fields.pop().unwrap_or_default();
    let mut out = String::new();
    for (field, width) in fields.iter().zip(DISPLAY_COLUMNS) {
        out.push_str(field);
        // `padEnd` counts UTF-16 units; the fields are ASCII but for a
        // target, where characters are close enough.
        for _ in field.chars().count()..width {
            out.push(' ');
        }
    }
    out.push_str(last);
    out
}

/// `formatTime`: the local time of day on the 24-hour clock, with
/// milliseconds.
pub fn local_time(timestamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_else(|_| timestamp.to_owned())
}

/// `scrubSupportText`: home folders, e-mail addresses, bearer tokens and
/// the paths and queries of links are taken out.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let here = &chars[index..];
        let at_word_start = index == 0 || !is_word(chars[index - 1]);
        // `/(?:Users|home)/<name>`
        if let Some(skip) = ["/Users/", "/home/"].iter().find_map(|prefix| {
            starts_with(here, prefix).then(|| {
                let name = here[prefix.len()..]
                    .iter()
                    .take_while(|c| **c != '/' && !c.is_whitespace())
                    .count();
                (name > 0).then_some(prefix.len() + name)
            })?
        }) {
            out.push('~');
            index += skip;
            continue;
        }
        // `X:\Users\<name>`
        if here.len() > 9 && here[0].is_ascii_alphabetic() && starts_with(&here[1..], ":\\Users\\")
        {
            let name = here[9..]
                .iter()
                .take_while(|c| **c != '\\' && !c.is_whitespace())
                .count();
            if name > 0 {
                out.push('~');
                index += 9 + name;
                continue;
            }
        }
        // `Bearer <token>`
        if at_word_start && lower_starts_with(here, "bearer") {
            let spaces = here[6..].iter().take_while(|c| c.is_whitespace()).count();
            let token = here[6 + spaces..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || "._~+/=-".contains(**c))
                .count();
            if spaces > 0 && token > 0 {
                out.extend(&here[..6 + spaces]);
                out.push_str("<redacted>");
                index += 6 + spaces + token;
                continue;
            }
        }
        // `https?://…` up to whitespace: its origin is kept.
        if at_word_start
            // i18n: a link's scheme
            && (lower_starts_with(here, "http://") || lower_starts_with(here, "https://"))
        {
            let length = here.iter().take_while(|c| !c.is_whitespace()).count();
            let raw: String = here[..length].iter().collect();
            match url::Url::parse(&raw) {
                Ok(url) => {
                    out.push_str(&url.origin().ascii_serialization());
                    out.push_str("/<redacted>");
                }
                Err(_) => out.push_str("<url>"),
            }
            index += length;
            continue;
        }
        // An e-mail address.
        if at_word_start
            && (here[0].is_ascii_alphanumeric() || "._%+-".contains(here[0]))
            && let Some(length) = email_at(
                &here
                    .iter()
                    .take_while(|c| !c.is_whitespace())
                    .collect::<String>(),
            )
        {
            out.push_str("<email>");
            index += length;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn starts_with(chars: &[char], prefix: &str) -> bool {
    prefix.chars().count() <= chars.len() && prefix.chars().zip(chars).all(|(a, b)| a == *b)
}

fn lower_starts_with(chars: &[char], prefix: &str) -> bool {
    prefix.chars().count() <= chars.len()
        && prefix
            .chars()
            .zip(chars)
            .all(|(a, b)| a == b.to_ascii_lowercase())
}

/// `\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b`, case-insensitive, at the
/// start of `text`: its length in characters.
fn email_at(text: &str) -> Option<usize> {
    let local = text
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || "._%+-".contains(*c))
        .count();
    if local == 0 || text[local..].chars().next() != Some('@') {
        return None;
    }
    let domain: Vec<char> = text[local + 1..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || ".-".contains(*c))
        .collect();
    // The longest domain that ends in a dot and two or more letters.
    (0..domain.len()).rev().find_map(|end| {
        let candidate = &domain[..=end];
        let dot = candidate.iter().rposition(|c| *c == '.')?;
        let tld = &candidate[dot + 1..];
        let after = domain.get(end + 1);
        (dot > 0
            && tld.len() >= 2
            && tld.iter().all(char::is_ascii_alphabetic)
            && after.is_none_or(|c| !is_word(*c)))
        .then_some(local + 1 + end + 1)
    })
}

/// `buildSupportReport`, with the version and host lines the main process
/// adds when it saves the report. `runtime` and `language` stand for the
/// user agent and `navigator.language`.
pub fn build(
    entries: &[Entry],
    workers: &[Worker],
    runtime: &str,
    language: &str,
    version: &str,
    host: &str,
    created: &str,
) -> String {
    // i18n: the report is written in English, as production's
    let mut lines = vec![
        "Picto support report".to_owned(),
        format!("Version\t{version}"),
        format!("Host\t{host}"),
        format!("Created\t{created}"),
        format!("Runtime\t{runtime}"),
        format!("Language\t{language}"),
        String::new(),
        // i18n: the report is written in English, as production's
        "Workers".to_owned(),
        "ID\tName\tState\tActive\tQueued\tAttention\tDetail".to_owned(),
    ];
    lines.extend(workers.iter().map(|worker| {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            worker.id,
            worker.label,
            worker.state,
            worker.active,
            worker.queued,
            worker.attention,
            worker.detail
        )
    }));
    lines.extend([
        String::new(),
        // i18n: the report is written in English, as production's
        "Logs".to_owned(),
        "Timestamp\tLevel\tSource\tTarget\tDuration\tMessage".to_owned(),
    ]);
    lines.extend(entries.iter().map(entry_line));
    lines.push(String::new());
    scrub(&lines.join("\n"))
}

/// `picto-support-<stamp>.txt`, the stamp being the time in UTC with its
/// colons and dot made dashes.
pub fn file_name(created: &str) -> String {
    format!("picto-support-{}.txt", created.replace([':', '.'], "-"))
}

/// The main process refuses an empty report and one over 10 MB.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

/// Writes the report where the user chose, readable by the user only on
/// Unix (`mode: 0o600`).
pub fn write(path: &std::path::Path, report: &str) -> std::io::Result<()> {
    if report.is_empty() || report.len() > MAX_BYTES {
        return Err(std::io::Error::other(
            "The support report is empty or too large.",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path)?, report.as_bytes())
}

/// The operating system, its version and architecture, as `platform()
/// release() arch()` write them for the host line.
pub fn host() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{platform} {} {arch}", os_release())
}

#[cfg(unix)]
fn os_release() -> String {
    std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

#[cfg(not(unix))]
fn os_release() -> String {
    std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| {
            // i18n: the output of a system command
            let start = text.find("Version ")? + 8;
            Some(
                text[start..]
                    .trim_end_matches(|c: char| c == ']' || c.is_whitespace())
                    .to_owned(),
            )
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Level, Source};

    fn entry(target: &str, message: &str) -> Entry {
        Entry {
            id: 1,
            level: Level::Debug,
            source: Source::Ipc,
            target: target.into(),
            message: message.into(),
            timestamp: "2026-08-26T12:00:00.000Z".into(),
            duration_ms: Some(16.5),
            native_duration_ms: Some(0.1),
        }
    }

    #[test]
    fn keeps_every_log_field_as_selectable_tab_delimited_text() {
        let entry = Entry {
            level: Level::Warn,
            target: "items.query".into(),
            duration_ms: Some(20.),
            native_duration_ms: Some(3.),
            message: "A long message is not truncated".into(),
            ..entry("", "")
        };
        assert_eq!(
            entry_line(&entry),
            "2026-08-26T12:00:00.000Z\tWARN\tipc\titems.query\t3.0 ms native / 20.0 ms total\tA long message is not truncated"
        );
        let plain = Entry {
            native_duration_ms: None,
            ..entry.clone()
        };
        assert!(entry_line(&plain).contains("\t20.0 ms\t"));
    }

    #[test]
    fn aligns_display_columns_without_truncating_long_messages() {
        let short = display_line(&entry("settings.get", "short message"), "17:47:01.582");
        let long = display_line(
            &entry("tags.namespace_counts", "a complete long message"),
            "17:47:01.582",
        );
        assert_eq!(short.find("0.1 ms native"), long.find("0.1 ms native"));
        assert_eq!(
            short.find("short message"),
            long.find("a complete long message")
        );
        assert!(long.contains("a complete long message"));
        assert!(short.starts_with("17:47:01.582  DEBUG   ipc       settings.get"));
    }

    #[test]
    fn scrubs_common_personal_paths_and_credentials() {
        let personal = ["", "Users", "example-user", "Pictures"].join("/");
        assert_eq!(
            scrub(&format!(
                "{personal} test@example.com Bearer abc.def https://example.test/private?id=3"
            )),
            "~/Pictures <email> Bearer <redacted> https://example.test/<redacted>"
        );
        assert_eq!(scrub(r"C:\Users\someone\x"), r"~\x");
        assert_eq!(scrub("/home/someone"), "~");
        assert_eq!(scrub("nothing to hide"), "nothing to hide");
    }

    #[test]
    fn includes_worker_state_and_logs_in_one_report() {
        let report = build(
            &[],
            &[Worker {
                id: "thumb".into(),
                label: "Thumbnails".into(),
                state: "working".into(),
                detail: "one queued".into(),
                active: 1,
                queued: 1,
                attention: 0,
            }],
            "Picto/test",
            "en",
            "1.0.0",
            "darwin 25.0.0 arm64",
            "2026-08-26T12:00:00.000Z",
        );
        assert!(report.starts_with("Picto support report\nVersion\t1.0.0\nHost\tdarwin"));
        assert!(report.contains("Workers\nID\tName\tState"));
        assert!(report.contains("thumb\tThumbnails\tworking\t1\t1\t0\tone queued"));
        assert!(report.contains("Logs\nTimestamp\tLevel\tSource"));
        assert_eq!(
            file_name("2026-08-26T12:00:00.000Z"),
            "picto-support-2026-08-26T12-00-00-000Z.txt"
        );
    }
}
