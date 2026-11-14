//! Whether a library may be opened (README, "Opening libraries"). A
//! disposable copy is opened as it always was (`application::guard`); a
//! library the user keeps only when every rule below holds, in this order:
//!
//! 1. it is a library (`library.sqlite` in a `.library` folder) and carries
//!    no copy's marker (a marked folder that fails the guard is a copy out
//!    of place, and is refused as the guard words it);
//! 2. the build permits it: a release build does; a debug or test build
//!    only when started with `--allow-real-libraries`;
//! 3. no automation seam is active in the process (`seams::automation`);
//! 4. the user asked for it: the Library Manager, Open Library…, a recent
//!    library, the switcher, the library this app last had open, or the
//!    command line with the flag; never a file the system handed over;
//! 5. the Electron app is not running (it holds no lock this app can see);
//! 6. the core finds its schema current, read without writing anything;
//! 7. the user agreed, once per library (path and database file), that this
//!    app reads and writes it directly.
//!
//! Nothing here writes: the checks read the folder, the system's process
//! list and the database with SQLite's `immutable` flag.
use std::path::{Path, PathBuf};

/// The command-line flag that lets a debug or test build open libraries
/// the user keeps.
pub const FLAG: &str = "--allow-real-libraries";

/// Where a request to open a library came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Chosen in the Library Manager (a row, Open Library, New Library).
    Manager,
    /// Chosen in Open Library…'s folder dialog.
    OpenDialog,
    /// A row of File › Recent Libraries.
    Recent,
    /// A row of the sidebar's library switcher.
    Switcher,
    /// The library this app had open last, at start.
    LastOpened,
    /// The command line; `flag` is whether `--allow-real-libraries` was on
    /// it.
    CommandLine { flag: bool },
    /// A file the system handed over: a launch with files, Open With, a
    /// drop on the Dock icon, a second launch.
    Associated,
}

impl Origin {
    /// Whether the user chose the library in this app.
    fn chosen_by_the_user(self) -> bool {
        match self {
            Origin::Manager
            | Origin::OpenDialog
            | Origin::Recent
            | Origin::Switcher
            | Origin::LastOpened => true,
            Origin::CommandLine { flag } => flag,
            Origin::Associated => false,
        }
    }
}

/// What this process is: its build, its flag, its seams.
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    /// A release build (no test seams compiled in).
    pub release: bool,
    /// Started with [`FLAG`].
    pub flag: bool,
    /// The automation seam active, if any.
    pub seam: Option<&'static str>,
    /// Test builds only: a scratch folder whose generated libraries count
    /// as permitted (rules 2 and 3), for the off-screen tool
    /// (`PICTO_GPUI_TEST_LIBRARIES`).
    pub test_folder: Option<PathBuf>,
}

impl Facts {
    /// Whether libraries the user keeps may be opened at all in this
    /// process (rules 2 and 3), and why not.
    pub fn permits(&self) -> Result<(), Refusal> {
        if let Some(seam) = self.seam {
            return Err(Refusal::Seam(seam));
        }
        if !self.release && !self.flag {
            return Err(Refusal::Build);
        }
        Ok(())
    }

    /// Whether `root` (canonical) may be opened as far as rules 2 and 3 go:
    /// the process permits it, or (in a test build) it lies directly in the
    /// test folder.
    pub fn permits_library(&self, root: &Path) -> Result<(), Refusal> {
        if self
            .test_folder
            .as_deref()
            .is_some_and(|folder| root.parent() == Some(folder))
        {
            return Ok(());
        }
        self.permits()
    }
}

/// A library's identity for the user's consent: where it is, and its
/// database file on disk, so a different library later put at the same
/// path is asked about again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub path: String,
    pub database: String,
}

/// Why a library was not opened. Each says so in words the user can act on.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    Missing(PathBuf),
    NotALibrary(PathBuf),
    /// A marked folder the guard refused: its words.
    CopyOutOfPlace(String),
    Build,
    Seam(&'static str),
    Origin,
    PictoRunning(String),
    ProbeFailed(String),
    Incompatible(String),
}

