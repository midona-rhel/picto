//! Deletions that cloud sync carries from one device of a library to
//! another. Two libraries stand for the two devices: the second is a copy of
//! the first with its own device id, as a library opened from the cloud is.
//! The deletions travel as the JSON that the cloud file holds.

use picto_library::database::WorkPriority;
use picto_library::predicate::{FilterClause, FilterExpr, ItemSort, TextField, ViewQuerySpec};
use picto_library::selection::SelectionTarget;
use picto_library::{
    AppliedDeletions, FolderId, ImmutableMediaFacts, Library, Lifecycle, OutgoingCursor,
    PreparedCollectionImport, PreparedImport, Rating, ReceivedDeletions, RootId, SmartFolderId,
    SmartFolderInput, SyncedDeletion,
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

/// One device: its library, its device id, and the number of the last
/// deletion file it wrote.
struct Device {
    library: Library,
    id: &'static str,
    written: u32,
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

    /// Deletes Trash items permanently, with undo for up to 500 items.
    fn delete(&self, keys: &[&str]) {
        self.library
            .permanently_delete(&self.target(keys), DELETED_AT_MS, 500, false)
            .unwrap();
    }

    /// Gives the Trash moves that this device has not sent yet the time
    /// `at_ms`, in place of the time at which the test ran.
    fn moved_at(&self, at_ms: i64) {
        write(
            &self.library,
            &format!(
                "UPDATE cloud_journal SET created_at_ms = {at_ms}
                 WHERE operation_kind = 'root.trash'"
            ),
        );
    }

    /// Takes the deletions that this device has not sent yet, as the sync
    /// does when it publishes: it reads them, records that they were
    /// written, and removes their journal rows.
    fn take(&mut self) -> Vec<SyncedDeletion> {
        let revision = self.library.database().revision().unwrap();
        let outgoing = self
            .library
            .outgoing_deletions(revision, OutgoingCursor::default(), 10_000)
            .unwrap();
        assert!(outgoing.next.is_none());
        self.written += 1;
        self.library
            .record_written_deletions(self.written, 0, &outgoing.deletions)
            .unwrap();
        write(
            &self.library,
            &format!("DELETE FROM cloud_journal WHERE revision <= {revision}"),
        );
        // The deletions cross between the devices as JSON.
        serde_json::from_str(&serde_json::to_string(&outgoing.deletions).unwrap()).unwrap()
    }

    /// Applies the deletion file `sequence` of the device `from`.
    fn receive(&self, from: &Device, deletions: &[SyncedDeletion]) -> AppliedDeletions {
        self.library
            .apply_synced_deletions(&ReceivedDeletions {
                device_id: from.id,
                sequence: from.written,
                created_at_ms: 0,
                deletions,
            })
            .unwrap()
    }
}

/// Sends what `from` deleted to `to`.
fn sync(from: &mut Device, to: &Device) -> AppliedDeletions {
    let deletions = from.take();
    to.receive(from, &deletions)
}

/// Two devices that sync one library. Each holds the items `one`, `two` and
/// `three` in All, a collection with two members ([`post`]), the tag
/// `keep:tag` on `one`, the folder `Folder` with `two` in it and a folder
/// `Inner` below it, and the smart folder `Smart`.
fn devices() -> (TempDir, Device, Device) {
    let directory = TempDir::new().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    {
        let library = Library::create(first.join("library.sqlite")).unwrap();
        write(
            &library,
            "UPDATE cloud_state SET provider = 'dropbox', device_id = 'device-a'",
        );
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
    for entry in std::fs::read_dir(&first).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), second.join(entry.file_name())).unwrap();
    }
    let a = Library::open(first.join("library.sqlite")).unwrap();
    let b = Library::open(second.join("library.sqlite")).unwrap();
    write(&b, "UPDATE cloud_state SET device_id = 'device-b'");
    write(&a, "DELETE FROM cloud_journal");
    write(&b, "DELETE FROM cloud_journal");
    (
        directory,
        Device { library: a, id: "device-a", written: 0 },
        Device { library: b, id: "device-b", written: 0 },
    )
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
    let (_directory, mut a, mut b) = devices();

    a.trash(&["one", "two"]);
    let applied = sync(&mut a, &b);
    assert_eq!(applied.trashed, 2);
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
    assert_eq!(b.status("two"), Some(Lifecycle::Trash));
    assert_eq!(b.status("three"), Some(Lifecycle::Active));

    // The second device restores one item, and the first deletes the other.
    b.restore(&["one"]);
    assert_eq!(sync(&mut b, &a).restored, 1);
    assert_eq!(a.status("one"), Some(Lifecycle::Active));
    assert_eq!(a.status("two"), Some(Lifecycle::Trash));

    a.delete(&["two"]);
    assert_eq!(sync(&mut a, &b).deleted, 1);
    assert_eq!(b.status("two"), None);
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
    assert_eq!(b.status("three"), Some(Lifecycle::Active));
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM library_root"), 3);
    // The deleted item cannot come back through a later import on the
    // second device either: its stable key is recorded as deleted.
    assert_eq!(
        scalar(
            &b.library,
            "SELECT COUNT(*) FROM deletion_tombstone WHERE stable_key = 'two'"
        ),
        1
    );
}

