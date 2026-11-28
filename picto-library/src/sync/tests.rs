//! Checks of recording and applying with real library operations. Once the
//! recorder has caught up after one or more operations, a full comparison of
//! the library with the synced values must find nothing to record, and
//! libraries that exchange their change records must end with the same
//! library. `cloud` checks libraries that sync through a cloud folder.

mod cloud;

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::local::Local;
use super::{SyncKind, SyncRecord};
use crate::database::WorkPriority;
use crate::model::{
    DuplicateResolutionChoice, FileId, FolderId, GroupRequest, ImmutableMediaFacts, Lifecycle,
    MediaId, PreparedImport, Rating, RepairedMediaContent, RootId, RootKind, SmartFolderId,
    SmartFolderInput, SourceIdentity,
};
use crate::predicate::{FilterClause, FilterExpr, SetMatchMode, ViewQuerySpec};
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

/// A new library with sync on, in a folder of its own, because the sync file
/// is next to the library database.
fn open(directory: &std::path::Path, name: &str) -> Library {
    std::fs::create_dir(directory.join(name)).unwrap();
    let library = Library::create(directory.join(name).join("library.sqlite")).unwrap();
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

fn smart_folders(library: &Library) -> Vec<SmartFolderId> {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(connection
                .prepare("SELECT smart_folder_id FROM smart_folder_definition ORDER BY smart_folder_id")?
                .query_map([], |row| row.get(0).map(SmartFolderId))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn smart_parent(library: &Library, smart_folder: SmartFolderId) -> Option<SmartFolderId> {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(
                "SELECT parent_id FROM smart_folder_definition WHERE smart_folder_id = ?1",
                [smart_folder.0],
                |row| row.get::<_, Option<u32>>(0),
            )?)
        })
        .unwrap()
        .map(SmartFolderId)
}

fn full_name(tag: &crate::TagRecord) -> String {
    if tag.namespace.is_empty() {
        tag.subname.clone()
    } else {
        format!("{}:{}", tag.namespace, tag.subname)
    }
}

/// Random rules for a smart folder: a tag rule, a folder rule, a rating rule
/// and the groups around them.
fn rules(random: &mut Random, tags: &[crate::TagRecord], folders: &[FolderId]) -> ViewQuerySpec {
    let mut clauses = Vec::new();
    let tag_ids = (0..random.below(3)).filter_map(|_| random.pick(tags)).map(|tag| tag.tag_id).collect::<Vec<_>>();
    if !tag_ids.is_empty() || random.below(4) == 0 {
        clauses.push(FilterExpr::Clause(FilterClause::Tags { tag_ids, mode: SetMatchMode::Any }));
    }
    let folder_ids = (0..random.below(3)).filter_map(|_| random.pick(folders)).collect::<Vec<_>>();
    if !folder_ids.is_empty() {
        clauses.push(FilterExpr::Clause(FilterClause::Folders { folder_ids, mode: SetMatchMode::All }));
    }
    if random.below(2) == 0 {
        clauses.push(FilterExpr::Clause(FilterClause::Ratings { ratings: vec![Rating::ALL[random.below(6)]] }));
    }
    let filter = match random.below(3) {
        0 => FilterExpr::All(clauses),
        1 => FilterExpr::Any(clauses),
        _ => FilterExpr::All(vec![FilterExpr::Not(Box::new(FilterExpr::Any(clauses)))]),
    };
    ViewQuerySpec { filter, ..ViewQuerySpec::default() }
}

/// One or two views by their scope: of a folder, of a smart folder, or of
/// the views without an id.
fn random_scopes(random: &mut Random, folders: &[FolderId], smart_folders: &[SmartFolderId]) -> Vec<String> {
    (0..random.below(3))
        .filter_map(|_| match random.below(3) {
            0 => random.pick(folders).map(|folder| format!("folder:{}", folder.0)),
            1 => random.pick(smart_folders).map(|smart_folder| format!("smart:{}", smart_folder.0)),
            _ => Some(["grid:defaults", "system:trash"][random.below(2)].to_owned()),
        })
        .collect()
}

/// Sets one field of a JSON object in `setting` or `view_pref`, as the
/// settings of the app are changed.
fn patch(library: &Library, table: &'static str, key: &str, field: &str, value: Value) -> crate::Result<()> {
    let field = field.to_owned();
    library.update_auxiliary_json("test.patch", "Change settings", table, key, table == "setting", move |stored| {
        let mut object = stored.and_then(|stored| serde_json::from_str::<Value>(stored).ok()).unwrap_or_else(|| json!({}));
        object[field] = value;
        Ok(Some(object.to_string()))
    })?;
    Ok(())
}

fn file_of(library: &Library, root: RootId) -> FileId {
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row("SELECT file_id FROM media_item WHERE media_id = ?1", [root.0], |row| row.get(0))?)
        })
        .map(FileId)
        .unwrap()
}

