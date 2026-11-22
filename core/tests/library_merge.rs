//! What a user gets from merging one library into another, checked on two
//! small libraries that these tests make in a temporary folder.
//!
//! The libraries are described through the library's ordinary reads (the
//! details of each item, the folders, the tags and the smart folders) and
//! through the files in their folders, never through the code that the merge
//! reads the other library with.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use picto_core::library_application::LibraryApplication;
use picto_core::library_merge::{self, MergeRequest};
use picto_library::predicate::{FilterClause, FilterExpr, SetMatchMode, ViewQuerySpec};
use picto_library::selection::SelectionTarget;
use picto_library::{
    FolderId, ImmutableMediaFacts, Lifecycle, PreparedCollectionImport, PreparedImport, Rating,
    RootId, RootKind, SmartFolderInput, TaskId, TaskState,
};
use sha2::{Digest, Sha256};

/// One file to put into a test library.
struct File<'a> {
    name: &'a str,
    /// The colour of the picture. Two files of the same colour have the same
    /// content and so the same hash.
    color: [u8; 3],
    lifecycle: Lifecycle,
    rating: Rating,
    notes: Option<&'a str>,
    tags: &'a [&'a str],
    folders: &'a [FolderId],
    urls: &'a [&'a str],
}

impl<'a> File<'a> {
    fn new(name: &'a str, color: [u8; 3]) -> Self {
        Self {
            name,
            color,
            lifecycle: Lifecycle::Active,
            rating: Rating::Unrated,
            notes: None,
            tags: &[],
            folders: &[],
            urls: &[],
        }
    }
}