#[test]
fn a_deleted_collection_is_deleted_with_its_members() {
    let (_directory, mut a, b) = devices();
    let post = post(&a);
    a.trash(&[&post]);
    a.delete(&[&post]);
    let applied = sync(&mut a, &b);
    assert_eq!(applied.deleted, 1);
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
}

#[test]
fn a_deleted_tag_folder_and_smart_folder_are_deleted_on_the_other_device() {
    let (_directory, mut a, b) = devices();
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    a.library.delete_tag(tag, DELETED_AT_MS).unwrap();
    a.library
        .delete_folders(&[folder(&a, "Folder").unwrap()])
        .unwrap();
    a.library
        .delete_smart_folder(smart_folder(&a, "Smart").unwrap())
        .unwrap();

    let applied = sync(&mut a, &b);

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
    let (_directory, mut a, b) = devices();
    let parent = folder(&b, "Folder").unwrap();
    b.library.create_folder("Only here", Some(parent)).unwrap();

    a.library
        .delete_folders(&[folder(&a, "Folder").unwrap()])
        .unwrap();
    let applied = sync(&mut a, &b);

    // `Inner` was deleted on the first device and is deleted here. `Folder`
    // holds a folder that the first device never had, so it stays with it.
    assert_eq!(applied.folders, 1);
    assert_eq!(folder(&b, "Inner"), None);
    assert!(folder(&b, "Folder").is_some());
    assert!(folder(&b, "Only here").is_some());
}

#[test]
fn a_permanent_deletion_wins_over_an_edit_on_the_other_device() {
    let (_directory, mut a, b) = devices();
    // The second device rates and restores nothing: the item is in All there
    // and gets five stars before the deletion arrives.
    b.library
        .set_rating(&b.target(&["one"]), Rating::Five)
        .unwrap();
    a.trash(&["one"]);
    a.delete(&["one"]);

    assert_eq!(sync(&mut a, &b).deleted, 1);
    assert_eq!(b.status("one"), None);
}

#[test]
fn an_item_trashed_on_one_device_and_edited_on_the_other_is_in_the_trash_with_the_edit() {
    let (_directory, mut a, b) = devices();
    b.library
        .set_rating(&b.target(&["one"]), Rating::Four)
        .unwrap();
    a.trash(&["one"]);

    sync(&mut a, &b);

    let one = b.library.details(b.root("one").unwrap()).unwrap();
    assert_eq!(one.lifecycle, Lifecycle::Trash);
    assert_eq!(one.rating, Rating::Four);
}

#[test]
fn the_later_of_two_trash_moves_wins_on_both_devices() {
    let (_directory, mut a, mut b) = devices();
    a.trash(&["one"]);
    a.moved_at(1_000);
    sync(&mut a, &b);

    // Before the devices sync again, the first restores the item at 2000
    // and the second, which still has it in the Trash, restores it and
    // moves it to the Trash again at 3000.
    a.restore(&["one"]);
    a.moved_at(2_000);
    b.restore(&["one"]);
    b.trash(&["one"]);
    b.moved_at(3_000);

    let from_a = a.take();
    let from_b = b.take();
    b.receive(&a, &from_a);
    a.receive(&b, &from_b);

    assert_eq!(a.status("one"), Some(Lifecycle::Trash));
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));
}

#[test]
fn a_move_made_after_a_synced_move_wins_when_this_devices_clock_is_behind() {
    let (_directory, mut a, mut b) = devices();
    // The first device's clock is far ahead: its Trash move has a time in
    // the year 2128.
    a.trash(&["one"]);
    a.moved_at(5_000_000_000_000);
    sync(&mut a, &b);
    assert_eq!(b.status("one"), Some(Lifecycle::Trash));

    // The second device restores the item after it has taken the move over.
    // Its clock says an earlier time, and the restore still wins.
    b.restore(&["one"]);
    assert_eq!(sync(&mut b, &a).restored, 1);
    assert_eq!(a.status("one"), Some(Lifecycle::Active));
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
}

