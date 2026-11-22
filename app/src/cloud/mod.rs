//! Cloud sync. A library publishes verified snapshots of itself, and its
//! original files, into the folder of a Google Drive or Dropbox desktop app
//! that is installed on this device. That desktop app uploads them to the
//! provider. Picto signs in to nothing: the desktop app holds the account and
//! uses the network. The core does the sync (`core/src/cloud/`, its `cloud.*`
//! commands and its snapshot worker). This app only sends the core commands
//! and shows what the core answers.
//!
//! [`Cloud`] (`model.rs`) holds the sync state of the open library.
//!
//! [`bar`] draws the sync bar along the bottom of the main window, and the
//! progress dialog that blocks the window while a library joined from the
//! cloud restores its media.
//!
//! This file finds the provider folders on this device ([`detect`],
//! [`validate`]) and the libraries in them ([`discover`]). No library has to
//! be open for that. It also decides how the state of a sync is shown
//! ([`present`]).
pub mod bar;
mod model;

pub use model::Cloud;
pub use picto_core::cloud::provider::DetectedProviderRoot as Root;
pub use picto_core::cloud::snapshot::RestorePoint;
pub use picto_core::cloud::{CloudConfiguration as Configuration, CloudLibraryOption};
pub use picto_core::cloud::CloudSyncStatus as Status;

use crate::i18n::{en, t};

/// Returns the provider's name as the interface shows it, "Google Drive" or
/// "Dropbox", in the interface's language.
pub fn provider_name(provider: &str) -> &'static str {
    if provider == "google_drive" {
        t("Google Drive")
    } else {
        t("Dropbox")
    }
}

/// Returns the name of the provider's icon.
pub fn provider_icon(provider: &str) -> &'static str {
    if provider == "google_drive" {
        "cloud-brand-google-drive"
    } else {
        "cloud-brand-dropbox"
    }
}

/// A library that was found in a provider's folder, together with that folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub root: Root,
    pub library: CloudLibraryOption,
}

/// Returns the retention that the Cloud section shows: the values stored under
/// `daily`, `weekly` and `yearly` for the snapshots, and the number of days
/// for which deleted files are kept. Where the configuration holds no value,
/// the default is used: 30, 26, 5 and 7.
pub fn retention(configuration: &Configuration) -> [i64; 4] {
    let days = |key: &str, fallback: i64| {
        configuration
            .retention
            .get(key)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(fallback)
    };
    [
        days("daily", 30),
        days("weekly", 26),
        days("yearly", 5),
        days("deleted_blobs_days", 7),
    ]
}

/// Returns the provider folders that this device has. The core's command
/// `cloud.providers.detect` calls the same core function.
///
/// While an automation seam is active (in a check or a test build), it returns
/// only the folders that the variables `PICTO_GOOGLE_DRIVE_ROOT` and
/// `PICTO_DROPBOX_ROOT` name, and it does not look at the user's own desktop
/// apps. So no check lists or writes the folder of a real account. The core's
/// validation still looks for installed apps under the home folder, so a check
/// that connects a library must run with `HOME` (`USERPROFILE` on Windows) set
/// to another folder (`app/README.md`, "Cloud").
pub fn detect() -> Vec<Root> {
    if crate::seams::automation().is_some() {
        return named_roots();
    }
    picto_core::cloud::provider::detect_roots()
}

/// Returns the folders that `PICTO_GOOGLE_DRIVE_ROOT` and `PICTO_DROPBOX_ROOT`
/// name, each in the form in which the core lists a provider folder.
fn named_roots() -> Vec<Root> {
    [
        // i18n: the account labels the core gives these folders
        ("PICTO_GOOGLE_DRIVE_ROOT", "google_drive", "Google Drive"),
        // i18n: the account labels the core gives these folders
        ("PICTO_DROPBOX_ROOT", "dropbox", "Dropbox"),
    ]
    .into_iter()
    .filter_map(|(variable, provider, label)| {
        let path = std::path::PathBuf::from(std::env::var_os(variable)?);
        path.is_dir().then(|| Root {
            provider: provider.to_owned(),
            account_label: label.to_owned(),
            path: path.to_string_lossy().into_owned(),
        })
    })
    .collect()
}