fn picture(color: [u8; 3]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(8, 8, image::Rgb(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn hash_of(color: [u8; 3]) -> String {
    hex::encode(Sha256::digest(picture(color)))
}

/// Stores the file's original and a thumbnail in the library's blob store
/// and returns the import of the file.
fn prepared(application: &LibraryApplication, file: &File<'_>) -> PreparedImport {
    let bytes = picture(file.color);
    let hash = hex::encode(Sha256::digest(&bytes));
    application
        .blobs()
        .write_original(&hash, &bytes, Some("png"))
        .unwrap();
    application
        .blobs()
        .write_thumbnail(&hash, format!("thumbnail of {hash}").as_bytes(), "jpg")
        .unwrap();
    let path = application
        .blobs()
        .original_path_with_ext(&hash, Some("png"))
        .unwrap();
    PreparedImport {
        stable_key: format!("test:{}", file.name),
        media_name: file.name.into(),
        file_path: path.to_string_lossy().into_owned(),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: bytes.len() as u64,
            width: Some(8),
            height: Some(8),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: hash,
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: file.lifecycle,
        rating: file.rating,
        notes: file.notes.map(str::to_owned),
        tags: file.tags.iter().map(|tag| (*tag).to_owned()).collect(),
        folders: file.folders.to_vec(),
        source_urls: file.urls.iter().map(|url| (*url).to_owned()).collect(),
        source_identity: None,
        imported_at_ms: IMPORTED_AT,
        captured_at_ms: None,
    }
}

fn add(application: &LibraryApplication, file: File<'_>) -> RootId {
    application
        .library()
        .ingest(&prepared(application, &file))
        .unwrap()
        .0
}

const SHARED: [u8; 3] = [200, 10, 10];
const SHARED_UNRATED: [u8; 3] = [10, 200, 10];
const ONLY_KEPT: [u8; 3] = [10, 10, 200];
const INBOX: [u8; 3] = [1, 2, 3];
const TRASH: [u8; 3] = [4, 5, 6];
const PAGE_ONE: [u8; 3] = [7, 8, 9];
const PAGE_TWO: [u8; 3] = [10, 11, 12];
const EDITED: [u8; 3] = [13, 14, 15];

/// The time at which every file of the test libraries is imported.
const IMPORTED_AT: i64 = 1_700_000_000_000;
/// The time at which the other library's item "Edited later" was changed.
const EDITED_AT: i64 = 1_750_000_000_000;
/// The time at which the other library's collection was changed.
const SET_CHANGED_AT: i64 = 1_760_000_000_000;

/// Makes the library that is kept: two files that the other library holds
/// too, one that only this library holds, and a smart folder for the items
/// that were changed after the middle of the time between the import and the
/// first change in the other library.
fn kept_library(folder: &Path) -> LibraryApplication {
    let application = LibraryApplication::create(folder.join("Kept.library")).unwrap();
    application
        .library()
        .create_smart_folder(SmartFolderInput {
            name: "Changed lately".into(),
            parent_id: None,
            icon: None,
            color: None,
            notes: None,
            view: ViewQuerySpec {
                filter: FilterExpr::All(vec![FilterExpr::Clause(FilterClause::ModifiedAt {
                    minimum_ms: Some(((IMPORTED_AT + EDITED_AT) / 2) as u64),
                    maximum_ms: None,
                })]),
                sort: Default::default(),
            },
        })
        .unwrap();
    let (shared, _) = application.library().create_folder("Shared", None).unwrap();
    add(
        &application,
        File {
            rating: Rating::Three,
            notes: Some("the kept library's note"),
            tags: &["a"],
            folders: &[shared],
            urls: &["https://example.test/kept"],
            ..File::new("Kept name", SHARED)
        },
    );
    add(&application, File::new("Unrated here", SHARED_UNRATED));
    add(
        &application,
        File {
            tags: &["a"],
            ..File::new("Only in kept", ONLY_KEPT)
        },
    );
    application
}

/// Makes the other library and closes it, as Picto closes a library. It
/// holds the two shared files with other values, a file in the Inbox, a file
/// in the Trash, a file that was changed after its import, a collection of
/// two files, a folder with a colour that gives its items a tag, a smart
/// folder and a tag that no item carries. Each file of the collection has a
/// note, and the collection has a note of its own that was written later.
fn other_library(folder: &Path) -> PathBuf {
    let root = folder.join("Other.library");
    let application = LibraryApplication::create(&root).unwrap();
    let library = application.library();
    let (shared, _) = library.create_folder("Shared", None).unwrap();
    let (only, _) = library.create_folder("Only B", None).unwrap();
    library
        .set_folder_metadata(only, Some("star"), Some("#123456"), Some("folder notes"))
        .unwrap();
    let (deep, _) = library.create_folder("Deep", Some(only)).unwrap();
    let (auto_tags, _) = library
        .ensure_tag_definitions(&["series:from-folder".into()])
        .unwrap();
    library
        .set_folder_auto_tags(deep, auto_tags, IMPORTED_AT)
        .unwrap();
    library
        .ensure_tag_definitions(&["unused-b".into()])
        .unwrap();
    add(
        &application,
        File {
            rating: Rating::Five,
            notes: Some("the other library's note"),
            tags: &["b", "creator:someone"],
            folders: &[shared, deep],
            urls: &["https://example.test/other"],
            ..File::new("Other name", SHARED)
        },
    );
    add(
        &application,
        File {
            rating: Rating::Four,
            notes: Some("a note only the other library has"),
            ..File::new("Rated there", SHARED_UNRATED)
        },
    );
    add(
        &application,
        File {
            lifecycle: Lifecycle::Inbox,
            tags: &["b"],
            ..File::new("In the inbox", INBOX)
        },
    );
    add(
        &application,
        File {
            lifecycle: Lifecycle::Trash,
            ..File::new("In the trash", TRASH)
        },
    );
    let edited = add(&application, File::new("Edited later", EDITED));
    library
        .set_notes(
            &SelectionTarget::Explicit {
                root_ids: vec![edited],
            },
            Some("a note written after the import".into()),
            EDITED_AT,
        )
        .unwrap();
    let pages = [
        File {
            notes: Some("the note of page one"),
            tags: &["b"],
            folders: &[only],
            ..File::new("Page one", PAGE_ONE)
        },
        File {
            notes: Some("the note of page two"),
            tags: &["b"],
            folders: &[only],
            ..File::new("Page two", PAGE_TWO)
        },
    ]
    .iter()
    .map(|file| prepared(&application, file))
    .collect();
    let (set, _) = library
        .ingest_collection(&PreparedCollectionImport {
            members: pages,
            cover_index: 0,
            existing_root_id: None,
            name: Some("A set".into()),
            modified_at_ms: IMPORTED_AT,
        })
        .unwrap();
    library
        .set_notes(
            &SelectionTarget::Explicit {
                root_ids: vec![set],
            },
            Some("the set's own note".into()),
            SET_CHANGED_AT,
        )
        .unwrap();
    let b = library
        .tags()
        .unwrap()
        .into_iter()
        .find(|tag| tag.subname == "b")
        .unwrap()
        .tag_id;
    library
        .create_smart_folder(SmartFolderInput {
            name: "Tagged b".into(),
            parent_id: None,
            icon: None,
            color: None,
            notes: None,
            view: ViewQuerySpec {
                filter: FilterExpr::All(vec![FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: vec![b],
                    mode: SetMatchMode::Any,
                })]),
                sort: Default::default(),
            },
        })
        .unwrap();
    library.database().checkpoint_wal().unwrap();
    drop(application);
    root
}

/// One item of a library, as a user sees it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Item {
    name: String,
    collection: bool,
    files: Vec<String>,
    status: String,
    rating: String,
    notes: Option<String>,
    /// The note of each file, in the order of `files`.
    file_notes: Vec<Option<String>>,
    /// The time of the item's last change.
    changed_at: i64,
    urls: Vec<String>,
    tags: BTreeSet<String>,
    folders: BTreeSet<String>,
}

