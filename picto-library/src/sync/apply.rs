//! Writing what the synced values show into the library. A batch of change
//! records from another computer is merged into the synced values, and every
//! entity whose visible state the batch can have changed is written into the
//! library tables and the projection, in the transaction of the write. The
//! library then matches the synced values, so the recorder that runs at the
//! end of the write finds nothing to record.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{OptionalExtension, Transaction};
use serde_json::Value;

use super::local::{lifecycle_from_name, rating_from_number, Local};
use super::values::{Synced, View};
use super::{merge, Clock, SyncKind, SyncRecord};
use crate::bitmap::{BitmapDomain, BitmapKey};
use crate::history::{
    FolderDefinitionState, SemanticChange, StructuralFolderPosition, StructuralMediaNoteState,
    StructuralRootState, StructuralState,
};
use crate::model::{FileId, FolderId, MediaId, RootId, RootKind};
use crate::ordering::OrderOwnerKind;
use crate::projection::ProjectionSnapshot;
use crate::{LibraryDatabase, LibraryError, Result};

/// What a projection changed: the roots, for the receipt, and the renderer
/// resources.
#[derive(Default)]
pub(crate) struct Projected {
    pub roots: RoaringBitmap,
    pub resources: BTreeSet<String>,
}

type Key = (SyncKind, String);

/// Merges `records` from another computer into the synced values and writes
/// what changed into the library and `snapshot`.
pub(crate) fn apply(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    records: &[SyncRecord],
    protected: &BTreeSet<FileId>,
) -> Result<Projected> {
    let mut clock = Clock::load(transaction)?;
    let mut synced = Synced::new(transaction);
    // The entities whose visibility a record can change, and the
    // collections an item leaves or joins.
    let mut watched = BTreeSet::new();
    for record in records {
        let key = (record.kind, record.key.clone());
        match record.field.as_str() {
            "deleted" | "file" | "renamed_to" => {
                watched.insert(key);
            }
            "collection" => {
                let raw = synced.raw(record.kind, &record.key)?;
                for value in [raw.get("collection").map(|stored| &stored.value), Some(&record.value)]
                    .into_iter()
                    .flatten()
                {
                    if let Some(collection) = value.get(0).and_then(Value::as_str) {
                        watched.insert((SyncKind::Collection, collection.to_owned()));
                    }
                }
            }
            _ => {}
        }
    }
    let mut before = Vec::new();
    for (kind, key) in &watched {
        before.push(synced.state(*kind, key)?);
    }
    let merged = merge(transaction, &mut clock, records)?;
    clock.save(transaction)?;
    synced.forget();
    let mut keys = merged
        .iter()
        .map(|record| (record.kind, record.key.clone()))
        .collect::<BTreeSet<_>>();
    for ((kind, key), before) in watched.into_iter().zip(before) {
        if synced.state(kind, &key)? == before {
            continue;
        }
        keys.insert((kind, key.clone()));
        let mut forwarding = vec![key];
        while let Some(key) = forwarding.pop() {
            for (referrer_kind, referrer, field) in synced.referrers(kind, &key)? {
                if kind == SyncKind::Tag && field == "renamed_to" {
                    if keys.insert((referrer_kind, referrer.clone())) {
                        forwarding.push(referrer);
                    }
                    continue;
                }
                // Folders are written as a whole tree, and a folder that
                // appears or disappears takes its member list with it.
                if kind != SyncKind::Folder && referrer_kind != SyncKind::Folder {
                    keys.insert((referrer_kind, referrer));
                }
            }
        }
    }
    project(transaction, revision, snapshot, keys, HashSet::new(), protected)
}

