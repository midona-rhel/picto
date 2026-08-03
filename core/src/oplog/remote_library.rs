//! Remote library registry on a file share.
//!
//! Layout on the share: `<share root>/Picto/<library name>/`, one directory
//! per library, each holding a `picto-library.json` manifest plus the sync
//! data (`oplog/…`). Multiple libraries per share are expected.
//!
//! Cardinal rule: this module never deletes or overwrites anything remote.
//! A name collision is the user's to resolve — delete it themselves on the
//! share, or pick another name.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::backend::SyncBackend;
use super::backend_fs::FsBackend;

pub const PICTO_DIR: &str = "Picto";
pub const MANIFEST_KEY: &str = "picto-library.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteLibraryManifest {
    pub format_version: u32,
    pub library_uuid: String,
    pub name: String,
    pub created_at: String,
    pub created_by_device: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteLibraryInfo {
    pub name: String,
    pub library_uuid: Option<String>,
    pub created_at: Option<String>,
    /// False when the directory exists but its manifest is missing/unreadable.
    pub valid: bool,
}

pub fn remote_library_root(share_root: &Path, name: &str) -> PathBuf {
    share_root.join(PICTO_DIR).join(name)
}

fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Library name cannot be empty".into());
    }
    if trimmed.contains(['/', '\\', ':']) || trimmed.starts_with('.') {
        return Err("Library name cannot contain path separators or start with a dot".into());
    }
    Ok(())
}

/// Enumerate libraries under `<share>/Picto/`. A missing `Picto/` directory
/// is an empty list, not an error.
pub fn list_remote_libraries(share_root: &Path) -> Result<Vec<RemoteLibraryInfo>, String> {
    let picto = share_root.join(PICTO_DIR);
    let Ok(entries) = fs::read_dir(&picto) else {
        return Ok(Vec::new());
    };
    let mut libraries = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        match read_remote_manifest(share_root, &name) {
            Ok(manifest) => libraries.push(RemoteLibraryInfo {
                name,
                library_uuid: Some(manifest.library_uuid),
                created_at: Some(manifest.created_at),
                valid: true,
            }),
            Err(_) => libraries.push(RemoteLibraryInfo {
                name,
                library_uuid: None,
                created_at: None,
                valid: false,
            }),
        }
    }
    libraries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(libraries)
}

pub fn read_remote_manifest(
    share_root: &Path,
    name: &str,
) -> Result<RemoteLibraryManifest, String> {
    let path = remote_library_root(share_root, name).join(MANIFEST_KEY);
    let json = fs::read_to_string(&path)
        .map_err(|e| format!("No readable manifest at {}: {e}", path.display()))?;
    serde_json::from_str(&json).map_err(|e| format!("Invalid library manifest: {e}"))
}

/// Create a new remote library directory + manifest. Refuses if the
/// directory already exists in any form — never deletes, never overwrites.
pub fn create_remote_library(
    share_root: &Path,
    manifest: &RemoteLibraryManifest,
) -> Result<(), String> {
    validate_name(&manifest.name)?;
    let dir = remote_library_root(share_root, &manifest.name);
    if dir.exists() {
        return Err(format!(
            "A library named \"{}\" already exists on this share. Picto never deletes or \
             overwrites remote libraries — remove it yourself on the share, connect to it \
             instead, or choose a different name.",
            manifest.name
        ));
    }
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create library directory: {e}"))?;
    let backend = FsBackend::open(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    backend
        .put(MANIFEST_KEY, &json)
        .map_err(|e| format!("Failed to write library manifest: {e}"))?;
    Ok(())
}

// ── Share root detection ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ShareRootCandidate {
    /// User-facing service label, e.g. "Google Drive (alice@…)".
    pub label: String,
    pub path: String,
    pub provider: String,
}

/// Detect sync-client folders on this machine so the user picks a service,
/// not a filesystem path. The manual folder picker remains the fallback.
pub fn detect_share_roots() -> Vec<ShareRootCandidate> {
    let Some(home) =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
    else {
        return Vec::new();
    };
    detect_share_roots_in(&home)
}