impl Refusal {
    /// The title and message a notification or the Library Manager shows.
    pub fn words(&self) -> (String, String) {
        use crate::i18n::en;
        // Production's main process writes these in English, without keys.
        let not_opened = en("Could not open Picto Library").to_owned();
        match self {
            Refusal::Missing(path) => (
                en("Library Not Found").to_owned(),
                format!(
                    "{}\n{}",
                    en("The library could not be found."),
                    path.display()
                ),
            ),
            Refusal::NotALibrary(path) => (
                en("Invalid Library").to_owned(),
                format!(
                    "{}\n{}",
                    en("The selected folder is not a library."),
                    path.display()
                ),
            ),
            Refusal::CopyOutOfPlace(message) => (not_opened, message.clone()),
            Refusal::Build => (
                not_opened,
                en("This build opens disposable library copies only. Start it with --allow-real-libraries to open a library you keep.").to_owned(),
            ),
            Refusal::Seam(seam) => (
                not_opened,
                format!(
                    "{} ({seam})",
                    en("A library you keep is never opened while the app is driven by automation.")
                ),
            ),
            Refusal::Origin => (
                not_opened,
                en("A library you keep is opened only from the Library Manager, Open Library…, Recent Libraries or the library switcher.").to_owned(),
            ),
            Refusal::PictoRunning(_) => (
                en("Quit Picto first").to_owned(),
                en("Picto is running. It must not have a library open while this app has it open: quit Picto, then open the library again.").to_owned(),
            ),
            Refusal::ProbeFailed(error) => (
                not_opened,
                format!(
                    "{} ({error})",
                    en("Whether Picto is running could not be checked, so the library was not opened.")
                ),
            ),
            Refusal::Incompatible(message) => (
                not_opened,
                format!(
                    "{message}\n{}",
                    en("This app does not update libraries. Open the library in Picto to update it.")
                ),
            ),
        }
    }
}

/// What a request came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// A disposable copy the guard accepted: opened and detached as always.
    Copy(PathBuf),
    /// A library the user keeps that passed every rule.
    Real {
        root: PathBuf,
        identity: Identity,
    },
    /// Every rule held but the user has not yet agreed for this library:
    /// ask (naming its path), keep the answer, and ask again.
    Ask {
        root: PathBuf,
        identity: Identity,
    },
    Refused(Refusal),
}

/// What the checks ask the system; injectable for tests.
pub trait Probes {
    /// The Electron app's process, if one runs.
    fn picto_process(&self) -> Result<Option<String>, String>;
    /// The identity of a database file on disk.
    fn file_identity(&self, database: &Path) -> std::io::Result<String> {
        crate::libraries::platform::file_identity(database)
    }
}

/// The system itself.
pub struct System;

impl Probes for System {
    fn picto_process(&self) -> Result<Option<String>, String> {
        crate::libraries::platform::picto_process()
    }
}

/// Decides whether `path` may be opened. `consents` are the identities the
/// user agreed to.
pub fn admit(
    path: &Path,
    origin: Origin,
    facts: &Facts,
    consents: &[Identity],
    probes: &dyn Probes,
) -> Decision {
    let Ok(root) = path.canonicalize() else {
        return Decision::Refused(Refusal::Missing(path.to_path_buf()));
    };
    // A disposable copy, exactly as before.
    match crate::application::guard(&root) {
        Ok(copy) => {
            return if origin == Origin::Associated {
                // Production switches to an opened `.library`; this app
                // opens no library from a file the system handed over.
                Decision::Refused(Refusal::Origin)
            } else {
                Decision::Copy(copy)
            };
        }
        Err(refused) if root.join(crate::application::MARKER).exists() => {
            return Decision::Refused(Refusal::CopyOutOfPlace(refused));
        }
        Err(_) => {}
    }
    // 1. A library.
    let database = root.join("library.sqlite");
    let named = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".library"));
    if !named || !root.is_dir() || !database.is_file() {
        return Decision::Refused(Refusal::NotALibrary(root));
    }
    // 2 and 3. The build and the seams.
    if let Err(refusal) = facts.permits_library(&root) {
        return Decision::Refused(refusal);
    }
    // 4. Chosen by the user.
    if !origin.chosen_by_the_user() {
        return Decision::Refused(Refusal::Origin);
    }
    // 5. The Electron app.
    match probes.picto_process() {
        Ok(None) => {}
        Ok(Some(process)) => return Decision::Refused(Refusal::PictoRunning(process)),
        Err(error) => return Decision::Refused(Refusal::ProbeFailed(error)),
    }
    // 6. The schema, as the core judges it.
    if let Err(message) = compatibility(&root) {
        return Decision::Refused(Refusal::Incompatible(message));
    }
    // 7. The user's consent.
    let identity = match probes.file_identity(&database) {
        Ok(database) => Identity {
            path: root.to_string_lossy().into_owned(),
            database,
        },
        Err(error) => {
            return Decision::Refused(Refusal::ProbeFailed(format!(
                "{}: {error}",
                database.display()
            )));
        }
    };
    if consents.contains(&identity) {
        Decision::Real { root, identity }
    } else {
        Decision::Ask { root, identity }
    }
}