/// What a library holds, as a user sees it, and the files of its blob store.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Described {
    items: Vec<Item>,
    folders: BTreeSet<(String, Option<String>)>,
    smart_folders: Vec<(String, usize)>,
    tags: BTreeSet<String>,
    blobs: BTreeSet<String>,
}

fn describe(application: &LibraryApplication) -> Described {
    let library = application.library();
    let folders = library.folders().unwrap();
    let path = |id: FolderId| {
        let mut names = Vec::new();
        let mut current = Some(id);
        while let Some(id) = current {
            let folder = folders.iter().find(|folder| folder.folder_id == id).unwrap();
            names.push(folder.name.clone());
            current = folder.parent_id;
        }
        names.reverse();
        names.join("/")
    };
    let tag_names = library
        .tags()
        .unwrap()
        .into_iter()
        .map(|tag| {
            let name = if tag.namespace.is_empty() {
                tag.subname
            } else {
                format!("{}:{}", tag.namespace, tag.subname)
            };
            (tag.tag_id, name)
        })
        .collect::<BTreeMap<_, _>>();
    let root_ids = library
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let mut statement =
                    connection.prepare("SELECT root_id FROM library_root ORDER BY root_id")?;
                let ids = statement
                    .query_map([], |row| row.get::<_, u32>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(ids)
            },
        )
        .unwrap();
    let mut items = root_ids
        .into_iter()
        .map(|id| {
            let details = library.details(RootId(id)).unwrap();
            Item {
                name: details.root.name.clone(),
                collection: details.root.kind == RootKind::Collection,
                files: details
                    .media
                    .iter()
                    .map(|media| media.facts.content_hash.clone())
                    .collect(),
                status: format!("{:?}", details.lifecycle),
                rating: format!("{:?}", details.rating),
                notes: details.root.notes.clone(),
                file_notes: details
                    .media
                    .iter()
                    .map(|media| media.media_notes.clone())
                    .collect(),
                changed_at: details.root.modified_at_ms,
                urls: details.root.source_urls.clone(),
                tags: details
                    .tag_ids
                    .iter()
                    .map(|id| tag_names[id].clone())
                    .collect(),
                folders: details.folder_ids.iter().map(|id| path(*id)).collect(),
            }
        })
        .collect::<Vec<_>>();
    items.sort();
    let mut blobs = BTreeSet::new();
    for kind in ["f", "t"] {
        files_under(&application.root().join("blobs").join(kind), &mut |file| {
            blobs.insert(format!(
                "{kind}/{}",
                file.file_name().unwrap().to_string_lossy()
            ));
        });
    }
    Described {
        items,
        folders: folders
            .iter()
            .map(|folder| (path(folder.folder_id), folder.color.clone()))
            .collect(),
        smart_folders: library
            .smart_folders()
            .unwrap()
            .into_iter()
            .map(|folder| (folder.name, folder.count as usize))
            .collect(),
        tags: tag_names.into_values().collect(),
        blobs,
    }
}