/// Returns the installed provider folder that `path` is, or an error that says
/// why `path` is not one. The core's command `cloud.provider.validate` calls
/// the same core function. While an automation seam is active, only the
/// folders that `named_roots` returns are accepted.
pub fn validate(provider: &str, path: &std::path::Path) -> Result<Root, String> {
    if crate::seams::automation().is_some() {
        let chosen = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let named = named_roots().into_iter().find(|root| {
            root.provider == provider
                && std::path::Path::new(&root.path)
                    .canonicalize()
                    .is_ok_and(|named| named == chosen)
        });
        if named.is_none() {
            // i18n: the core's own message, in English
            let name = if provider == "google_drive" { "Google Drive" } else { "Dropbox" };
            return Err(format!(
                "The selected folder is not the root of an installed {name} account"
            ));
        }
    }
    picto_core::cloud::provider::validate_root(provider, path)
}

/// Returns the Picto libraries in a provider's folder, newest first. The
/// core's command `cloud.libraries.discover` calls the same core function. It
/// reads only the files `picto/*/library.json` in that folder.
pub fn discover(root: &Root) -> Result<Vec<Choice>, String> {
    let libraries = futures::executor::block_on(picto_core::cloud::discover_libraries(
        &root.provider,
        &root.path,
    ))?;
    Ok(libraries
        .into_iter()
        .map(|library| Choice {
            root: root.clone(),
            library,
        })
        .collect())
}

/// Returns the message that the Library Manager shows after Delete Cloud
/// Data…. `removal` is the answer of the core's command `cloud.data.delete`.
/// When the library's folder is still in the cloud folder, the message
/// counts the files and folders that were left in it and names the first of
/// them: Picto did not write them, or it could not remove them. The locale
/// files have no keys for these texts, so they are shown in English in
/// every language.
pub fn removal_message(removal: &serde_json::Value) -> String {
    let removed = en("Cloud data deleted. Cloud sync is off for this library.");
    let removal: picto_core::cloud::provider::CloudDataRemoval =
        match serde_json::from_value(removal.clone()) {
            Ok(removal) => removal,
            Err(_) => return removed.to_owned(),
        };
    if removal.folder_removed || removal.kept_entries == 0 {
        return removed.to_owned();
    }
    let mut names = removal.kept.join(", ");
    if removal.kept_entries > removal.kept.len() as u64 {
        names.push_str(", …");
    }
    crate::i18n::fill(
        en("Cloud data deleted. Cloud sync is off for this library. The library's folder in the cloud folder was not removed, because it still holds {value0} files or folders that Picto did not write or could not remove: {value1}"),
        &[&removal.kept_entries, &names],
    )
}

/// How the state of a library's sync is shown: whether a sync is running, the
/// label and its tone, and the progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Presentation {
    pub active: bool,
    pub label: &'static str,
    pub tone: Tone,
    pub completed: i64,
    pub total: Option<i64>,
    pub remaining: Option<i64>,
    /// Identifies the work that the time estimate measures. When it changes,
    /// the estimate starts again.
    pub work_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Positive,
    Neutral,
    Warning,
    Negative,
}

/// Returns how `status` is shown. Without a status the label is "Unavailable".
/// A sync is active while the state is `reconciling`. The progress is exact
/// when the status has a total number of units. Otherwise the remaining work
/// of an active sync is the number of pending changes plus the number of
/// pending files.
pub fn present(status: Option<&Status>) -> Presentation {
    let Some(status) = status else {
        return Presentation {
            active: false,
            label: t("Unavailable"),
            tone: Tone::Neutral,
            completed: 0,
            total: None,
            remaining: None,
            work_key: None,
        };
    };
    let pending = status.pending_mutations.max(0) + status.pending_blobs.max(0);
    let exact = status.total_units.is_some_and(|total| total > 0);
    let active = status.state == "reconciling";
    let completed = if exact {
        status.completed_units.max(0)
    } else {
        0
    };
    let total = exact.then(|| completed.max(status.total_units.unwrap_or(0)));
    let remaining = match total {
        Some(total) => Some((total - completed).max(0)),
        None if active => Some(pending),
        None => None,
    };
    let work_key = match total {
        Some(total) => Some(format!("phase:{}:{total}", status.phase)),
        // i18n: a key the estimate is measured against, not shown
        None if active => Some("pending".to_owned()),
        None => None,
    };
    let idle = |label, tone| Presentation {
        active: false,
        label,
        tone,
        completed,
        total,
        remaining: None,
        work_key: None,
    };
    match status.state.as_str() {
        "error" => idle(t("Needs attention"), Tone::Negative),
        "offline" => idle(t("Offline"), Tone::Warning),
        "paused" => idle(t("Paused"), Tone::Neutral),
        _ if active => Presentation {
            active: true,
            label: t("Syncing"),
            tone: Tone::Positive,
            completed,
            total,
            remaining,
            work_key,
        },
        _ => idle(t("Idle"), Tone::Neutral),
    }
}

