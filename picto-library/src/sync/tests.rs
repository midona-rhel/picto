//! Checks of recording and applying with real library operations. After
//! every operation a full comparison of the library with the synced values
//! must find nothing to record, and libraries that exchange their change
//! records must end with the same library.

use std::collections::BTreeMap;

use super::local::Local;
use super::{SyncKind, SyncRecord};
use crate::database::WorkPriority;
use crate::model::{
    FolderId, GroupRequest, ImmutableMediaFacts, Lifecycle, MediaId, PreparedImport, Rating,
    RepairedMediaContent, RootId, RootKind,
};
use crate::selection::SelectionTarget;
use crate::Library;

/// A small random number generator, so that a failing seed can be run again.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<T: Clone>(&mut self, values: &[T]) -> Option<T> {
        (!values.is_empty()).then(|| values[self.below(values.len())].clone())
    }
}

const TAGS: [&str; 6] = ["a", "b", "c", "d", "g:x", "g:y"];
const NAMES: [&str; 3] = ["x", "y", "z"];

fn hash(n: u64) -> String {
    format!("{n:064x}")
}

fn open(directory: &std::path::Path, name: &str) -> Library {
    let library = Library::create(directory.join(format!("{name}.sqlite"))).unwrap();
    library.start_sync_recording().unwrap();
    library
}