fn files_under(folder: &Path, each: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, each);
        } else {
            each(&path);
        }
    }
}

/// Returns every file in the library's folder with its bytes.
fn bytes_on_disk(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    files_under(root, &mut |file| {
        files.insert(
            file.strip_prefix(root).unwrap().to_path_buf(),
            std::fs::read(file).unwrap(),
        );
    });
    files
}

fn backups(application: &LibraryApplication) -> Vec<PathBuf> {
    std::fs::read_dir(application.root())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().contains(".pre-merge-"))
        .collect()
}

fn start(application: &LibraryApplication, other: &Path) -> TaskId {
    let started = library_merge::start(
        application,
        &MergeRequest {
            path: other.to_path_buf(),
        },
    )
    .unwrap();
    TaskId(started.task_id)
}

fn state(application: &LibraryApplication, task: TaskId) -> TaskState {
    application.library().task_state(task).unwrap().unwrap()
}

/// Does once what the scan worker and the ingest worker do in one round:
/// scans the tasks that need it, undoes the tasks marked for undo, imports
/// up to `limit` queued jobs and marks finished tasks as finished.
fn work(application: &LibraryApplication, limit: usize) {
    picto_core::import_tasks::scan_pending_tasks(application, &|| false).unwrap();
    picto_core::import_tasks::settle_reverts(application).unwrap();
    picto_core::library_ingest_runtime::run_batch(application, limit).unwrap();
    application
        .library()
        .settle_finished_tasks(1_800_000_000_000)
        .unwrap();
}

/// Works until the task is over, then once more, as the workers keep going
/// after a task has ended.
fn finish(application: &LibraryApplication, task: TaskId) -> TaskState {
    for _ in 0..200 {
        work(application, 64);
        if matches!(
            state(application, task),
            TaskState::Finished | TaskState::Reverted
        ) {
            work(application, 64);
            while picto_core::library_media_runtime::drain_blob_cleanup(application, 64).unwrap()
                > 0
            {}
            return state(application, task);
        }
    }
    panic!("the merge did not end");
}

