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
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::backend::SyncBackend;
use super::backend_fs::FsBackend;

pub const PICTO_DIR: &str = "Picto";
pub const MANIFEST_KEY: &str = "picto-library.json";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;

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

/// Resolve a remote library directory without allowing the library name to
/// escape `<share>/Picto`.
pub fn checked_remote_library_root(share_root: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;

    let picto = share_root.join(PICTO_DIR);
    validate_picto_directory(&picto)?;

    let root = picto.join(name);
    debug_assert_eq!(root.parent(), Some(picto.as_path()));

    match fs::symlink_metadata(&root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "Remote library directory is a symlink and is not allowed: {}",
                    root.display()
                ));
            }
            if !metadata.is_dir() {
                return Err(format!(
                    "Remote library path is not a directory: {}",
                    root.display()
                ));
            }

            // Check the resolved parent as well as the lexical path. This
            // keeps an existing directory from escaping through a replaced
            // or redirected parent.
            let resolved_picto = fs::canonicalize(&picto)
                .map_err(|e| format!("Cannot resolve remote Picto directory: {e}"))?;
            let resolved_root = fs::canonicalize(&root)
                .map_err(|e| format!("Cannot resolve remote library directory: {e}"))?;
            if resolved_root.parent() != Some(resolved_picto.as_path()) {
                return Err(format!(
                    "Remote library directory must be directly under {}",
                    picto.display()
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Cannot inspect remote library directory {}: {error}",
                root.display()
            ));
        }
    }

    Ok(root)
}

fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Library name cannot be empty".into());
    }
    if trimmed != name {
        return Err("Library name cannot begin or end with whitespace".into());
    }
    let path = Path::new(name);
    if path.is_absolute()
        || name.contains(['/', '\\', ':'])
        || name.starts_with('.')
        || name.contains('\0')
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.components().count() != 1
    {
        return Err(
            "Library name must be one relative directory name without path separators".into(),
        );
    }
    Ok(())
}

fn validate_picto_directory(picto: &Path) -> Result<(), String> {
    match fs::symlink_metadata(picto) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "Remote Picto directory cannot be a symlink: {}",
            picto.display()
        )),
        Ok(metadata) if !metadata.is_dir() => Err(format!(
            "Remote Picto path is not a directory: {}",
            picto.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Cannot inspect remote Picto directory {}: {error}",
            picto.display()
        )),
    }
}

/// Enumerate libraries under `<share>/Picto/`. A missing `Picto/` directory
/// is an empty list, not an error.
pub fn list_remote_libraries(share_root: &Path) -> Result<Vec<RemoteLibraryInfo>, String> {
    let picto = share_root.join(PICTO_DIR);
    validate_picto_directory(&picto)?;
    let entries = match fs::read_dir(&picto) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot list remote Picto directory: {error}")),
    };
    let mut libraries = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || validate_name(&name).is_err() {
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
    open_remote_library(share_root, name).map(|(manifest, _)| manifest)
}

pub fn open_remote_library(
    share_root: &Path,
    name: &str,
) -> Result<(RemoteLibraryManifest, FsBackend), String> {
    let root = checked_remote_library_root(share_root, name)?;
    let picto = share_root.join(PICTO_DIR);
    let backend = FsBackend::open_existing_contained(&root, &picto)
        .map_err(|error| format!("Cannot open remote library: {error}"))?;
    let bytes = backend
        .get_limited(MANIFEST_KEY, MAX_MANIFEST_BYTES)
        .map_err(|error| format!("Cannot read remote library manifest: {error}"))?
        .ok_or_else(|| "Remote library manifest is missing".to_string())?;
    let json = std::str::from_utf8(&bytes)
        .map_err(|e| format!("Remote library manifest is not UTF-8: {e}"))?;
    let manifest: RemoteLibraryManifest =
        serde_json::from_str(&json).map_err(|e| format!("Invalid library manifest: {e}"))?;
    if manifest.format_version != 1 {
        return Err(format!(
            "Unsupported remote library format {}; this build requires format 1",
            manifest.format_version
        ));
    }
    if manifest.name != name {
        return Err(format!(
            "Remote library manifest name {:?} does not match directory {:?}",
            manifest.name, name
        ));
    }
    if manifest.library_uuid.trim().is_empty() {
        return Err("Remote library manifest has no library identity".into());
    }
    Ok((manifest, backend))
}