fn prepared(content_hash: String, lifecycle: Lifecycle, tags: Vec<String>, folders: Vec<FolderId>) -> PreparedImport {
    PreparedImport {
        stable_key: format!("test:{content_hash}"),
        media_name: format!("{}.png", &content_hash[60..]),
        file_path: format!("/test/{content_hash}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 100,
            width: Some(10),
            height: Some(10),
            duration_ms: None,
            frame_count: Some(1),
            content_hash,
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle,
        rating: Rating::Unrated,
        notes: None,
        tags,
        folders,
        source_urls: Vec::new(),
        source_identity: None,
        imported_at_ms: 1_000,
        captured_at_ms: None,
    }
}

fn roots(library: &Library) -> Vec<RootId> {
    let snapshot = library.projections().snapshot();
    let mut roots = Vec::new();
    for members in snapshot.root_kinds.values() {
        roots.extend(members.iter().map(RootId));
    }
    roots.sort();
    roots
}

fn kind_roots(library: &Library, kind: RootKind) -> Vec<RootId> {
    let snapshot = library.projections().snapshot();
    snapshot
        .root_kinds
        .get(&kind)
        .map(|members| members.iter().map(RootId).collect())
        .unwrap_or_default()
}

fn folders(library: &Library) -> Vec<FolderId> {
    let mut folders = library.projections().snapshot().folder_orders.keys().copied().collect::<Vec<_>>();
    folders.sort();
    folders
}

fn one(root: RootId) -> SelectionTarget {
    SelectionTarget::Explicit { root_ids: vec![root] }
}

/// Runs one random operation of the library. Operations that the library
/// refuses (for example a rename to a name that exists) change nothing.
fn operate(library: &Library, random: &mut Random, next_hash: &mut u64, time: &mut i64) -> String {
    *time += 1;
    let now = *time;
    let roots = roots(library);
    let folders = folders(library);
    let tags = library.tags().unwrap();
    let choice = random.below(27);
    let root = random.pick(&roots);
    let folder = random.pick(&folders);
    let tag = random.pick(&tags);
    let name = TAGS[random.below(TAGS.len())].to_owned();
    let result: crate::Result<()> = (|| {
        match choice {
            0 | 1 => {
                *next_hash += 1;
                let lifecycle = if random.below(2) == 0 { Lifecycle::Inbox } else { Lifecycle::Active };
                let tags = if random.below(2) == 0 { vec![name.clone()] } else { Vec::new() };
                let folders = folder.filter(|_| random.below(2) == 0).into_iter().collect();
                library.ingest(&prepared(hash(*next_hash % 24), lifecycle, tags, folders))?;
            }
            2 | 3 => {
                if let Some(root) = root {
                    library.apply_tags(&one(root), std::slice::from_ref(&name), true)?;
                }
            }
            4 => {
                if let (Some(root), Some(tag)) = (root, &tag) {
                    let full = if tag.namespace.is_empty() { tag.subname.clone() } else { format!("{}:{}", tag.namespace, tag.subname) };
                    library.apply_tags(&one(root), &[full], false)?;
                }
            }
            5 => {
                if let Some(tag) = &tag {
                    match library.projections().snapshot().tag_ids_by_name.get(&name).copied() {
                        Some(existing) if existing != tag.tag_id => library.merge_tags(tag.tag_id, existing)?,
                        _ => library.rename_tag(tag.tag_id, &name)?,
                    };
                }
            }
            6 => {
                if let Some(tag) = &tag {
                    library.delete_tag(tag.tag_id)?;
                }
            }
            7 => {
                let groups = library.tag_namespaces()?;
                if let Some(group) = random.pick(&groups) {
                    let target = ["g", "h", ""][random.below(3)];
                    library.rename_or_merge_tag_namespace(group.namespace_id, target)?;
                }
            }
            8 => {
                if let Some(root) = root {
                    library.set_rating(&one(root), Rating::ALL[random.below(6)])?;
                }
            }
            9 => {
                if let Some(root) = root {
                    library.set_lifecycle(&one(root), Lifecycle::ALL[random.below(3)])?;
                }
            }
            10 => {
                if let Some(root) = root {
                    match random.below(3) {
                        0 => library.rename_root(root, NAMES[random.below(3)], now)?,
                        1 => library.set_notes(&one(root), Some(NAMES[random.below(3)].into()), now)?,
                        _ => library.set_source_urls(&one(root), vec![format!("https://{}", random.below(3))], now)?,
                    };
                }
            }
            11 => {
                if let Some(root) = root {
                    library.set_lifecycle(&one(root), Lifecycle::Trash)?;
                    library.permanently_delete(&one(root), now, 100, false)?;
                }
            }
            12 => {
                library.undo()?;
            }
            13 => {
                let parent = folder.filter(|_| random.below(2) == 0);
                library.create_folder(NAMES[random.below(3)], parent)?;
            }
            14 => {
                if let Some(folder) = folder {
                    match random.below(3) {
                        0 => library.rename_folder(folder, NAMES[random.below(3)])?,
                        1 => library.set_folder_metadata(folder, Some("star"), Some("#ff0000"), Some("n"))?,
                        _ => library.set_folder_watch(folder, &format!("/watched/{}", random.below(2)), true, Lifecycle::Active)?,
                    };
                }
            }
            15 => {
                if let Some(folder) = folder {
                    library.move_folder(folder, random.pick(&folders).filter(|_| random.below(3) != 0))?;
                }
            }
            16 => {
                if let Some(folder) = folder {
                    library.delete_folders(&[folder])?;
                }
            }
            17 | 18 => {
                if let (Some(root), Some(folder)) = (root, folder) {
                    library.add_to_folder(&one(root), folder)?;
                }
            }
            19 => {
                if let (Some(root), Some(folder)) = (root, folder) {
                    library.remove_from_folder(&one(root), folder)?;
                }
            }
            20 => {
                if let Some(folder) = folder {
                    let mut order = library.projections().snapshot().folder_orders[&folder].as_ref().clone();
                    if order.len() > 1 {
                        let moved = order.remove(random.below(order.len()));
                        order.insert(random.below(order.len() + 1), moved);
                        library.reorder_folder_items(folder, &order)?;
                    }
                }
            }
            21 => {
                if let Some(folder) = folder {
                    let parent = library.projections().snapshot().folder_parents.get(&folder).copied().flatten();
                    let mut siblings = library
                        .folders()?
                        .into_iter()
                        .filter(|record| record.parent_id == parent)
                        .map(|record| record.folder_id)
                        .collect::<Vec<_>>();
                    if siblings.len() > 1 {
                        let moved = siblings.remove(random.below(siblings.len()));
                        siblings.insert(random.below(siblings.len() + 1), moved);
                        library.reorder_folder_children(parent, &siblings)?;
                    }
                }
            }
            22 => {
                if let (Some(folder), Some(tag)) = (folder, &tag) {
                    let tags = if random.below(2) == 0 { vec![tag.tag_id] } else { Vec::new() };
                    library.set_folder_auto_tags(folder, tags)?;
                }
            }
            23 => {
                let standalone = kind_roots(library, RootKind::Media);
                let collections = kind_roots(library, RootKind::Collection);
                let mut members = Vec::new();
                for _ in 0..2 {
                    members.extend(random.pick(&standalone));
                }
                members.extend(random.pick(&collections).filter(|_| random.below(3) == 0));
                members.sort();
                members.dedup();
                if members.len() > 1 {
                    library.organize_into_collection(&GroupRequest {
                        target: SelectionTarget::Explicit { root_ids: members.clone() },
                        cover_root_id: members[0],
                        winning_collection_id: None,
                        name: Some(NAMES[random.below(3)].into()),
                        notes: None,
                        modified_at_ms: now,
                    })?;
                }
            }
            24 => {
                if let Some(collection) = random.pick(&kind_roots(library, RootKind::Collection)) {
                    let members = library.projections().snapshot().collection_orders[&collection].as_ref().clone();
                    match random.below(4) {
                        0 => {
                            library.ungroup_collection(collection, now)?;
                        }
                        1 => {
                            library.detach_collection_member(collection, members[random.below(members.len())], now)?;
                        }
                        2 => {
                            let mut order = members.clone();
                            let moved = order.remove(random.below(order.len()));
                            order.insert(random.below(order.len() + 1), moved);
                            library.reorder_collection(collection, order)?;
                        }
                        _ => {
                            library.set_collection_cover(collection, members[random.below(members.len())])?;
                        }
                    }
                }
            }
            25 => {
                if let (Some(folder), Some(root)) = (folder, root) {
                    library.set_folder_cover(folder, root)?;
                }
            }
            _ => {
                let media = random.pick(&kind_roots(library, RootKind::Media));
                if let Some(media) = media {
                    let expected_hash = library.database().read(WorkPriority::VisibleRead, |connection| {
                        Ok(connection.query_row(
                            "SELECT file.content_hash FROM media_item media
                             JOIN media_file file ON file.file_id = media.file_id WHERE media.media_id = ?1",
                            [media.0],
                            |row| row.get::<_, String>(0),
                        )?)
                    })?;
                    *next_hash += 1;
                    library.repair_media_content(
                        MediaId(media.0),
                        &RepairedMediaContent {
                            expected_hash,
                            content_hash: hash(1_000 + *next_hash),
                            file_path: "/test/repaired.png".into(),
                            size_bytes: 120,
                        },
                    )?;
                }
            }
        }
        Ok(())
    })();
    let tag = tag.map(|tag| format!("{}:{}", tag.namespace, tag.subname));
    let outcome = result.map_or_else(|error| error.to_string(), |_| "done".into());
    format!("operation {choice} on root {root:?}, folder {folder:?}, tag {tag:?}, name {name}: {outcome}")
}

fn assert_recorded(library: &Library, context: &str) {
    let differences = library.sync_differences().unwrap();
    assert!(differences.is_empty(), "{context}: a full comparison records {differences:?}");
}

/// Every synced value of a library as change records.
fn synced_records(library: &Library) -> Vec<SyncRecord> {
    library.database().read(WorkPriority::VisibleRead, super::all_records).unwrap()
}

/// The library in sync keys: every entity with its state, and every order.
fn dump(library: &Library) -> BTreeMap<String, String> {
    let snapshot = library.projections().snapshot();
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            let mut local = Local::new(connection, &snapshot)?;
            let mut out = BTreeMap::new();
            for (kind, sql) in [
                (SyncKind::Item, "SELECT media_id FROM media_item"),
                (SyncKind::Collection, "SELECT local_id FROM library_item WHERE item_kind = 2"),
                (SyncKind::Tag, "SELECT tag_id FROM tag_definition"),
                (SyncKind::TagGroup, "SELECT namespace_id FROM tag_namespace"),
                (SyncKind::Folder, "SELECT folder_id FROM folder_definition"),
            ] {
                let ids = connection
                    .prepare(sql)?
                    .query_map([], |row| row.get::<_, u32>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for id in ids {
                    let key = local.key(kind, id)?.unwrap_or_default();
                    let view = local.view(kind, id)?;
                    let mut line = format!("{view:?}");
                    if kind == SyncKind::Folder {
                        let members = snapshot.folder_orders[&FolderId(id)]
                            .iter()
                            .map(|root| local.root_key(*root).map(|key| key.map(|key| key.1)))
                            .collect::<crate::Result<Vec<_>>>()?;
                        let children = connection
                            .prepare("SELECT stable_key FROM folder_definition WHERE parent_id = ?1 ORDER BY display_order, folder_id")?
                            .query_map([id], |row| row.get::<_, String>(0))?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        line.push_str(&format!(" members {members:?} children {children:?}"));
                    }
                    if kind == SyncKind::Collection {
                        let members = snapshot.collection_orders[&RootId(id)]
                            .iter()
                            .map(|media| local.key(SyncKind::Item, media.0))
                            .collect::<crate::Result<Vec<_>>>()?;
                        line.push_str(&format!(" members {members:?}"));
                    }
                    out.insert(format!("{} {key}", kind.as_str()), line);
                }
            }
            let top = connection
                .prepare("SELECT stable_key FROM folder_definition WHERE parent_id IS NULL ORDER BY display_order, folder_id")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.insert("top level folders".into(), format!("{top:?}"));
            Ok(out)
        })
        .unwrap()
}

