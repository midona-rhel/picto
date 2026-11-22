//! The tombstones that the devices of one library share. Three libraries
//! stand for three devices: the second and the third are copies of the first
//! with device ids of their own, as a library opened from the cloud is. The
//! entries travel as the JSON that the cloud files hold.

use std::collections::BTreeMap;

use picto_library::database::WorkPriority;
use picto_library::predicate::{FilterClause, FilterExpr, ItemSort, TextField, ViewQuerySpec};
use picto_library::selection::SelectionTarget;
use picto_library::{
    AppliedEntries, EntryKind, EntryState, FolderId, ImmutableMediaFacts, Library, Lifecycle,
    PreparedCollectionImport, PreparedImport, Rating, RootId, SharedEntry, SmartFolderId,
    SmartFolderInput,
};
use tempfile::TempDir;

const IMPORTED_AT_MS: i64 = 1_700_000_000_000;
const DELETED_AT_MS: i64 = 1_700_000_500_000;

fn imported(key: &str, hash: &str) -> PreparedImport {
    PreparedImport {
        stable_key: key.into(),
        media_name: format!("{key}.png"),
        file_path: format!("/tmp/{key}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 1_024,
            width: Some(100),
            height: Some(100),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: hash.into(),
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: Lifecycle::Active,
        rating: Rating::Unrated,
        notes: None,
        tags: Vec::new(),
        folders: Vec::new(),
        source_urls: Vec::new(),
        source_identity: None,
        imported_at_ms: IMPORTED_AT_MS,
        captured_at_ms: None,
    }
}

/// An import of `key` that is made a minute from now, which is later than
/// every deletion that a test makes.
fn imported_later(key: &str, hash: &str) -> PreparedImport {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    PreparedImport {
        imported_at_ms: now + 60_000,
        ..imported(key, hash)
    }
}

fn write(library: &Library, sql: &str) {
    library
        .database()
        .maintenance_write(WorkPriority::Cloud, |transaction| {
            transaction.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
}

fn scalar(library: &Library, sql: &str) -> i64 {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(sql, [], |row| row.get(0))?)
        })
        .unwrap()
}

