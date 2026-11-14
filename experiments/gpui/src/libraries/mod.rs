//! Opening libraries: disposable copies as before, and the libraries the
//! user keeps under the rules of [`admission`] (README, "Opening
//! libraries"). This module also keeps production's list of libraries
//! (`libraryHistory`, `pinnedLibraries`, `lastLibrary`) in this app's own
//! state, never in the Electron app's `config.json`, and the user's consent
//! per library.
//!
//! - [`admission`]: the rules, pure, with the system's probes injectable.
//! - [`platform`]: whether the Electron app runs, and a file's identity.
//! - [`host`]: the main window's content (the gate or a library's shell),
//!   opening, switching and closing, the consent dialog, and the notice
//!   that blocks the window while the Electron app runs.
//! - [`manager`]: the Library Manager window.
pub mod admission;
pub mod host;
pub mod manager;
pub(crate) mod platform;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use admission::{Facts, Identity, Origin};

/// Whether this process was started with `--allow-real-libraries`. Set
/// once, from the command line, before anything is opened.
static FLAG: OnceLock<bool> = OnceLock::new();

pub fn set_flag(flag: bool) {
    let _ = FLAG.set(flag);
}

/// What this process is, for [`admission`].
pub fn facts() -> Facts {
    Facts {
        release: !cfg!(debug_assertions) && !cfg!(test),
        flag: FLAG.get().copied().unwrap_or(false),
        seam: crate::seams::automation(),
        test_folder: test_folder(),
    }
}

/// `PICTO_GPUI_TEST_LIBRARIES=<folder>`, in test builds only (the off-screen
/// tool): the libraries directly in that folder count as permitted by the
/// build and the seams, so the real-library path can be photographed with
/// generated libraries. The folder must lie inside a temporary folder; any
/// other is ignored, so it cannot be pointed at a library the user keeps.
#[cfg(test)]
fn test_folder() -> Option<PathBuf> {
    let folder = PathBuf::from(std::env::var_os("PICTO_GPUI_TEST_LIBRARIES")?)
        .canonicalize()
        .ok()?;
    let temporary = [
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/tmp"),
    ];
    let inside = temporary
        .iter()
        .filter_map(|folder| folder.canonicalize().ok())
        .any(|temporary| folder.starts_with(&temporary) && folder != temporary);
    if inside {
        Some(folder)
    } else {
        eprintln!(
            "picto: PICTO_GPUI_TEST_LIBRARIES={} is not inside a temporary folder; ignored",
            folder.display()
        );
        None
    }
}

#[cfg(not(test))]
fn test_folder() -> Option<PathBuf> {
    None
}

/// What kind of library is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A disposable copy: marked, in a scratch folder, detached on open.
    Copy,
    /// A library the user keeps: opened as production opens it.
    Real,
}

/// A library's name as production shows it: its folder's name without
/// `.library` (`getLibraryName`).
pub fn name(path: &str) -> String {
    crate::shell::library::split(path).0
}

/// Whether libraries the user keeps may be listed and looked at in this
/// process: only where they may be opened. A process that cannot open
/// them (a debug build without the flag, any automation) reads nothing of
/// them, not even whether their folders exist; in a test build only the
/// test folder's libraries are listed.
pub fn listed(path: &str, facts: &Facts) -> bool {
    let path = Path::new(path);
    match &facts.test_folder {
        Some(folder) if path.parent() == Some(folder.as_path()) => true,
        _ => facts.permits().is_ok(),
    }
}

/// The consents kept in the state, as identities.
pub fn consents(cx: &gpui::App) -> Vec<Identity> {
    crate::state::get(cx)
        .library_consents
        .iter()
        .map(|(path, database)| Identity {
            path: path.clone(),
            database: database.clone(),
        })
        .collect()
}

