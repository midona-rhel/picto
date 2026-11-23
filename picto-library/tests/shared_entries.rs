//! The entries that the devices of one library share. Three libraries stand
//! for three devices: the second and the third are copies of the first with
//! device ids of their own, as a library opened from the cloud is. The
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
        while self.library.settle_journal(2).unwrap() {}
        let mut entries = Vec::new();
        self.library
            .visit_own_entries(None, &mut |entry| entries.push(entry))
            .unwrap();
        self.library.attach_values(&mut entries).unwrap();
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
                    "SELECT kind, key, state, clock_ms, clock_counter, device_id
                     FROM shared_entry ORDER BY 1, 2",
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
    assert_eq!(deletion.value, Some(serde_json::json!({"file": "hash-on-a"})));

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
            .value
            .clone()
    };
    assert_eq!(file_of("one"), Some(serde_json::json!({"file": "hash-one"})));
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

/// Makes the three devices record the items that they import from now on,
/// as connecting a library to a cloud folder does.
fn start_recording(devices: [&Device; 3]) {
    for device in devices {
        device
            .library
            .database()
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                picto_library::reset_published_entries(transaction)
            })
            .unwrap();
    }
}

impl Device {
    /// Does for the items that wait in the ingest queue what the cloud
    /// worker and the ingest worker do: gives each the paths of its files
    /// and imports it. Returns the number of items that were imported, and
    /// leaves out those that were deleted since they were queued.
    fn import_awaited(&self) -> usize {
        let awaited = self
            .library
            .awaited_shared_imports(100, "9999-01-01T00:00:00Z")
            .unwrap();
        let mut imported = 0;
        for item in awaited {
            let paths = item
                .import
                .record
                .files
                .iter()
                .map(|file| format!("/tmp/{}", file.facts.content_hash))
                .collect();
            let import = picto_library::SharedImport {
                paths,
                ..item.import
            };
            match self.library.ingest_shared(&import) {
                Ok(_) => imported += 1,
                Err(picto_library::LibraryError::ImportDeleted) => {}
                Err(error) => panic!("{error}"),
            }
            write(
                &self.library,
                &format!(
                    "DELETE FROM ingest_job WHERE ingest_job_id = {}",
                    item.ingest_job_id
                ),
            );
        }
        imported
    }

    /// The names of the item's tags and the keys and names of its folders,
    /// each list in order.
    fn tags_and_folders(&self, key: &str) -> (Vec<String>, Vec<String>) {
        let details = self.library.details(self.root(key).unwrap()).unwrap();
        let mut tags = details
            .tag_ids
            .iter()
            .map(|tag| {
                lines(
                    &self.library,
                    &format!(
                        "SELECT namespace.display_name || ':' || tag.subname
                         FROM tag_definition tag JOIN tag_namespace namespace
                           ON namespace.namespace_id = tag.namespace_id
                         WHERE tag.tag_id = {}",
                        tag.0
                    ),
                )
                .remove(0)
                .trim_start_matches("Text(\"")
                .trim_end_matches("\")")
                .to_owned()
            })
            .collect::<Vec<_>>();
        tags.sort();
        let mut folders = details
            .folder_ids
            .iter()
            .map(|folder| {
                lines(
                    &self.library,
                    &format!(
                        "SELECT stable_key || ' ' || name FROM folder_definition
                         WHERE folder_id = {}",
                        folder.0
                    ),
                )
                .remove(0)
                .trim_start_matches("Text(\"")
                .trim_end_matches("\")")
                .to_owned()
            })
            .collect::<Vec<_>>();
        folders.sort();
        (tags, folders)
    }
}

/// An import of the key `new` with a name, notes, a source link, a rating
/// and two tags, into the folder `folder` and into the Inbox.
fn rich_import(folder: FolderId) -> PreparedImport {
    PreparedImport {
        media_name: "Evening.png".into(),
        lifecycle: Lifecycle::Inbox,
        rating: Rating::Four,
        notes: Some("Taken at the lake".into()),
        tags: vec!["keep:tag".into(), "place:lake".into()],
        folders: vec![folder],
        source_urls: vec!["https://example.com/evening".into()],
        ..imported_later("new", "hash-new")
    }
}