/// Returns the rows of a query as lines of text.
fn lines(library: &Library, sql: &str) -> Vec<String> {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            let mut statement = connection.prepare(sql)?;
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| {
                            row.get::<_, rusqlite::types::Value>(column)
                                .map(|value| format!("{value:?}"))
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map(|values| values.join("|"))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
}

/// One device: its library and its device id.
struct Device {
    library: Library,
    id: &'static str,
}

impl Device {
    fn root(&self, key: &str) -> Option<RootId> {
        self.library
            .database()
            .read(WorkPriority::VisibleRead, |connection| {
                use rusqlite::OptionalExtension;
                Ok(connection
                    .query_row(
                        "SELECT root.root_id FROM library_root root
                         JOIN library_item item ON item.local_id = root.root_id
                         WHERE item.stable_key = ?1",
                        [key],
                        |row| row.get::<_, u32>(0),
                    )
                    .optional()?)
            })
            .unwrap()
            .map(RootId)
    }

    fn target(&self, keys: &[&str]) -> SelectionTarget {
        SelectionTarget::Explicit {
            root_ids: keys.iter().map(|key| self.root(key).unwrap()).collect(),
        }
    }

    /// Returns the item's status, or `None` when the library does not hold
    /// the item.
    fn status(&self, key: &str) -> Option<Lifecycle> {
        let root = self.root(key)?;
        Some(self.library.details(root).unwrap().lifecycle)
    }

    /// Returns whether the library holds a tombstone for the item.
    fn tombstoned(&self, key: &str) -> bool {
        scalar(
            &self.library,
            &format!("SELECT COUNT(*) FROM deletion_tombstone WHERE stable_key = '{key}'"),
        ) == 1
    }

    fn trash(&self, keys: &[&str]) {
        self.library
            .set_lifecycle(&self.target(keys), Lifecycle::Trash)
            .unwrap();
    }

    fn restore(&self, keys: &[&str]) {
        self.library
            .set_lifecycle(&self.target(keys), Lifecycle::Active)
            .unwrap();
    }

    /// Moves items to the Trash and deletes them permanently, with undo for
    /// up to 500 items.
    fn delete(&self, keys: &[&str]) {
        self.trash(keys);
        self.library
            .permanently_delete(&self.target(keys), DELETED_AT_MS, 500, false)
            .unwrap();
    }

    /// Sets the library's clock to a time far after today, as the clock of a
    /// device is whose wall clock is wrong.
    fn clock_ahead(&self) {
        write(
            &self.library,
            "UPDATE cloud_state SET hlc_physical_ms = 5000000000000, hlc_logical = 0",
        );
    }

    /// Returns every entry that this device made, as a sync writes them
    /// into the cloud folder.
    fn take(&self) -> Vec<SharedEntry> {
        while self.library.settle_trash_moves(2).unwrap() {}
        let mut entries = Vec::new();
        self.library
            .visit_own_entries(None, &mut |entry| entries.push(entry))
            .unwrap();
        // The entries cross between the devices as JSON.
        serde_json::from_str(&serde_json::to_string(&entries).unwrap()).unwrap()
    }

    /// Merges entries that the device `from` made.
    fn receive(&self, from: &Device, entries: &[SharedEntry]) -> AppliedEntries {
        self.library.apply_shared_entries(from.id, entries).unwrap()
    }

    /// What the tests compare of a library: its items with their status,
    /// its tags, folders and smart folders, its tombstones, and its shared
    /// entries with their clock times and devices.
    fn content(&self) -> BTreeMap<&'static str, Vec<String>> {
        let snapshot = self.library.projections().snapshot();
        let mut items = lines(
            &self.library,
            "SELECT item.stable_key, root.root_id FROM library_root root
             JOIN library_item item ON item.local_id = root.root_id ORDER BY 1",
        );
        for item in &mut items {
            let root: u32 = item.rsplit('(').next().unwrap().trim_end_matches(')').parse().unwrap();
            let status = Lifecycle::ALL
                .into_iter()
                .find(|lifecycle| snapshot.lifecycle(*lifecycle).contains(root));
            *item = format!("{} {status:?}", item.split('|').next().unwrap());
        }
        BTreeMap::from([
            ("items", items),
            (
                "tags",
                lines(&self.library, "SELECT stable_key FROM tag_definition ORDER BY 1"),
            ),
            (
                "folders",
                lines(&self.library, "SELECT stable_key FROM folder_definition ORDER BY 1"),
            ),
            (
                "smart folders",
                lines(
                    &self.library,
                    "SELECT stable_key FROM smart_folder_definition ORDER BY 1",
                ),
            ),
            (
                "tombstones",
                lines(
                    &self.library,
                    "SELECT stable_key, deleted_at_ms FROM deletion_tombstone ORDER BY 1",
                ),
            ),
            (
                "entries",
                lines(
                    &self.library,
                    "SELECT object_kind, object_key, mutation_id, hlc_physical_ms, hlc_logical,
                            device_id
                     FROM cloud_tombstone WHERE object_kind != 'blob' ORDER BY 1, 2",
                ),
            ),
        ])
    }
}

/// Sends everything `from` made to `to`.
fn sync(from: &Device, to: &Device) -> AppliedEntries {
    to.receive(from, &from.take())
}

