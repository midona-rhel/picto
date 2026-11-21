//! Decides whether a library may be opened (`app/README.md`, "Opening a
//! library you keep"). A disposable copy is opened when
//! `application::guard` accepts it. A library the user keeps is opened only
//! when every rule below holds. The rules are checked in this order:
//!
//! 1. The folder is a library (a `.library` folder that holds
//!    `library.sqlite`) and has no marker file of a copy. A folder that has
//!    the marker file and that the guard refuses is a copy in the wrong
//!    place, and it is refused with the guard's message.
//! 2. The build permits it. A release build does. A debug or test build
//!    does only when it was started with `--allow-real-libraries`.
//! 3. No automation seam is active in the process (`seams::automation`).
//! 4. The user asked for the library: in the Library Manager, with Open
//!    Library…, with a row of Recent Libraries or of the switcher, by
//!    having had it open last, or on the command line with the flag. A file
//!    that the system handed over never opens a library.
//! 5. No other Picto is running. Picto 0.6.19 and earlier hold no lock that
//!    this app can see, so the list of processes is read instead.
//! 6. The core finds the library's schema current. The schema is read
//!    without writing anything.
//! 7. The user agreed, once for each library (its path and its database
//!    file), that this app reads and writes the library directly.
//!
//! Nothing in this module writes. The checks read the folder, the system's
//! list of processes, and the database with SQLite's `immutable` flag.
use std::path::{Path, PathBuf};

/// The command-line flag that lets a debug or test build open libraries
/// the user keeps.
pub const FLAG: &str = "--allow-real-libraries";

/// Where a request to open a library came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The user chose the library in the Library Manager: a row, Open
    /// Library or New Library.
    Manager,
    /// The user chose the library in the folder dialog of Open Library….
    OpenDialog,
    /// The user chose a row of File › Recent Libraries.
    Recent,
    /// The user chose a row of the library switcher in the sidebar.
    Switcher,
    /// The app is starting and opens the library it had open last.
    LastOpened,
    /// The library was named on the command line. `flag` is whether
    /// `--allow-real-libraries` was on the command line too.
    CommandLine { flag: bool },
    /// The system handed the library over as a file: a launch with files,
    /// Open With, a drop on the Dock icon, or a second launch.
    Associated,
}

impl Origin {
    /// Returns whether a request from this origin counts as the user asking
    /// for the library (rule 4).
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

/// The facts about this process that the rules decide on: its build, its
/// flag, its automation seam and the test folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    /// Whether this is a release build. A release build holds no test seams.
    pub release: bool,
    /// Whether the process was started with [`FLAG`].
    pub flag: bool,
    /// The name of the automation seam that is active, if one is.
    pub seam: Option<&'static str>,
    /// In a test build only: a folder whose libraries pass rules 2 and 3
    /// whatever the build and the seam are. The off-screen tool names the
    /// folder with `PICTO_GPUI_TEST_LIBRARIES` and puts generated libraries
    /// in it.
    pub test_folder: Option<PathBuf>,
}

impl Facts {
    /// Returns whether this process may open libraries the user keeps at
    /// all (rules 2 and 3), or the reason why it may not.
    pub fn permits(&self) -> Result<(), Refusal> {
        if let Some(seam) = self.seam {
            return Err(Refusal::Seam(seam));
        }
        if !self.release && !self.flag {
            return Err(Refusal::Build);
        }
        Ok(())
    }

    /// Returns whether the library at `root` passes rules 2 and 3: the
    /// process permits it, or, in a test build, the library is directly in
    /// the test folder. `root` must be canonical.
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

/// What the user's consent is given for: the library's path and the
/// identity of its database file on disk. Because the database file is part
/// of it, the user is asked again when a different library is later put at
/// the same path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub path: String,
    pub database: String,
}

/// The reason a library was not opened. [`Refusal::words`] returns the title
/// and the message that tell the user the reason.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    Missing(PathBuf),
    NotALibrary(PathBuf),
    /// The folder has the marker file of a copy and the guard refused it.
    /// The text is the guard's message.
    CopyOutOfPlace(String),
    Build,
    Seam(&'static str),
    Origin,
    PictoRunning(String),
    ProbeFailed(String),
    Incompatible(String),
}