/// Recent Libraries' rows: the listed libraries, pinned ones first, and
/// the open library (a copy is never in the history) checked.
pub fn recent(cx: &gpui::App) -> Vec<crate::app_menu::Recent> {
    let facts = facts();
    let state = crate::state::get(cx);
    let current = host::current(cx).map(|(root, _)| root.to_string_lossy().into_owned());
    let (pinned, rest) = history::recent_rows(&state.library_history, &state.pinned_libraries);
    let mut rows = pinned
        .iter()
        .map(|path| (path, true))
        .chain(rest.iter().map(|path| (path, false)))
        .filter(|(path, _)| listed(path, &facts))
        .map(|(path, pinned)| crate::app_menu::Recent {
            path: path.clone(),
            name: name(path),
            pinned,
            current: current.as_deref() == Some(path.as_str()),
        })
        .collect::<Vec<_>>();
    if let Some(current) = current
        && !rows.iter().any(|row| row.current)
    {
        rows.push(crate::app_menu::Recent {
            name: name(&current),
            path: current,
            pinned: false,
            current: true,
        });
    }
    rows
}

/// Clear History: only the open library stays listed.
pub fn clear_history(cx: &mut gpui::App) {
    let current = host::current(cx).map(|(root, _)| root.to_string_lossy().into_owned());
    crate::state::update(cx, |state| {
        history::clear(
            &mut state.library_history,
            &mut state.pinned_libraries,
            current.as_deref(),
        )
    });
    crate::set_app_menus(cx);
    host::host(cx).update(cx, |_, cx| cx.notify());
}

/// The list of libraries, as production's `globalConfig.mjs` keeps it.
pub mod history {
    /// `addLibraryToHistory`: first, once, and the last library.
    pub fn add(history: &mut Vec<String>, last: &mut Option<String>, path: &str) {
        history.retain(|known| known != path);
        history.insert(0, path.to_owned());
        *last = Some(path.to_owned());
    }

    /// `removeLibraryFromHistory`: out of the history and Quick Access; the
    /// last library becomes the first left.
    pub fn remove(
        history: &mut Vec<String>,
        pinned: &mut Vec<String>,
        last: &mut Option<String>,
        path: &str,
    ) {
        history.retain(|known| known != path);
        pinned.retain(|known| known != path);
        if last.as_deref() == Some(path) {
            *last = history.first().cloned();
        }
    }

    /// Clear History: only the open library stays, in both lists.
    pub fn clear(history: &mut Vec<String>, pinned: &mut Vec<String>, current: Option<&str>) {
        history.retain(|known| Some(known.as_str()) == current);
        pinned.retain(|known| Some(known.as_str()) == current);
        if let Some(current) = current
            && history.is_empty()
        {
            history.push(current.to_owned());
        }
    }

    /// `togglePinned`: pinned at the end, or unpinned.
    pub fn toggle_pinned(pinned: &mut Vec<String>, path: &str) {
        if pinned.iter().any(|known| known == path) {
            pinned.retain(|known| known != path);
        } else {
            pinned.push(path.to_owned());
        }
    }

    /// `updateLibraryPath`: a renamed library keeps its places.
    pub fn rename(
        history: &mut [String],
        pinned: &mut [String],
        last: &mut Option<String>,
        from: &str,
        to: &str,
    ) {
        for known in history.iter_mut().chain(pinned.iter_mut()) {
            if known == from {
                *known = to.to_owned();
            }
        }
        if last.as_deref() == Some(from) {
            *last = Some(to.to_owned());
        }
    }

    /// The Recent Libraries submenu's rows (`buildRecentLibrariesMenu`):
    /// pinned libraries in the history first, then the rest.
    pub fn recent_rows(history: &[String], pinned: &[String]) -> (Vec<String>, Vec<String>) {
        let first = pinned
            .iter()
            .filter(|path| history.contains(path))
            .cloned()
            .collect();
        let rest = history
            .iter()
            .filter(|path| !pinned.contains(path))
            .cloned()
            .collect();
        (first, rest)
    }
}