/// Three devices that sync one library. Each holds the items `one`, `two`
/// and `three` in All, a collection with two members ([`post`]), the tag
/// `keep:tag` on `one`, the folder `Folder` with `two` in it and a folder
/// `Inner` below it, and the smart folder `Smart`.
fn devices() -> (TempDir, Device, Device, Device) {
    let directory = TempDir::new().unwrap();
    let folders = ["first", "second", "third"].map(|name| directory.path().join(name));
    for folder in &folders {
        std::fs::create_dir_all(folder).unwrap();
    }
    {
        let library = Library::create(folders[0].join("library.sqlite")).unwrap();
        let mut one = imported("one", "hash-one");
        one.tags = vec!["keep:tag".into()];
        library.ingest(&one).unwrap();
        let (two, _) = library.ingest(&imported("two", "hash-two")).unwrap();
        library.ingest(&imported("three", "hash-three")).unwrap();
        library
            .ingest_collection(&PreparedCollectionImport {
                members: vec![
                    imported("post", "hash-post-1"),
                    imported("post-2", "hash-post-2"),
                ],
                cover_index: 0,
                existing_root_id: None,
                name: Some("Post".into()),
                modified_at_ms: IMPORTED_AT_MS,
            })
            .unwrap();
        let (folder, _) = library.create_folder("Folder", None).unwrap();
        library.create_folder("Inner", Some(folder)).unwrap();
        library
            .add_to_folder(&SelectionTarget::Explicit { root_ids: vec![two] }, folder)
            .unwrap();
        library
            .create_smart_folder(SmartFolderInput {
                name: "Smart".into(),
                parent_id: None,
                icon: None,
                color: None,
                notes: None,
                view: ViewQuerySpec {
                    filter: FilterExpr::Clause(FilterClause::Text {
                        field: TextField::Name,
                        query: "one".into(),
                    }),
                    sort: ItemSort::default(),
                },
            })
            .unwrap();
        library.database().checkpoint_wal().unwrap();
    }
    for folder in &folders[1..] {
        for entry in std::fs::read_dir(&folders[0]).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
    }
    let [a, b, c] = ["device-a", "device-b", "device-c"].map(|id| {
        let folder = &folders[(id.as_bytes()[7] - b'a') as usize];
        let library = Library::open(folder.join("library.sqlite")).unwrap();
        write(
            &library,
            &format!("UPDATE cloud_state SET device_id = '{id}'; DELETE FROM cloud_journal;"),
        );
        Device { library, id }
    });
    (directory, a, b, c)
}