/// Runs one random operation of the library. Operations that the library
/// refuses (for example a rename to a name that exists) change nothing.
fn operate(library: &Library, random: &mut Random, next_hash: &mut u64, time: &mut i64) -> String {
    *time += 1;
    let now = *time;
    let roots = roots(library);
    let folders = folders(library);
    let tags = library.tags().unwrap();
    let smart_folders = smart_folders(library);
    let choice = random.below(37);
    let root = random.pick(&roots);
    let folder = random.pick(&folders);
    let tag = random.pick(&tags);
    let smart_folder = random.pick(&smart_folders);
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
            26 => {
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
            27 | 28 => {
                let name = NAMES[random.below(3)];
                let parent_id = random.pick(&smart_folders).filter(|_| random.below(2) == 0);
                let icon = ["star", ""][random.below(2)];
                let notes = NAMES[random.below(3)];
                let input = SmartFolderInput {
                    name: name.into(),
                    parent_id,
                    icon: Some(icon.into()),
                    color: None,
                    notes: (random.below(2) == 0).then(|| notes.into()),
                    view: rules(random, &tags, &folders),
                };
                match smart_folder.filter(|_| choice == 28) {
                    Some(smart_folder) => {
                        library.update_smart_folder(smart_folder, input)?;
                    }
                    None => {
                        library.create_smart_folder(input)?;
                    }
                }
            }
            29 => {
                if let Some(smart_folder) = smart_folder {
                    library.delete_smart_folder(smart_folder)?;
                }
            }
            30 => {
                if let Some(smart_folder) = smart_folder {
                    let parent = smart_parent(library, smart_folder);
                    let mut siblings = smart_folders
                        .iter()
                        .copied()
                        .filter(|sibling| smart_parent(library, *sibling) == parent)
                        .collect::<Vec<_>>();
                    if siblings.len() > 1 {
                        let moved = siblings.remove(random.below(siblings.len()));
                        siblings.insert(random.below(siblings.len() + 1), moved);
                        library.reorder_smart_folder_children(parent, &siblings)?;
                    }
                }
            }
            31 => {
                // The settings: a plain field, the starred tags and group
                // colours by name, the Quick Access list by scope, and two
                // fields that stay on this computer.
                let value = match random.below(6) {
                    0 => ("gridSpacing", json!(random.below(3))),
                    1 => ("starredTags", json!(tag.iter().map(full_name).collect::<Vec<_>>())),
                    2 => ("tagGroupColors", json!({ TAGS[4].split(':').next().unwrap(): NAMES[random.below(3)] })),
                    3 => ("sidebarQuickAccess", json!(random_scopes(random, &folders, &smart_folders))),
                    4 => ("zoomFactor", json!(random.below(3))),
                    _ => ("subscriptionDefaultSchedule", json!(NAMES[random.below(3)])),
                };
                patch(library, "setting", "application", value.0, value.1)?;
            }
            32 => {
                let scope = random_scopes(random, &folders, &smart_folders).pop().unwrap_or_else(|| "grid:defaults".into());
                if random.below(4) == 0 {
                    library.update_auxiliary_json("test.view", "Reset view", "view_pref", &scope, false, |_| Ok(None))?;
                } else {
                    patch(library, "view_pref", &scope, "sort_field", json!(NAMES[random.below(3)]))?;
                }
            }
            33 | 34 => {
                if choice == 34 && random.below(3) == 0 {
                    library.clear_recent_views()?;
                } else if let Some(root) = root {
                    library.record_recent_view(root, now)?;
                }
            }
            35 => {
                let standalone = kind_roots(library, RootKind::Media);
                if let (Some(a), Some(b)) = (random.pick(&standalone), random.pick(&standalone)) {
                    let (a, b) = (file_of(library, a), file_of(library, b));
                    if a != b {
                        library.replace_detected_duplicate_pairs(&[(a, b, 3)], now)?;
                        if random.below(3) != 0 {
                            library.resolve_duplicate(a, b, DuplicateResolutionChoice::KeepBoth, now)?;
                        }
                    }
                }
            }
            _ => {
                // An import of a file that the library may have already,
                // from a source, which adds the source to its item.
                *next_hash += 1;
                let mut input = prepared(hash(*next_hash % 24), Lifecycle::Active, Vec::new(), Vec::new());
                input.source_identity = Some(SourceIdentity {
                    source_key: "site".into(),
                    source_item_key: format!("post-{}", random.below(4)),
                    source_text: Some(NAMES[random.below(3)].into()),
                    source_attempt_id: None,
                });
                library.ingest(&input)?;
            }
        }
        Ok(())
    })();
    let tag = tag.map(|tag| format!("{}:{}", tag.namespace, tag.subname));
    let outcome = result.map_or_else(|error| error.to_string(), |_| "done".into());
    format!("operation {choice} on root {root:?}, folder {folder:?}, tag {tag:?}, name {name}: {outcome}")
}

