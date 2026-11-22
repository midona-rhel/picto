//! Opens libraries. A disposable copy is opened when `application::guard`
//! accepts it. A library the user keeps is opened under the rules of
//! [`admission`] (`app/README.md`, "Opening a library you keep"). This
//! module also keeps the list of libraries (`libraryHistory`,
//! `pinnedLibraries` and `lastLibrary` in the state file) and the user's
//! consent for each library. Both are kept in this app's own state file.
//!
//! - [`admission`] holds the rules. Its functions have no side effects,
//!   and its checks of the system can be replaced in tests.
//! - [`platform`] finds out whether another Picto is running, and returns
//!   a file's identity.
//! - [`host`] owns the main window's content, which is the gate or a
//!   library's shell. It opens, switches and closes libraries, and shows
//!   the consent dialog and the notice that blocks the window while
//!   another Picto is running.
//! - [`manager`] is the Library Manager window.
//! - [`files`] does Delete… and Move… on disk.
//! - [`seed`] fills the list on the first run. It reads, once, the
//!   `config.json` that Picto 0.6.19 or earlier wrote.
pub mod admission;
pub mod files;
pub mod host;
pub mod manager;
pub(crate) mod platform;
pub mod seed;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use admission::{Facts, Identity, Origin};

/// Whether this process was started with `--allow-real-libraries`. It is set
/// once, from the command line, before any library is opened.
static FLAG: OnceLock<bool> = OnceLock::new();

pub fn set_flag(flag: bool) {
    let _ = FLAG.set(flag);
}

/// Returns the facts about this process that [`admission`] decides on: whether
/// it is a release build, whether it was started with the flag, which
/// automation seam is active, and the test folder.
pub fn facts() -> Facts {
    Facts {
        release: !cfg!(debug_assertions) && !cfg!(test),
        flag: FLAG.get().copied().unwrap_or(false),
        seam: crate::seams::automation(),
        test_folder: test_folder(),
    }
}

/// Returns the folder named by `PICTO_GPUI_TEST_LIBRARIES=<folder>`. Only test
/// builds read the variable; the off-screen tool sets it. The libraries
/// directly in that folder may be opened in spite of the build and the
/// automation seam, so that the off-screen tool can take pictures of the code
/// path for real libraries, using generated libraries. The folder must be
/// inside a temporary folder. Any other folder is ignored, so the variable
/// cannot name a folder that holds a library the user keeps.
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
    /// A disposable copy. It has the marker file, it is in a temporary folder,
    /// and it is detached when it is opened.
    Copy,
    /// A library the user keeps. It is opened as it is: it is not detached,
    /// and no file of a copy is written into it.
    Real,
}

/// Returns the canonical root of the library that is open in the main window,
/// when `root` is that library. Returns `None` when no library is open, when
/// another library is open, or when the open library is being closed. It does
/// not matter how the library was admitted: as a disposable copy that the
/// guard accepted, or as a library the user keeps that `admission::admit`
/// allowed with the user's consent.
///
/// The edits that write files themselves (rendered and regenerated thumbnails,
/// Set as Thumbnail, the library's cover) may write only inside the returned
/// folder. Delete Permanently and Empty Trash run only when this returns a
/// folder. All of them use this one function.
pub fn writable(root: &Path, cx: &gpui::App) -> Option<PathBuf> {
    let open = host::current(cx).map(|(open, _)| open);
    writable_in(open.as_deref(), root)
}

/// Does the work of [`writable`]. `open` is the root of the library that is
/// open now, or `None`.
pub fn writable_in(open: Option<&Path>, root: &Path) -> Option<PathBuf> {
    let open = open?.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    (open == root).then_some(root)
}

