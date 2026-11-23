//! The shared entries of the changes to items that the devices of a library
//! hold: a name, notes, source links, a rating, a tag that is added or
//! removed, and a folder that an item is put into or taken out of. The
//! entries, their keys and the merge rule are described in `shared.rs`.
//!
//! # How a change is recorded
//!
//! A library that records items ([`records_items`]) writes one row of
//! `cloud_journal` for each kind of change that a write makes, with the roots
//! that changed and the time of the library's clock
//! ([`journal_shared_changes`]). The row is written from the history entry
//! of the write, which holds exactly what the write changed, so a change of a
//! million items costs one row when the user makes it. Undo and Redo, the
//! automatic tagging and an import that changes an item which the library
//! holds write the same rows. The cloud queue turns the rows into entries,
//! at most a given number of roots in one transaction ([`settle_journal_in`]).
//!
//! An entry is stored without a value. When a sync writes it into the cloud
//! folder, it gets the value that the library holds then
//! ([`attach_field_values`]): the entry is this device's, so no later change
//! of the same field was made or merged, and the library holds the value of
//! the entry.
//!
//! # How a merged entry is applied
//!
//! A merged entry that wins changes the library through the code that Undo
//! and Redo use ([`apply_semantic_change`]), in the publication of the merge
//! and without a history entry ([`apply_field_entries`]). An entry for an
//! item that the library does not hold yet is stored with its value, and the
//! import of the item reads it ([`held_fields`]).

use std::collections::BTreeMap;

use super::*;

/// The kind of the journal rows that hold the roots of one kind of change.
pub(super) const ENTRY_ROW: &str = "entry.roots";

/// The character between the two parts of the key of an `item_tag` or
/// `folder_item` entry. The second part is the stable key of a tag or a
/// folder, which never holds it.
const PAIR: char = '\t';

/// Returns the item's key and the tag's or folder's key of the key of an
/// `item_tag` or `folder_item` entry.
fn split_pair(key: &str) -> Option<(&str, &str)> {
    key.rsplit_once(PAIR)
}

/// One row to write into the journal: the kind and the state of the entries,
/// the stable key of the tag or folder for the kinds that name one, and the
/// roots.
type EntryRows = Vec<(EntryKind, EntryState, Option<String>, RoaringBitmap)>;

fn stable_key_of(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    id_column: &'static str,
    id: u32,
) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(transaction
        .prepare_cached(&format!(
            "SELECT stable_key FROM {table} WHERE {id_column} = ?1"
        ))?
        .query_row([id], |row| row.get(0))
        .optional()?)
}