/// Returns the stable key of the library's one collection.
fn post(device: &Device) -> String {
    device
        .library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(
                "SELECT stable_key FROM library_item WHERE item_kind = 2",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap()
}

fn folder(device: &Device, name: &str) -> Option<FolderId> {
    device
        .library
        .folders()
        .unwrap()
        .into_iter()
        .find(|folder| folder.name == name)
        .map(|folder| folder.folder_id)
}

fn smart_folder(device: &Device, name: &str) -> Option<SmartFolderId> {
    device
        .library
        .smart_folders()
        .unwrap()
        .into_iter()
        .find(|folder| folder.name == name)
        .map(|folder| folder.smart_folder_id)
}

fn has_tag(device: &Device, name: &str) -> bool {
    device
        .library
        .projections()
        .snapshot()
        .tag_ids_by_name
        .contains_key(name)
}

#[test]
fn a_trash_move_a_restore_and_a_permanent_deletion_reach_the_other_device() {
    let (_directory, a, b, _c) = devices();

    a.trash(&["one", "two"]);
    assert_eq!(sync(&a, &b).trashed, 2);
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
    assert_eq!(b.status("two"), Some(Lifecycle::Trash));
    assert_eq!(b.status("three"), Some(Lifecycle::Active));

    // The second device restores one item, and the first deletes the other.
    b.restore(&["one"]);
    assert_eq!(sync(&b, &a).restored, 1);
    assert_eq!(a.status("one"), Some(Lifecycle::Active));
    assert_eq!(a.status("two"), Some(Lifecycle::Trash));

    a.delete(&["two"]);
    assert_eq!(sync(&a, &b).deleted, 1);
    assert_eq!(b.status("two"), None);
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
    assert_eq!(b.status("three"), Some(Lifecycle::Active));
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM library_root"), 3);
    // The second device holds the item's tombstone, so an import of the
    // item that is not later than the deletion is refused there too.
    assert!(b.tombstoned("two"));
    assert!(b.library.ingest(&imported("two", "hash-two")).is_err());
}

#[test]
fn a_deleted_collection_is_deleted_with_its_members() {
    let (_directory, a, b, _c) = devices();
    let post = post(&a);
    a.delete(&[&post]);
    assert_eq!(sync(&a, &b).deleted, 1);
    assert_eq!(b.status(&post), None);
    assert_eq!(
        scalar(
            &b.library,
            "SELECT COUNT(*) FROM media_item media JOIN library_item item
               ON item.local_id = media.media_id
             WHERE item.stable_key IN ('post', 'post-2')"
        ),
        0
    );
    assert!(b.tombstoned("post") && b.tombstoned("post-2"));
}

#[test]
fn a_deleted_tag_folder_and_smart_folder_are_deleted_on_the_other_device() {
    let (_directory, a, b, _c) = devices();
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    a.library.delete_tag(tag, DELETED_AT_MS).unwrap();
    a.library
        .delete_folders(&[folder(&a, "Folder").unwrap()])
        .unwrap();
    a.library
        .delete_smart_folder(smart_folder(&a, "Smart").unwrap())
        .unwrap();

    let applied = sync(&a, &b);

    assert_eq!((applied.tags, applied.folders, applied.smart_folders), (1, 2, 1));
    assert!(!has_tag(&b, "keep:tag"));
    assert_eq!(folder(&b, "Folder"), None);
    assert_eq!(folder(&b, "Inner"), None);
    assert_eq!(smart_folder(&b, "Smart"), None);
    // The items that had the tag or were in the folder stay.
    let one = b.library.details(b.root("one").unwrap()).unwrap();
    assert!(one.tag_ids.is_empty());
    let two = b.library.details(b.root("two").unwrap()).unwrap();
    assert!(two.folder_ids.is_empty());
    assert_eq!(two.lifecycle, Lifecycle::Active);
}

#[test]
fn a_folder_with_a_folder_that_only_the_other_device_has_stays() {
    let (_directory, a, b, _c) = devices();
    let parent = folder(&b, "Folder").unwrap();
    b.library.create_folder("Only here", Some(parent)).unwrap();

    a.library
        .delete_folders(&[folder(&a, "Folder").unwrap()])
        .unwrap();
    let applied = sync(&a, &b);

    // `Inner` was deleted on the first device and is deleted here. `Folder`
    // holds a folder that the first device never had, so it stays with it.
    assert_eq!(applied.folders, 1);
    assert_eq!(folder(&b, "Inner"), None);
    assert!(folder(&b, "Folder").is_some());
    assert!(folder(&b, "Only here").is_some());
}

#[test]
fn a_permanent_deletion_wins_over_an_edit_and_a_trash_move_keeps_an_edit() {
    let (_directory, a, b, _c) = devices();
    b.library
        .set_rating(&b.target(&["one", "two"]), Rating::Five)
        .unwrap();
    a.delete(&["one"]);
    a.trash(&["two"]);

    let applied = sync(&a, &b);

    assert_eq!((applied.deleted, applied.trashed), (1, 1));
    assert_eq!(b.status("one"), None);
    let two = b.library.details(b.root("two").unwrap()).unwrap();
    assert_eq!((two.lifecycle, two.rating), (Lifecycle::Trash, Rating::Five));
}

#[test]
fn the_later_of_two_trash_moves_wins_on_both_devices() {
    let (_directory, a, b, _c) = devices();
    a.trash(&["one"]);
    sync(&a, &b);

    // Before the devices sync again, the first restores the item. The
    // second, whose clock is ahead, restores it and moves it to the Trash
    // again. The second device's moves are the later ones.
    a.restore(&["one"]);
    b.clock_ahead();
    b.restore(&["one"]);
    b.trash(&["one"]);

    let (from_a, from_b) = (a.take(), b.take());
    b.receive(&a, &from_a);
    a.receive(&b, &from_b);

    assert_eq!(a.status("one"), Some(Lifecycle::Trash));
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
}

#[test]
fn a_move_made_after_a_merged_move_is_the_later_one_whatever_the_wall_clock_says() {
    let (_directory, a, b, _c) = devices();
    // The first device's clock is far ahead of the second device's.
    a.clock_ahead();
    a.trash(&["one"]);
    sync(&a, &b);
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));

    // The second device restores the item after it has merged the move.
    b.restore(&["one"]);
    assert_eq!(sync(&b, &a).restored, 1);
    assert_eq!(a.status("one"), Some(Lifecycle::Active));
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
    // And the first device's next move is later again.
    a.trash(&["one"]);
    assert_eq!(sync(&a, &b).trashed, 1);
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
}