fn detect_share_roots_in(home: &Path) -> Vec<ShareRootCandidate> {
    let mut found = Vec::new();
    let mut push = |label: String, path: PathBuf, provider: &str| {
        if path.is_dir() {
            found.push(ShareRootCandidate {
                label,
                path: path.display().to_string(),
                provider: provider.to_string(),
            });
        }
    };

    // macOS file-provider mounts: ~/Library/CloudStorage/<Provider>-<account>/
    let cloud_storage = home.join("Library").join("CloudStorage");
    if let Ok(entries) = fs::read_dir(&cloud_storage) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(account) = name.strip_prefix("GoogleDrive-") {
                push(
                    format!("Google Drive ({account})"),
                    entry.path().join("My Drive"),
                    "google-drive",
                );
            } else if let Some(account) = name.strip_prefix("OneDrive-") {
                push(format!("OneDrive ({account})"), entry.path(), "onedrive");
            } else if name.starts_with("Dropbox") {
                push("Dropbox".to_string(), entry.path(), "dropbox");
            }
        }
    }

    push("Dropbox".to_string(), home.join("Dropbox"), "dropbox");
    push(
        "iCloud Drive".to_string(),
        home.join("Library")
            .join("Mobile Documents")
            .join("com~apple~CloudDocs"),
        "icloud",
    );
    #[cfg(windows)]
    {
        for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
            if let Some(path) = std::env::var_os(var) {
                push("OneDrive".to_string(), PathBuf::from(path), "onedrive");
            }
        }
        // Drive for Desktop mounts as a drive letter holding "My Drive".
        for letter in b'D'..=b'Z' {
            let candidate = PathBuf::from(format!("{}:\\My Drive", letter as char));
            if candidate.is_dir() {
                push(
                    format!("Google Drive ({}:)", letter as char),
                    candidate,
                    "google-drive",
                );
            }
        }
        // Legacy Backup & Sync location.
        push(
            "Google Drive".to_string(),
            home.join("Google Drive"),
            "google-drive",
        );
    }

    // A provider can be discovered twice (e.g. Dropbox via CloudStorage and
    // ~/Dropbox) — keep the first hit per path.
    found.dedup_by(|a, b| a.path == b.path);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manifest(name: &str, uuid: &str) -> RemoteLibraryManifest {
        RemoteLibraryManifest {
            format_version: 1,
            library_uuid: uuid.into(),
            name: name.into(),
            created_at: "2026-07-17T00:00:00Z".into(),
            created_by_device: "dev-test".into(),
        }
    }

    #[test]
    fn create_list_and_read_round_trip() {
        let share = TempDir::new().unwrap();
        create_remote_library(share.path(), &manifest("Main", "uuid-1")).unwrap();
        create_remote_library(share.path(), &manifest("Second", "uuid-2")).unwrap();

        let listed = list_remote_libraries(share.path()).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Main");
        assert_eq!(listed[0].library_uuid.as_deref(), Some("uuid-1"));
        assert!(listed.iter().all(|l| l.valid));

        let read = read_remote_manifest(share.path(), "Second").unwrap();
        assert_eq!(read.library_uuid, "uuid-2");
    }

    #[test]
    fn existing_library_is_never_touched() {
        let share = TempDir::new().unwrap();
        create_remote_library(share.path(), &manifest("Main", "uuid-1")).unwrap();
        let err = create_remote_library(share.path(), &manifest("Main", "uuid-other")).unwrap_err();
        assert!(err.contains("never deletes"), "got: {err}");
        // Original manifest is intact.
        assert_eq!(
            read_remote_manifest(share.path(), "Main")
                .unwrap()
                .library_uuid,
            "uuid-1"
        );
        // Even a bare directory (no manifest) blocks creation.
        std::fs::create_dir_all(share.path().join(PICTO_DIR).join("Bare")).unwrap();
        assert!(create_remote_library(share.path(), &manifest("Bare", "u")).is_err());
    }

    #[test]
    fn detects_known_sync_client_folders() {
        let home = TempDir::new().unwrap();
        let gdrive = home
            .path()
            .join("Library/CloudStorage/GoogleDrive-alice@example.com/My Drive");
        std::fs::create_dir_all(&gdrive).unwrap();
        std::fs::create_dir_all(home.path().join("Dropbox")).unwrap();

        let found = detect_share_roots_in(home.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].provider, "google-drive");
        assert!(found[0].label.contains("alice@example.com"));
        assert_eq!(found[0].path, gdrive.display().to_string());
        assert_eq!(found[1].provider, "dropbox");
    }

    #[test]
    fn invalid_names_rejected_and_broken_manifests_flagged() {
        let share = TempDir::new().unwrap();
        assert!(create_remote_library(share.path(), &manifest("a/b", "u")).is_err());
        assert!(create_remote_library(share.path(), &manifest("", "u")).is_err());

        let broken = share.path().join(PICTO_DIR).join("Broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(MANIFEST_KEY), b"not json").unwrap();
        let listed = list_remote_libraries(share.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].valid);
    }
}