/// Returns `path` as the path of a file inside the library at `root`, or an
/// error message that says the file was not written. `root` must be canonical.
/// This is the same check as the one for files passed to the desktop
/// (`desktop::inside`), but for a file that may not exist yet.
///
/// The file's folder is made canonical. If part of the folder does not exist
/// yet, the nearest part that exists is resolved to its real location, so a
/// link on the way that points out of the library is found, and the parts that
/// do not exist yet must be plain names. The file itself must not be a
/// symbolic link. Then replacing or removing the file changes only that name,
/// and never the file a link points at.
pub fn confine(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let outside = || {
        format!(
            "{} is not inside the open library {}, so it was not written.",
            path.display(),
            root.display()
        )
    };
    let plain = |name: &std::ffi::OsStr| {
        matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [std::path::Component::Normal(_)]
        )
    };
    let name = path
        .file_name()
        .filter(|name| plain(name))
        .ok_or_else(outside)?;
    if path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(outside());
    }
    let mut existing = path.parent().ok_or_else(outside)?.to_path_buf();
    let mut rest = Vec::new();
    let mut folder = loop {
        match existing.canonicalize() {
            Ok(canonical) => break canonical,
            // This part is a symbolic link whose target does not exist.
            Err(_) if existing.symlink_metadata().is_ok() => return Err(outside()),
            Err(_) => {
                let part = existing
                    .file_name()
                    .filter(|part| plain(part))
                    .ok_or_else(outside)?
                    .to_owned();
                rest.push(part);
                existing = existing.parent().ok_or_else(outside)?.to_path_buf();
            }
        }
    };
    for part in rest.into_iter().rev() {
        folder.push(part);
    }
    let confined = folder.join(name);
    if crate::desktop::inside(root, &confined) {
        Ok(confined)
    } else {
        Err(outside())
    }
}

/// Returns the name that the app shows for the library at `path`: the name
/// of its folder without `.library`.
pub fn name(path: &str) -> String {
    crate::shell::library::split(path).0
}

/// Returns whether the library at `path` may be listed and read in this
/// process. A library the user keeps may be listed only in a process that may
/// open it. A process that may not open such libraries (a debug build started
/// without the flag, or any automation) reads nothing about them, not even
/// whether their folders exist. A library directly in the test folder of a
/// test build is always listed.
pub fn listed(path: &str, facts: &Facts) -> bool {
    let path = Path::new(path);
    match &facts.test_folder {
        Some(folder) if path.parent() == Some(folder.as_path()) => true,
        _ => facts.permits().is_ok(),
    }
}

/// Returns the consents that the state holds, each as an [`Identity`].
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

/// Returns the rows of the Recent Libraries menu: the libraries that may be
/// listed, with the pinned ones first. The open library's row is marked as
/// current. If the open library is not among the rows, a row for it is added
/// at the end; a copy is never among them, because a copy is not in the
/// history.
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

/// Does Clear History: removes every library except the open one from the
/// history and from the pinned libraries, then rebuilds the app's menus and
/// redraws the main window's content.
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

/// The operations on the list of libraries: the history, the pinned
/// libraries, the last library, and each library's colour and icon.
pub mod history {
    /// Puts `path` first in the history, removes any earlier entry for it,
    /// and makes it the last library.
    pub fn add(history: &mut Vec<String>, last: &mut Option<String>, path: &str) {
        history.retain(|known| known != path);
        history.insert(0, path.to_owned());
        *last = Some(path.to_owned());
    }

    /// Removes `path` from the history and from the pinned libraries (Quick
    /// Access). If it was the last library, the last library becomes the
    /// first one that remains in the history.
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

    /// Does the work of Clear History: removes every library except the open
    /// one (`current`) from the history and from the pinned libraries. If the
    /// open library was not in the history, it is added to it.
    pub fn clear(history: &mut Vec<String>, pinned: &mut Vec<String>, current: Option<&str>) {
        history.retain(|known| Some(known.as_str()) == current);
        pinned.retain(|known| Some(known.as_str()) == current);
        if let Some(current) = current
            && history.is_empty()
        {
            history.push(current.to_owned());
        }
    }

    /// Unpins `path` if it is pinned. Otherwise adds it at the end of the
    /// pinned libraries.
    pub fn toggle_pinned(pinned: &mut Vec<String>, path: &str) {
        if pinned.iter().any(|known| known == path) {
            pinned.retain(|known| known != path);
        } else {
            pinned.push(path.to_owned());
        }
    }

    /// Replaces the path `from` with `to` in the history and in the pinned
    /// libraries, at the same positions, and in the last library.
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

    /// Moves the colour and icon stored for the path `from` (`libraryMeta` in
    /// the state file) to the path `to`, so a library that was renamed, moved
    /// or located keeps them.
    pub fn rename_look(
        looks: &mut std::collections::BTreeMap<String, crate::state::LibraryLook>,
        from: &str,
        to: &str,
    ) {
        if let Some(look) = looks.remove(from) {
            looks.insert(to.to_owned(), look);
        }
    }