#[test]
fn an_item_imported_on_one_device_is_imported_on_the_others_as_it_was() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let folder_id = folder(&a, "Folder").unwrap();
    // The first device also puts the item into a folder that the others do
    // not hold yet.
    let (album, _) = a.library.create_folder("Album", None).unwrap();
    let mut import = rich_import(folder_id);
    import.folders.push(album);
    a.library.ingest(&import).unwrap();

    let entries = a.take();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].kind, entries[0].state),
        (EntryKind::Item, EntryState::Add)
    );
    assert_eq!(entries[0].key, "new");

    // The other devices queue the item. Nothing is in their library until
    // its file is there.
    for device in [&b, &c] {
        let applied = device.receive(&a, &entries);
        assert_eq!((applied.queued, applied.aliased), (1, 0));
        assert_eq!(device.status("new"), None);
        // Merging the same entry again queues nothing.
        assert_eq!(device.receive(&a, &entries).won, 0);
        assert_eq!(device.import_awaited(), 1);
    }

    let on_a = a.library.details(a.root("new").unwrap()).unwrap();
    for device in [&b, &c] {
        let details = device.library.details(device.root("new").unwrap()).unwrap();
        assert_eq!(details.lifecycle, Lifecycle::Inbox);
        assert_eq!(details.rating, Rating::Four);
        assert_eq!(details.root.name, "Evening.png");
        assert_eq!(details.root.notes.as_deref(), Some("Taken at the lake"));
        assert_eq!(details.root.source_urls, ["https://example.com/evening"]);
        assert_eq!(details.root.imported_at_ms, on_a.root.imported_at_ms);
        assert_eq!(details.media[0].facts, on_a.media[0].facts);
        assert_eq!(device.tags_and_folders("new"), a.tags_and_folders("new"));
        // The merged import is not on the undo stack, and the device made
        // no entry of its own for it.
        assert!(!device.library.history().state().can_undo);
        assert!(device.take().is_empty());
        assert_eq!(
            lines(&device.library, "SELECT device_id FROM shared_entry WHERE key = 'new'"),
            lines(&a.library, "SELECT device_id FROM shared_entry WHERE key = 'new'"),
        );
    }
    // The tag that the import created has the same key on every device.
    let tag_key = "SELECT stable_key FROM tag_definition WHERE subname = 'lake'";
    assert_eq!(lines(&b.library, tag_key), lines(&a.library, tag_key));
    assert_eq!(lines(&c.library, tag_key), lines(&a.library, tag_key));
}

#[test]
fn a_library_that_never_synced_records_no_item() {
    let (_directory, a, b, _c) = devices();
    a.library.ingest(&imported_later("new", "hash-new")).unwrap();
    assert!(a.take().is_empty());
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM shared_entry"), 0);
    // Its deletions are recorded as before.
    a.delete(&["new"]);
    assert_eq!(a.take().len(), 1);
    assert_eq!(sync(&a, &b).queued, 0);
}

#[test]
fn a_move_to_the_trash_that_arrives_before_the_item_is_kept_for_it() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    a.library.ingest(&imported_later("new", "hash-new")).unwrap();
    let added = a.take();
    a.trash(&["new"]);
    let all = a.take();
    let moved = all
        .iter()
        .filter(|entry| entry.kind == EntryKind::Status)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(moved.len(), 1);

    // The second device gets the move first and the item afterwards. The
    // third gets both before the item's file is there.
    b.receive(&a, &moved);
    assert_eq!(b.status("new"), None);
    b.receive(&a, &added);
    assert_eq!(b.import_awaited(), 1);
    c.receive(&a, &all);
    assert_eq!(c.import_awaited(), 1);
    for device in [&b, &c] {
        assert_eq!(device.status("new"), Some(Lifecycle::Trash), "{}", device.id);
    }

    // A device that already holds the item moves it at once.
    a.restore(&["new"]);
    sync(&a, &b);
    assert_eq!(b.status("new"), Some(Lifecycle::Active));
}