fn assert_recorded(library: &Library, context: &str) {
    library.record_sync_changes().unwrap();
    let differences = library.sync_differences().unwrap();
    assert!(differences.is_empty(), "{context}: a full comparison records {differences:?}");
}

/// Every synced value of a library as change records.
fn synced_records(library: &Library) -> Vec<SyncRecord> {
    library.synced_records().unwrap()
}

/// The library in sync keys: every entity with its state, and every order.
fn dump(library: &Library) -> BTreeMap<String, String> {
    let snapshot = library.projections().snapshot();
    library
        .database()
        .read(WorkPriority::VisibleRead, |connection| {
            let mut local = Local::new(connection, &snapshot);
            let mut out = BTreeMap::new();
            for (kind, sql) in [
                (SyncKind::Item, "SELECT media_id FROM media_item"),
                (SyncKind::Collection, "SELECT local_id FROM library_item WHERE item_kind = 2"),
                (SyncKind::Tag, "SELECT tag_id FROM tag_definition"),
                (SyncKind::TagGroup, "SELECT namespace_id FROM tag_namespace"),
                (SyncKind::Folder, "SELECT folder_id FROM folder_definition"),
                (SyncKind::SmartFolder, "SELECT smart_folder_id FROM smart_folder_definition"),
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
                    if kind == SyncKind::SmartFolder {
                        let children = connection
                            .prepare(
                                "SELECT stable_key FROM smart_folder_definition WHERE parent_id = ?1
                                 ORDER BY display_order, smart_folder_id",
                            )?
                            .query_map([id], |row| row.get::<_, String>(0))?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        line.push_str(&format!(" children {children:?}"));
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
            let top = connection
                .prepare(
                    "SELECT stable_key FROM smart_folder_definition WHERE parent_id IS NULL
                     ORDER BY display_order, smart_folder_id",
                )?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.insert("top level smart folders".into(), format!("{top:?}"));
            for (kind, key) in local.all_rows()? {
                if let Some(view) = local.row_view(kind, &key)? {
                    out.insert(format!("{} {key}", kind.as_str()), format!("{view:?}"));
                }
            }
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
            // The recorder runs after one operation or after several.
            if random.below(2) == 0 {
                assert_recorded(&library, &format!("seed {seed} after {log:?}"));
            }
        }
        assert_recorded(&library, &format!("seed {seed} after {log:?}"));
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
        library.record_sync_changes().unwrap();
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
                if random.below(2) == 0 {
                    assert_recorded(&libraries[index], &format!("seed {seed} after {log:?}"));
                }
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
    library.record_sync_changes().unwrap();
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
    from.record_sync_changes().unwrap();
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

#[test]
fn a_crash_after_a_write_of_synced_values_records_nothing_that_write_left_unfinished() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    let (old, _) = first.create_folder("old", None).unwrap();
    let (new, _) = first.create_folder("new", None).unwrap();
    for n in 1..=12 {
        first.ingest(&prepared(hash(n), Lifecycle::Active, vec!["a".into()], vec![old])).unwrap();
    }
    deliver(&first, &second);
    // Records that take several library transactions to write: the tag is
    // deleted, which a transaction writes for every item at once, and the
    // items move to the other folder, which each item's own transaction
    // writes; the new folder's cover is the last of them.
    first.delete_tag(first.projections().snapshot().tag_ids_by_name["a"]).unwrap();
    let items = (1..=12).map(|n| root_of(&first, &hash(n))).collect::<Vec<_>>();
    let moved = SelectionTarget::Explicit { root_ids: items.clone() };
    first.add_to_folder(&moved, new).unwrap();
    first.remove_from_folder(&moved, old).unwrap();
    first.set_folder_cover(new, items[11]).unwrap();
    first.record_sync_changes().unwrap();
    let outbox = first.sync_outbox().unwrap();
    first.remove_from_sync_outbox(&outbox).unwrap();
    second.apply_sync_records_partly(&outbox, 1, true).unwrap();
    drop(second);
    let second = Library::open(directory.path().join("second").join("library.sqlite")).unwrap();
    second.record_sync_changes().unwrap();
    assert_eq!(second.sync_outbox().unwrap(), Vec::new(), "the second library recorded values of its own");
    assert_recorded(&second, "after the crash");
    assert_same(&first, &second, "after the crash");
}

#[test]
fn a_change_made_while_records_from_another_computer_are_written_is_kept() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    let (folder, _) = first.create_folder("folder", None).unwrap();
    // The tag is known before the items, so that its records are written
    // first.
    first.ingest(&prepared(hash(0), Lifecycle::Active, vec!["a".into()], Vec::new())).unwrap();
    deliver(&first, &second);
    first.apply_tags(&one(root_of(&first, &hash(0))), &["a".into()], false).unwrap();
    for n in 1..=3 {
        first.ingest(&prepared(hash(n), Lifecycle::Active, vec!["a".into()], Vec::new())).unwrap();
    }
    deliver(&first, &second);
    // Records that take several library transactions to write: the tag is
    // renamed, which changes every item, and the items are added to the
    // folder, which is written after the rename.
    first.rename_tag(first.projections().snapshot().tag_ids_by_name["a"], "d").unwrap();
    let items = (1..=3).map(|n| root_of(&first, &hash(n))).collect::<Vec<_>>();
    first.add_to_folder(&SelectionTarget::Explicit { root_ids: items }, folder).unwrap();
    first.record_sync_changes().unwrap();
    let outbox = first.sync_outbox().unwrap();
    first.remove_from_sync_outbox(&outbox).unwrap();
    second.apply_sync_records_partly(&outbox, 2, false).unwrap();
    // The second library rates the items after the rename was written,
    // while the items still wait to be added to the folder.
    let items = (1..=3).map(|n| root_of(&second, &hash(n))).collect::<Vec<_>>();
    second.set_rating(&SelectionTarget::Explicit { root_ids: items }, Rating::Four).unwrap();
    second.record_sync_changes().unwrap();
    deliver(&second, &first);
    for library in [&first, &second] {
        for n in 1..=3 {
            let root = root_of(library, &hash(n));
            assert!(library.projections().snapshot().rating(Rating::Four).contains(root.0), "item {n} lost its rating");
            assert_eq!(tag_names(library, root), ["d"]);
        }
    }
    assert_same(&first, &second, "after the rating");
}

/// A folder that never had members and one whose members were all removed
/// are in the folder maps of a library that was opened again, as they are
/// in the library before it is closed, and a full comparison after the
/// library is opened again finds nothing to record.
#[test]
fn a_library_with_empty_folders_records_nothing_after_it_is_opened_again() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("library").join("library.sqlite");
    let library = open(directory.path(), "library");
    let (empty, _) = library.create_folder("empty", None).unwrap();
    let (emptied, _) = library.create_folder("emptied", Some(empty)).unwrap();
    library.ingest(&prepared(hash(1), Lifecycle::Active, Vec::new(), vec![emptied])).unwrap();
    library.remove_from_folder(&one(root_of(&library, &hash(1))), emptied).unwrap();
    assert_recorded(&library, "before the library is opened again");
    let folder_maps = |library: &Library| {
        let snapshot = library.projections().snapshot();
        let members = snapshot.folders.iter().map(|(folder, members)| (*folder, members.len())).collect::<BTreeMap<_, _>>();
        (snapshot.folder_orders.as_ref().clone(), members)
    };
    let before = folder_maps(&library);
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(folder_maps(&library), before);
    assert_recorded(&library, "after the library is opened again");
    let root = one(root_of(&library, &hash(1)));
    library.add_to_folder(&root, empty).unwrap();
    library.remove_from_folder(&root, empty).unwrap();
    library.move_folder(emptied, None).unwrap();
    assert_recorded(&library, "after the folders are changed");
}

/// The times in Recently viewed, by content hash, most recent first.
fn recently_viewed(library: &Library) -> Vec<(String, i64)> {
    let snapshot = library.projections().snapshot();
    snapshot.recent_views.iter().map(|(root, viewed_at)| (roots_hash(library, *root), *viewed_at)).collect()
}

#[test]
fn undoing_clear_recently_viewed_brings_back_the_earlier_view_times_everywhere() {
    let directory = tempfile::tempdir().unwrap();
    let (first, second) = (open(directory.path(), "first"), open(directory.path(), "second"));
    for n in 1..=2 {
        first.ingest(&prepared(hash(n), Lifecycle::Active, Vec::new(), Vec::new())).unwrap();
        first.record_recent_view(root_of(&first, &hash(n)), 10 * n as i64).unwrap();
    }
    deliver(&first, &second);
    // The first item is viewed again after Recently viewed was cleared.
    // Undoing the clear puts back the list from before, so the first item
    // has its earlier time again, which is not later than the others.
    first.clear_recent_views().unwrap();
    first.record_sync_changes().unwrap();
    first.record_recent_view(root_of(&first, &hash(1)), 100).unwrap();
    first.record_sync_changes().unwrap();
    first.undo().unwrap().expect("the clear can be undone");
    assert_recorded(&first, "after the undo");
    deliver(&first, &second);
    let expected = vec![(hash(2), 20), (hash(1), 10)];
    assert_eq!(recently_viewed(&first), expected);
    assert_eq!(recently_viewed(&second), expected);
}