impl Refusal {
    /// Returns the title and the message that a notification or the Library
    /// Manager shows for this refusal.
    pub fn words(&self) -> (String, String) {
        use crate::i18n::en;
        // None of these texts has a key in the locale files, so each is
        // shown in English in every language.
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

/// The result of a request to open a library.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// A disposable copy that the guard accepted. It is detached when it is
    /// opened.
    Copy(PathBuf),
    /// A library the user keeps that passed every rule.
    Real {
        root: PathBuf,
        identity: Identity,
    },
    /// A library the user keeps that passed every rule except the last: the
    /// user has not yet given consent for this library. The caller asks the
    /// user, naming the library's path, stores the consent if the user
    /// agrees, and then calls `admit` again.
    Ask {
        root: PathBuf,
        identity: Identity,
    },
    Refused(Refusal),
}

/// The questions that the rules ask the system. Tests give `admit` their
/// own answers through this trait.
pub trait Probes {
    /// Returns a text that names another Picto that is running, `None` when
    /// none is running, or an error when the list of processes could not be
    /// read.
    fn picto_process(&self) -> Result<Option<String>, String>;
    /// Returns a text that identifies the database file on disk.
    fn file_identity(&self, database: &Path) -> std::io::Result<String> {
        crate::libraries::platform::file_identity(database)
    }
}

/// Asks the system itself (`libraries::platform`).
pub struct System;

impl Probes for System {
    fn picto_process(&self) -> Result<Option<String>, String> {
        crate::libraries::platform::picto_process()
    }
}

/// Decides whether the library at `path` may be opened. `consents` holds the
/// identities of the libraries that the user has agreed to.
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
    // A folder that the guard accepts is a disposable copy. The numbered
    // rules below do not apply to it.
    match crate::application::guard(&root) {
        Ok(copy) => {
            return if origin == Origin::Associated {
                // This app opens no library from a file that the system
                // handed over, and that includes a disposable copy.
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
    // Rule 1: the folder is a library.
    let database = root.join("library.sqlite");
    let named = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".library"));
    if !named || !root.is_dir() || !database.is_file() {
        return Decision::Refused(Refusal::NotALibrary(root));
    }
    // Rules 2 and 3: the build permits it and no automation seam is active.
    if let Err(refusal) = facts.permits_library(&root) {
        return Decision::Refused(refusal);
    }
    // Rule 4: the user asked for the library.
    if !origin.chosen_by_the_user() {
        return Decision::Refused(Refusal::Origin);
    }
    // Rule 5: no other Picto is running.
    match probes.picto_process() {
        Ok(None) => {}
        Ok(Some(process)) => return Decision::Refused(Refusal::PictoRunning(process)),
        Err(error) => return Decision::Refused(Refusal::ProbeFailed(error)),
    }
    // Rule 6: the core finds the schema current.
    if let Err(message) = compatibility(&root) {
        return Decision::Refused(Refusal::Incompatible(message));
    }
    // Rule 7: the user has given consent for this library.
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

/// Decides whether New Library may make a library at `path`. Nothing may
/// exist at `path` yet, and rules 2, 3 and 5 must hold. The other rules need
/// a library that already exists.
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

/// Checks that the core finds the schema of the library at `root` current
/// (`picto_library::schema::validate`). The database is opened with SQLite's
/// `immutable` flag, so no lock is taken, no journal or shared-memory file
/// is made, and nothing is written. The error is the core's own message
/// (`incompatible library: …`). When the core opens a library with an older
/// schema, it migrates the schema in place. This app refuses such a library
/// here, before the core opens it.
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

    /// Answers `picto_process` with the value it was given.
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

    /// Makes a library in `folder` that has the core's schema and no marker
    /// file of a copy, and returns its canonical path.
    pub fn real_library(folder: &Path, name: &str) -> PathBuf {
        let root = folder.join(format!("{name}.library"));
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        let mut database = rusqlite::Connection::open(root.join("library.sqlite")).unwrap();
        // The database is put in WAL mode, the journal mode that a library
        // has. A read that left a shared-memory file or a log file behind
        // would then show in a snapshot of the folder.
        database
            .query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .unwrap();
        picto_library::schema::create(&mut database).unwrap();
        drop(database);
        root.canonicalize().unwrap()
    }

    /// Returns every file and folder under `root` with its length and its
    /// modification time.
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
        // The process that runs this test is a test build, which counts as
        // an automation seam.
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

    /// Answers that no other Picto is running, and answers `file_identity`
    /// with the text it was given. The consent rule is then tested without
    /// depending on the identity that a file system gives a replaced file,
    /// which `platform::tests` covers.
    struct Database(&'static str);

    impl Probes for Database {
        fn picto_process(&self) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn file_identity(&self, _: &Path) -> std::io::Result<String> {
            Ok(self.0.to_owned())
        }
    }

    #[test]
    fn consent_is_asked_once_per_library_and_its_database() {
        let folder = tempfile::tempdir().unwrap();
        let root = real_library(folder.path(), "Kept");
        let first = Database("first");
        let decision = admit(&root, Origin::Manager, &release(), &[], &first);
        let Decision::Ask { identity, .. } = decision else {
            panic!("{decision:?}");
        };
        assert_eq!(identity.path, root.to_string_lossy());
        assert_eq!(identity.database, "first");
        let consents = [identity];
        assert!(matches!(
            admit(&root, Origin::Manager, &release(), &consents, &first),
            Decision::Real { .. }
        ));
        // When another library is put at the same path, the user is asked
        // again.
        let replaced = Database("second");
        assert!(matches!(
            admit(&root, Origin::Manager, &release(), &consents, &replaced),
            Decision::Ask { .. }
        ));
        // A consent does not cover the same database at another path.
        let elsewhere = real_library(folder.path(), "Other");
        assert!(matches!(
            admit(&elsewhere, Origin::Manager, &release(), &consents, &first),
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
        // This database is not a Picto database.
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
        // The empty `.library` folder was refused, and nothing was written
        // into it.
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
        // A copy is admitted as a copy whatever the build, the flag, the
        // seam, the consents and the running processes are.
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
        // A folder that has the marker file and that the guard refuses is
        // never admitted as a library the user keeps.
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