fn assert_same(a: &Library, b: &Library, context: &str) {
    let (a, b) = (dump(a), dump(b));
    for key in a.keys().chain(b.keys()) {
        assert_eq!(a.get(key), b.get(key), "{context}: {key} differs");
    }
}

/// Projecting the synced values of `library` into an empty library gives the
/// same library.
fn assert_projects(library: &Library, directory: &std::path::Path, name: &str) {
    let copy = open(directory, name);
    copy.apply_sync_records(&synced_records(library)).unwrap();
    assert_recorded(&copy, "the projected copy");
    assert_same(library, &copy, "projecting into an empty library");
}

#[test]
fn every_operation_is_recorded_and_the_synced_values_project_to_the_same_library() {
    let directory = tempfile::tempdir().unwrap();
    for seed in 1..=20u64 {
        let library = open(directory.path(), &format!("single-{seed}"));
        let mut random = Random(seed * 7_919);
        let (mut next_hash, mut time) = (0, 0);
        let mut log = Vec::new();
        for _ in 0..250 {
            log.push(operate(&library, &mut random, &mut next_hash, &mut time));
            assert_recorded(&library, &format!("seed {seed} after {log:?}"));
        }
        assert_projects(&library, directory.path(), &format!("single-{seed}-copy"));
    }
}

