//! Fills this app's list of libraries on the first run from the list of an
//! Electron-based Picto (version 0.6.19 or earlier). Such a Picto keeps its
//! list in `config.json`: `libraryHistory`, `pinnedLibraries`, and each
//! library's colour and icon in `libraryMeta`. The file is in the folder
//! `picto` of the per-user application data: on macOS
//! `~/Library/Application Support/picto/config.json`, on Windows
//! `%APPDATA%\picto\config.json`, on Linux
//! `$XDG_CONFIG_HOME/picto/config.json` or `~/.config/picto/config.json`
//! (`state::config_folder`).
//!
//! This happens once only, when this app's own list is empty and was never
//! seeded before. It happens only in a process that is allowed to list
//! libraries the user keeps (`Facts::permits`: never in a debug build that was
//! started without the flag, and never under automation). The file is only
//! read, and its entries are copied into this app's own state.
//! `librariesSeeded` records that this was done, so it is never done again.
//! Seeding only adds libraries to the list: it gives no consent, opens no
//! library and sets no last library. If the file is missing, unreadable or
//! malformed, nothing is seeded and no error is reported.
use std::path::{Path, PathBuf};

/// The libraries that the `config.json` of an Electron-based Picto (version
/// 0.6.19 or earlier) lists, with their colours and icons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seed {
    pub history: Vec<String>,
    pub pinned: Vec<String>,
    /// The colour and icon of each library from `libraryMeta`, keyed by the
    /// library's path.
    pub looks: std::collections::BTreeMap<String, crate::state::LibraryLook>,
}

/// Returns the path of the `config.json` of an Electron-based Picto (version
/// 0.6.19 or earlier) on this platform, or `None` when the per-user
/// configuration folder is not known.
pub fn earlier_version_config() -> Option<PathBuf> {
    // i18n: the names of a folder and of a file on disk
    crate::state::config_folder().map(|folder| folder.join("picto").join("config.json"))
}

/// Returns the libraries that the `config.json` at `path` lists, or `None`
/// when the file is missing, unreadable or not a JSON object. Only entries
/// that are absolute paths are taken, each path once; other entries are
/// skipped. A colour or icon is taken only for a library that is in the
/// history or pinned.
pub fn read(path: &Path) -> Option<Seed> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let object = value.as_object()?;
    let paths = |key: &str| {
        let mut paths: Vec<String> = Vec::new();
        for entry in object
            .get(key)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(path) = entry.as_str()
                && Path::new(path).is_absolute()
                && !paths.iter().any(|known| known == path)
            {
                paths.push(path.to_owned());
            }
        }
        paths
    };
    let history = paths("libraryHistory");
    let pinned = paths("pinnedLibraries");
    let looks = object
        .get("libraryMeta")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(path, _)| history.contains(path) || pinned.contains(path))
        .map(|(path, look)| (path.clone(), crate::state::LibraryLook::from_value(look)))
        .filter(|(_, look)| !look.is_default())
        .collect();
    Some(Seed {
        history,
        pinned,
        looks,
    })
}

/// Copies the libraries of `seed` into `state`, if `state` was never seeded
/// and its list of libraries is empty. It sets `libraries_seeded` whenever it
/// was not set, whatever `seed` returns, so seeding is tried only once.
/// Returns whether any library was added to the list.
pub fn apply(state: &mut crate::state::State, seed: impl FnOnce() -> Option<Seed>) -> bool {
    if state.libraries_seeded {
        return false;
    }
    state.libraries_seeded = true;
    if !state.library_history.is_empty() || !state.pinned_libraries.is_empty() {
        return false;
    }
    let Some(seed) = seed() else {
        return false;
    };
    state.library_history = seed.history;
    state.pinned_libraries = seed.pinned;
    state.library_meta.extend(seed.looks);
    !state.library_history.is_empty() || !state.pinned_libraries.is_empty()
}