    /// Returns the paths for the rows of the Recent Libraries submenu in two
    /// lists: first the pinned libraries that are in the history, then the
    /// libraries of the history that are not pinned.
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

/// Checks the name and the place of a new library. Returns the path of the
/// new library's folder, `<name>.library` in `parent`, or an error message
/// when the name or the place is not allowed.
pub fn new_library_path(name: &str, parent: &Path) -> Result<PathBuf, String> {
    use crate::i18n::en;
    let name = name.trim();
    if name.is_empty() {
        return Err(en("Library name cannot be empty").to_owned());
    }
    if name.contains(['/', '\\']) {
        return Err(en("Library name cannot contain slashes").to_owned());
    }
    // A library may not be created inside another library's folder.
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
        // New Library only creates a library. It does not open one that is
        // already there, so this is an error.
        return Err(en(
            "A library with that name already exists in the selected folder. Choose another location or open the existing library.",
        )
        .to_owned());
    }
    Ok(path)
}

/// Returns the library that the folder dialog of Locate… found. This is the
/// chosen folder itself, or the folder named `name` inside it, whichever
/// holds `library.sqlite`. Returns `None` when neither does.
pub fn located(chosen: &Path, name: &std::ffi::OsStr) -> Option<PathBuf> {
    [chosen.to_path_buf(), chosen.join(name)]
        .into_iter()
        .find(|candidate| candidate.join("library.sqlite").is_file())
}