/// Libraries that send each other their change records in random order and
/// at random times end with the same library. Each library's records are
/// sent as files in the order they were written, as change files are read.
fn exchange(count: usize, seed: u64) {
    let directory = tempfile::tempdir().unwrap();
    let libraries = (0..count).map(|index| open(directory.path(), &format!("library-{index}"))).collect::<Vec<_>>();
    let mut files: Vec<Vec<Vec<SyncRecord>>> = vec![Vec::new(); count];
    let mut read = vec![vec![0usize; count]; count];
    let mut random = Random(seed);
    let (mut next_hash, mut time) = (0, 0);
    let send = |library: &Library, files: &mut Vec<Vec<SyncRecord>>| {
        let outbox = library.sync_outbox().unwrap();
        if !outbox.is_empty() {
            library.remove_from_sync_outbox(&outbox).unwrap();
            files.push(outbox);
        }
    };
    let mut log = Vec::new();
    for step in 0..400 {
        let index = random.below(count);
        match random.below(10) {
            0 | 1 => send(&libraries[index], &mut files[index]),
            2 | 3 => {
                let from = random.below(count);
                if from != index && read[index][from] < files[from].len() {
                    log.push(format!("{step}: library {index} reads a file of {from}"));
                    apply(&libraries[index], &files[from][read[index][from]], &format!("seed {seed} after {log:?}"));
                    read[index][from] += 1;
                }
            }
            _ => {
                log.push(format!("{step}: library {index} {}", operate(&libraries[index], &mut random, &mut next_hash, &mut time)));
                assert_recorded(&libraries[index], &format!("seed {seed} after {log:?}"));
            }
        }
    }
    for index in 0..count {
        send(&libraries[index], &mut files[index]);
    }
    for index in 0..count {
        for from in (0..count).filter(|from| *from != index) {
            while read[index][from] < files[from].len() {
                apply(&libraries[index], &files[from][read[index][from]], &format!("seed {seed} at the end after {log:?}"));
                read[index][from] += 1;
            }
        }
    }
    for index in 1..count {
        assert_same(&libraries[0], &libraries[index], &format!("seed {seed}, library {index} after {log:?}"));
    }
    assert_projects(&libraries[0], directory.path(), "copy");
}