#[test]
fn an_item_that_is_deleted_before_its_file_arrives_is_never_imported() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    a.library.ingest(&imported_later("new", "hash-new")).unwrap();
    assert_eq!(sync(&a, &b).queued, 1);
    a.delete(&["new"]);

    // The deletion takes the item out of the queue.
    sync(&a, &b);
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM ingest_job"), 0);
    assert_eq!(b.import_awaited(), 0);
    assert_eq!(b.status("new"), None);
    assert!(b.tombstoned("new"));

    // A device that gets only the deletion queues nothing.
    let deletion = a.take();
    c.receive(&a, &deletion);
    assert_eq!(scalar(&c.library, "SELECT COUNT(*) FROM ingest_job"), 0);
    assert_eq!(c.status("new"), None);
}

#[test]
fn the_same_file_under_two_keys_is_one_item_and_a_deletion_reaches_it_under_both() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    // Two devices import the same picture, each under a key of its own.
    a.library
        .ingest(&imported_later("key-on-a", "hash-same"))
        .unwrap();
    b.library
        .ingest(&imported_later("key-on-b", "hash-same"))
        .unwrap();

    let applied = sync(&a, &b);
    assert_eq!((applied.queued, applied.aliased), (0, 1));
    let applied = sync(&b, &a);
    assert_eq!((applied.queued, applied.aliased), (0, 1));
    for device in [&a, &b] {
        assert_eq!(
            scalar(
                &device.library,
                "SELECT COUNT(*) FROM media_file WHERE content_hash = 'hash-same'"
            ),
            1
        );
        assert_eq!(scalar(&device.library, "SELECT COUNT(*) FROM library_root"), 5);
        assert_eq!(
            scalar(&device.library, "SELECT COUNT(*) FROM library_item_alias"),
            1
        );
    }

    // A move to the Trash that names the other device's key moves the item.
    a.trash(&["key-on-a"]);
    sync(&a, &b);
    assert_eq!(b.status("key-on-b"), Some(Lifecycle::Trash));

    // The first device deletes the item. The second device holds it under
    // its own key and deletes it too. The third device never held it.
    a.library
        .permanently_delete(&a.target(&["key-on-a"]), DELETED_AT_MS, 0, false)
        .unwrap();
    assert_eq!(sync(&a, &b).deleted, 1);
    assert_eq!(b.status("key-on-b"), None);
    assert!(b.tombstoned("key-on-a") && b.tombstoned("key-on-b"));
    sync(&a, &c);
    sync(&b, &c);
    assert_eq!(c.import_awaited(), 0);
    assert_eq!(scalar(&c.library, "SELECT COUNT(*) FROM library_root"), 4);
}

#[test]
fn an_imported_collection_arrives_with_its_files_in_order_and_its_key() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let mut members = ["hash-s-1", "hash-s-2", "hash-s-3"]
        .iter()
        .enumerate()
        .map(|(index, hash)| PreparedImport {
            tags: vec!["series:tag".into()],
            ..imported_later(&format!("series-{index}"), hash)
        })
        .collect::<Vec<_>>();
    members[1].media_name = "Second page.png".into();
    let (collection, _) = a
        .library
        .ingest_collection(&PreparedCollectionImport {
            members,
            cover_index: 1,
            existing_root_id: None,
            name: Some("Series".into()),
            modified_at_ms: IMPORTED_AT_MS,
        })
        .unwrap();
    let key = a.library.details(collection).unwrap().root.stable_key;

    let entries = a.take();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, key);
    assert_eq!(sync(&a, &b).queued, 1);
    assert_eq!(b.import_awaited(), 1);

    let of = |device: &Device| {
        let details = device.library.details(device.root(&key).unwrap()).unwrap();
        (
            details.root.name.clone(),
            details.lifecycle,
            details
                .media
                .iter()
                .map(|media| (media.media_name.clone(), media.facts.content_hash.clone()))
                .collect::<Vec<_>>(),
            details
                .media
                .iter()
                .position(|media| media.media_id == details.root.cover_media_id),
            device.tags_and_folders(&key).0,
        )
    };
    assert_eq!(of(&b), of(&a));
    assert_eq!(of(&b).2.len(), 3);
    assert_eq!(of(&b).3, Some(1));
    // The files have the keys that they have on the first device.
    let member_keys = "SELECT item.stable_key FROM media_item media
        JOIN library_item item ON item.local_id = media.media_id
        JOIN media_file file ON file.file_id = media.file_id
        WHERE file.content_hash LIKE 'hash-s-%' ORDER BY 1";
    assert_eq!(lines(&b.library, member_keys), lines(&a.library, member_keys));
    assert!(b.take().is_empty());
    // Merging the entry again queues nothing, because every file is here.
    write(
        &b.library,
        &format!("DELETE FROM shared_entry WHERE kind = 'item' AND key = '{key}'"),
    );
    assert_eq!(b.receive(&a, &entries).queued, 0);

    // Deleting the collection deletes it on the other device.
    a.delete(&[&key]);
    assert_eq!(sync(&a, &b).deleted, 1);
    assert_eq!(b.status(&key), None);
    drop(c);
}