#[test]
fn an_item_imported_again_after_its_deletion_stays_in_every_order_of_arrival() {
    let (_directory, a, b, c) = devices();
    a.delete(&["one"]);
    let deletion = a.take();
    b.receive(&a, &deletion);
    assert!(b.status("one").is_none() && b.tombstoned("one"));

    // The second device imports the item again, later than the deletion.
    b.library.ingest(&imported_later("one", "hash-one")).unwrap();
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
    assert!(!b.tombstoned("one"));
    let added = b.take();
    assert!(added.iter().any(|entry| {
        (entry.kind, entry.key.as_str(), entry.state) == (EntryKind::Item, "one", EntryState::Add)
    }));

    // The first device still publishes the older tombstone. It does not
    // delete the item that was added again.
    assert_eq!(b.receive(&a, &deletion).won, 0);
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
    // The first device learns that the item was added again: its tombstone
    // goes, so the item can be imported there again as well.
    a.receive(&b, &added);
    assert!(!a.tombstoned("one"));
    a.library.ingest(&imported("one", "hash-one")).unwrap();
    assert_eq!(a.status("one"), Some(Lifecycle::Active));

    // A third device gets the two entries in either order and ends without
    // a tombstone in both.
    c.receive(&b, &added);
    c.receive(&a, &deletion);
    assert_eq!(c.status("one"), Some(Lifecycle::Active));
    assert!(!c.tombstoned("one"));
}

#[test]
fn an_item_with_another_file_under_the_same_stable_key_is_not_deleted() {
    let (_directory, a, b, _c) = devices();
    // After the copies were made, both devices import a file from the same
    // path in an import task of the same number, so the two items get one
    // stable key. The files are different pictures.
    let key = "task:9:/photos/a.png";
    a.library.ingest(&imported(key, "hash-on-a")).unwrap();
    b.library.ingest(&imported(key, "hash-on-b")).unwrap();

    // The first device deletes its item. The entry names the file's hash.
    a.delete(&[key]);
    let entries = a.take();
    let deletion = entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Item && entry.key == key)
        .unwrap();
    assert_eq!(deletion.state, EntryState::Delete);
    assert_eq!(deletion.file.as_deref(), Some("hash-on-a"));

    // The second device merges the entry and keeps its own picture. The
    // entry is stored and the tombstone is written, as for an item that the
    // library does not hold.
    let applied = b.receive(&a, &entries);
    assert_eq!(applied.deleted, 0);
    assert_eq!(b.status(key), Some(Lifecycle::Active));
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM library_root"), 5);
    assert!(b.tombstoned(key));
    assert_eq!(b.receive(&a, &entries).won, 0);
    assert_eq!(b.status(key), Some(Lifecycle::Active));

    // An item with the same file is deleted, and so is a collection, whose
    // entry names no file.
    let post = post(&a);
    a.delete(&["one", &post]);
    let entries = a.take();
    let file_of = |key: &str| {
        entries
            .iter()
            .find(|entry| entry.kind == EntryKind::Item && entry.key == key)
            .unwrap()
            .file
            .clone()
    };
    assert_eq!(file_of("one").as_deref(), Some("hash-one"));
    assert_eq!(file_of(&post), None);
    assert_eq!(b.receive(&a, &entries).deleted, 2);
    assert_eq!(b.status("one"), None);
    assert_eq!(b.status(&post), None);
    assert_eq!(b.status(key), Some(Lifecycle::Active));
}

