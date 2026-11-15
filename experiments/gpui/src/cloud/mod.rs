//! Cloud sync, as production has it: a library publishes verified
//! snapshots of itself, and its originals, into the folder of a Google
//! Drive or Dropbox desktop app already installed on this device, which
//! carries them to the provider. Picto signs in to nothing: the desktop app
//! owns the account and the network. The core owns the sync (`core/src/
//! cloud/`, its commands `cloud.*` and its snapshot worker); this app only
//! asks it, as production's renderer does.
//!
//! - [`Cloud`] (`model.rs`): the open library's sync state, read as
//!   production's `CloudSyncBar`, `cloudSettle` and Settings read it.
//! - [`bar`]: the sync bar along the bottom of the main window and the
//!   blocking progress dialog while a joined library restores its media.
//! - Here: the providers' folders this device has (`cloud.providers.detect`,
//!   `cloud.provider.validate`) and the libraries found in them
//!   (`cloud.libraries.discover`), which need no library open, and how sync
//!   is presented (`librarySyncPresentation.ts`).
pub mod bar;
mod model;

pub use model::Cloud;
pub use picto_core::cloud::provider::DetectedProviderRoot as Root;
pub use picto_core::cloud::snapshot::RestorePoint;
pub use picto_core::cloud::{CloudConfiguration as Configuration, CloudLibraryOption};
pub use picto_core::cloud::CloudSyncStatus as Status;

use crate::i18n::t;

/// The provider's name as production writes it.
pub fn provider_name(provider: &str) -> &'static str {
    if provider == "google_drive" {
        t("Google Drive")
    } else {
        t("Dropbox")
    }
}

/// The provider's glyph (`IconBrandGoogleDrive`, `IconBrandDropbox`).
pub fn provider_icon(provider: &str) -> &'static str {
    if provider == "google_drive" {
        "cloud-brand-google-drive"
    } else {
        "cloud-brand-dropbox"
    }
}

/// A library found in a provider's folder, with the folder it is in
/// (`CloudLibraryChoice`).
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub root: Root,
    pub library: CloudLibraryOption,
}

/// The retention the Cloud section shows: daily, weekly and yearly
/// snapshots and the days deleted files are kept, production's defaults
/// where the stored value is missing.
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

/// The provider folders this device has (`cloud.providers.detect`).
///
/// While an automation seam is active (a check, a test build), only the
/// folders the core's own variables name (`PICTO_GOOGLE_DRIVE_ROOT`,
/// `PICTO_DROPBOX_ROOT`) are listed, read here, and the user's own desktop
/// apps are never looked at: no check lists or writes a real account's
/// folder. The core's validation still looks for installed apps under the
/// home folder, so a check that connects runs with `HOME` (`USERPROFILE`)
/// pointed elsewhere (README, "Cloud").
pub fn detect() -> Vec<Root> {
    if crate::seams::automation().is_some() {
        return named_roots();
    }
    picto_core::cloud::provider::detect_roots()
}

/// The folders `PICTO_GOOGLE_DRIVE_ROOT` and `PICTO_DROPBOX_ROOT` name, as
/// the core lists them.
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

/// `cloud.provider.validate`: the installed provider folder `path` is, or
/// why it is not one. Under automation only the named folders are taken.
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

/// `cloud.libraries.discover`: the Picto libraries in a provider's folder,
/// newest first. Reads only the folder's `picto/*/library.json`.
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

/// How a library's sync shows (`CloudSyncPresentation`).
#[derive(Clone, Debug, PartialEq)]
pub struct Presentation {
    pub active: bool,
    pub label: &'static str,
    pub tone: Tone,
    pub completed: i64,
    pub total: Option<i64>,
    pub remaining: Option<i64>,
    /// What the time estimate is measured against; it starts again when
    /// this changes.
    pub work_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Positive,
    Neutral,
    Warning,
    Negative,
}

/// `presentCloudSync`.
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

/// When a sync measured its remaining work (`SyncRateSample`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: std::time::Instant,
    pub remaining: i64,
}

/// `estimateRemainingSeconds`: from the rate the remaining work fell at.
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

/// The samples once `presentation` is taken in at `now`, and the estimate
/// (`LibraryManager`'s rate tracking).
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

/// `formatRemainingTime`.
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

/// `formatLastSync`: how long ago, or the date and time after a day.
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

/// `formatBytes` (Settings' and the sync bar's): `512 B`, `1.50 KB`,
/// `12.3 MB`.
pub fn bytes(value: i64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    // i18n: units production writes in English
    let units = ["KB", "MB", "GB", "TB"];
    let mut amount = value as f64 / 1024.;
    let mut unit = 0;
    while amount >= 1024. && unit < units.len() - 1 {
        amount /= 1024.;
        unit += 1;
    }
    format!(
        "{amount:.precision$} {}",
        units[unit],
        precision = if amount >= 10. { 1 } else { 2 }
    )
}