#[test]
fn a_move_made_after_a_synced_move_wins_in_the_same_millisecond_and_after_the_clock_went_back() {
    let (_directory, mut a, mut b) = devices();
    // The second device, whose id is the greater one, moves two items to
    // the Trash, and the first device takes the moves over.
    b.trash(&["one", "two"]);
    b.moved_at(5_000);
    sync(&mut b, &a);

    // The first device restores `one` in the millisecond of that move. By
    // the time and the device ids alone, the restore would be the earlier
    // of the two.
    a.restore(&["one"]);
    a.moved_at(5_000);
    assert_eq!(sync(&mut a, &b).restored, 1);
    // Then its clock is set back, and it restores `two` at a time before
    // the second device's move.
    a.restore(&["two"]);
    a.moved_at(4_000);
    assert_eq!(sync(&mut a, &b).restored, 1);

    for key in ["one", "two"] {
        assert_eq!(a.status(key), Some(Lifecycle::Active), "{key}");
        assert_eq!(b.status(key), Some(Lifecycle::Active), "{key}");
    }

    // The second device has taken the restores over. It moves `two` to the
    // Trash again with its clock set back, and that move is the last one.
    b.trash(&["two"]);
    b.moved_at(1_000);
    assert_eq!(sync(&mut b, &a).trashed, 1);
    assert_eq!(a.status("two"), Some(Lifecycle::Trash));
    assert_eq!(b.status("two"), Some(Lifecycle::Trash));
}

#[test]
fn a_device_that_was_away_neither_brings_items_back_nor_deletes_new_ones() {
    let (_directory, mut a, mut b) = devices();
    // While the second device is away, the first deletes `one`. The second
    // imports a new item and moves `three` to the Trash.
    a.trash(&["one"]);
    a.delete(&["one"]);
    let while_away = a.take();
    b.library.ingest(&imported("new", "hash-new")).unwrap();
    b.trash(&["three"]);

    // The second device comes back: it sends first and receives afterwards.
    let from_b = b.take();
    a.receive(&b, &from_b);
    let applied = b.receive(&a, &while_away);

    assert_eq!(applied.deleted, 1);
    assert_eq!(b.status("one"), None);
    assert_eq!(a.status("one"), None);
    assert_eq!(b.status("new"), Some(Lifecycle::Active));
    assert_eq!(a.status("three"), Some(Lifecycle::Trash));
    // The first device holds exactly what it held, less `one`.
    assert_eq!(scalar(&a.library, "SELECT COUNT(*) FROM library_root"), 3);
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM library_root"), 4);
}

#[test]
fn an_item_with_the_same_key_and_another_file_is_left_alone() {
    let (_directory, mut a, b) = devices();
    // After the copy was made, both devices import a file under the same
    // stable key, as two imports of one path do. The files differ.
    a.library
        .ingest(&imported("task:9:/photos/a.png", "hash-on-a"))
        .unwrap();
    b.library
        .ingest(&imported("task:9:/photos/a.png", "hash-on-b"))
        .unwrap();

    a.trash(&["task:9:/photos/a.png"]);
    assert_eq!(sync(&mut a, &b).trashed, 0);
    a.delete(&["task:9:/photos/a.png"]);
    assert_eq!(sync(&mut a, &b).deleted, 0);

    assert_eq!(b.status("task:9:/photos/a.png"), Some(Lifecycle::Active));
}

#[test]
fn an_item_imported_after_the_deletion_stays() {
    let (_directory, mut a, b) = devices();
    a.trash(&["one"]);
    a.delete(&["one"]);
    let deletions = a.take();

    // The second device deletes its copy itself and imports the same file
    // again, later than the first device's deletion.
    b.trash(&["one"]);
    b.library
        .permanently_delete(&b.target(&["one"]), DELETED_AT_MS, 0, false)
        .unwrap();
    let mut again = imported("one", "hash-one");
    again.imported_at_ms = DELETED_AT_MS + 1_000;
    b.library.ingest(&again).unwrap();

    assert_eq!(b.receive(&a, &deletions).deleted, 0);
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
}

#[test]
fn a_collection_that_gained_a_member_on_the_other_device_is_not_deleted() {
    let (_directory, mut a, b) = devices();
    let post = post(&a);
    b.library
        .ingest_collection(&PreparedCollectionImport {
            members: vec![imported("post-3", "hash-post-3")],
            cover_index: 0,
            existing_root_id: b.root(&post),
            name: None,
            modified_at_ms: IMPORTED_AT_MS,
        })
        .unwrap();
    assert_eq!(
        b.library.details(b.root(&post).unwrap()).unwrap().media.len(),
        3
    );

    a.trash(&[&post]);
    a.delete(&[&post]);
    let applied = sync(&mut a, &b);

    // The record names two files and the collection holds three here, so
    // neither the Trash move nor the deletion is made.
    assert_eq!((applied.trashed, applied.deleted), (0, 0));
    assert_eq!(b.status(&post), Some(Lifecycle::Active));
}