#[test]
fn a_merge_puts_everything_of_both_libraries_into_the_kept_one() {
    let folder = tempfile::tempdir().unwrap();
    let kept = kept_library(folder.path());
    let other = other_library(folder.path());
    let other_before = bytes_on_disk(&other);

    assert_eq!(
        library_merge::inspect(&MergeRequest {
            path: other.clone()
        })
        .unwrap(),
        library_merge::MergeSummary {
            name: "Other".into(),
            items: 6,
            files: 7,
        }
    );
    let task = start(&kept, &other);
    assert_eq!(backups(&kept).len(), 1, "the backup is made at the start");
    assert_eq!(finish(&kept, task), TaskState::Finished);

    let after = describe(&kept);
    let names = |tags: &[&str]| tags.iter().map(|tag| (*tag).to_owned()).collect::<BTreeSet<_>>();
    let item = |name: &str| {
        after
            .items
            .iter()
            .find(|item| item.name == name)
            .unwrap_or_else(|| panic!("no item named {name}: {:#?}", after.items))
    };
    assert_eq!(after.items.len(), 7, "{:#?}", after.items);

    // A file that both libraries hold is one item. It keeps the kept
    // library's name, rating and notes, and has the tags, the folders and
    // the source links of both.
    let shared = item("Kept name");
    assert_eq!(shared.files, [hash_of(SHARED)]);
    assert_eq!(shared.rating, "Three");
    assert_eq!(shared.changed_at, IMPORTED_AT);
    assert_eq!(shared.notes.as_deref(), Some("the kept library's note"));
    assert_eq!(
        shared.tags,
        names(&["a", "b", "creator:someone", "series:from-folder"])
    );
    assert_eq!(shared.folders, names(&["Shared", "Only B/Deep"]));
    assert_eq!(
        shared.urls,
        ["https://example.test/kept", "https://example.test/other"]
    );

    // Where the kept library's item has no rating and no notes, it gets the
    // other library's.
    let unrated = item("Unrated here");
    assert_eq!(unrated.rating, "Four");
    assert_eq!(
        unrated.notes.as_deref(),
        Some("a note only the other library has")
    );

    // An item that only the other library holds arrives with its status.
    assert_eq!(item("In the inbox").status, "Inbox");
    assert_eq!(item("In the inbox").tags, names(&["b"]));
    assert_eq!(item("In the trash").status, "Trash");

    // An item that was changed after its import arrives with the time of
    // that change, and not with the time of its import.
    let edited = item("Edited later");
    assert_eq!(edited.changed_at, EDITED_AT);
    assert_eq!(
        edited.notes.as_deref(),
        Some("a note written after the import")
    );
    assert_eq!(item("Only in kept").status, "Active");

    let set = item("A set");
    assert!(set.collection);
    assert_eq!(set.files, [hash_of(PAGE_ONE), hash_of(PAGE_TWO)]);
    assert_eq!(set.folders, names(&["Only B"]));
    // The collection arrives with its own note, which differs from the note
    // of its cover file, with the note of each of its files, and with the
    // time of its last change.
    assert_eq!(set.notes.as_deref(), Some("the set's own note"));
    assert_eq!(
        set.file_notes,
        [
            Some("the note of page one".to_owned()),
            Some("the note of page two".to_owned())
        ]
    );
    assert_eq!(set.changed_at, SET_CHANGED_AT);

    assert!(after
        .folders
        .contains(&("Only B".to_owned(), Some("#123456".to_owned()))));
    assert!(after.tags.contains("unused-b"));
    // The kept library's smart folder for items that were changed lately
    // holds the item that was changed after its import and the collection.
    // The other library's smart folder names the kept library's tag `b`,
    // which the shared file, the Inbox file and the collection carry. A
    // smart folder counts the active items only.
    assert_eq!(
        after.smart_folders,
        [
            ("Changed lately".to_owned(), 2),
            ("Tagged b".to_owned(), 2)
        ]
    );

    // The files and the thumbnails were copied.
    for color in [INBOX, TRASH, PAGE_ONE, PAGE_TWO] {
        let hash = hash_of(color);
        assert!(after.blobs.contains(&format!("f/{hash}.png")));
        assert!(after.blobs.contains(&format!("t/{hash}.jpg")));
        assert_eq!(
            kept.blobs().read_thumbnail(&hash).unwrap().unwrap(),
            format!("thumbnail of {hash}").as_bytes()
        );
    }

    assert_eq!(backups(&kept), Vec::<PathBuf>::new());
    assert_eq!(bytes_on_disk(&other), other_before);

    // A second merge of the same library adds nothing.
    let task = start(&kept, &other);
    assert_eq!(finish(&kept, task), TaskState::Finished);
    assert_eq!(describe(&kept), after);
    assert_eq!(bytes_on_disk(&other), other_before);
}

#[test]
fn a_cancelled_merge_leaves_the_kept_library_as_it_was() {
    let folder = tempfile::tempdir().unwrap();
    let kept = kept_library(folder.path());
    let other = other_library(folder.path());
    let other_before = bytes_on_disk(&other);
    let before = describe(&kept);

    let task = start(&kept, &other);
    // Part of the merge is done: the tags, folders and smart folders, and
    // the first three items, two of which the kept library already held.
    work(&kept, 3);
    assert_eq!(state(&kept, task), TaskState::Running);
    assert_ne!(describe(&kept), before);

    kept.library()
        .stop_task(task, true, 1_800_000_000_000)
        .unwrap();
    assert_eq!(finish(&kept, task), TaskState::Reverted);

    assert_eq!(describe(&kept), before);
    assert_eq!(backups(&kept), Vec::<PathBuf>::new());
    assert_eq!(bytes_on_disk(&other), other_before);
}