/// `createLibrary`'s checks: the new library's folder, `<name>.library` in
/// `parent`, or production's words for why not.
pub fn new_library_path(name: &str, parent: &Path) -> Result<PathBuf, String> {
    use crate::i18n::en;
    let name = name.trim();
    if name.is_empty() {
        return Err(en("Library name cannot be empty").to_owned());
    }
    if name.contains(['/', '\\']) {
        return Err(en("Library name cannot contain slashes").to_owned());
    }
    // `isInsideLibraryPackage`: no library inside another.
    if parent.ancestors().any(|folder| {
        folder
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_lowercase().ends_with(".library"))
    }) {
        return Err(en("A library cannot be created inside another Picto library").to_owned());
    }
    let path = parent.join(format!("{name}.library"));
    if path.exists() {
        // Production opens what is there; this app makes only new ones.
        return Err(en(
            "A library with that name already exists in the selected folder. Choose another location or open the existing library.",
        )
        .to_owned());
    }
    Ok(path)
}

/// `renameLibrary`'s checks: the renamed folder beside the old one.
pub fn renamed_path(path: &Path, name: &str) -> Result<PathBuf, String> {
    use crate::i18n::en;
    let name = name.trim();
    if name.is_empty() {
        return Err(en("Library name cannot be empty").to_owned());
    }
    if name.contains(['/', '\\']) {
        return Err(en("Library name cannot contain slashes").to_owned());
    }
    let parent = path.parent().unwrap_or(Path::new("/"));
    let renamed = parent.join(format!("{name}.library"));
    if renamed != path && renamed.exists() {
        // Production's main process writes it in English, without a key.
        return Err(format!(
            "A library named \"{name}\" already exists at that location"
        ));
    }
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::history;
    use super::*;
    use admission::Decision;

    #[test]
    fn the_history_is_kept_as_production_keeps_it() {
        let (mut list, mut pinned, mut last) = (Vec::new(), Vec::new(), None);
        history::add(&mut list, &mut last, "/a/One.library");
        history::add(&mut list, &mut last, "/a/Two.library");
        history::add(&mut list, &mut last, "/a/One.library");
        assert_eq!(list, ["/a/One.library", "/a/Two.library"]);
        assert_eq!(last.as_deref(), Some("/a/One.library"));
        history::toggle_pinned(&mut pinned, "/a/Two.library");
        assert_eq!(
            history::recent_rows(&list, &pinned),
            (vec!["/a/Two.library".into()], vec!["/a/One.library".into()])
        );
        history::rename(
            &mut list,
            &mut pinned,
            &mut last,
            "/a/Two.library",
            "/a/Deux.library",
        );
        assert_eq!(pinned, ["/a/Deux.library"]);
        history::remove(&mut list, &mut pinned, &mut last, "/a/One.library");
        assert_eq!(
            (list.clone(), last.clone()),
            (
                vec!["/a/Deux.library".into()],
                Some("/a/Deux.library".into())
            )
        );
        assert!(pinned == ["/a/Deux.library"]);
        history::add(&mut list, &mut last, "/a/Three.library");
        history::clear(&mut list, &mut pinned, Some("/a/Three.library"));
        assert_eq!(list, ["/a/Three.library"]);
        assert!(pinned.is_empty());
        history::clear(&mut list, &mut pinned, None);
        assert!(list.is_empty());
    }

    #[test]
    fn a_new_library_is_named_and_placed_as_production_requires() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            new_library_path(" Art ", folder.path()).unwrap(),
            folder.path().join("Art.library")
        );
        assert!(new_library_path("  ", folder.path()).is_err());
        assert!(new_library_path("a/b", folder.path()).is_err());
        assert!(new_library_path("Art", &folder.path().join("Main.library").join("x")).is_err());
        std::fs::create_dir(folder.path().join("Art.library")).unwrap();
        assert!(new_library_path("Art", folder.path()).is_err());
        assert!(renamed_path(&folder.path().join("Old.library"), "Art").is_err());
        assert_eq!(
            renamed_path(&folder.path().join("Old.library"), "New").unwrap(),
            folder.path().join("New.library")
        );
    }

    /// The path a library the user keeps takes from the Library Manager to
    /// an open application and on to the next library, with the probes
    /// injected: admitted only with consent; opened as it is (not
    /// detached, nothing of a copy written); locked while open; a switch
    /// closes it, after which its old application reaches nothing, and
    /// opens the next.
    #[test]
    fn a_kept_library_opens_as_it_is_and_a_switch_closes_it_first() {
        use crate::application::Application;
        use admission::tests::{not_running, real_library};
        let folder = tempfile::tempdir().unwrap();
        let watched = folder.path().join("watched");
        std::fs::create_dir(&watched).unwrap();
        let one = real_library(folder.path(), "One");
        rusqlite::Connection::open(one.join("library.sqlite"))
            .unwrap()
            .execute(
                "INSERT INTO folder_definition (folder_id, stable_key, name, watch_path, watch_enabled, display_order)
                 VALUES (1, 'f1', 'Watched', ?1, 1, 0)",
                [watched.to_string_lossy()],
            )
            .unwrap();
        let two = real_library(folder.path(), "Two");
        let release = Facts {
            release: true,
            flag: false,
            seam: None,
            test_folder: None,
        };
        // Asked first; opened once the user agreed.
        let Decision::Ask { identity, .. } =
            admission::admit(&one, Origin::Manager, &release, &[], &not_running())
        else {
            panic!("not asked");
        };
        assert!(matches!(
            admission::admit(&one, Origin::Manager, &release, &[identity], &not_running()),
            Decision::Real { .. }
        ));
        let (events, _received) = std::sync::mpsc::channel();
        let first = Application::new(one.clone(), Kind::Real, events.clone());
        first.open(|_| {}).unwrap();
        // Held while open: another application could not open it.
        assert!(picto_core::state::lock_library(&one).is_err());
        let watch: i64 = rusqlite::Connection::open(one.join("library.sqlite"))
            .unwrap()
            .query_row("SELECT watch_enabled FROM folder_definition", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(watch, 1, "a kept library was detached");
        assert!(!one.join(crate::application::MARKER).exists());
        assert!(!one.join(".gpui-detached").exists());
        // The switch: the first closes (and lets go), the second opens.
        first.close();
        assert!(
            futures::executor::block_on(first.dispatch("sidebar.counts", serde_json::json!({})))
                .unwrap()
                .is_err()
        );
        drop(picto_core::state::lock_library(&one).unwrap());
        let second = Application::new(two.clone(), Kind::Real, events);
        second.open(|_| {}).unwrap();
        assert!(picto_core::state::lock_library(&two).is_err());
        let counts =
            futures::executor::block_on(second.dispatch("sidebar.counts", serde_json::json!({})))
                .unwrap()
                .unwrap();
        assert_eq!(counts["all"].as_u64(), Some(0));
        // The first application still reaches nothing.
        assert!(
            futures::executor::block_on(first.dispatch("sidebar.counts", serde_json::json!({})))
                .unwrap()
                .is_err()
        );
        second.close();
        drop(picto_core::state::lock_library(&two).unwrap());
    }

    #[test]
    fn a_process_that_cannot_open_kept_libraries_does_not_list_them() {
        let folder = tempfile::tempdir().unwrap();
        let inside = folder.path().join("A.library");
        let facts = Facts {
            release: false,
            flag: false,
            seam: Some("a test build"),
            test_folder: Some(folder.path().to_path_buf()),
        };
        assert!(listed(&inside.to_string_lossy(), &facts));
        assert!(!listed("/Users/me/Pictures/Main.library", &facts));
        let release = Facts {
            release: true,
            flag: false,
            seam: None,
            test_folder: None,
        };
        assert!(listed("/Users/me/Pictures/Main.library", &release));
    }
}