/// Create a new remote library directory + manifest. Refuses if the
/// directory already exists in any form — never deletes, never overwrites.
pub fn create_remote_library(
    share_root: &Path,
    manifest: &RemoteLibraryManifest,
) -> Result<(), String> {
    validate_name(&manifest.name)?;
    let mut dir = checked_remote_library_root(share_root, &manifest.name)?;
    if fs::symlink_metadata(&dir).is_ok() {
        return Err(format!(
            "A library named \"{}\" already exists on this share. Picto never deletes or \
             overwrites remote libraries — remove it yourself on the share, connect to it \
             instead, or choose a different name.",
            manifest.name
        ));
    }
    let picto = share_root.join(PICTO_DIR);
    fs::create_dir_all(&picto)
        .map_err(|e| format!("Failed to create remote Picto directory: {e}"))?;
    dir = checked_remote_library_root(share_root, &manifest.name)?;
    match fs::create_dir(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!(
                "A library named \"{}\" already exists on this share. Picto never deletes or \
                 overwrites remote libraries — remove it yourself on the share, connect to it \
                 instead, or choose a different name.",
                manifest.name
            ));
        }
        Err(error) => return Err(format!("Failed to create library directory: {error}")),
    }
    let backend = FsBackend::open_existing_contained(&dir, &picto).map_err(|e| e.to_string())?;
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

    #[test]
    fn checked_root_rejects_traversal_and_absolute_names() {
        let share = TempDir::new().unwrap();
        for name in ["../outside", "/tmp/outside", "C:\\outside", "..", "."] {
            assert!(
                checked_remote_library_root(share.path(), name).is_err(),
                "name should be rejected: {name}"
            );
            assert!(
                read_remote_manifest(share.path(), name).is_err(),
                "read should reject name: {name}"
            );
            assert!(
                create_remote_library(share.path(), &manifest(name, "u")).is_err(),
                "create should reject name: {name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_library_directories_are_not_listed_or_followed() {
        use std::os::unix::fs::symlink;

        let share = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        create_remote_library(outside.path(), &manifest("Outside", "outside-uuid")).unwrap();

        let picto = share.path().join(PICTO_DIR);
        std::fs::create_dir_all(&picto).unwrap();
        symlink(
            outside.path().join(PICTO_DIR).join("Outside"),
            picto.join("Linked"),
        )
        .unwrap();

        assert!(list_remote_libraries(share.path()).unwrap().is_empty());
        assert!(checked_remote_library_root(share.path(), "Linked").is_err());
        assert!(read_remote_manifest(share.path(), "Linked").is_err());
        assert!(create_remote_library(share.path(), &manifest("Linked", "new")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_picto_directory_is_rejected() {
        use std::os::unix::fs::symlink;

        let share = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        symlink(outside.path(), share.path().join(PICTO_DIR)).unwrap();

        assert!(list_remote_libraries(share.path()).is_err());
        assert!(checked_remote_library_root(share.path(), "Main").is_err());
        assert!(create_remote_library(share.path(), &manifest("Main", "u")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_manifest_is_rejected() {
        use std::os::unix::fs::symlink;

        let share = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let library = share.path().join(PICTO_DIR).join("Main");
        std::fs::create_dir_all(&library).unwrap();
        let external_manifest = outside.path().join("manifest.json");
        std::fs::write(
            &external_manifest,
            serde_json::to_vec(&manifest("Main", "u")).unwrap(),
        )
        .unwrap();
        symlink(&external_manifest, library.join(MANIFEST_KEY)).unwrap();

        assert!(read_remote_manifest(share.path(), "Main").is_err());
        let listed = list_remote_libraries(share.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].valid);
    }
}