#[test]
fn additions_and_deletions_give_the_same_library_in_every_order() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let mut steps = Vec::new();
    a.library.ingest(&imported_later("x", "hash-x")).unwrap();
    a.library.ingest(&imported_later("y", "hash-y")).unwrap();
    steps.push(a.take());
    a.trash(&["x", "y", "one"]);
    steps.push(a.take());
    a.library
        .permanently_delete(&a.target(&["y"]), DELETED_AT_MS, 0, false)
        .unwrap();
    a.restore(&["one"]);
    steps.push(a.take());

    // The second device merges the steps in order and imports after each.
    // The third merges them in the reverse order, every entry twice, and
    // imports at the end.
    for step in &steps {
        b.receive(&a, step);
        b.import_awaited();
    }
    for step in steps.iter().rev() {
        for entry in step.iter().rev() {
            c.receive(&a, std::slice::from_ref(entry));
            c.receive(&a, std::slice::from_ref(entry));
        }
    }
    c.import_awaited();

    assert_eq!(b.content(), c.content());
    assert_eq!(b.status("x"), Some(Lifecycle::Trash));
    assert_eq!(b.status("y"), None);
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
    assert_eq!(scalar(&c.library, "SELECT COUNT(*) FROM ingest_job"), 0);
}

impl Device {
    /// What the tests compare of one item: its name, notes, source links,
    /// rating and status, and the names of its tags and folders.
    fn fields(&self, key: &str) -> String {
        let details = self.library.details(self.root(key).unwrap()).unwrap();
        let (tags, folders) = self.tags_and_folders(key);
        format!(
            "{} | {:?} | {:?} | {:?} | {:?} | {tags:?} | {folders:?}",
            details.root.name,
            details.root.notes,
            details.root.source_urls,
            details.rating,
            details.lifecycle,
        )
    }

    /// The number of journal rows that wait to be turned into entries.
    fn waiting_rows(&self) -> i64 {
        scalar(
            &self.library,
            "SELECT COUNT(*) FROM cloud_journal WHERE operation_kind = 'entry.roots'",
        )
    }
}

/// Changes on the device `a`: a name, notes, a source link and a rating for
/// `one`, a new tag on `one` and `two`, the tag `keep:tag` taken from `one`,
/// `three` put into the folder `Folder` and `two` taken out of it, and `two`
/// moved from All into the Inbox.
fn edit(a: &Device) {
    let now = IMPORTED_AT_MS + 1_000;
    let folder = folder(a, "Folder").unwrap();
    a.library
        .rename_root(a.root("one").unwrap(), "First", now)
        .unwrap();
    a.library
        .set_notes(&a.target(&["one"]), Some("A note".into()), now)
        .unwrap();
    a.library
        .set_source_urls(&a.target(&["one"]), vec!["https://example.com/one".into()], now)
        .unwrap();
    a.library
        .set_rating(&a.target(&["one"]), Rating::Four)
        .unwrap();
    a.library
        .apply_tags(&a.target(&["one", "two"]), &["new:tag".into()], true)
        .unwrap();
    a.library
        .apply_tags(&a.target(&["one"]), &["keep:tag".into()], false)
        .unwrap();
    a.library.add_to_folder(&a.target(&["three"]), folder).unwrap();
    a.library
        .remove_from_folder(&a.target(&["two"]), folder)
        .unwrap();
    a.library
        .set_lifecycle(&a.target(&["two"]), Lifecycle::Inbox)
        .unwrap();
}