#[test]
fn an_item_that_was_imported_on_another_device_after_the_deletion_stays_there() {
    let (_directory, a, b, _c) = devices();
    // Both devices import an item under one stable key after the copies
    // were made. The first deletes its item. The second imports its item
    // later than that deletion, before the deletion arrives.
    a.library.ingest(&imported("new", "hash-new")).unwrap();
    a.delete(&["new"]);
    b.library.ingest(&imported_later("new", "hash-new")).unwrap();

    let applied = sync(&a, &b);

    assert_eq!(applied.deleted, 0);
    assert_eq!(b.status("new"), Some(Lifecycle::Active));
    assert!(!b.tombstoned("new"));
    // The second device says that the item was added again, and the first
    // device's tombstone goes.
    sync(&b, &a);
    assert!(!a.tombstoned("new"));
}

#[test]
fn an_item_that_is_deleted_after_it_was_imported_again_ends_deleted_on_both() {
    let (_directory, a, b, _c) = devices();
    a.delete(&["one"]);
    sync(&a, &b);
    // The second device imports the item again, and the first takes that
    // over and imports it again too.
    b.library.ingest(&imported_later("one", "hash-one")).unwrap();
    sync(&b, &a);
    a.library.ingest(&imported_later("one", "hash-one")).unwrap();
    assert_eq!(a.status("one"), Some(Lifecycle::Active));

    // Then the first device deletes it. That deletion is the last word.
    a.delete(&["one"]);
    assert_eq!(sync(&a, &b).deleted, 1);
    sync(&b, &a);

    for device in [&a, &b] {
        assert_eq!(device.status("one"), None, "{}", device.id);
        assert!(device.tombstoned("one"), "{}", device.id);
    }
}

#[test]
fn a_device_that_was_away_neither_brings_items_back_nor_deletes_new_ones() {
    let (_directory, a, b, _c) = devices();
    // While the second device is away, the first deletes `one`. The second
    // imports a new item and moves `three` to the Trash.
    a.delete(&["one"]);
    b.library.ingest(&imported_later("new", "hash-new")).unwrap();
    b.trash(&["three"]);

    // The second device comes back. Both sync, the second first.
    sync(&b, &a);
    assert_eq!(sync(&a, &b).deleted, 1);
    sync(&b, &a);

    assert_eq!(b.status("one"), None);
    assert_eq!(a.status("one"), None);
    assert!(a.tombstoned("one") && b.tombstoned("one"));
    assert_eq!(b.status("new"), Some(Lifecycle::Active));
    assert_eq!(a.status("three"), Some(Lifecycle::Trash));
    // The first device holds exactly what it held, less `one`.
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM library_root"), 3);
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM library_root"), 4);
}

#[test]
fn the_same_entries_twice_or_in_another_order_give_the_same_library() {
    let (_directory, a, b, c) = devices();
    // The first device makes changes in four steps, and its entries are
    // taken after each step.
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    let mut steps = Vec::new();
    a.trash(&["one", "two", "three"]);
    steps.push(a.take());
    a.restore(&["one"]);
    a.library.delete_tag(tag, DELETED_AT_MS).unwrap();
    steps.push(a.take());
    a.library
        .permanently_delete(&a.target(&["two"]), DELETED_AT_MS, 500, false)
        .unwrap();
    a.library
        .delete_folders(&[folder(&a, "Folder").unwrap()])
        .unwrap();
    steps.push(a.take());
    a.trash(&["one"]);
    a.library
        .delete_smart_folder(smart_folder(&a, "Smart").unwrap())
        .unwrap();
    steps.push(a.take());

    // The second device merges them in order. The third merges them in the
    // reverse order, every step twice, and one entry at a time.
    for step in &steps {
        b.receive(&a, step);
    }
    for step in steps.iter().rev() {
        c.receive(&a, step);
        for entry in step.iter().rev() {
            c.receive(&a, std::slice::from_ref(entry));
        }
    }

    assert_eq!(b.content(), c.content());
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
    assert_eq!(b.status("two"), None);
    assert_eq!(b.status("three"), Some(Lifecycle::Trash));
    assert!(!has_tag(&b, "keep:tag"));
    assert_eq!(folder(&b, "Folder"), None);
    assert_eq!(smart_folder(&b, "Smart"), None);
    // Merging everything once more changes nothing and writes nothing.
    let revision = c.library.database().revision().unwrap();
    for step in &steps {
        assert_eq!(c.receive(&a, step), AppliedEntries::default());
    }
    assert_eq!(c.library.database().revision().unwrap(), revision);
}