/// Whether a new library may be made at `path` (New Library): the rules
/// that do not need a library yet (2, 3 and 5), and nothing there already.
pub fn admit_new(path: &Path, facts: &Facts, probes: &dyn Probes) -> Result<(), Refusal> {
    if path.exists() {
        return Err(Refusal::NotALibrary(path.to_path_buf()));
    }
    let parent = path
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .ok_or_else(|| Refusal::Missing(path.to_path_buf()))?;
    let name = path.file_name().unwrap_or_default();
    facts.permits_library(&parent.join(name))?;
    match probes.picto_process() {
        Ok(None) => Ok(()),
        Ok(Some(process)) => Err(Refusal::PictoRunning(process)),
        Err(error) => Err(Refusal::ProbeFailed(error)),
    }
}

/// Whether the core finds the library's schema current
/// (`picto_library::schema::validate`), read through SQLite's `immutable`
/// flag: no lock, no journal, no shared-memory file, nothing written. The
/// error is the core's own message (`incompatible library: …`). The core's
/// open would migrate an older schema in place; this app never asks it to.
pub fn compatibility(root: &Path) -> Result<(), String> {
    let database = root.join("library.sqlite");
    let mut address = url::Url::from_file_path(&database)
        .map_err(|()| format!("{} is not an absolute path", database.display()))?;
    address.set_query(Some("mode=ro&immutable=1"));
    let connection = rusqlite::Connection::open_with_flags(
        address.as_str(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("{}: {error}", database.display()))?;
    picto_library::schema::validate(&connection)
        .map(drop)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A probe that answers what it is told.
    pub struct Fake {
        pub running: Result<Option<String>, String>,
    }

    impl Probes for Fake {
        fn picto_process(&self) -> Result<Option<String>, String> {
            self.running.clone()
        }
    }

    pub fn not_running() -> Fake {
        Fake { running: Ok(None) }
    }

    fn release() -> Facts {
        Facts {
            release: true,
            flag: false,
            seam: None,
            test_folder: None,
        }
    }

    /// A library as production makes one, outside any scratch folder.
    pub fn real_library(folder: &Path, name: &str) -> PathBuf {
        let root = folder.join(format!("{name}.library"));
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        let mut database = rusqlite::Connection::open(root.join("library.sqlite")).unwrap();
        // In the journal mode a library keeps, so a read that would leave
        // its shared-memory or log file behind shows in a snapshot.
        database
            .query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .unwrap();
        picto_library::schema::create(&mut database).unwrap();
        drop(database);
        root.canonicalize().unwrap()
    }

    /// Every file under `root` with its length and modification time.
    pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
        let mut files = BTreeMap::new();
        let mut folders = vec![root.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    folders.push(entry.path());
                }
                files.insert(entry.path(), (meta.len(), meta.modified().unwrap()));
            }
        }
        files
    }

    fn agreed(root: &Path) -> Vec<Identity> {
        vec![Identity {
            path: root.to_string_lossy().into_owned(),
            database: System.file_identity(&root.join("library.sqlite")).unwrap(),
        }]
    }

    #[test]
    fn a_kept_library_opens_with_every_rule_held_and_nothing_written() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        let before = snapshot(&root);
        let decision = admit(
            &root,
            Origin::Manager,
            &release(),
            &agreed(&root),
            &not_running(),
        );
        assert!(matches!(decision, Decision::Real { root: ref opened, .. } if *opened == root));
        assert_eq!(snapshot(&root), before);
    }

    #[test]
    fn a_debug_build_refuses_a_kept_library_without_the_flag() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        let debug = Facts {
            release: false,
            ..release()
        };
        assert_eq!(
            admit(
                &root,
                Origin::Manager,
                &debug,
                &agreed(&root),
                &not_running()
            ),
            Decision::Refused(Refusal::Build)
        );
        let flagged = Facts {
            flag: true,
            ..debug
        };
        assert!(matches!(
            admit(
                &root,
                Origin::Manager,
                &flagged,
                &agreed(&root),
                &not_running()
            ),
            Decision::Real { .. }
        ));
    }

    #[test]
    fn an_automation_seam_refuses_a_kept_library_even_with_the_flag() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        for facts in [
            Facts {
                release: false,
                flag: true,
                seam: Some("PICTO_GPUI_POINTER"),
                test_folder: None,
            },
            Facts {
                seam: Some("a test build"),
                ..release()
            },
        ] {
            assert_eq!(
                admit(
                    &root,
                    Origin::Manager,
                    &facts,
                    &agreed(&root),
                    &not_running()
                ),
                Decision::Refused(Refusal::Seam(facts.seam.unwrap()))
            );
        }
        // The process this test runs in is a test build: a seam.
        assert_eq!(
            crate::libraries::facts().permits(),
            Err(Refusal::Seam("a test build"))
        );
    }

    #[test]
    fn only_a_library_the_user_chose_is_opened() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        let consents = agreed(&root);
        for origin in [Origin::Associated, Origin::CommandLine { flag: false }] {
            assert_eq!(
                admit(&root, origin, &release(), &consents, &not_running()),
                Decision::Refused(Refusal::Origin),
                "{origin:?}"
            );
        }
        for origin in [
            Origin::Manager,
            Origin::OpenDialog,
            Origin::Recent,
            Origin::Switcher,
            Origin::LastOpened,
            Origin::CommandLine { flag: true },
        ] {
            assert!(
                matches!(
                    admit(&root, origin, &release(), &consents, &not_running()),
                    Decision::Real { .. }
                ),
                "{origin:?}"
            );
        }
    }

    #[test]
    fn consent_is_asked_once_per_library_and_its_database() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        let decision = admit(&root, Origin::Manager, &release(), &[], &not_running());
        let Decision::Ask { identity, .. } = decision else {
            panic!("{decision:?}");
        };
        assert_eq!(identity.path, root.to_string_lossy());
        assert!(matches!(
            admit(
                &root,
                Origin::Manager,
                &release(),
                &[identity.clone()],
                &not_running()
            ),
            Decision::Real { .. }
        ));
        // Another library put at the same path is asked about again.
        std::fs::remove_dir_all(&root).unwrap();
        let again = real_library(folder.path(), "Kept");
        assert!(matches!(
            admit(
                &again,
                Origin::Manager,
                &release(),
                &[identity],
                &not_running()
            ),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn a_kept_library_is_refused_while_picto_runs_or_cannot_be_checked() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        let running = Fake {
            running: Ok(Some("/Applications/Picto.app/Contents/MacOS/Picto".into())),
        };
        assert!(matches!(
            admit(&root, Origin::Manager, &release(), &agreed(&root), &running),
            Decision::Refused(Refusal::PictoRunning(_))
        ));
        let unknown = Fake {
            running: Err("no process list".into()),
        };
        assert!(matches!(
            admit(&root, Origin::Manager, &release(), &agreed(&root), &unknown),
            Decision::Refused(Refusal::ProbeFailed(_))
        ));
    }

    #[test]
    fn an_incompatible_schema_is_refused_with_the_cores_words_and_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Old");
        rusqlite::Connection::open(root.join("library.sqlite"))
            .unwrap()
            .execute(
                "UPDATE library_meta SET schema_fingerprint = 'picto-library-schema-1-2026-08-29-fts-substring'",
                [],
            )
            .unwrap();
        let before = snapshot(&root);
        let Decision::Refused(Refusal::Incompatible(message)) = admit(
            &root,
            Origin::Manager,
            &release(),
            &agreed(&root),
            &not_running(),
        ) else {
            panic!("an old schema was admitted");
        };
        assert!(
            message.starts_with("incompatible library: expected generation 1"),
            "{message}"
        );
        assert_eq!(snapshot(&root), before);
        // Not a Picto database at all.
        let other = folder.path().join("Other.library");
        std::fs::create_dir(&other).unwrap();
        rusqlite::Connection::open(other.join("library.sqlite"))
            .unwrap()
            .execute_batch("CREATE TABLE t(x)")
            .unwrap();
        assert!(matches!(
            admit(&other, Origin::Manager, &release(), &[], &not_running()),
            Decision::Refused(Refusal::Incompatible(message))
                if message.contains("missing schema-generation-1 metadata")
        ));
    }

    #[test]
    fn what_is_not_a_library_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let empty = folder.path().join("Empty.library");
        std::fs::create_dir(&empty).unwrap();
        let unnamed = folder.path().join("Pictures");
        std::fs::create_dir(&unnamed).unwrap();
        std::fs::write(unnamed.join("library.sqlite"), b"").unwrap();
        for path in [&empty, &unnamed] {
            assert!(matches!(
                admit(path, Origin::Manager, &release(), &[], &not_running()),
                Decision::Refused(Refusal::NotALibrary(_))
            ));
        }
        assert!(matches!(
            admit(
                &folder.path().join("Gone.library"),
                Origin::Manager,
                &release(),
                &[],
                &not_running()
            ),
            Decision::Refused(Refusal::Missing(_))
        ));
        // Production creates a library in an empty `.library` folder; this
        // app wrote nothing there.
        assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
    }

    #[test]
    fn a_copy_opens_as_before_and_a_marked_folder_elsewhere_is_refused() {
        let scratch = tempfile::Builder::new()
            .prefix("picto-gpui-library.")
            .tempdir()
            .unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let source = real_library(elsewhere.path(), "Source");
        let copy = scratch.path().join("Source.library");
        std::fs::create_dir(&copy).unwrap();
        std::fs::write(copy.join("library.sqlite"), b"copy").unwrap();
        std::fs::write(
            copy.join(crate::application::MARKER),
            format!("{}\n", source.display()),
        )
        .unwrap();
        // Whatever the build, flag, seams or consent: a copy is a copy.
        let debug = Facts {
            release: false,
            flag: false,
            seam: Some("a test build"),
            test_folder: None,
        };
        let running = Fake {
            running: Ok(Some("Picto".into())),
        };
        assert_eq!(
            admit(
                &copy,
                Origin::CommandLine { flag: false },
                &debug,
                &[],
                &running
            ),
            Decision::Copy(copy.canonicalize().unwrap())
        );
        // A marked folder the guard refuses is never taken for a library
        // the user keeps.
        let moved = elsewhere.path().join("Moved.library");
        std::fs::create_dir(&moved).unwrap();
        std::fs::write(moved.join("library.sqlite"), b"copy").unwrap();
        std::fs::write(
            moved.join(crate::application::MARKER),
            format!("{}\n", source.display()),
        )
        .unwrap();
        assert!(matches!(
            admit(&moved, Origin::Manager, &release(), &[], &not_running()),
            Decision::Refused(Refusal::CopyOutOfPlace(_))
        ));
    }

    #[test]
    fn a_new_library_is_made_only_where_a_kept_one_could_be_opened() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("New.library");
        assert_eq!(admit_new(&path, &release(), &not_running()), Ok(()));
        let debug = Facts {
            release: false,
            ..release()
        };
        assert_eq!(
            admit_new(&path, &debug, &not_running()),
            Err(Refusal::Build)
        );
        let running = Fake {
            running: Ok(Some("Picto".into())),
        };
        assert!(matches!(
            admit_new(&path, &release(), &running),
            Err(Refusal::PictoRunning(_))
        ));
        std::fs::create_dir(&path).unwrap();
        assert!(admit_new(&path, &release(), &not_running()).is_err());
    }

    #[test]
    fn the_test_folder_admits_only_libraries_directly_inside_it() {
        let folder = tempfile::tempdir().unwrap();
        let inside = real_library(folder.path(), "Inside");
        let deeper = real_library(&folder.path().join("deeper"), "Deeper");
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = real_library(elsewhere.path(), "Outside");
        let facts = Facts {
            release: false,
            flag: false,
            seam: Some("a test build"),
            test_folder: Some(folder.path().canonicalize().unwrap()),
        };
        assert!(matches!(
            admit(
                &inside,
                Origin::Manager,
                &facts,
                &agreed(&inside),
                &not_running()
            ),
            Decision::Real { .. }
        ));
        for refused in [&deeper, &outside] {
            assert_eq!(
                admit(
                    refused,
                    Origin::Manager,
                    &facts,
                    &agreed(refused),
                    &not_running()
                ),
                Decision::Refused(Refusal::Seam("a test build"))
            );
        }
    }
}