/// Checks the new name of a library. Returns the path of the renamed folder,
/// `<name>.library` in the same parent folder as the old one, or an error
/// message when the name is not allowed or a library with that name is already
/// there.
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
        // The locale files have no key for this message, so it is shown in
        // English in every language.
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
    fn the_history_is_kept_as_the_earlier_version_kept_it() {
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
    fn a_new_library_is_named_and_placed_as_the_earlier_version_required() {
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

    /// Follows a library the user keeps from the Library Manager to an open
    /// application and then to a switch to another library. The checks of the
    /// system are replaced by test values. The library is admitted only with
    /// the user's consent. It is opened as it is: it is not detached, and no
    /// file of a copy is written into it. It is locked while it is open. A
    /// switch closes it, after which its old application answers no command,
    /// and then opens the next library.
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
        // A release build admits the library without asking the user.
        assert!(matches!(
            admission::admit(&one, Origin::Manager, &release, &[], &not_running()),
            Decision::Real { .. }
        ));
        let (events, _received) = std::sync::mpsc::channel();
        let first = Application::new(one.clone(), Kind::Real, events.clone());
        first.open(|_| {}).unwrap();
        // The library is locked while it is open, so another application could
        // not open it.
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
        // This is the switch: the first application closes and releases its
        // lock, then the second opens.
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
        // The first application still answers no command.
        assert!(
            futures::executor::block_on(first.dispatch("sidebar.counts", serde_json::json!({})))
                .unwrap()
                .is_err()
        );
        second.close();
        drop(picto_core::state::lock_library(&two).unwrap());
    }

    /// The edits that cannot be undone (Delete Permanently, Empty Trash,
    /// rendered and regenerated thumbnails, Set as Thumbnail, the cover) all
    /// call `writable`. It refuses when no library is open and when the
    /// library is not the open one. It allows the open library, whichever way
    /// that library was admitted.
    #[test]
    fn edits_are_refused_unless_their_library_is_the_open_one() {
        let folder = tempfile::tempdir().unwrap();
        let kept = admission::tests::real_library(folder.path(), "Kept");
        let other = admission::tests::real_library(folder.path(), "Other");
        assert_eq!(writable_in(None, &kept), None);
        assert_eq!(writable_in(Some(&other), &kept), None);
        assert_eq!(writable_in(Some(&kept), &kept), Some(kept.clone()));
        // `writable` reads the open library from the app. In a new app no
        // library is open.
        let cx = gpui::TestAppContext::single();
        cx.update(|cx| {
            assert_eq!(writable(&kept, cx), None);
            assert!(host::current(cx).is_none());
        });
    }

    /// Every path that an edit writes or removes is checked to be inside the
    /// open library's folder, as the paths passed to the desktop are.
    #[cfg(unix)]
    #[test]
    fn writes_are_confined_to_the_open_library() {
        let folder = tempfile::tempdir().unwrap();
        let root = admission::tests::real_library(folder.path(), "Kept");
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("blobs/t/ab")).unwrap();
        // These are allowed: a file that exists, a file that does not exist
        // yet, and a file in folders that do not exist yet.
        for inside in [
            root.join("library.sqlite"),
            root.join(".picto-library-cover.png"),
            root.join("blobs/t/ab/abcd.png"),
            root.join("blobs/t/cd/ef/new.png"),
        ] {
            assert_eq!(
                confine(&root, &inside).unwrap(),
                inside,
                "{}",
                inside.display()
            );
        }
        // These are refused: the library's folder itself, a file in a sibling
        // folder, a path that leaves the library with `..`, and a file
        // somewhere else.
        for refused in [
            root.clone(),
            folder
                .path()
                .canonicalize()
                .unwrap()
                .join("Kept.library2/x.png"),
            root.join("blobs/../../x.png"),
            root.join("blobs/t/new/../../../../x.png"),
            outside.join("x.png"),
        ] {
            assert!(confine(&root, &refused).is_err(), "{}", refused.display());
        }
        // A folder that is a symbolic link pointing out of the library is
        // refused.
        std::os::unix::fs::symlink(&outside, root.join("blobs/f")).unwrap();
        assert!(confine(&root, &root.join("blobs/f/x.png")).is_err());
        assert!(confine(&root, &root.join("blobs/f/new/x.png")).is_err());
        // A file that is itself a symbolic link is refused, whether it points
        // outside the library or inside it.
        std::os::unix::fs::symlink(outside.join("x.png"), root.join("dangling.png")).unwrap();
        std::os::unix::fs::symlink(root.join("library.sqlite"), root.join("inner.png")).unwrap();
        assert!(confine(&root, &root.join("dangling.png")).is_err());
        assert!(confine(&root, &root.join("inner.png")).is_err());
        // A folder that is a symbolic link to a target that does not exist is
        // refused.
        std::os::unix::fs::symlink(outside.join("gone"), root.join("blobs/g")).unwrap();
        assert!(confine(&root, &root.join("blobs/g/x.png")).is_err());
        // The same check applies to the thumbnail this app renders
        // (`rendered_target`) and to the old thumbnail that is renamed to a
        // backup while a new one is made (`set_aside`).
        let jpeg = root.join("blobs/t/ab/abcd.jpg");
        assert_eq!(
            crate::grid::thumbnails::rendered_target(&root, &jpeg).unwrap(),
            root.join("blobs/t/ab/abcd.png")
        );
        assert!(
            crate::grid::thumbnails::rendered_target(&root, &root.join("blobs/f/x.jpg")).is_err()
        );
        std::fs::write(outside.join("old.jpg"), b"kept").unwrap();
        assert!(
            crate::grid::thumbnails::set_aside(
                &root,
                &root.join("blobs/f/old.jpg"),
                &root.join("blobs/t/ab/old.jpg.backup")
            )
            .is_err()
        );
        assert_eq!(std::fs::read(outside.join("old.jpg")).unwrap(), b"kept");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    }

    /// Locate… finds the library when the chosen folder is the library, or
    /// when the library of the same name is inside the chosen folder. In any
    /// other case it finds nothing.
    #[test]
    fn locate_finds_the_library_chosen_or_the_one_inside_the_folder() {
        let folder = tempfile::tempdir().unwrap();
        let kept = admission::tests::real_library(folder.path(), "Kept");
        let name = std::ffi::OsStr::new("Kept.library");
        assert_eq!(located(&kept, name), Some(kept.clone()));
        assert_eq!(
            located(&folder.path().canonicalize().unwrap(), name),
            Some(kept.clone())
        );
        assert_eq!(
            located(folder.path(), std::ffi::OsStr::new("Other.library")),
            None
        );
        let empty = folder.path().join("Empty.library");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(located(&empty, name), None);
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
        assert!(!listed("/home/me/Pictures/Main.library", &facts));
        let release = Facts {
            release: true,
            flag: false,
            seam: None,
            test_folder: None,
        };
        assert!(listed("/home/me/Pictures/Main.library", &release));
    }
}