#[test]
fn a_merge_that_is_cancelled_after_every_item_arrived_is_undone_too() {
    let folder = tempfile::tempdir().unwrap();
    let kept = kept_library(folder.path());
    let other = other_library(folder.path());
    let before = describe(&kept);

    let task = start(&kept, &other);
    assert_eq!(finish(&kept, task), TaskState::Finished);
    kept.library()
        .stop_task(task, true, 1_800_000_000_000)
        .unwrap();
    assert_eq!(finish(&kept, task), TaskState::Reverted);

    assert_eq!(describe(&kept), before);
}

#[test]
fn a_merge_that_cannot_copy_a_file_changes_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let kept = kept_library(folder.path());
    let other = other_library(folder.path());
    let before = describe(&kept);
    // The other library lost the original of one of its items.
    let hash = hash_of(TRASH);
    std::fs::remove_file(
        other
            .join("blobs/f")
            .join(&hash[..2])
            .join(&hash[2..4])
            .join(format!("{hash}.png")),
    )
    .unwrap();
    let other_before = bytes_on_disk(&other);

    let task = start(&kept, &other);
    assert_eq!(finish(&kept, task), TaskState::Reverted);

    let record = kept.library().task(task).unwrap().unwrap();
    assert!(
        record
            .error
            .as_deref()
            .is_some_and(|error| error.contains(&hash)),
        "{:?}",
        record.error
    );
    assert_eq!(describe(&kept), before);
    assert_eq!(backups(&kept), Vec::<PathBuf>::new());
    assert_eq!(bytes_on_disk(&other), other_before);
}

#[test]
fn a_library_that_is_open_elsewhere_or_cannot_be_read_is_refused_before_anything_changes() {
    let folder = tempfile::tempdir().unwrap();
    let kept = kept_library(folder.path());
    let other = other_library(folder.path());
    let before = describe(&kept);
    let other_before = bytes_on_disk(&other);
    let refused = |path: &Path| {
        library_merge::start(
            &kept,
            &MergeRequest {
                path: path.to_path_buf(),
            },
        )
        .unwrap_err()
    };

    // Another application has the other library open: it holds the lock.
    let held = picto_core::state::lock_library(&other).unwrap();
    let error = refused(&other);
    assert!(
        error.starts_with(picto_core::state::LIBRARY_IN_USE_MESSAGE),
        "{error}"
    );
    assert!(library_merge::inspect(&MergeRequest {
        path: other.clone()
    })
    .is_err());
    drop(held);

    assert!(refused(kept.root()).contains("itself"));
    assert!(refused(&folder.path().join("Missing.library")).contains("Could not find"));

    // A library whose schema is not the current one is not read.
    let old = folder.path().join("Old.library");
    std::fs::create_dir(&old).unwrap();
    std::fs::copy(other.join("library.sqlite"), old.join("library.sqlite")).unwrap();
    rusqlite::Connection::open(old.join("library.sqlite"))
        .unwrap()
        .execute(
            "UPDATE library_meta SET schema_fingerprint = 'an-earlier-schema'",
            [],
        )
        .unwrap();
    let error = refused(&old);
    assert!(error.contains("incompatible"), "{error}");

    assert_eq!(describe(&kept), before);
    assert_eq!(backups(&kept), Vec::<PathBuf>::new());
    assert!(kept.library().tasks(true).unwrap().is_empty());
    // The lock file that the test took the lock on is the only difference.
    let mut other_after = bytes_on_disk(&other);
    other_after.remove(Path::new(picto_core::state::LIBRARY_LOCK_FILE));
    assert_eq!(other_after, other_before);
}