/// The remaining work of a sync at one moment. The time estimate is computed
/// from several of these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: std::time::Instant,
    pub remaining: i64,
}

/// Returns the estimated number of seconds until `remaining` units are done,
/// or `None` if there is no estimate yet. The rate is taken from two samples:
/// the latest one, and the earliest one that had more remaining work. The
/// estimate is `remaining` divided by the work done between them per second.
/// At least one second must lie between the two samples.
pub fn estimate_remaining(samples: &[Sample], remaining: i64) -> Option<u64> {
    if remaining <= 0 || samples.len() < 2 {
        return None;
    }
    let latest = samples.last()?;
    let baseline = samples
        .iter()
        .find(|sample| sample.remaining > latest.remaining)?;
    let elapsed = latest.at.duration_since(baseline.at).as_secs_f64();
    let completed = (baseline.remaining - latest.remaining) as f64;
    if elapsed < 1. || completed <= 0. {
        return None;
    }
    Some(((remaining as f64) / (completed / elapsed)).ceil().max(1.) as u64)
}

/// Adds the remaining work of `presentation` at the time `now` to `samples`,
/// and returns the estimated number of remaining seconds. The samples start
/// again when the sync is not active, when the presentation's `work_key`
/// differs from `key`, or when the remaining work has grown. Samples older
/// than 30 seconds are removed, and at most 12 samples are kept.
pub fn track(samples: &mut Vec<Sample>, key: &mut Option<String>, presentation: &Presentation, now: std::time::Instant) -> Option<u64> {
    let (Some(remaining), true) = (presentation.remaining, presentation.active) else {
        samples.clear();
        *key = None;
        return None;
    };
    if presentation.work_key != *key {
        *key = presentation.work_key.clone();
        *samples = vec![Sample { at: now, remaining }];
        return None;
    }
    match samples.last() {
        Some(previous) if remaining <= previous.remaining => {
            if remaining != previous.remaining {
                samples.retain(|sample| now.duration_since(sample.at).as_secs() <= 30);
                samples.push(Sample { at: now, remaining });
                let excess = samples.len().saturating_sub(12);
                samples.drain(..excess);
            }
        }
        _ => {
            *samples = vec![Sample { at: now, remaining }];
            return None;
        }
    }
    estimate_remaining(samples, remaining)
}

/// Returns the estimate as text: "Less than a minute remaining" below 60
/// seconds, minutes below an hour, and hours with minutes otherwise. Returns
/// `None` when there is no estimate.
pub fn remaining_time(seconds: Option<u64>) -> Option<String> {
    let seconds = seconds?;
    Some(if seconds < 60 {
        t("Less than a minute remaining").to_owned()
    } else if seconds < 3600 {
        crate::tf!("About {value0} min remaining", seconds.div_ceil(60))
    } else {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600).div_ceil(60);
        if minutes > 0 {
            crate::tf!("About {value0} hr {value1} min remaining", hours, minutes)
        } else {
            crate::tf!("About {value0} hr remaining", hours)
        }
    })
}

/// Returns the time of the last sync as text: "Never" when there was none, how
/// long ago it was when that is less than a day, and the date and the time
/// otherwise. A value that is not an RFC 3339 time is returned unchanged.
pub fn last_sync(value: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> String {
    let Some(value) = value else {
        return t("Never").to_owned();
    };
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(value) else {
        return value.to_owned();
    };
    let minutes = (now - at.with_timezone(&chrono::Utc)).num_minutes().max(0);
    if minutes < 1 {
        return t("Just now").to_owned();
    }
    if minutes < 60 {
        return crate::tf!("{value0} min ago", minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return crate::tf!("{value0} hr ago", hours);
    }
    crate::i18n::format::date_time(at.with_timezone(&chrono::Local).naive_local())
}

#[cfg(test)]
mod tests {
    use super::removal_message;

    /// The message after Delete Cloud Data… says that the cloud data is
    /// deleted, and it names what was left when the folder could not be
    /// removed.
    #[test]
    fn the_message_after_delete_cloud_data_names_what_was_left() {
        let gone = serde_json::json!({
            "removed_files": 9, "folder_removed": true, "kept_entries": 0, "kept": [],
        });
        assert_eq!(
            removal_message(&gone),
            "Cloud data deleted. Cloud sync is off for this library."
        );
        let left = serde_json::json!({
            "removed_files": 9, "folder_removed": false, "kept_entries": 3,
            "kept": ["notes.txt", "extra"],
        });
        let message = removal_message(&left);
        assert!(message.contains("still holds 3 files or folders"), "{message}");
        assert!(message.ends_with("notes.txt, extra, …"), "{message}");
    }
}