/// Applies records from another library. The library then matches the
/// synced values, and applying recorded nothing of its own.
fn apply(library: &Library, records: &[SyncRecord], context: &str) {
    let before = library.sync_outbox().unwrap();
    library.apply_sync_records(records).unwrap();
    let after = library.sync_outbox().unwrap();
    let recorded = after.iter().filter(|record| !before.contains(record)).collect::<Vec<_>>();
    assert!(recorded.is_empty(), "{context}: applying {records:?} recorded {recorded:?}");
    assert_recorded(library, context);
}

#[test]
fn two_and_three_libraries_that_exchange_records_end_the_same() {
    for seed in 1..=10u64 {
        exchange(2, seed * 104_729);
        exchange(3, seed * 1_299_709);
    }
}

/// Sends every record that `from` has not sent yet to `to`.
fn deliver(from: &Library, to: &Library) {
    let outbox = from.sync_outbox().unwrap();
    from.remove_from_sync_outbox(&outbox).unwrap();
    apply(to, &outbox, "deliver");
}

fn tag_names(library: &Library, root: RootId) -> Vec<String> {
    let snapshot = library.projections().snapshot();
    let mut names = snapshot
        .tag_ids_by_name
        .iter()
        .filter(|(_, tag)| snapshot.tags.get(tag).is_some_and(|members| members.contains(root.0)))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn root_of(library: &Library, content_hash: &str) -> RootId {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(RootId(connection.query_row(
                "SELECT media.media_id FROM media_file file
                 JOIN media_item media ON media.file_id = file.file_id WHERE file.content_hash = ?1",
                [content_hash],
                |row| row.get(0),
            )?))
        })
        .unwrap()
}