#[test]
fn edits_of_items_reach_the_other_devices_in_every_order() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    edit(&a);
    // Each change wrote journal rows and no entry.
    assert_eq!(a.waiting_rows(), 8);
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM shared_entry"), 0);
    let entries = a.take();
    assert_eq!(a.waiting_rows(), 0);
    assert_eq!(entries.len(), 10);

    // The second device merges them in one call. The third merges them one
    // at a time, in the reverse order, and each twice.
    let applied = b.receive(&a, &entries);
    assert!(applied.edited > 0);
    for entry in entries.iter().rev() {
        c.receive(&a, std::slice::from_ref(entry));
        c.receive(&a, std::slice::from_ref(entry));
    }

    assert_eq!(
        a.fields("one"),
        "First | Some(\"A note\") | [\"https://example.com/one\"] | Four | Active \
         | [\"new:tag\"] | []"
    );
    for device in [&b, &c] {
        for key in ["one", "two", "three"] {
            assert_eq!(device.fields(key), a.fields(key), "{key} on {}", device.id);
        }
        // Nothing of it can be undone there, and the device made no entry
        // of its own.
        assert!(!device.library.history().state().can_undo);
        assert!(device.take().is_empty());
        assert_eq!(device.waiting_rows(), 0);
    }
    assert_eq!(b.status("two"), Some(Lifecycle::Inbox));
    assert!(a.fields("three").contains("Folder"));
    assert!(!a.fields("two").contains("Folder"));
    // Merging everything again changes nothing.
    assert_eq!(b.receive(&a, &entries).won, 0);
}

#[test]
fn a_library_that_never_synced_records_no_edit() {
    let (_directory, a, _b, _c) = devices();
    edit(&a);
    assert_eq!(a.waiting_rows(), 0);
    assert!(a.take().is_empty());
}

#[test]
fn a_change_of_many_items_writes_one_row_when_it_is_made() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    a.library
        .apply_tags(&a.target(&["one", "two", "three"]), &["bulk:tag".into()], true)
        .unwrap();
    assert_eq!(a.waiting_rows(), 1);
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM shared_entry"), 0);
    // The rows are turned into entries a part at a time.
    assert!(a.library.settle_journal(2).unwrap());
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM shared_entry"), 2);
    assert!(!a.library.settle_journal(2).unwrap());
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM shared_entry"), 3);
    assert_eq!(sync(&a, &b).edited, 1);
    for key in ["one", "two", "three"] {
        assert!(b.fields(key).contains("bulk:tag"));
    }
}

#[test]
fn the_later_change_of_one_field_wins_and_changes_of_other_fields_are_kept() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let now = IMPORTED_AT_MS + 1_000;
    // Both devices rename the item, the second one later. The first also
    // gives it a rating, and the second a tag.
    a.library
        .rename_root(a.root("one").unwrap(), "Name on A", now)
        .unwrap();
    a.library
        .set_rating(&a.target(&["one"]), Rating::Two)
        .unwrap();
    let from_a = a.take();
    b.clock_ahead();
    b.library
        .rename_root(b.root("one").unwrap(), "Name on B", now)
        .unwrap();
    b.library
        .apply_tags(&b.target(&["one"]), &["from:b".into()], true)
        .unwrap();
    let from_b = b.take();

    a.receive(&b, &from_b);
    b.receive(&a, &from_a);
    c.receive(&a, &from_a);
    c.receive(&b, &from_b);
    for device in [&a, &b, &c] {
        let fields = device.fields("one");
        assert!(fields.starts_with("Name on B |"), "{fields}");
        assert!(fields.contains("Two") && fields.contains("from:b"), "{fields}");
    }
    assert_eq!(a.fields("one"), b.fields("one"));
    assert_eq!(a.fields("one"), c.fields("one"));
}