/// Adds to `rows` what `change` changed of the kinds that are shared.
/// `forward` says whether the change is made (`true`) or taken back. A change
/// of status is left out: `journal_trash_moves` records it. A tag or a folder
/// that the library no longer holds was deleted by the same write, and its
/// own entry tells the other devices.
fn collect_rows(
    transaction: &rusqlite::Transaction<'_>,
    change: &SemanticChange,
    forward: bool,
    rows: &mut EntryRows,
) -> Result<()> {
    match change {
        SemanticChange::Compound(changes) => {
            for change in changes {
                collect_rows(transaction, change, forward, rows)?;
            }
        }
        SemanticChange::Bitmap { key, before, after } => {
            let (old, new) = if forward {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            match key.domain {
                BitmapDomain::Tag => {
                    if let Some(tag) =
                        stable_key_of(transaction, "tag_definition", "tag_id", key.key_id)?
                    {
                        rows.push((EntryKind::ItemTag, EntryState::Add, Some(tag.clone()), new - old));
                        rows.push((EntryKind::ItemTag, EntryState::Remove, Some(tag), old - new));
                    }
                }
                // The roots that got this rating. A root that lost it got
                // another one, which another change of the same write holds.
                BitmapDomain::Rating => {
                    rows.push((EntryKind::Rating, EntryState::Set, None, new - old));
                }
                BitmapDomain::Lifecycle => {}
            }
        }
        SemanticChange::Order {
            owner_kind: OrderOwnerKind::Folder,
            owner_id,
            before,
            after,
        } => {
            let (old, new) = if forward { (before, after) } else { (after, before) };
            if let Some(folder) =
                stable_key_of(transaction, "folder_definition", "folder_id", *owner_id)?
            {
                let old = old.iter().copied().collect::<RoaringBitmap>();
                let new = new.iter().copied().collect::<RoaringBitmap>();
                rows.push((EntryKind::FolderItem, EntryState::Add, Some(folder.clone()), &new - &old));
                rows.push((EntryKind::FolderItem, EntryState::Remove, Some(folder), &old - &new));
            }
        }
        SemanticChange::RootText { before, after } => {
            let mut changed = [RoaringBitmap::new(), RoaringBitmap::new(), RoaringBitmap::new()];
            for (old, new) in before.iter().zip(after.iter()) {
                if old.name != new.name {
                    changed[0].insert(new.root_id.0);
                }
                if old.notes != new.notes {
                    changed[1].insert(new.root_id.0);
                }
                if old.source_urls != new.source_urls {
                    changed[2].insert(new.root_id.0);
                }
            }
            let [names, notes, urls] = changed;
            rows.push((EntryKind::Name, EntryState::Set, None, names));
            rows.push((EntryKind::Notes, EntryState::Set, None, notes));
            rows.push((EntryKind::Urls, EntryState::Set, None, urls));
        }
        _ => {}
    }
    Ok(())
}

/// Writes `rows` into the journal with one new clock time. Rows of the same
/// kind, state and tag or folder become one row, and a row without roots is
/// left out.
fn write_rows(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    rows: EntryRows,
) -> Result<()> {
    let mut merged = Vec::<(EntryKind, EntryState, Option<String>, RoaringBitmap)>::new();
    for (kind, state, with, roots) in rows {
        if roots.is_empty() {
            continue;
        }
        match merged
            .iter_mut()
            .find(|row| row.0 == kind && row.1 == state && row.2 == with)
        {
            Some(row) => row.3 |= roots,
            None => merged.push((kind, state, with, roots)),
        }
    }
    if merged.is_empty() {
        return Ok(());
    }
    let (at_ms, logical) = tick(transaction)?;
    for (kind, state, with, roots) in merged {
        insert_cloud_journal(
            transaction,
            revision,
            ENTRY_ROW,
            Some(&roots),
            serde_json::json!({
                "kind": kind.as_str(),
                "state": state.as_str(),
                "with": with,
                "at_ms": at_ms,
                "logical": logical,
            }),
            now_ms(),
        )?;
    }
    Ok(())
}

/// Records what `change` changed of the kinds that the devices share, in a
/// library that records items. `forward` is `false` when the change is taken
/// back by an undo. Nothing is written, and the clock stays, when the change
/// holds none of those kinds.
pub(super) fn journal_shared_changes(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    change: &SemanticChange,
    forward: bool,
) -> Result<()> {
    if !records_items(transaction)? {
        return Ok(());
    }
    let mut rows = Vec::new();
    collect_rows(transaction, change, forward, &mut rows)?;
    write_rows(transaction, revision, rows)
}

/// Records what an import gave to items that the library already held: the
/// tags in `tags`, the folders in `folders`, the ratings in `ratings`, and
/// the names, notes and source links that `texts` holds as text changes.
pub(super) fn journal_import_changes(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    tags: &[(RootId, crate::TagId)],
    folders: &[(RootId, FolderId)],
    ratings: &[(RootId, Rating)],
    texts: &[SemanticChange],
) -> Result<()> {
    if (tags.is_empty() && folders.is_empty() && ratings.is_empty() && texts.is_empty())
        || !records_items(transaction)?
    {
        return Ok(());
    }
    let mut rows = Vec::new();
    for (root, tag) in tags {
        if let Some(key) = stable_key_of(transaction, "tag_definition", "tag_id", tag.0)? {
            rows.push((
                EntryKind::ItemTag,
                EntryState::Add,
                Some(key),
                RoaringBitmap::from_iter([root.0]),
            ));
        }
    }
    for (root, folder) in folders {
        if let Some(key) = stable_key_of(transaction, "folder_definition", "folder_id", folder.0)? {
            rows.push((
                EntryKind::FolderItem,
                EntryState::Add,
                Some(key),
                RoaringBitmap::from_iter([root.0]),
            ));
        }
    }
    for (root, _) in ratings {
        rows.push((
            EntryKind::Rating,
            EntryState::Set,
            None,
            RoaringBitmap::from_iter([root.0]),
        ));
    }
    for change in texts {
        collect_rows(transaction, change, true, &mut rows)?;
    }
    write_rows(transaction, revision, rows)
}

/// Turns the roots of the pending journal rows of shared changes into
/// entries of this device, at most `limit` roots, and returns whether rows
/// are left. A row holds the roots that one change touched, what the change
/// was, and the clock time of the change. The rows are written by the change
/// itself, which stays as fast as it was for any number of roots; the entries
/// are written here, on the cloud queue. An entry is not stored over a later
/// one.
pub(super) fn settle_journal_in(
    transaction: &rusqlite::Transaction<'_>,
    limit: usize,
) -> Result<bool> {
    use rusqlite::OptionalExtension;
    let device_id = own_device_id(transaction)?;
    let mut room = limit;
    loop {
        let row = transaction
            .prepare_cached(
                "SELECT journal_id, operation_kind, target_bitmap, payload_json, created_at_ms
                 FROM cloud_journal WHERE operation_kind IN ('root.trash', ?1)
                 ORDER BY journal_id LIMIT 1",
            )?
            .query_row([ENTRY_ROW], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .optional()?;
        let Some((journal_id, operation, targets, payload, created_at_ms)) = row else {
            return Ok(false);
        };
        if room == 0 {
            return Ok(true);
        }
        let payload: serde_json::Value = serde_json::from_str(&payload)?;
        // A row of a move into or out of the Trash names the status. Every
        // other row names the kind and the state of its entries.
        let named = if operation == "root.trash" {
            let state = match payload["lifecycle"].as_u64() {
                Some(key) if key == u64::from(Lifecycle::Trash.bitmap_key()) => EntryState::Trash,
                Some(key) if key == u64::from(Lifecycle::Inbox.bitmap_key()) => EntryState::Inbox,
                _ => EntryState::Active,
            };
            Some((EntryKind::Status, state))
        } else {
            payload["kind"]
                .as_str()
                .and_then(EntryKind::parse)
                .zip(payload["state"].as_str().and_then(EntryState::parse))
        };
        let Some((kind, state)) = named else {
            transaction.execute("DELETE FROM cloud_journal WHERE journal_id = ?1", [journal_id])?;
            continue;
        };
        let with = payload["with"]
            .as_str()
            .map(|other| format!("{PAIR}{other}"))
            .unwrap_or_default();
        let at_ms = payload["at_ms"].as_i64().unwrap_or(created_at_ms);
        let logical = payload["logical"].as_i64().unwrap_or(0);
        let mut roots = targets
            .map(|bytes| RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(bytes)))
            .transpose()?
            .unwrap_or_default();
        let taken = roots.iter().take(room).collect::<Vec<_>>();
        room -= taken.len();
        transaction.execute(
            "INSERT INTO shared_entry
                 (kind, key, state, value, clock_ms, clock_counter, device_id)
             SELECT ?2, item.stable_key || ?7, ?3, NULL, ?4, ?5, ?6
             FROM json_each(?1) ids
             CROSS JOIN library_item item ON item.local_id = ids.value
             JOIN library_root root ON root.root_id = item.local_id
             WHERE true
             ON CONFLICT(kind, key) DO UPDATE SET
                 state = excluded.state,
                 value = excluded.value,
                 clock_ms = excluded.clock_ms,
                 clock_counter = excluded.clock_counter,
                 device_id = excluded.device_id
             WHERE (excluded.clock_ms, excluded.clock_counter, excluded.device_id)
                 > (shared_entry.clock_ms, shared_entry.clock_counter,
                    shared_entry.device_id)",
            rusqlite::params![
                serde_json::to_string(&taken)?,
                kind.as_str(),
                state.as_str(),
                at_ms,
                logical,
                device_id,
                with
            ],
        )?;
        for root in &taken {
            roots.remove(*root);
        }
        if roots.is_empty() {
            transaction.execute("DELETE FROM cloud_journal WHERE journal_id = ?1", [journal_id])?;
        } else {
            transaction.execute(
                "UPDATE cloud_journal SET target_bitmap = ?2 WHERE journal_id = ?1",
                rusqlite::params![journal_id, crate::bitmap::encode(&roots)?],
            )?;
        }
    }
}

/// Returns the whole name of the tag with the stable key `key`.
fn tag_name(connection: &rusqlite::Connection, key: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(connection
        .prepare_cached(
            "SELECT namespace.display_name, tag.subname FROM tag_definition tag
             JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
             WHERE tag.stable_key = ?1",
        )?
        .query_row([key], |row| {
            Ok(whole_tag_name(
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
            ))
        })
        .optional()?)
}

/// Fills in the value of every entry among `entries` that names a field of
/// an item, from the library as it is now: the name, the notes, the source
/// links or the rating of the item for a `set` entry, and the name of the
/// tag or the folder for an `add` entry, with which a device that does not
/// hold the tag or the folder creates it. The entries are entries of this
/// device. An entry whose item, tag or folder the library no longer holds
/// stays without a value.
pub(super) fn attach_field_values(
    connection: &rusqlite::Connection,
    snapshot: &ProjectionSnapshot,
    entries: &mut [SharedEntry],
) -> Result<()> {
    use rusqlite::OptionalExtension;
    for entry in entries {
        entry.value = match (entry.kind, entry.state) {
            (EntryKind::Name | EntryKind::Notes | EntryKind::Urls, EntryState::Set) => connection
                .prepare_cached(
                    "SELECT root.name, root.notes, root.source_urls_json
                     FROM library_item item
                     JOIN library_root root ON root.root_id = item.local_id
                     WHERE item.stable_key = ?1",
                )?
                .query_row([&entry.key], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .optional()?
                .map(|(name, notes, urls)| {
                    Ok::<_, LibraryError>(match entry.kind {
                        EntryKind::Name => serde_json::json!(name),
                        // An item without notes has the empty text, because
                        // an entry without a value carries nothing.
                        EntryKind::Notes => serde_json::json!(notes.unwrap_or_default()),
                        _ => serde_json::from_str(&urls)?,
                    })
                })
                .transpose()?,
            (EntryKind::Rating, EntryState::Set) => connection
                .prepare_cached(
                    "SELECT root.root_id FROM library_item item
                     JOIN library_root root ON root.root_id = item.local_id
                     WHERE item.stable_key = ?1",
                )?
                .query_row([&entry.key], |row| row.get::<_, u32>(0))
                .optional()?
                .and_then(|root| {
                    Rating::ALL
                        .into_iter()
                        .find(|rating| snapshot.rating(*rating).contains(root))
                })
                .map(|rating| serde_json::json!(rating.bitmap_key())),
            (EntryKind::ItemTag, EntryState::Add) => split_pair(&entry.key)
                .map(|(_, tag)| tag_name(connection, tag))
                .transpose()?
                .flatten()
                .map(|name| serde_json::json!({ "name": name })),
            (EntryKind::FolderItem, EntryState::Add) => split_pair(&entry.key)
                .map(|(_, folder)| {
                    connection
                        .prepare_cached("SELECT name FROM folder_definition WHERE stable_key = ?1")?
                        .query_row([folder], |row| row.get::<_, String>(0))
                        .optional()
                })
                .transpose()?
                .flatten()
                .map(|name| serde_json::json!({ "name": name })),
            _ => continue,
        };
    }
    Ok(())
}

/// Returns whether `kind` is one of the kinds of this file.
pub(super) fn is_field(kind: EntryKind) -> bool {
    matches!(
        kind,
        EntryKind::Name
            | EntryKind::Notes
            | EntryKind::Urls
            | EntryKind::Rating
            | EntryKind::ItemTag
            | EntryKind::FolderItem
    )
}

/// Returns the stable key of the item that an entry of one of the kinds of
/// this file is about.
pub(super) fn item_key(entry: &SharedEntry) -> &str {
    match entry.kind {
        EntryKind::ItemTag | EntryKind::FolderItem => {
            split_pair(&entry.key).map_or(entry.key.as_str(), |(item, _)| item)
        }
        _ => entry.key.as_str(),
    }
}

/// Returns the folders that the `folder_item` entries among `entries` put an
/// item into, each with the name that the entry carries. The merge creates
/// those that the library does not hold before it applies the entries.
pub(super) fn named_folders(entries: &[SharedEntry]) -> Vec<SharedName> {
    let mut folders = BTreeMap::new();
    for entry in entries {
        if entry.kind != EntryKind::FolderItem || entry.state != EntryState::Add {
            continue;
        }
        let name = entry.value.as_ref().and_then(|value| value["name"].as_str());
        if let (Some((_, key)), Some(name)) = (split_pair(&entry.key), name) {
            folders.entry(key.to_owned()).or_insert_with(|| name.to_owned());
        }
    }
    folders
        .into_iter()
        .map(|(key, name)| SharedName { key, name })
        .collect()
}

/// Applies the winning entries of the kinds of this file whose item the
/// library holds. `roots` gives the root of each item's key. Returns the
/// number of entries that changed the library. An entry that names a tag
/// which the library does not hold looks for the tag by the name it carries
/// and creates it. An entry that names a folder which the library does not
/// hold changes nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_field_entries(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    next: &mut ProjectionSnapshot,
    winners: &[&SharedEntry],
    roots: &HashMap<&str, u32>,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<u64> {
    let held = |entry: &SharedEntry| roots.get(item_key(entry)).copied();
    let mut changes = Vec::new();

    // Names, notes and source links.
    let mut texts = BTreeMap::<u32, Vec<&SharedEntry>>::new();
    for entry in winners.iter().filter(|entry| {
        matches!(entry.kind, EntryKind::Name | EntryKind::Notes | EntryKind::Urls)
            && entry.state == EntryState::Set
    }) {
        if let (Some(root), Some(_)) = (held(entry), entry.value.as_ref()) {
            texts.entry(root).or_default().push(entry);
        }
    }
    if !texts.is_empty() {
        let before = load_root_text(transaction, &texts.keys().copied().collect())?;
        let mut kept = Vec::new();
        let mut after = Vec::new();
        for state in before {
            let mut changed = state.clone();
            for entry in &texts[&state.root_id.0] {
                let value = entry.value.clone().unwrap_or_default();
                match entry.kind {
                    EntryKind::Name => {
                        if let Some(name) = value.as_str().filter(|name| !name.trim().is_empty()) {
                            changed.name = name.to_owned();
                        }
                    }
                    EntryKind::Notes => {
                        changed.notes = value
                            .as_str()
                            .filter(|notes| !notes.is_empty())
                            .map(str::to_owned);
                    }
                    _ => {
                        changed.source_urls = serde_json::from_value(value).unwrap_or_default();
                    }
                }
            }
            if changed != state {
                changed.modified_at_ms = now_ms();
                kept.push(state);
                after.push(changed);
            }
        }
        if !after.is_empty() {
            changes.push(SemanticChange::RootText {
                before: Arc::new(kept),
                after: Arc::new(after),
            });
        }
    }

    // Tags, one bitmap change for each tag.
    let mut tagged = BTreeMap::<&str, (RoaringBitmap, RoaringBitmap, Option<&str>)>::new();
    for entry in winners
        .iter()
        .filter(|entry| entry.kind == EntryKind::ItemTag)
    {
        let (Some(root), Some((_, tag))) = (held(entry), split_pair(&entry.key)) else {
            continue;
        };
        let change = tagged.entry(tag).or_default();
        if entry.state == EntryState::Add {
            change.0.insert(root);
            change.1.remove(root);
            change.2 = change
                .2
                .or_else(|| entry.value.as_ref().and_then(|value| value["name"].as_str()));
        } else {
            change.1.insert(root);
            change.0.remove(root);
        }
    }
    for (tag, (added, removed, name)) in tagged {
        use rusqlite::OptionalExtension;
        let tag_id = transaction
            .prepare_cached("SELECT tag_id FROM tag_definition WHERE stable_key = ?1")?
            .query_row([tag], |row| row.get::<_, u32>(0))
            .optional()?;
        // A tag for whose key the library holds a `delete` entry was
        // deleted, and an entry that is earlier than the deletion does not
        // create it again.
        let deleted = transaction
            .prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM shared_entry
                 WHERE kind = 'tag' AND key = ?1 AND state = 'delete')",
            )?
            .query_row([tag], |row| row.get::<_, bool>(0))?;
        let tag_id = match (tag_id, name) {
            (Some(tag_id), _) => crate::TagId(tag_id),
            (None, Some(name)) if !added.is_empty() && !deleted => {
                let name = ingest::canonical_tag_name(name)?;
                let tag_id = match next.tag_ids_by_name.get(&name).copied() {
                    Some(tag_id) => tag_id,
                    None => ingest::ensure_tag(transaction, &name)?,
                };
                Arc::make_mut(&mut next.tag_ids_by_name).insert(name, tag_id);
                resources.insert("tags".into());
                tag_id
            }
            _ => continue,
        };
        let key = BitmapKey {
            domain: BitmapDomain::Tag,
            key_id: tag_id.0,
        };
        let before = projection_bitmap(next, key);
        let mut after = &before | &added;
        after -= &removed;
        if after != before {
            changes.push(SemanticChange::Bitmap {
                key,
                before: Arc::new(before),
                after: Arc::new(after),
            });
            resources.extend(["tags", "sidebar"].map(str::to_owned));
        }
    }

    // Folders. The items that are added stand at the end of the folder, in
    // the order of the clock times of their entries.
    let mut placed = BTreeMap::<&str, Vec<(&SharedEntry, u32)>>::new();
    for entry in winners
        .iter()
        .filter(|entry| entry.kind == EntryKind::FolderItem)
    {
        if let (Some(root), Some((_, folder))) = (held(entry), split_pair(&entry.key)) {
            placed.entry(folder).or_default().push((entry, root));
        }
    }
    for (folder, mut entries) in placed {
        let Some(folder_id) = definition_ids(
            transaction,
            "folder_definition",
            "folder_id",
            &[folder.to_owned()],
        )?
        .into_iter()
        .next() else {
            continue;
        };
        let before = next
            .folder_orders
            .get(&FolderId(folder_id))
            .map(|roots| roots.iter().map(|root| root.0).collect::<Vec<_>>())
            .unwrap_or_default();
        entries.sort_by_key(|(entry, _)| (entry.at_ms, entry.logical));
        let mut after = before.clone();
        for (entry, root) in entries {
            let position = after.iter().position(|held| *held == root);
            match (entry.state, position) {
                (EntryState::Add, None) => after.push(root),
                (EntryState::Remove, Some(position)) => {
                    after.remove(position);
                }
                _ => {}
            }
        }
        if after != before {
            changes.push(SemanticChange::Order {
                owner_kind: OrderOwnerKind::Folder,
                owner_id: folder_id,
                before: Arc::new(before),
                after: Arc::new(after),
            });
        }
    }

    let mut changed = RoaringBitmap::new();
    let mut dependencies = Vec::new();
    for change in &changes {
        apply_semantic_change(transaction, revision, next, change, true, &mut changed, resources)?;
        history_dependencies(change, &mut dependencies);
    }

    // Ratings.
    let rating_keys = Rating::ALL
        .iter()
        .map(|rating| rating.bitmap_key())
        .collect::<Vec<_>>();
    let mut rated = 0;
    for rating in Rating::ALL {
        let mut moved = winners
            .iter()
            .filter(|entry| {
                entry.kind == EntryKind::Rating
                    && entry.state == EntryState::Set
                    && entry.value.as_ref().and_then(|value| value.as_u64())
                        == Some(u64::from(rating.bitmap_key()))
            })
            .filter_map(|entry| held(entry))
            .collect::<RoaringBitmap>();
        moved -= next.rating(rating);
        if moved.is_empty() {
            continue;
        }
        move_to_partition(
            transaction,
            revision,
            next,
            BitmapDomain::Rating,
            rating.bitmap_key(),
            &rating_keys,
            &moved,
        )?;
        rated += moved.len();
        changed |= &moved;
        resources.extend(["roots", "ratings"].map(str::to_owned));
    }

    if !changes.is_empty() {
        crate::smart::settle_affected_for(transaction, next, &changed, &dependencies)?;
        resources.insert("smart-folders".into());
    }
    *affected |= &changed;
    Ok(changes.len() as u64 + rated)
}