/// Writes what the synced values show for `keys` into the library and
/// `snapshot`. The entities in `revived` count as visible even when the
/// synced values delete them; an undo of Delete Permanently uses that to
/// write the latest synced values over the rows it brings back. The folder
/// tree is always written as a whole.
pub(crate) fn project(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    keys: BTreeSet<Key>,
    revived: HashSet<Key>,
    protected: &BTreeSet<FileId>,
) -> Result<Projected> {
    let mut synced = Synced::new(transaction);
    synced.revived = revived;
    let mut projected = Projected::default();
    projected.resources.extend(
        ["roots", "media", "sidebar", "tags", "folders", "collections", "search", "navigation", "smart-folders"]
            .map(str::to_owned),
    );

    // Tags and tag groups that appear are created first, so that items and
    // folders can name them.
    let mut gone_tags = Vec::new();
    for (kind, key) in &keys {
        match kind {
            SyncKind::Tag => {
                let exists = snapshot.tag_ids_by_name.get(key).copied();
                match (synced.visible(*kind, key)?, exists) {
                    (true, None) => {
                        ensure_tag(transaction, snapshot, key)?;
                    }
                    (false, Some(tag_id)) => gone_tags.push(tag_id),
                    _ => {}
                }
            }
            SyncKind::TagGroup if synced.visible(*kind, key)? => {
                crate::ingest::ensure_namespace(transaction, key)?;
            }
            _ => {}
        }
    }

    let new_folders = folders(transaction, revision, snapshot, &mut synced)?;
    roots(transaction, revision, snapshot, &mut synced, &keys, protected, &mut projected)?;
    folder_covers_and_lists(transaction, revision, snapshot, &mut synced, &new_folders, &mut projected)?;
    remove_folders(transaction, revision, snapshot, &mut synced)?;

    for tag_id in gone_tags {
        if snapshot.tags.contains_key(&tag_id) || snapshot.tag_ids_by_name.values().any(|id| *id == tag_id) {
            let (members, _) =
                crate::mutation::remove_tag_in_transaction(transaction, snapshot, tag_id, None, revision)?;
            projected.roots |= members;
        }
    }
    // A tag group without tags that the synced values do not show is
    // removed. Creating a tag creates its group, so a group can be left
    // empty when its tags go.
    let empty_groups = transaction
        .prepare(
            "SELECT namespace_id, display_name FROM tag_namespace namespace
             WHERE NOT EXISTS (SELECT 1 FROM tag_definition tag WHERE tag.namespace_id = namespace.namespace_id)",
        )?
        .query_map([], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (namespace_id, name) in empty_groups {
        if !synced.visible(SyncKind::TagGroup, &name)? {
            transaction.execute("DELETE FROM tag_namespace WHERE namespace_id = ?1", [namespace_id])?;
        }
    }
    crate::smart::settle_affected_for(
        transaction,
        snapshot,
        &projected.roots,
        &[crate::predicate::DependencyChange::All],
    )?;
    snapshot.revision = revision;
    Ok(projected)
}

fn ensure_tag(transaction: &Transaction<'_>, snapshot: &mut ProjectionSnapshot, name: &str) -> Result<crate::TagId> {
    if let Some(tag_id) = snapshot.tag_ids_by_name.get(name) {
        return Ok(*tag_id);
    }
    let tag_id = crate::ingest::ensure_tag(transaction, name)?;
    Arc::make_mut(&mut snapshot.tag_ids_by_name).insert(name.to_owned(), tag_id);
    Arc::make_mut(&mut snapshot.tags).entry(tag_id).or_default();
    Ok(tag_id)
}

/// Creates the folders the synced values show that the library lacks, and
/// writes the name, notes, auto-tags, watch, parent and position of every
/// shown folder. Covers wait until the roots are written. Returns the
/// folders that were created.
fn folders(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    synced: &mut Synced<'_>,
) -> Result<Vec<FolderId>> {
    let tree = synced.folder_tree()?;
    let mut created = Vec::new();
    let mut ids = HashMap::new();
    {
        let local = Local::new(transaction, snapshot)?;
        for key in tree.place.keys() {
            match local.find(SyncKind::Folder, key)? {
                Some(id) => {
                    ids.insert(key.clone(), FolderId(id));
                }
                None => {
                    let folder_id = FolderId(LibraryDatabase::allocate_id(transaction)?);
                    ids.insert(key.clone(), folder_id);
                    created.push((folder_id, key.clone()));
                }
            }
        }
    }
    for (folder_id, key) in &created {
        let state = FolderDefinitionState {
            folder_id: *folder_id,
            stable_key: key.clone(),
            parent_id: None,
            name: String::new(),
            icon: None,
            color: None,
            notes: None,
            auto_tag_ids: Vec::new(),
            cover_root_id: None,
            watch_path: None,
            watch_enabled: false,
            watch_subfolders: false,
            watch_lifecycle: crate::Lifecycle::Inbox,
            display_order: 0,
        };
        change(transaction, revision, snapshot, SemanticChange::FolderDefinition {
            folder_id: *folder_id,
            before: None,
            after: Some(Box::new(state)),
        })?;
    }
    for (parent, children) in &tree.children {
        for (index, key) in children.iter().enumerate() {
            let folder_id = ids[key];
            let Some(view) = synced.view(SyncKind::Folder, key)? else { continue };
            let current = crate::mutation::load_folder_definition(transaction, folder_id)?
                .ok_or_else(|| LibraryError::InvalidState(format!("folder {key} disappeared")))?;
            let mut target = current.clone();
            target.parent_id = parent.as_ref().map(|parent| ids[parent]);
            target.display_order = index as i64;
            target.name = view.fields["name"].as_str().unwrap_or_default().to_owned();
            target.icon = view.fields["icon"].as_str().map(str::to_owned);
            target.color = view.fields["color"].as_str().map(str::to_owned);
            target.notes = view.fields["notes"].as_str().map(str::to_owned);
            let mut auto_tags = RoaringBitmap::new();
            for tag in view.fields["auto_tags"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                auto_tags.insert(ensure_tag(transaction, snapshot, tag)?.0);
            }
            target.auto_tag_ids = if auto_tags.is_empty() {
                Vec::new()
            } else {
                crate::ingest::encode_folder_auto_tags(&auto_tags)?
            };
            let watch = &view.fields["watch"];
            target.watch_path = watch.get("path").and_then(Value::as_str).map(str::to_owned);
            target.watch_enabled = watch.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            target.watch_subfolders = watch.get("subfolders").and_then(Value::as_bool).unwrap_or(false);
            target.watch_lifecycle = watch
                .get("lifecycle")
                .and_then(Value::as_str)
                .map_or(crate::Lifecycle::Inbox, lifecycle_from_name);
            if target != current {
                change(transaction, revision, snapshot, SemanticChange::FolderDefinition {
                    folder_id,
                    before: Some(Box::new(current)),
                    after: Some(Box::new(target)),
                })?;
            }
        }
    }
    Ok(created.into_iter().map(|(folder_id, _)| folder_id).collect())
}

/// Sets the cover of every shown folder, and the member list of each folder
/// that was created: every shown root that the synced values put in it, in
/// the order of their position keys.
fn folder_covers_and_lists(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    synced: &mut Synced<'_>,
    created: &[FolderId],
    projected: &mut Projected,
) -> Result<()> {
    let tree = synced.folder_tree()?;
    for key in tree.place.keys() {
        let (folder_id, cover) = {
            let local = Local::new(transaction, snapshot)?;
            let Some(folder_id) = local.find(SyncKind::Folder, key)? else { continue };
            let view = synced.view(SyncKind::Folder, key)?.unwrap_or_default();
            let cover = match (view.fields["cover"].get(0).and_then(Value::as_str), view.fields["cover"].get(1).and_then(Value::as_str)) {
                (Some(kind), Some(key)) => SyncKind::parse(kind)
                    .map(|kind| local.find(kind, key))
                    .transpose()?
                    .flatten(),
                _ => None,
            };
            (FolderId(folder_id), cover)
        };
        let current = crate::mutation::load_folder_definition(transaction, folder_id)?
            .ok_or_else(|| LibraryError::InvalidState(format!("folder {key} disappeared")))?;
        if current.cover_root_id.map(|root| root.0) != cover {
            let mut target = current.clone();
            target.cover_root_id = cover.map(RootId);
            change(transaction, revision, snapshot, SemanticChange::FolderDefinition {
                folder_id,
                before: Some(Box::new(current)),
                after: Some(Box::new(target)),
            })?;
        }
        if !created.contains(&folder_id) {
            continue;
        }
        let field = format!("folder:{key}");
        let mut members = Vec::new();
        for (kind, root, referrer_field) in synced.referrers(SyncKind::Folder, key)? {
            if referrer_field != field {
                continue;
            }
            let Some(view) = synced.view(kind, &root)? else { continue };
            if !view.folders.contains(key) {
                continue;
            }
            let position = synced.raw(kind, &root)?[&field].value.as_str().unwrap_or_default().to_owned();
            if let Some(root_id) = Local::new(transaction, snapshot)?.find(kind, &root)? {
                members.push((position, root, root_id));
            }
        }
        members.sort();
        let after = members.iter().map(|(_, _, root_id)| *root_id).collect::<Vec<_>>();
        projected.roots.extend(after.iter().copied());
        // The roots written above may already be in the new folder.
        let before = snapshot
            .folder_orders
            .get(&folder_id)
            .map(|order| order.iter().map(|root| root.0).collect())
            .unwrap_or_default();
        change(transaction, revision, snapshot, SemanticChange::Order {
            owner_kind: OrderOwnerKind::Folder,
            owner_id: folder_id.0,
            before: Arc::new(before),
            after: Arc::new(after),
        })?;
    }
    Ok(())
}

/// Removes the folders that the synced values no longer show, the deepest
/// first, with their member lists.
fn remove_folders(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    synced: &mut Synced<'_>,
) -> Result<()> {
    let tree = synced.folder_tree()?;
    let rows = transaction
        .prepare("SELECT folder_id, stable_key, parent_id FROM folder_definition")?
        .query_map([], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<u32>>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let parents = rows.iter().map(|(id, _, parent)| (*id, *parent)).collect::<HashMap<_, _>>();
    let depth = |mut id: u32| {
        let mut depth = 0;
        while let Some(Some(parent)) = parents.get(&id) {
            depth += 1;
            id = *parent;
            if depth > parents.len() {
                break;
            }
        }
        depth
    };
    let mut gone = rows
        .iter()
        .filter(|(_, key, _)| !tree.place.contains_key(key))
        .map(|(id, _, _)| (depth(*id), *id))
        .collect::<Vec<_>>();
    gone.sort_by(|a, b| b.cmp(a));
    for (_, id) in gone {
        let folder_id = FolderId(id);
        if let Some(order) = snapshot.folder_orders.get(&folder_id).cloned() {
            if !order.is_empty() {
                change(transaction, revision, snapshot, SemanticChange::Order {
                    owner_kind: OrderOwnerKind::Folder,
                    owner_id: id,
                    before: Arc::new(order.iter().map(|root| root.0).collect()),
                    after: Arc::new(Vec::new()),
                })?;
            }
        }
        let current = crate::mutation::load_folder_definition(transaction, folder_id)?;
        change(transaction, revision, snapshot, SemanticChange::FolderDefinition {
            folder_id,
            before: current.map(Box::new),
            after: None,
        })?;
    }
    Ok(())
}

/// Applies one change to the library and `snapshot` with the code that
/// undo and redo use.
fn change(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    change: SemanticChange,
) -> Result<()> {
    let mut affected = RoaringBitmap::new();
    let mut resources = BTreeSet::new();
    crate::mutation::apply_semantic_change(
        transaction,
        revision,
        snapshot,
        &change,
        true,
        &mut affected,
        &mut resources,
    )
}

/// What the synced values show for one item or collection.
struct Target {
    view: View,
    /// For a collection: its members as (item key, position key), in order.
    members: Vec<(String, String)>,
}

/// Writes items and collections. Every collection that holds or receives
/// one of the items is written with all its members. The new structure is
/// written with `restore_structure`, which undo uses for collections; items
/// that the synced values no longer show are then deleted as Delete
/// Permanently deletes them, which queues their files for removal.
#[allow(clippy::too_many_arguments)]
fn roots(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    synced: &mut Synced<'_>,
    keys: &BTreeSet<Key>,
    protected: &BTreeSet<FileId>,
    projected: &mut Projected,
) -> Result<()> {
    let mut items = keys
        .iter()
        .filter(|(kind, _)| *kind == SyncKind::Item)
        .map(|(_, key)| key.clone())
        .collect::<BTreeSet<_>>();
    let mut collections = keys
        .iter()
        .filter(|(kind, _)| *kind == SyncKind::Collection)
        .map(|(_, key)| key.clone())
        .collect::<BTreeSet<_>>();
    if items.is_empty() && collections.is_empty() {
        return Ok(());
    }
    let mut item_targets: BTreeMap<String, Option<Target>> = BTreeMap::new();
    let mut collection_targets: BTreeMap<String, Option<Target>> = BTreeMap::new();
    loop {
        let local = Local::new(transaction, snapshot)?;
        let mut grew = false;
        for key in items.clone() {
            if item_targets.contains_key(&key) {
                continue;
            }
            let view = synced.view(SyncKind::Item, &key)?;
            let mut owners = view.iter().filter_map(|view| view.owner.clone()).collect::<Vec<_>>();
            if let Some(media) = local.find(SyncKind::Item, &key)? {
                if let Some(owner) = snapshot.media_owner.get(media).filter(|owner| owner.0 != media) {
                    owners.extend(local.key(SyncKind::Collection, owner.0)?);
                }
            }
            for owner in owners {
                grew |= collections.insert(owner);
            }
            item_targets.insert(key, view.map(|view| Target { view, members: Vec::new() }));
        }
        for key in collections.clone() {
            if collection_targets.contains_key(&key) {
                continue;
            }
            let view = synced.view(SyncKind::Collection, &key)?;
            let members = if view.is_some() { synced.members(&key, None)? } else { Vec::new() };
            for (member, _) in &members {
                grew |= items.insert(member.clone());
            }
            if let Some(root) = local.find(SyncKind::Collection, &key)? {
                for media in snapshot.collection_orders.get(&RootId(root)).into_iter().flat_map(|order| order.iter()) {
                    if let Some(member) = local.key(SyncKind::Item, media.0)? {
                        grew |= items.insert(member);
                    }
                }
            }
            collection_targets.insert(key, view.map(|view| Target { view, members }));
        }
        if !grew && items.iter().all(|key| item_targets.contains_key(key)) {
            break;
        }
    }

    // Files and media rows for items the library lacks, and file facts.
    let mut media_ids = HashMap::new();
    let mut sizes = HashMap::new();
    for (key, target) in &item_targets {
        let existing = Local::new(transaction, snapshot)?.find(SyncKind::Item, key)?;
        let Some(target) = target else {
            if let Some(media) = existing {
                media_ids.insert(key.clone(), media);
            }
            continue;
        };
        let file = &target.view.fields["file"];
        let mime = file.get("mime").and_then(Value::as_str).unwrap_or("application/octet-stream");
        let size = file.get("size").and_then(Value::as_i64).unwrap_or(0).max(0);
        sizes.insert(key.clone(), size as u64);
        let facts = rusqlite::params![
            key,
            mime,
            size,
            file.get("width").and_then(Value::as_i64),
            file.get("height").and_then(Value::as_i64),
            file.get("duration_ms").and_then(Value::as_i64),
            file.get("frames").and_then(Value::as_i64),
        ];
        let media = match existing {
            Some(media) => {
                transaction.execute(
                    "UPDATE media_file SET mime = ?2, size_bytes = ?3, width = ?4, height = ?5,
                         duration_ms = ?6, frame_count = ?7
                     WHERE content_hash = ?1 AND (mime, size_bytes, width, height, duration_ms, frame_count)
                         IS NOT (?2, ?3, ?4, ?5, ?6, ?7)",
                    facts,
                )?;
                media
            }
            None => {
                let file_id = transaction
                    .query_row("SELECT file_id FROM media_file WHERE content_hash = ?1", [key], |row| {
                        row.get::<_, u32>(0)
                    })
                    .optional()?;
                let file_id = match file_id {
                    Some(file_id) => {
                        // A file that waits for removal is used again.
                        transaction.execute("DELETE FROM blob_cleanup_queue WHERE file_id = ?1", [file_id])?;
                        file_id
                    }
                    None => {
                        // The original is not on this computer yet, so the
                        // file has no path.
                        let file_id = LibraryDatabase::allocate_id(transaction)?;
                        transaction.execute(
                            "INSERT INTO media_file (content_hash, mime, size_bytes, width, height,
                                 duration_ms, frame_count, file_id, file_path)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '')",
                            rusqlite::params![
                                key,
                                mime,
                                size,
                                file.get("width").and_then(Value::as_i64),
                                file.get("height").and_then(Value::as_i64),
                                file.get("duration_ms").and_then(Value::as_i64),
                                file.get("frames").and_then(Value::as_i64),
                                file_id
                            ],
                        )?;
                        file_id
                    }
                };
                let media = LibraryDatabase::allocate_id(transaction)?;
                let stable_key = format!("sync:{key}");
                let taken = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM library_item WHERE stable_key = ?1)",
                    [&stable_key],
                    |row| row.get::<_, bool>(0),
                )?;
                let stable_key = if taken { format!("{stable_key}:{media}") } else { stable_key };
                transaction.execute(
                    "INSERT INTO library_item(local_id, stable_key, item_kind) VALUES (?1, ?2, 1)",
                    rusqlite::params![media, stable_key],
                )?;
                transaction.execute(
                    "INSERT INTO media_item(media_id, media_name, media_notes, file_id) VALUES (?1, '', NULL, ?2)",
                    rusqlite::params![media, file_id],
                )?;
                media
            }
        };
        if mime.starts_with("image/") {
            Arc::make_mut(&mut snapshot.image_media).insert(media);
        } else {
            Arc::make_mut(&mut snapshot.image_media).remove(media);
        }
        media_ids.insert(key.clone(), media);
    }
    let mut collection_ids = HashMap::new();
    {
        let local = Local::new(transaction, snapshot)?;
        for (key, target) in &collection_targets {
            match local.find(SyncKind::Collection, key)? {
                Some(root) => {
                    collection_ids.insert(key.clone(), root);
                }
                None if target.is_some() => {
                    collection_ids.insert(key.clone(), LibraryDatabase::allocate_id(transaction)?);
                }
                None => {}
            }
        }
    }
    let affected = media_ids
        .values()
        .chain(collection_ids.values())
        .copied()
        .collect::<RoaringBitmap>();
    let expected = crate::mutation::capture_structure(transaction, snapshot, &affected, None)?;

    // The structure the synced values show.
    let mut replacement = StructuralState {
        roots: Arc::new(Vec::new()),
        media_notes: Arc::new(Vec::new()),
        memberships: HashMap::new(),
        folders: BTreeMap::new(),
        collections: BTreeMap::new(),
    };
    let mut roots = Vec::new();
    let mut media_notes = Vec::new();
    let mut folder_targets: BTreeMap<String, Vec<(String, String, u32)>> = BTreeMap::new();
    let mut shown_roots = Vec::new();
    for (key, target) in &item_targets {
        let Some(target) = target else { continue };
        if target.view.owner.is_some() {
            continue;
        }
        let media = media_ids[key];
        let stable_key = transaction.query_row(
            "SELECT stable_key FROM library_item WHERE local_id = ?1",
            [media],
            |row| row.get::<_, String>(0),
        )?;
        roots.push(root_state(RootId(media), stable_key, RootKind::Media, &target.view, MediaId(media), 1, sizes[key])?);
        shown_roots.push((SyncKind::Item, key.clone(), media, &target.view));
    }
    for (key, target) in &collection_targets {
        let Some(target) = target else { continue };
        let root = collection_ids[key];
        let members = target.members.iter().map(|(member, _)| MediaId(media_ids[member])).collect::<Vec<_>>();
        for (member, _) in &target.members {
            let notes = item_targets[member]
                .as_ref()
                .and_then(|target| target.view.fields["notes"].as_str().map(str::to_owned));
            media_notes.push(StructuralMediaNoteState { media_id: MediaId(media_ids[member]), notes });
        }
        let cover = target.view.fields["cover"].as_str().unwrap_or_default();
        let total = target.members.iter().map(|(member, _)| sizes.get(member).copied().unwrap_or(0)).sum();
        roots.push(root_state(
            RootId(root),
            key.clone(),
            RootKind::Collection,
            &target.view,
            MediaId(media_ids.get(cover).copied().unwrap_or(members[0].0)),
            members.len() as u32,
            total,
        )?);
        replacement.collections.insert(RootId(root), members);
        shown_roots.push((SyncKind::Collection, key.clone(), root, &target.view));
    }
    for (kind, key, root, view) in &shown_roots {
        let lifecycle = lifecycle_from_name(view.fields["lifecycle"].as_str().unwrap_or_default());
        let rating = rating_from_number(&view.fields["rating"]);
        let mut keys = vec![
            BitmapKey { domain: BitmapDomain::Lifecycle, key_id: lifecycle.bitmap_key() },
            BitmapKey { domain: BitmapDomain::Rating, key_id: rating.bitmap_key() },
        ];
        for tag in &view.tags {
            keys.push(BitmapKey { domain: BitmapDomain::Tag, key_id: ensure_tag(transaction, snapshot, tag)?.0 });
        }
        for bitmap_key in keys {
            replacement.memberships.entry(bitmap_key).or_default().insert(*root);
        }
        for folder in &view.folders {
            let field = format!("folder:{folder}");
            let position = synced.raw(*kind, key)?[&field].value.as_str().unwrap_or_default().to_owned();
            folder_targets.entry(folder.clone()).or_default().push((position, key.clone(), *root));
        }
    }
    replacement.roots = Arc::new(roots);
    replacement.media_notes = Arc::new(media_notes);
    replacement.folders = folder_positions(transaction, snapshot, synced, &affected, folder_targets)?;
    crate::mutation::restore_structure(transaction, revision, snapshot, &affected, &expected, &replacement)?;

    // Names of items, which the structure does not hold for members.
    for (key, target) in &item_targets {
        let Some(target) = target else { continue };
        transaction.execute(
            "UPDATE media_item SET media_name = ?2 WHERE media_id = ?1 AND media_name IS NOT ?2",
            rusqlite::params![media_ids[key], target.view.fields["name"].as_str().unwrap_or_default()],
        )?;
    }
    projected.roots |= &affected;

    let gone = item_targets
        .iter()
        .filter(|(_, target)| target.is_none())
        .filter_map(|(key, _)| media_ids.get(key).copied())
        .collect::<RoaringBitmap>();
    if !gone.is_empty() {
        let (next, _, _) =
            crate::mutation::delete_roots(transaction, revision, snapshot, &gone, super::now_ms(), protected)?;
        *snapshot = next;
    }
    Ok(())
}