#[test]
fn a_synced_deletion_is_not_on_the_undo_stack_and_local_undo_still_works() {
    let (_directory, mut a, b) = devices();
    // The second device makes an edit of its own, which undo must restore.
    b.library
        .set_rating(&b.target(&["three"]), Rating::Five)
        .unwrap();
    let entries = b.library.history().state().entries;

    a.trash(&["one", "two"]);
    a.delete(&["two"]);
    sync(&mut a, &b);

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
fn an_undone_local_deletion_restores_everything_and_is_not_sent() {
    let (_directory, mut a, b) = devices();
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    a.trash(&["two"]);
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
    assert!(has_tag(&a, "keep:tag"));
    let two = a.library.details(a.root("two").unwrap()).unwrap();
    assert_eq!(two.folder_ids, vec![folder(&a, "Folder").unwrap()]);

    let applied = sync(&mut a, &b);
    assert_eq!(
        applied,
        AppliedDeletions {
            receipt: applied.receipt.clone(),
            ..Default::default()
        }
    );
    assert_eq!(b.status("two"), Some(Lifecycle::Active));
    assert!(has_tag(&b, "keep:tag"));
    assert!(folder(&b, "Folder").is_some());
}

#[test]
fn a_redone_deletion_is_sent() {
    let (_directory, mut a, b) = devices();
    let tag = a.library.projections().snapshot().tag_ids_by_name["keep:tag"];
    a.trash(&["two"]);
    a.delete(&["two"]);
    a.library.delete_tag(tag, DELETED_AT_MS).unwrap();
    for _ in 0..3 {
        a.library.undo().unwrap().unwrap();
    }
    // The undone deletions are not sent. The redone ones are.
    sync(&mut a, &b);
    assert_eq!(b.status("two"), Some(Lifecycle::Active));
    for _ in 0..3 {
        a.library.redo().unwrap().unwrap();
    }

    let applied = sync(&mut a, &b);
    assert_eq!((applied.deleted, applied.tags), (1, 1));
    assert_eq!(b.status("two"), None);
    assert!(!has_tag(&b, "keep:tag"));
}

#[test]
fn a_file_is_applied_once_and_in_order() {
    let (_directory, mut a, b) = devices();
    a.trash(&["one"]);
    let first = a.take();
    a.restore(&["one"]);
    let second = a.take();

    // The second file cannot be applied before the first.
    assert!(b
        .library
        .apply_synced_deletions(&ReceivedDeletions {
            device_id: a.id,
            sequence: 2,
            created_at_ms: 0,
            deletions: &second,
        })
        .is_err());
    assert_eq!(b.status("one"), Some(Lifecycle::Active));

    let apply = |sequence, deletions: &[SyncedDeletion]| {
        b.library
            .apply_synced_deletions(&ReceivedDeletions {
                device_id: a.id,
                sequence,
                created_at_ms: 0,
                deletions,
            })
            .unwrap()
    };
    assert_eq!(apply(1, &first).trashed, 1);
    assert_eq!(apply(2, &second).restored, 1);
    // The first file again changes nothing.
    let revision = b.library.database().revision().unwrap();
    assert_eq!(apply(1, &first), AppliedDeletions::default());
    assert_eq!(b.library.database().revision().unwrap(), revision);
    assert_eq!(b.status("one"), Some(Lifecycle::Active));
}

#[test]
fn a_deletion_made_while_sync_is_off_is_not_sent() {
    let (_directory, mut a, b) = devices();
    write(&a.library, "UPDATE cloud_state SET provider = NULL");
    a.trash(&["one"]);
    a.delete(&["one"]);
    write(&a.library, "UPDATE cloud_state SET provider = 'dropbox'");

    let deletions = a.take();
    assert!(
        !deletions
            .iter()
            .any(|deletion| matches!(deletion, SyncedDeletion::ItemDelete { .. })),
        "{deletions:?}"
    );
    b.receive(&a, &deletions);
    assert_eq!(scalar(&b.library, "SELECT COUNT(*) FROM library_root"), 4);
}

#[test]
fn outgoing_deletions_are_read_in_pages() {
    let (_directory, a, _b) = devices();
    a.trash(&["one", "two", "three"]);
    a.delete(&["one"]);
    let revision = a.library.database().revision().unwrap();

    let whole = a
        .library
        .outgoing_deletions(revision, OutgoingCursor::default(), 100)
        .unwrap();
    assert_eq!(whole.deletions.len(), 3);
    assert!(whole.next.is_none());

    let mut paged = Vec::new();
    let mut cursor = Some(OutgoingCursor::default());
    while let Some(after) = cursor {
        let page = a.library.outgoing_deletions(revision, after, 1).unwrap();
        assert!(page.deletions.len() <= 1);
        paged.extend(page.deletions);
        cursor = page.next;
    }
    assert_eq!(paged, whole.deletions);
}