/// What the entries that the library holds give an item of another device
/// that is imported now.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct HeldFields {
    pub name: Option<String>,
    /// `Some(None)` when an entry cleared the notes.
    pub notes: Option<Option<String>>,
    pub urls: Option<Vec<String>>,
    pub rating: Option<Rating>,
    /// The tags and folders to add, by stable key and name, and the stable
    /// keys of those to leave out.
    pub tags: Vec<SharedName>,
    pub folders: Vec<SharedName>,
    pub without: Vec<String>,
}

/// Returns what the entries that the library holds for the item `key` give
/// the item. These are entries that were merged before the item was here,
/// which are stored with their values.
pub(super) fn held_fields(connection: &rusqlite::Connection, key: &str) -> Result<HeldFields> {
    let mut fields = HeldFields::default();
    let mut single = connection.prepare_cached(
        "SELECT kind, value FROM shared_entry
         WHERE kind IN ('name', 'notes', 'urls', 'rating') AND key = ?1 AND state = 'set'
           AND value IS NOT NULL",
    )?;
    let rows = single
        .query_map([key], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (kind, value) in rows {
        let value: serde_json::Value = serde_json::from_str(&value)?;
        match kind.as_str() {
            "name" => fields.name = value.as_str().map(str::to_owned),
            "notes" => {
                fields.notes = Some(
                    value
                        .as_str()
                        .filter(|notes| !notes.is_empty())
                        .map(str::to_owned),
                );
            }
            "urls" => fields.urls = serde_json::from_value(value).ok(),
            _ => {
                fields.rating = Rating::ALL
                    .into_iter()
                    .find(|rating| Some(u64::from(rating.bitmap_key())) == value.as_u64());
            }
        }
    }
    // The keys of the entries for the item's tags and folders start with the
    // item's key and the separator.
    let first = format!("{key}{PAIR}");
    let end = format!("{key}{}", char::from_u32(PAIR as u32 + 1).unwrap_or('\n'));
    let mut pairs = connection.prepare_cached(
        "SELECT kind, key, state, value FROM shared_entry
         WHERE kind IN ('item_tag', 'folder_item') AND key >= ?1 AND key < ?2",
    )?;
    let rows = pairs
        .query_map([&first, &end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (kind, pair, state, value) in rows {
        let Some((item, other)) = split_pair(&pair) else {
            continue;
        };
        if item != key {
            continue;
        }
        let name = value
            .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
            .and_then(|value| value["name"].as_str().map(str::to_owned));
        match (state.as_str(), name) {
            ("add", Some(name)) => {
                let named = SharedName {
                    key: other.to_owned(),
                    name,
                };
                if kind == "item_tag" {
                    fields.tags.push(named);
                } else {
                    fields.folders.push(named);
                }
            }
            ("remove", _) => fields.without.push(other.to_owned()),
            _ => {}
        }
    }
    Ok(fields)
}