#[test]
fn an_undo_and_a_redo_of_an_edit_reach_the_other_device() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let before = a.fields("one");
    edit(&a);
    sync(&a, &b);
    let edited = a.fields("one");
    assert_eq!(b.fields("one"), edited);

    // Every change is undone, and the other device takes that over.
    while a.library.undo().unwrap().is_some() {}
    assert_eq!(a.fields("one"), before);
    sync(&a, &b);
    for key in ["one", "two", "three"] {
        assert_eq!(b.fields(key), a.fields(key), "{key}");
    }

    while a.library.redo().unwrap().is_some() {}
    assert_eq!(a.fields("one"), edited);
    sync(&a, &b);
    for key in ["one", "two", "three"] {
        assert_eq!(b.fields(key), a.fields(key), "{key}");
    }
}

#[test]
fn an_edit_that_arrives_before_its_item_is_given_to_the_item_at_its_import() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let folder_id = folder(&a, "Folder").unwrap();
    a.library.ingest(&rich_import(folder_id)).unwrap();
    // The entry that adds the item, with the record of the item as it was
    // imported.
    let added = a.take();
    let now = IMPORTED_AT_MS + 1_000;
    a.library
        .rename_root(a.root("new").unwrap(), "Morning.png", now)
        .unwrap();
    a.library
        .set_notes(&a.target(&["new"]), None, now)
        .unwrap();
    a.library
        .set_rating(&a.target(&["new"]), Rating::One)
        .unwrap();
    a.library
        .apply_tags(&a.target(&["new"]), &["late:tag".into()], true)
        .unwrap();
    a.library
        .apply_tags(&a.target(&["new"]), &["place:lake".into()], false)
        .unwrap();
    a.library
        .remove_from_folder(&a.target(&["new"]), folder_id)
        .unwrap();
    let edits = a
        .take()
        .into_iter()
        .filter(|entry| entry.kind != EntryKind::Item)
        .collect::<Vec<_>>();
    assert_eq!(edits.len(), 6);

    // The second device gets the edits first. They change nothing, because
    // the item is not there. Then it gets the item and imports it.
    assert_eq!(b.receive(&a, &edits).edited, 0);
    assert_eq!(b.receive(&a, &added).queued, 1);
    assert_eq!(b.import_awaited(), 1);
    assert_eq!(b.fields("new"), a.fields("new"));
    assert!(b.fields("new").starts_with("Morning.png | None |"));

    // The third device gets them in the other order.
    c.receive(&a, &added);
    c.receive(&a, &edits);
    assert_eq!(c.import_awaited(), 1);
    assert_eq!(c.fields("new"), a.fields("new"));
}

#[test]
fn an_import_that_changes_an_item_the_library_holds_shares_what_it_gave() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    // A subscription brings `one` again, with a tag that it did not have.
    let mut again = imported("one", "hash-one");
    again.tags = vec!["from:source".into()];
    a.library.ingest(&again).unwrap();
    let entries = a.take();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, EntryKind::ItemTag);
    assert_eq!(sync(&a, &b).edited, 1);
    assert_eq!(b.fields("one"), a.fields("one"));
    assert!(b.fields("one").contains("from:source"));
}

#[test]
fn an_edit_of_a_deleted_item_and_a_tag_of_a_deleted_tag_bring_nothing_back() {
    let (_directory, a, b, c) = devices();
    start_recording([&a, &b, &c]);
    let now = IMPORTED_AT_MS + 1_000;
    // The second device edits an item and uses a tag that the first device
    // deletes at the same time.
    b.library
        .rename_root(b.root("one").unwrap(), "Renamed", now)
        .unwrap();
    b.library
        .apply_tags(&b.target(&["two"]), &["keep:tag".into()], true)
        .unwrap();
    let edits = b.take();
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    a.library.delete_tag(tag, DELETED_AT_MS).unwrap();
    a.delete(&["one"]);

    sync(&a, &b);
    a.receive(&b, &edits);
    // The third device gets the deletions first and the edits afterwards.
    sync(&a, &c);
    c.receive(&b, &edits);
    for device in [&a, &b, &c] {
        assert_eq!(device.status("one"), None, "{}", device.id);
        assert!(!has_tag(device, "keep:tag"), "{}", device.id);
        assert!(!device.fields("two").contains("keep:tag"), "{}", device.id);
    }
}