fn root_state(
    root_id: RootId,
    stable_key: String,
    kind: RootKind,
    view: &View,
    cover_media_id: MediaId,
    media_count: u32,
    total_size_bytes: u64,
) -> Result<StructuralRootState> {
    let fields = &view.fields;
    Ok(StructuralRootState {
        root_id,
        stable_key,
        kind,
        name: fields["name"].as_str().unwrap_or_default().to_owned(),
        notes: fields["notes"].as_str().map(str::to_owned),
        source_urls_json: serde_json::to_string(&fields["links"])?,
        cover_media_id,
        imported_at_ms: fields["imported_at"].as_i64().unwrap_or(0),
        captured_at_ms: fields["captured_at"].as_i64(),
        modified_at_ms: fields["modified_at"].as_i64().unwrap_or(0),
        media_count,
        total_size_bytes,
    })
}

/// The positions in each folder of the roots that the structure writes.
/// The members the write does not touch keep their order; each written root
/// goes where its position key sorts among them, found by binary search.
fn folder_positions(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    synced: &mut Synced<'_>,
    affected: &RoaringBitmap,
    targets: BTreeMap<String, Vec<(String, String, u32)>>,
) -> Result<BTreeMap<FolderId, Vec<StructuralFolderPosition>>> {
    let local = Local::new(transaction, snapshot)?;
    let mut positions = BTreeMap::new();
    for (folder, mut entries) in targets {
        let Some(folder_id) = local.find(SyncKind::Folder, &folder)?.map(FolderId) else { continue };
        let field = format!("folder:{folder}");
        entries.sort();
        let retained = snapshot
            .folder_orders
            .get(&folder_id)
            .into_iter()
            .flat_map(|order| order.iter())
            .filter(|root| !affected.contains(root.0))
            .copied()
            .collect::<Vec<_>>();
        let mut keys: HashMap<u32, (String, String)> = HashMap::new();
        let mut slots = Vec::with_capacity(entries.len());
        for (position, key, _) in &entries {
            let mut low = 0;
            let mut high = retained.len();
            while low < high {
                let middle = (low + high) / 2;
                let root = retained[middle].0;
                if let std::collections::hash_map::Entry::Vacant(entry) = keys.entry(root) {
                    let (kind, member) = local
                        .root_key(RootId(root))?
                        .ok_or_else(|| LibraryError::InvalidState(format!("root {root} has no sync key")))?;
                    let member_position = synced
                        .raw(kind, &member)?
                        .get(&field)
                        .and_then(|stored| stored.value.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    entry.insert((member_position, member));
                }
                if keys[&root] < (position.clone(), key.clone()) {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            slots.push(low);
        }
        let mut list = Vec::new();
        for (index, (slot, (_, _, root))) in slots.iter().zip(&entries).enumerate() {
            let next_unaffected = retained.get(*slot).copied();
            let previous = if index > 0 && slots[index - 1] == *slot {
                Some(RootId(entries[index - 1].2))
            } else {
                slot.checked_sub(1).map(|before| retained[before])
            };
            list.push(StructuralFolderPosition {
                root_id: RootId(*root),
                previous,
                next_unaffected,
                ordinal: slot + index,
            });
        }
        positions.insert(folder_id, list);
    }
    Ok(positions)
}