#[test]
fn a_tag_added_under_an_old_name_ends_under_the_new_name() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    first.ingest(&prepared(hash(1), Lifecycle::Active, vec!["a".into()], Vec::new())).unwrap();
    first.ingest(&prepared(hash(2), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    deliver(&first, &second);
    // The first library renames `a` to `d` while the second tags the other
    // item with `a`.
    first.rename_tag(first.projections().snapshot().tag_ids_by_name["a"], "d").unwrap();
    second.apply_tags(&one(root_of(&second, &hash(2))), &["a".into()], true).unwrap();
    deliver(&first, &second);
    deliver(&second, &first);
    for library in [&first, &second] {
        assert_eq!(tag_names(library, root_of(library, &hash(1))), ["d"]);
        assert_eq!(tag_names(library, root_of(library, &hash(2))), ["d"]);
        assert!(!library.projections().snapshot().tag_ids_by_name.contains_key("a"));
    }
    assert_same(&first, &second, "after the rename");
}

#[test]
fn a_tag_name_created_again_does_not_come_back_on_items_that_were_renamed() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    first.ingest(&prepared(hash(1), Lifecycle::Active, vec!["a".into()], Vec::new())).unwrap();
    first.ingest(&prepared(hash(2), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    deliver(&first, &second);
    // The first library renames `a` to `d`. Before that reaches the
    // second, the second tags the other item with `a`, which the first
    // then shows as `d`. Later the first creates `a` again; the second
    // comparison keeps the other item on `d` everywhere.
    first.rename_tag(first.projections().snapshot().tag_ids_by_name["a"], "d").unwrap();
    second.apply_tags(&one(root_of(&second, &hash(2))), &["a".into()], true).unwrap();
    deliver(&second, &first);
    assert_eq!(tag_names(&first, root_of(&first, &hash(2))), ["d"]);
    first.apply_tags(&one(root_of(&first, &hash(1))), &["a".into()], true).unwrap();
    deliver(&first, &second);
    deliver(&second, &first);
    for library in [&first, &second] {
        assert_eq!(tag_names(library, root_of(library, &hash(1))), ["a", "d"]);
        assert_eq!(tag_names(library, root_of(library, &hash(2))), ["d"]);
    }
    assert_same(&first, &second, "after the tag came back");
}

#[test]
fn two_libraries_that_add_at_the_same_place_keep_both_and_a_later_insert_goes_between() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    let (folder, _) = first.create_folder("shared", None).unwrap();
    first.ingest(&prepared(hash(1), Lifecycle::Active, Vec::new(), vec![folder])).unwrap();
    deliver(&first, &second);
    let second_folder = folders(&second)[0];
    // Both add an item at the end: equal position keys, ordered by hash.
    first.ingest(&prepared(hash(3), Lifecycle::Active, Vec::new(), vec![folder])).unwrap();
    second.ingest(&prepared(hash(2), Lifecycle::Active, Vec::new(), vec![second_folder])).unwrap();
    deliver(&first, &second);
    deliver(&second, &first);
    assert_same(&first, &second, "after both added");
    let order = |library: &Library, folder| {
        library.projections().snapshot().folder_orders[&folder]
            .iter()
            .map(|root| roots_hash(library, *root))
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&first, folder), [hash(1), hash(2), hash(3)]);
    // An item put between the two tied items gets a key between them; the
    // right one of them gets a new key too.
    first.ingest(&prepared(hash(4), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
    first.add_to_folder(&one(root_of(&first, &hash(4))), folder).unwrap();
    let mut target = first.projections().snapshot().folder_orders[&folder].as_ref().clone();
    let added = target.pop().unwrap();
    target.insert(2, added);
    first.reorder_folder_items(folder, &target).unwrap();
    deliver(&first, &second);
    assert_eq!(order(&second, second_folder), [hash(1), hash(2), hash(4), hash(3)]);
    assert_same(&first, &second, "after the insert between");
}

fn roots_hash(library: &Library, root: RootId) -> String {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(
                "SELECT file.content_hash FROM media_item media
                 JOIN media_file file ON file.file_id = media.file_id WHERE media.media_id = ?1",
                [root.0],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap()
}

#[test]
fn undoing_delete_permanently_keeps_what_another_library_changed() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    first.ingest(&prepared(hash(1), Lifecycle::Trash, vec!["a".into()], Vec::new())).unwrap();
    deliver(&first, &second);
    let root = root_of(&first, &hash(1));
    first.permanently_delete(&one(root), 10, 100, false).unwrap();
    // The second library rates and tags the item before the delete
    // reaches it; its changes arrive before the undo.
    let other = root_of(&second, &hash(1));
    second.set_rating(&one(other), Rating::Four).unwrap();
    second.apply_tags(&one(other), &["b".into()], true).unwrap();
    deliver(&second, &first);
    first.undo().unwrap().expect("the delete can be undone");
    assert_recorded(&first, "after the undo");
    deliver(&first, &second);
    for library in [&first, &second] {
        let root = root_of(library, &hash(1));
        assert_eq!(tag_names(library, root), ["a", "b"]);
        assert!(library.projections().snapshot().rating(Rating::Four).contains(root.0));
    }
    assert_same(&first, &second, "after the undo");
}
