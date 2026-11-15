//! The first run's list of libraries, seeded from the Electron app's. That
//! app keeps its list in `config.json` (`globalConfig.mjs`:
//! `libraryHistory`, `pinnedLibraries`, and each library's colour and icon
//! in `libraryMeta`) in its folder of the per-user
//! application data (`app.getPath('appData')`/`picto`): on macOS
//! `~/Library/Application Support/picto/config.json`, on Windows
//! `%APPDATA%\picto\config.json`, on Linux
//! `$XDG_CONFIG_HOME/picto/config.json` or `~/.config/picto/config.json`
//! (`state::config_folder`).
//!
//! Once only (this app's own list empty and never seeded before), and only
//! in a process that may list libraries the user keeps at all
//! (`Facts::permits`: never in a debug build without the flag, never under
//! automation), the file is read, read-only, and its entries copied into
//! this app's own state; that it was done is kept (`librariesSeeded`), so it
//! never is again. Seeding only lists: it gives no consent, opens nothing
//! and sets no last library. A missing, unreadable or malformed file seeds
//! nothing and is not an error.
use std::path::{Path, PathBuf};

/// What a production `config.json` lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seed {
    pub history: Vec<String>,
    pub pinned: Vec<String>,
    /// `libraryMeta`'s colours and icons, by path.
    pub looks: std::collections::BTreeMap<String, crate::state::LibraryLook>,
}

/// Where production keeps its `config.json` on this platform.
pub fn production_config() -> Option<PathBuf> {
    // i18n: a folder and a file name production writes
    crate::state::config_folder().map(|folder| folder.join("picto").join("config.json"))
}

/// The libraries the `config.json` at `path` lists, or `None` when it is
/// missing, unreadable or not what production writes. Only absolute paths
/// are taken, each once; anything else in an entry is left out.
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

/// Seeds `state` from `seed` if it never was and its list is empty, and
/// records that it was tried, whatever the file held. Hands back whether
/// anything was listed.
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

/// At start: seeds the list once, in a process that may list libraries the
/// user keeps.
pub fn once(cx: &mut gpui::App) {
    if crate::state::get(cx).libraries_seeded || super::facts().permits().is_err() {
        return;
    }
    let Some(path) = production_config() else {
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
    fn the_list_is_read_from_productions_keys() {
        let (_folder, path) = fixture(
            r##"{
              "libraryHistory": ["/Users/me/Pictures/Main.library", "/Volumes/Art/Art.library",
                                 "relative.library", 3, "/Users/me/Pictures/Main.library"],
              "pinnedLibraries": ["/Volumes/Art/Art.library"],
              "cloudLocations": {},
              "lastLibrary": "/Users/me/Pictures/Main.library",
              "windowState": { "main": null },
              "libraryMeta": {
                "/Volumes/Art/Art.library": { "color": "#40C057", "icon": "IconStar", "imageHash": "ab" },
                "/Elsewhere/Unlisted.library": { "color": "#40C057" }
              }
            }"##,
        );
        let seed = read(&path).unwrap();
        if cfg!(windows) {
            // Unix paths are not absolute there.
            assert!(seed.history.is_empty());
            return;
        }
        assert_eq!(
            seed.history,
            [
                "/Users/me/Pictures/Main.library",
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
        // A folder where the file should be.
        assert_eq!(read(folder.path()), None);
        for text in ["", "{", "[1, 2]", "null", "\"text\""] {
            let (_folder, path) = fixture(text);
            assert_eq!(read(&path), None, "{text}");
        }
        // Production's keys with the wrong types list nothing.
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
        // It lists only: no last library, no consent.
        assert_eq!(state.last_library, None);
        assert!(state.library_consents.is_empty());
        // Never again, even once the list is emptied.
        state.library_history.clear();
        state.pinned_libraries.clear();
        assert!(!apply(&mut state, seed));
        assert!(state.library_history.is_empty());
        // A list of this app's own is never replaced, and is not seeded
        // later either.
        let mut own = State {
            library_history: vec!["/c/Own.library".into()],
            ..State::default()
        };
        assert!(!apply(&mut own, seed));
        assert_eq!(own.library_history, ["/c/Own.library"]);
        assert!(own.libraries_seeded);
        // Missing: tried, and never again.
        let mut fresh = State::default();
        assert!(!apply(&mut fresh, || None));
        assert!(fresh.libraries_seeded);
    }

    /// The test process is a test build, an automation seam: it never
    /// reads the user's real file.
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