/// Called when the app starts. Seeds the list of libraries, if it was never
/// seeded and this process is allowed to list libraries the user keeps.
pub fn once(cx: &mut gpui::App) {
    if crate::state::get(cx).libraries_seeded || super::facts().permits().is_err() {
        return;
    }
    let Some(path) = earlier_version_config() else {
        return;
    };
    crate::state::update(cx, |state| {
        apply(state, || read(&path));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;

    fn fixture(text: &str) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("config.json");
        std::fs::write(&path, text).unwrap();
        (folder, path)
    }

    #[test]
    fn the_list_is_read_from_the_earlier_versions_keys() {
        let (_folder, path) = fixture(
            r##"{
              "libraryHistory": ["/home/me/Pictures/Main.library", "/Volumes/Art/Art.library",
                                 "relative.library", 3, "/home/me/Pictures/Main.library"],
              "pinnedLibraries": ["/Volumes/Art/Art.library"],
              "cloudLocations": {},
              "lastLibrary": "/home/me/Pictures/Main.library",
              "windowState": { "main": null },
              "libraryMeta": {
                "/Volumes/Art/Art.library": { "color": "#40C057", "icon": "IconStar", "imageHash": "ab" },
                "/Elsewhere/Unlisted.library": { "color": "#40C057" }
              }
            }"##,
        );
        let seed = read(&path).unwrap();
        if cfg!(windows) {
            // On Windows a Unix path is not absolute, so no entry is taken.
            assert!(seed.history.is_empty());
            return;
        }
        assert_eq!(
            seed.history,
            [
                "/home/me/Pictures/Main.library",
                "/Volumes/Art/Art.library"
            ]
        );
        assert_eq!(seed.pinned, ["/Volumes/Art/Art.library"]);
        assert_eq!(
            seed.looks.into_iter().collect::<Vec<_>>(),
            [(
                "/Volumes/Art/Art.library".to_owned(),
                crate::state::LibraryLook {
                    color: Some("#40C057".into()),
                    icon: Some("IconStar".into()),
                    cloud_library_id: None,
                }
            )]
        );
    }

    #[test]
    fn a_missing_unreadable_or_malformed_file_seeds_nothing() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(read(&folder.path().join("config.json")), None);
        // The path is a folder, not a file.
        assert_eq!(read(folder.path()), None);
        for text in ["", "{", "[1, 2]", "null", "\"text\""] {
            let (_folder, path) = fixture(text);
            assert_eq!(read(&path), None, "{text}");
        }
        // When `libraryHistory` and `pinnedLibraries` hold values of the
        // wrong type, no library is listed.
        let (_folder, path) = fixture(r#"{"libraryHistory": "x", "pinnedLibraries": {}}"#);
        assert_eq!(read(&path), Some(Seed::default()));
        let mut state = State::default();
        assert!(!apply(&mut state, || read(&path)));
        assert!(state.libraries_seeded);
        assert!(state.library_history.is_empty());
    }

    #[test]
    fn seeding_happens_once_into_an_empty_list_and_only_lists() {
        let seed = || {
            Some(Seed {
                history: vec!["/a/Main.library".into(), "/b/Art.library".into()],
                pinned: vec!["/b/Art.library".into()],
                looks: Default::default(),
            })
        };
        let mut state = State::default();
        assert!(apply(&mut state, seed));
        assert_eq!(state.library_history, ["/a/Main.library", "/b/Art.library"]);
        assert_eq!(state.pinned_libraries, ["/b/Art.library"]);
        assert!(state.libraries_seeded);
        // Seeding only adds libraries to the list. It sets no last library and
        // gives no consent.
        assert_eq!(state.last_library, None);
        assert!(state.library_consents.is_empty());
        // Seeding does not happen a second time, even after the list was
        // emptied.
        state.library_history.clear();
        state.pinned_libraries.clear();
        assert!(!apply(&mut state, seed));
        assert!(state.library_history.is_empty());
        // A list that this app already has is not replaced. The state is
        // marked as seeded, so it is not seeded later either.
        let mut own = State {
            library_history: vec!["/c/Own.library".into()],
            ..State::default()
        };
        assert!(!apply(&mut own, seed));
        assert_eq!(own.library_history, ["/c/Own.library"]);
        assert!(own.libraries_seeded);
        // When the file is missing, the state is still marked as seeded, so
        // seeding is not tried again.
        let mut fresh = State::default();
        assert!(!apply(&mut fresh, || None));
        assert!(fresh.libraries_seeded);
    }

    /// The test process is a test build, which counts as automation, so `once`
    /// never reads the user's real file.
    #[test]
    fn a_process_that_cannot_list_kept_libraries_never_reads_the_file() {
        assert!(crate::libraries::facts().permits().is_err());
        let cx = gpui::TestAppContext::single();
        cx.update(|cx| {
            crate::state::init(cx);
            once(cx);
            assert!(!crate::state::get(cx).libraries_seeded);
        });
    }
}