#[test]
fn a_merged_deletion_is_not_on_the_undo_stack_and_local_undo_still_works() {
    let (_directory, a, b, _c) = devices();
    // The second device makes an edit of its own, which undo must restore.
    b.library
        .set_rating(&b.target(&["three"]), Rating::Five)
        .unwrap();
    let entries = b.library.history().state().entries;

    a.trash(&["one"]);
    a.delete(&["two"]);
    sync(&a, &b);

    assert_eq!(b.library.history().state().entries, entries);
    b.library.undo().unwrap();
    let three = b.library.details(b.root("three").unwrap()).unwrap();
    assert_eq!(three.rating, Rating::Unrated);
    // The undo restored the rating and nothing that came through sync.
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
    assert_eq!(b.status("two"), None);
    assert!(!b.library.history().state().can_undo);
}

#[test]
fn an_undone_deletion_deletes_nothing_elsewhere_and_a_redone_one_does() {
    let (_directory, a, b, _c) = devices();
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    a.delete(&["two"]);
    a.library.delete_tag(tag, DELETED_AT_MS).unwrap();
    a.library
        .delete_folders(&[folder(&a, "Folder").unwrap()])
        .unwrap();

    // Undo the folders, the tag, the permanent deletion and the Trash move.
    for _ in 0..4 {
        a.library.undo().unwrap().unwrap();
    }
    assert_eq!(a.status("two"), Some(Lifecycle::Active));
    assert!(has_tag(&a, "keep:tag") && !a.tombstoned("two"));
    let two = a.library.details(a.root("two").unwrap()).unwrap();
    assert_eq!(two.folder_ids, vec![folder(&a, "Folder").unwrap()]);

    let applied = sync(&a, &b);
    assert_eq!(
        (applied.deleted, applied.trashed, applied.tags, applied.folders),
        (0, 0, 0, 0)
    );
    assert_eq!(b.status("two"), Some(Lifecycle::Active));
    assert!(has_tag(&b, "keep:tag") && folder(&b, "Folder").is_some());

    // Redo all four. The second device now makes the same deletions.
    for _ in 0..4 {
        a.library.redo().unwrap().unwrap();
    }
    let applied = sync(&a, &b);
    assert_eq!((applied.deleted, applied.tags, applied.folders), (1, 1, 2));
    assert_eq!(b.status("two"), None);
    assert!(!has_tag(&b, "keep:tag") && folder(&b, "Folder").is_none());
}

#[test]
fn a_deleted_item_keeps_no_status_entry() {
    let (_directory, a, _b, _c) = devices();
    // A status entry is written only for an item that still exists.
    a.trash(&["one", "two"]);
    a.library
        .permanently_delete(&a.target(&["one"]), DELETED_AT_MS, 0, false)
        .unwrap();
    let entries = a.take();
    let named = |kind: EntryKind, key: &str| {
        entries
            .iter()
            .filter(|entry| entry.kind == kind && entry.key == key)
            .map(|entry| entry.state)
            .collect::<Vec<_>>()
    };
    assert_eq!(named(EntryKind::Item, "one"), [EntryState::Delete]);
    assert!(named(EntryKind::Status, "one").is_empty());
    assert_eq!(named(EntryKind::Status, "two"), [EntryState::Trash]);
    assert_eq!(entries.len(), 2);
}
