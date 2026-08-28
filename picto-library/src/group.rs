use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{params, Transaction};
use uuid::Uuid;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::LibraryDatabase;
use crate::model::{GroupRequest, LabColor, Lifecycle, MediaId, Rating, RootId, RootKind};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{color_cell, ProjectionSnapshot};
use crate::{LibraryError, Result};

pub(crate) struct GroupResult {
    pub collection_id: RootId,
    pub affected: RoaringBitmap,
    pub snapshot: ProjectionSnapshot,
}

pub(crate) struct UngroupResult {
    pub roots: Vec<RootId>,
    pub affected: RoaringBitmap,
    pub snapshot: ProjectionSnapshot,
}

pub(crate) struct DetachResult {
    pub root_ids: Vec<RootId>,
    pub affected: RoaringBitmap,
    pub snapshot: ProjectionSnapshot,
}

pub(crate) fn detach_many(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    collection_id: RootId,
    media_ids: &[MediaId],
    target_lifecycle: Option<Lifecycle>,
    modified_at_ms: i64,
) -> Result<DetachResult> {
    let members = snapshot
        .collection_orders
        .get(&collection_id)
        .cloned()
        .ok_or_else(|| {
            LibraryError::InvalidInput(format!("root {collection_id} is not a collection"))
        })?;
    let selected = media_ids
        .iter()
        .map(|media_id| media_id.0)
        .collect::<RoaringBitmap>();
    if selected.is_empty() {
        return Err(LibraryError::InvalidInput(
            "detaching collection members requires at least one media item".into(),
        ));
    }
    if selected.len() != media_ids.len() as u64 {
        return Err(LibraryError::InvalidInput(
            "collection members must be unique".into(),
        ));
    }
    for media_id in media_ids {
        if !members.contains(media_id) {
            return Err(LibraryError::InvalidInput(format!(
                "media {media_id} is not a member of collection {collection_id}"
            )));
        }
    }
    let collection = load_roots(transaction, &[collection_id])?
        .into_iter()
        .next()
        .ok_or_else(|| LibraryError::NotFound(format!("collection {collection_id}")))?;
    let lifecycle = common_lifecycle(&snapshot, &[collection_id.0].into_iter().collect())?;
    let detached_lifecycle = target_lifecycle.unwrap_or(lifecycle);
    let rating = rating_for(&snapshot, collection_id)?;
    let inherited_tags = snapshot
        .tags
        .iter()
        .filter_map(|(tag_id, roots)| roots.contains(collection_id.0).then_some(*tag_id))
        .collect::<Vec<_>>();
    let inherited_folders = snapshot
        .folders
        .iter()
        .filter_map(|(folder_id, roots)| roots.contains(collection_id.0).then_some(*folder_id))
        .collect::<Vec<_>>();
    let mut media_rows = Vec::with_capacity(media_ids.len());
    let mut statement = transaction.prepare_cached(
        "SELECT media.media_name, file.mime, file.size_bytes, file.width, file.height,
                file.duration_ms, file.palette_json
         FROM media_item media JOIN media_file file ON file.file_id = media.file_id
         WHERE media.media_id = ?1",
    )?;
    for media_id in members
        .iter()
        .filter(|media_id| selected.contains(media_id.0))
    {
        let row = statement.query_row([media_id.0], |row| {
            Ok((
                row.get::<_, String>(0)?,
                CoverFacts {
                    mime: row.get(1)?,
                    size_bytes: row.get::<_, i64>(2)? as u64,
                    width: row.get(3)?,
                    height: row.get(4)?,
                    duration_ms: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
                    palette: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                },
            ))
        })?;
        media_rows.push((*media_id, row.0, row.1));
    }
    drop(statement);
    let remaining = members
        .iter()
        .copied()
        .filter(|member| !selected.contains(member.0))
        .collect::<Vec<_>>();
    let detached_size = media_rows.iter().try_fold(0u64, |total, (_, _, facts)| {
        total
            .checked_add(facts.size_bytes)
            .ok_or_else(|| LibraryError::InvalidState("detached media size overflow".into()))
    })?;
    let remaining_size = collection
        .total_size_bytes
        .checked_sub(detached_size)
        .ok_or_else(|| LibraryError::InvalidState("collection size underflow".into()))?;
    let removes_collection = remaining.is_empty();
    let next_cover = remaining
        .first()
        .copied()
        .filter(|_| selected.contains(collection.cover_media_id.0));

    if removes_collection {
        transaction.execute("DELETE FROM root_fts WHERE root_id = ?1", [collection_id.0])?;
        transaction.execute(
            "DELETE FROM library_root WHERE root_id = ?1",
            [collection_id.0],
        )?;
        ordering::delete(transaction, OrderOwnerKind::Collection, collection_id.0)?;
        transaction.execute(
            "DELETE FROM library_item WHERE local_id = ?1",
            [collection_id.0],
        )?;
    } else {
        let next_cover = next_cover.unwrap_or(collection.cover_media_id);
        transaction.execute(
            "UPDATE library_root
             SET cover_media_id = ?2, modified_at_ms = ?3, media_count = ?4,
                 total_size_bytes = ?5
             WHERE root_id = ?1",
            params![
                collection_id.0,
                next_cover.0,
                modified_at_ms,
                remaining.len() as i64,
                i64::try_from(remaining_size).map_err(|_| LibraryError::InvalidState(
                    "collection size exceeds SQLite range".into()
                ))?
            ],
        )?;
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Collection,
            collection_id.0,
            &remaining.iter().map(|media| media.0).collect::<Vec<_>>(),
        )?;
    }
    let mut insert_root = transaction.prepare_cached(
        "INSERT INTO library_root
             (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
              captured_at_ms, modified_at_ms, media_count, total_size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?1, ?5, ?6, ?7, 1, ?8)",
    )?;
    let urls = serde_json::to_string(&collection.urls)?;
    for (media_id, media_name, facts) in &media_rows {
        insert_root.execute(params![
            media_id.0,
            media_name,
            collection.notes,
            urls,
            collection.imported_at_ms,
            collection.captured_at_ms,
            modified_at_ms,
            i64::try_from(facts.size_bytes).map_err(|_| LibraryError::InvalidState(
                "media size exceeds SQLite range".into()
            ))?
        ])?;
        crate::fts::mark_one(transaction, RootId(media_id.0), modified_at_ms)?;
    }
    drop(insert_root);

    for value in Lifecycle::ALL {
        let lifecycle_members = Arc::make_mut(&mut snapshot.lifecycle)
            .entry(value)
            .or_default();
        if removes_collection {
            lifecycle_members.remove(collection_id.0);
        }
        if value == detached_lifecycle {
            *lifecycle_members |= &selected;
        }
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Lifecycle,
                key_id: value.bitmap_key(),
            },
            lifecycle_members,
        )?;
    }
    let rating_members = Arc::make_mut(&mut snapshot.ratings)
        .entry(rating)
        .or_default();
    if removes_collection {
        rating_members.remove(collection_id.0);
    }
    *rating_members |= &selected;
    bitmap::replace(
        transaction,
        revision,
        BitmapKey {
            domain: BitmapDomain::Rating,
            key_id: rating.bitmap_key(),
        },
        rating_members,
    )?;
    for tag_id in &inherited_tags {
        let roots = Arc::make_mut(&mut snapshot.tags)
            .entry(*tag_id)
            .or_default();
        if removes_collection {
            roots.remove(collection_id.0);
        }
        *roots |= &selected;
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Tag,
                key_id: tag_id.0,
            },
            roots,
        )?;
    }
    for folder_id in &inherited_folders {
        let before = snapshot.folder_orders[folder_id].as_ref();
        let position = before
            .iter()
            .position(|root| *root == collection_id)
            .ok_or_else(|| {
                LibraryError::InvalidState(format!(
                    "folder {} bitmap and order disagree",
                    folder_id.0
                ))
            })?;
        let mut after = before.clone();
        let insertion = if removes_collection {
            after.remove(position);
            position
        } else {
            position + 1
        };
        after.splice(
            insertion..insertion,
            media_rows.iter().map(|(media_id, _, _)| RootId(media_id.0)),
        );
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Folder,
            folder_id.0,
            &after.iter().map(|root| root.0).collect::<Vec<_>>(),
        )?;
        Arc::make_mut(&mut snapshot.folder_orders).insert(*folder_id, Arc::new(after.clone()));
        Arc::make_mut(&mut snapshot.folders)
            .insert(*folder_id, after.iter().map(|root| root.0).collect());
    }

    if removes_collection {
        Arc::make_mut(&mut snapshot.collection_orders).remove(&collection_id);
    } else {
        Arc::make_mut(&mut snapshot.collection_orders)
            .insert(collection_id, Arc::new(remaining.clone()));
    }
    let owners = Arc::make_mut(&mut snapshot.media_owner);
    for (media_id, _, _) in &media_rows {
        owners.insert(media_id.0, RootId(media_id.0));
    }
    if removes_collection {
        remove_root_projections(&mut snapshot, &[collection_id.0].into_iter().collect());
    } else {
        Arc::make_mut(&mut snapshot.media_count).insert(collection_id.0, remaining.len() as u64);
        Arc::make_mut(&mut snapshot.total_bytes).insert(collection_id.0, remaining_size);
        Arc::make_mut(&mut snapshot.modified_at)
            .insert(collection_id.0, modified_at_ms.max(0) as u64);
        refresh_root_mime_projection(transaction, &mut snapshot, collection_id)?;
        if next_cover.is_some() {
            refresh_cover_projection(transaction, &mut snapshot, collection_id)?;
        }
    }
    for (media_id, _, facts) in &media_rows {
        add_media_root_projection(
            &mut snapshot,
            RootId(media_id.0),
            facts,
            collection.imported_at_ms,
            collection.captured_at_ms,
            modified_at_ms,
            inherited_tags.len() as u64,
            inherited_folders.len() as u64,
            collection.notes.is_some(),
            !collection.urls.is_empty(),
        );
    }
    let mut affected = selected.clone();
    affected.insert(collection_id.0);
    transaction.execute(
        "INSERT INTO cloud_journal
             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
         VALUES (?1, 'collection.detach', ?2, ?3, ?4)",
        params![
            revision as i64,
            crate::bitmap::encode(&affected)?,
            serde_json::json!({
                "collection_id": collection_id.0,
                "media_ids": media_ids.iter().map(|media_id| media_id.0).collect::<Vec<_>>(),
                "target_lifecycle": target_lifecycle,
            })
            .to_string(),
            modified_at_ms
        ],
    )?;
    snapshot.revision = revision;
    Ok(DetachResult {
        root_ids: media_rows
            .iter()
            .map(|(media_id, _, _)| RootId(media_id.0))
            .collect(),
        affected,
        snapshot,
    })
}

pub(crate) fn set_cover(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    collection_id: RootId,
    cover_media_id: MediaId,
) -> Result<()> {
    let members = snapshot
        .collection_orders
        .get(&collection_id)
        .ok_or_else(|| {
            LibraryError::InvalidInput(format!("root {collection_id} is not a collection"))
        })?;
    if !members.contains(&cover_media_id) {
        return Err(LibraryError::InvalidInput(format!(
            "media {cover_media_id} is not a member of collection {collection_id}"
        )));
    }
    transaction.execute(
        "UPDATE library_root SET cover_media_id = ?2 WHERE root_id = ?1",
        params![collection_id.0, cover_media_id.0],
    )?;
    let facts = load_cover_facts(transaction, cover_media_id)?;
    remove_cover_projection(snapshot, collection_id);
    add_cover_projection(snapshot, collection_id, &facts);
    Ok(())
}

pub(crate) fn refresh_cover_projection(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    root_id: RootId,
) -> Result<()> {
    let cover_media_id = transaction
        .query_row(
            "SELECT cover_media_id FROM library_root WHERE root_id = ?1",
            [root_id.0],
            |row| row.get::<_, u32>(0).map(MediaId),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                LibraryError::NotFound(format!("root {root_id}"))
            }
            error => error.into(),
        })?;
    let facts = load_cover_facts(transaction, cover_media_id)?;
    remove_cover_projection(snapshot, root_id);
    add_cover_projection(snapshot, root_id, &facts);
    Ok(())
}

struct RootInput {
    root_id: RootId,
    kind: RootKind,
    name: String,
    notes: Option<String>,
    urls: Vec<String>,
    cover_media_id: MediaId,
    imported_at_ms: i64,
    captured_at_ms: Option<i64>,
    total_size_bytes: u64,
}

struct CoverFacts {
    mime: String,
    size_bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    palette: Vec<LabColor>,
}

pub(crate) fn ungroup(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    collection_id: RootId,
    modified_at_ms: i64,
) -> Result<UngroupResult> {
    let members = snapshot
        .collection_orders
        .get(&collection_id)
        .cloned()
        .ok_or_else(|| {
            LibraryError::InvalidInput(format!("root {collection_id} is not a collection"))
        })?;
    let collection = load_roots(transaction, &[collection_id])?
        .into_iter()
        .next()
        .ok_or_else(|| LibraryError::NotFound(format!("collection {collection_id}")))?;
    let lifecycle = common_lifecycle(&snapshot, &[collection_id.0].into_iter().collect())?;
    let rating = rating_for(&snapshot, collection_id)?;
    let inherited_tags = snapshot
        .tags
        .iter()
        .filter_map(|(tag_id, roots)| roots.contains(collection_id.0).then_some(*tag_id))
        .collect::<Vec<_>>();
    let inherited_folders = snapshot
        .folders
        .iter()
        .filter_map(|(folder_id, roots)| roots.contains(collection_id.0).then_some(*folder_id))
        .collect::<Vec<_>>();
    let new_roots = members
        .iter()
        .map(|media| RootId(media.0))
        .collect::<Vec<_>>();
    let new_bitmap = new_roots
        .iter()
        .map(|root| root.0)
        .collect::<RoaringBitmap>();

    let mut media_rows = Vec::with_capacity(members.len());
    let mut statement = transaction.prepare_cached(
        "SELECT media.media_name, file.mime, file.size_bytes, file.width, file.height,
                file.duration_ms, file.palette_json
         FROM media_item media JOIN media_file file ON file.file_id = media.file_id
         WHERE media.media_id = ?1",
    )?;
    for media_id in members.iter() {
        let row = statement.query_row([media_id.0], |row| {
            Ok((
                row.get::<_, String>(0)?,
                CoverFacts {
                    mime: row.get(1)?,
                    size_bytes: row.get::<_, i64>(2)? as u64,
                    width: row.get(3)?,
                    height: row.get(4)?,
                    duration_ms: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
                    palette: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                },
            ))
        })?;
        media_rows.push((*media_id, row.0, row.1));
    }
    drop(statement);

    transaction.execute("DELETE FROM root_fts WHERE root_id = ?1", [collection_id.0])?;
    transaction.execute(
        "DELETE FROM library_root WHERE root_id = ?1",
        [collection_id.0],
    )?;
    ordering::delete(transaction, OrderOwnerKind::Collection, collection_id.0)?;
    transaction.execute(
        "DELETE FROM library_item WHERE local_id = ?1",
        [collection_id.0],
    )?;
    for (media_id, media_name, facts) in &media_rows {
        transaction.execute(
            "INSERT INTO library_root
                 (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
                  captured_at_ms, modified_at_ms, media_count, total_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?1, ?5, ?6, ?7, 1, ?8)",
            params![
                media_id.0,
                media_name,
                collection.notes,
                serde_json::to_string(&collection.urls)?,
                collection.imported_at_ms,
                collection.captured_at_ms,
                modified_at_ms,
                i64::try_from(facts.size_bytes).map_err(|_| LibraryError::InvalidState(
                    "media size exceeds SQLite range".into()
                ))?
            ],
        )?;
        crate::fts::mark_one(transaction, RootId(media_id.0), modified_at_ms)?;
    }

    for value in Lifecycle::ALL {
        let members = Arc::make_mut(&mut snapshot.lifecycle)
            .entry(value)
            .or_default();
        members.remove(collection_id.0);
        if value == lifecycle {
            *members |= &new_bitmap;
        }
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Lifecycle,
                key_id: value.bitmap_key(),
            },
            members,
        )?;
    }
    for value in Rating::ALL {
        let members = Arc::make_mut(&mut snapshot.ratings)
            .entry(value)
            .or_default();
        members.remove(collection_id.0);
        if value == rating {
            *members |= &new_bitmap;
        }
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Rating,
                key_id: value.bitmap_key(),
            },
            members,
        )?;
    }
    for tag_id in &inherited_tags {
        let roots = Arc::make_mut(&mut snapshot.tags)
            .entry(*tag_id)
            .or_default();
        roots.remove(collection_id.0);
        *roots |= &new_bitmap;
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Tag,
                key_id: tag_id.0,
            },
            roots,
        )?;
    }
    for folder_id in &inherited_folders {
        let before = snapshot.folder_orders[folder_id].as_ref();
        let position = before
            .iter()
            .position(|root| *root == collection_id)
            .ok_or_else(|| {
                LibraryError::InvalidState(format!(
                    "folder {} bitmap and order disagree",
                    folder_id.0
                ))
            })?;
        let mut after = before.clone();
        after.splice(position..=position, new_roots.iter().copied());
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Folder,
            folder_id.0,
            &after.iter().map(|root| root.0).collect::<Vec<_>>(),
        )?;
        Arc::make_mut(&mut snapshot.folder_orders).insert(*folder_id, Arc::new(after.clone()));
        Arc::make_mut(&mut snapshot.folders)
            .insert(*folder_id, after.iter().map(|root| root.0).collect());
    }

    Arc::make_mut(&mut snapshot.collection_orders).remove(&collection_id);
    remove_root_projections(&mut snapshot, &[collection_id.0].into_iter().collect());
    let owners = Arc::make_mut(&mut snapshot.media_owner);
    for (media_id, _, _) in &media_rows {
        owners.insert(media_id.0, RootId(media_id.0));
    }
    for (media_id, _, facts) in &media_rows {
        add_media_root_projection(
            &mut snapshot,
            RootId(media_id.0),
            facts,
            collection.imported_at_ms,
            collection.captured_at_ms,
            modified_at_ms,
            inherited_tags.len() as u64,
            inherited_folders.len() as u64,
            collection.notes.is_some(),
            !collection.urls.is_empty(),
        );
    }
    transaction.execute(
        "INSERT INTO cloud_journal
             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
         VALUES (?1, 'collection.ungroup', ?2, ?3, ?4)",
        params![
            revision as i64,
            crate::bitmap::encode(&new_bitmap)?,
            serde_json::json!({"collection_id": collection_id.0}).to_string(),
            modified_at_ms
        ],
    )?;
    snapshot.revision = revision;
    let mut affected = new_bitmap;
    affected.insert(collection_id.0);
    Ok(UngroupResult {
        roots: new_roots,
        affected,
        snapshot,
    })
}

pub(crate) fn organize(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    request: &GroupRequest,
) -> Result<GroupResult> {
    let ordered_roots = crate::selection::resolve_ordered(transaction, &snapshot, &request.target)?;
    if ordered_roots.len() < 2 {
        return Err(LibraryError::InvalidInput(
            "creating or merging a collection requires at least two roots".into(),
        ));
    }
    if !ordered_roots.contains(&request.cover_root_id) {
        return Err(LibraryError::InvalidInput(
            "collection cover must be one of the selected roots".into(),
        ));
    }
    if let Some(winner) = request.winning_collection_id {
        if !ordered_roots.contains(&winner)
            || !snapshot
                .root_kinds
                .get(&RootKind::Collection)
                .is_some_and(|roots| roots.contains(winner.0))
        {
            return Err(LibraryError::InvalidInput(
                "winning collection must be a selected collection".into(),
            ));
        }
    }

    let selected = ordered_roots
        .iter()
        .map(|root| root.0)
        .collect::<RoaringBitmap>();
    let lifecycle = common_lifecycle(&snapshot, &selected)?;
    let roots = load_roots(transaction, &ordered_roots)?;
    let cover = roots
        .iter()
        .find(|root| root.root_id == request.cover_root_id)
        .ok_or_else(|| LibraryError::InvalidState("cover root disappeared".into()))?;
    let cover_rating = rating_for(&snapshot, cover.root_id)?;
    let cover_facts = load_cover_facts(transaction, cover.cover_media_id)?;

    let mut members = Vec::new();
    let mut seen_members = HashSet::new();
    for root in &ordered_roots {
        if let Some(order) = snapshot.collection_orders.get(root) {
            for media_id in order.iter() {
                if !seen_members.insert(media_id.0) {
                    return Err(LibraryError::InvalidState(format!(
                        "media {} occurs in multiple selected roots",
                        media_id.0
                    )));
                }
                members.push(*media_id);
            }
        } else {
            let media_id = MediaId(root.0);
            if !seen_members.insert(media_id.0) {
                return Err(LibraryError::InvalidState(format!(
                    "media {} occurs more than once",
                    media_id.0
                )));
            }
            members.push(media_id);
        }
    }
    if !members.contains(&cover.cover_media_id) {
        return Err(LibraryError::InvalidState(
            "cover media is not a collection member".into(),
        ));
    }

    let collection_id = request
        .winning_collection_id
        .unwrap_or(RootId(LibraryDatabase::allocate_id(transaction)?));
    let existing_name = roots
        .iter()
        .find(|root| root.root_id == collection_id)
        .map(|root| root.name.as_str());
    let name = request
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .or(existing_name)
        .unwrap_or(&cover.name)
        .trim()
        .to_owned();
    let urls = roots
        .iter()
        .flat_map(|root| root.urls.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let total_size = roots
        .iter()
        .try_fold(0u64, |total, root| total.checked_add(root.total_size_bytes))
        .ok_or_else(|| LibraryError::InvalidState("collection size overflow".into()))?;
    let imported_at = roots
        .iter()
        .map(|root| root.imported_at_ms)
        .max()
        .unwrap_or(request.modified_at_ms);

    if request.winning_collection_id.is_none() {
        transaction.execute(
            "INSERT INTO library_item(local_id, stable_key, item_kind) VALUES (?1, ?2, 2)",
            params![collection_id.0, Uuid::new_v4().to_string()],
        )?;
    }

    for root in &roots {
        if root.root_id != collection_id {
            transaction.execute("DELETE FROM root_fts WHERE root_id = ?1", [root.root_id.0])?;
            transaction.execute(
                "DELETE FROM library_root WHERE root_id = ?1",
                [root.root_id.0],
            )?;
            if root.kind == RootKind::Collection {
                ordering::delete(transaction, OrderOwnerKind::Collection, root.root_id.0)?;
                transaction.execute(
                    "DELETE FROM library_item WHERE local_id = ?1",
                    [root.root_id.0],
                )?;
            }
        }
    }
    transaction.execute(
        "INSERT INTO library_root
             (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
              captured_at_ms, modified_at_ms, media_count, total_size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(root_id) DO UPDATE SET
             name = excluded.name,
             notes = excluded.notes,
             source_urls_json = excluded.source_urls_json,
             cover_media_id = excluded.cover_media_id,
             imported_at_ms = excluded.imported_at_ms,
             captured_at_ms = excluded.captured_at_ms,
             modified_at_ms = excluded.modified_at_ms,
             media_count = excluded.media_count,
             total_size_bytes = excluded.total_size_bytes",
        params![
            collection_id.0,
            name,
            cover.notes,
            serde_json::to_string(&urls)?,
            cover.cover_media_id.0,
            imported_at,
            cover.captured_at_ms,
            request.modified_at_ms,
            members.len() as i64,
            i64::try_from(total_size).map_err(|_| LibraryError::InvalidState(
                "collection size exceeds SQLite range".into()
            ))?
        ],
    )?;
    ordering::replace(
        transaction,
        revision,
        OrderOwnerKind::Collection,
        collection_id.0,
        &members.iter().map(|media| media.0).collect::<Vec<_>>(),
    )?;

    settle_partitions(
        transaction,
        revision,
        &mut snapshot,
        &selected,
        collection_id,
        lifecycle,
        cover_rating,
    )?;
    let tag_count = settle_tags(
        transaction,
        revision,
        &mut snapshot,
        &selected,
        collection_id,
    )?;
    let folder_count = settle_folders(
        transaction,
        revision,
        &mut snapshot,
        &selected,
        collection_id,
    )?;

    let collection_orders = Arc::make_mut(&mut snapshot.collection_orders);
    for root in &roots {
        collection_orders.remove(&root.root_id);
    }
    collection_orders.insert(collection_id, Arc::new(members.clone()));
    let owners = Arc::make_mut(&mut snapshot.media_owner);
    for media in &members {
        owners.insert(media.0, collection_id);
    }
    let has_image = members
        .iter()
        .any(|media_id| snapshot.image_media.contains(media_id.0));

    remove_root_projections(&mut snapshot, &selected);
    add_collection_projection(
        &mut snapshot,
        collection_id,
        &cover_facts,
        members.len() as u64,
        total_size,
        imported_at,
        cover.captured_at_ms,
        request.modified_at_ms,
        tag_count,
        folder_count,
        cover.notes.is_some(),
        !urls.is_empty(),
        has_image,
    );
    refresh_root_mime_projection(transaction, &mut snapshot, collection_id)?;

    crate::fts::mark_one(transaction, collection_id, request.modified_at_ms)?;
    transaction.execute(
        "INSERT INTO cloud_journal
             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
         VALUES (?1, 'collection.organize', ?2, ?3, ?4)",
        params![
            revision as i64,
            crate::bitmap::encode(&selected)?,
            serde_json::json!({"collection_id": collection_id.0}).to_string(),
            request.modified_at_ms
        ],
    )?;
    snapshot.revision = revision;
    let mut affected = selected;
    affected.insert(collection_id.0);
    Ok(GroupResult {
        collection_id,
        affected,
        snapshot,
    })
}

fn load_roots(transaction: &Transaction<'_>, roots: &[RootId]) -> Result<Vec<RootInput>> {
    let mut statement = transaction.prepare_cached(
        "SELECT root.root_id, item.item_kind, root.name, root.notes, root.source_urls_json,
                root.cover_media_id, root.imported_at_ms, root.captured_at_ms, root.total_size_bytes
         FROM library_root root
         JOIN library_item item ON item.local_id = root.root_id
         WHERE root.root_id = ?1",
    )?;
    roots
        .iter()
        .map(|root_id| {
            statement
                .query_row([root_id.0], |row| {
                    Ok(RootInput {
                        root_id: RootId(row.get(0)?),
                        kind: if row.get::<_, u8>(1)? == 1 {
                            RootKind::Media
                        } else {
                            RootKind::Collection
                        },
                        name: row.get(2)?,
                        notes: row.get(3)?,
                        urls: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                        cover_media_id: MediaId(row.get(5)?),
                        imported_at_ms: row.get(6)?,
                        captured_at_ms: row.get(7)?,
                        total_size_bytes: row.get::<_, i64>(8)? as u64,
                    })
                })
                .map_err(Into::into)
        })
        .collect()
}

fn load_cover_facts(transaction: &Transaction<'_>, media_id: MediaId) -> Result<CoverFacts> {
    transaction
        .query_row(
            "SELECT file.mime, file.width, file.height, file.duration_ms, file.palette_json
             FROM media_item media JOIN media_file file ON file.file_id = media.file_id
             WHERE media.media_id = ?1",
            [media_id.0],
            |row| {
                Ok(CoverFacts {
                    mime: row.get(0)?,
                    size_bytes: 0,
                    width: row.get(1)?,
                    height: row.get(2)?,
                    duration_ms: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
                    palette: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                })
            },
        )
        .map_err(Into::into)
}

fn common_lifecycle(snapshot: &ProjectionSnapshot, selected: &RoaringBitmap) -> Result<Lifecycle> {
    Lifecycle::ALL
        .into_iter()
        .find(|value| (snapshot.lifecycle(*value) & selected).len() == selected.len())
        .ok_or_else(|| LibraryError::InvalidInput("selected roots must share one lifecycle".into()))
}

fn rating_for(snapshot: &ProjectionSnapshot, root_id: RootId) -> Result<Rating> {
    Rating::ALL
        .into_iter()
        .find(|value| snapshot.rating(*value).contains(root_id.0))
        .ok_or_else(|| LibraryError::InvalidState(format!("root {root_id} has no rating")))
}

fn settle_partitions(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    selected: &RoaringBitmap,
    collection_id: RootId,
    lifecycle: Lifecycle,
    rating: Rating,
) -> Result<()> {
    for value in Lifecycle::ALL {
        let map = Arc::make_mut(&mut snapshot.lifecycle);
        let members = map.entry(value).or_default();
        *members -= selected;
        if value == lifecycle {
            members.insert(collection_id.0);
        }
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Lifecycle,
                key_id: value.bitmap_key(),
            },
            members,
        )?;
    }
    for value in Rating::ALL {
        let map = Arc::make_mut(&mut snapshot.ratings);
        let members = map.entry(value).or_default();
        *members -= selected;
        if value == rating {
            members.insert(collection_id.0);
        }
        bitmap::replace(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Rating,
                key_id: value.bitmap_key(),
            },
            members,
        )?;
    }
    Ok(())
}

fn settle_tags(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    selected: &RoaringBitmap,
    collection_id: RootId,
) -> Result<u64> {
    let mut count = 0;
    for (tag_id, members) in Arc::make_mut(&mut snapshot.tags) {
        if !(&*members & selected).is_empty() {
            *members -= selected;
            members.insert(collection_id.0);
            count += 1;
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                },
                members,
            )?;
        }
    }
    Ok(count)
}

fn settle_folders(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    selected: &RoaringBitmap,
    collection_id: RootId,
) -> Result<u64> {
    let mut count = 0;
    let folder_ids = snapshot.folder_orders.keys().copied().collect::<Vec<_>>();
    for folder_id in folder_ids {
        let before = snapshot.folder_orders[&folder_id].as_ref();
        let positions = before
            .iter()
            .enumerate()
            .filter_map(|(index, root)| selected.contains(root.0).then_some(index))
            .collect::<Vec<_>>();
        if positions.is_empty() {
            continue;
        }
        let insertion = *positions.first().expect("positions is not empty");
        let mut after = before
            .iter()
            .copied()
            .filter(|root| !selected.contains(root.0))
            .collect::<Vec<_>>();
        let insertion = insertion.min(after.len());
        after.insert(insertion, collection_id);
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Folder,
            folder_id.0,
            &after.iter().map(|root| root.0).collect::<Vec<_>>(),
        )?;
        Arc::make_mut(&mut snapshot.folder_orders).insert(folder_id, Arc::new(after.clone()));
        Arc::make_mut(&mut snapshot.folders)
            .insert(folder_id, after.iter().map(|root| root.0).collect());
        count += 1;
    }
    Ok(count)
}

pub(crate) fn remove_root_projections(snapshot: &mut ProjectionSnapshot, roots: &RoaringBitmap) {
    *Arc::make_mut(&mut snapshot.roots_with_images) -= roots;
    for members in Arc::make_mut(&mut snapshot.root_kinds).values_mut() {
        *members -= roots;
    }
    for members in Arc::make_mut(&mut snapshot.mime).values_mut() {
        *members -= roots;
    }
    for members in Arc::make_mut(&mut snapshot.mime_family).values_mut() {
        *members -= roots;
    }
    for members in Arc::make_mut(&mut snapshot.color_cells).values_mut() {
        *members -= roots;
    }
    for root_id in roots {
        Arc::make_mut(&mut snapshot.cover_palettes).remove(root_id);
        Arc::make_mut(&mut snapshot.tag_count).remove(root_id);
        Arc::make_mut(&mut snapshot.folder_count).remove(root_id);
        Arc::make_mut(&mut snapshot.total_bytes).remove(root_id);
        Arc::make_mut(&mut snapshot.media_count).remove(root_id);
        Arc::make_mut(&mut snapshot.width).remove(root_id);
        Arc::make_mut(&mut snapshot.height).remove(root_id);
        Arc::make_mut(&mut snapshot.duration).remove(root_id);
        Arc::make_mut(&mut snapshot.imported_at).remove(root_id);
        Arc::make_mut(&mut snapshot.captured_at).remove(root_id);
        Arc::make_mut(&mut snapshot.modified_at).remove(root_id);
        Arc::make_mut(&mut snapshot.notes_present).remove(root_id);
        Arc::make_mut(&mut snapshot.urls_present).remove(root_id);
    }
}

#[allow(clippy::too_many_arguments)]
fn add_collection_projection(
    snapshot: &mut ProjectionSnapshot,
    collection_id: RootId,
    cover: &CoverFacts,
    media_count: u64,
    total_size: u64,
    imported_at: i64,
    captured_at: Option<i64>,
    modified_at: i64,
    tag_count: u64,
    folder_count: u64,
    has_notes: bool,
    has_urls: bool,
    has_image: bool,
) {
    let id = collection_id.0;
    Arc::make_mut(&mut snapshot.root_kinds)
        .entry(RootKind::Collection)
        .or_default()
        .insert(id);
    add_cover_projection(snapshot, collection_id, cover);
    Arc::make_mut(&mut snapshot.tag_count).insert(id, tag_count);
    Arc::make_mut(&mut snapshot.folder_count).insert(id, folder_count);
    Arc::make_mut(&mut snapshot.total_bytes).insert(id, total_size);
    Arc::make_mut(&mut snapshot.media_count).insert(id, media_count);
    Arc::make_mut(&mut snapshot.imported_at).insert(id, imported_at.max(0) as u64);
    if let Some(value) = captured_at {
        Arc::make_mut(&mut snapshot.captured_at).insert(id, value.max(0) as u64);
    }
    Arc::make_mut(&mut snapshot.modified_at).insert(id, modified_at.max(0) as u64);
    if has_notes {
        Arc::make_mut(&mut snapshot.notes_present).insert(id);
    }
    if has_urls {
        Arc::make_mut(&mut snapshot.urls_present).insert(id);
    }
    if has_image {
        Arc::make_mut(&mut snapshot.roots_with_images).insert(id);
    }
}

fn remove_cover_projection(snapshot: &mut ProjectionSnapshot, root_id: RootId) {
    for roots in Arc::make_mut(&mut snapshot.color_cells).values_mut() {
        roots.remove(root_id.0);
    }
    Arc::make_mut(&mut snapshot.cover_palettes).remove(root_id.0);
    Arc::make_mut(&mut snapshot.width).remove(root_id.0);
    Arc::make_mut(&mut snapshot.height).remove(root_id.0);
    Arc::make_mut(&mut snapshot.duration).remove(root_id.0);
}

fn add_cover_projection(snapshot: &mut ProjectionSnapshot, root_id: RootId, cover: &CoverFacts) {
    let id = root_id.0;
    for color in &cover.palette {
        Arc::make_mut(&mut snapshot.color_cells)
            .entry(color_cell(color))
            .or_default()
            .insert(id);
    }
    Arc::make_mut(&mut snapshot.cover_palettes).insert(root_id.0, Arc::new(cover.palette.clone()));
    if let Some(value) = cover.width {
        Arc::make_mut(&mut snapshot.width).insert(id, value as u64);
    }
    if let Some(value) = cover.height {
        Arc::make_mut(&mut snapshot.height).insert(id, value as u64);
    }
    if let Some(value) = cover.duration_ms {
        Arc::make_mut(&mut snapshot.duration).insert(id, value);
    }
}

pub(crate) fn refresh_root_mime_projection(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    root_id: RootId,
) -> Result<()> {
    for roots in Arc::make_mut(&mut snapshot.mime).values_mut() {
        roots.remove(root_id.0);
    }
    for roots in Arc::make_mut(&mut snapshot.mime_family).values_mut() {
        roots.remove(root_id.0);
    }

    let media_ids = snapshot.collection_orders.get(&root_id).map_or_else(
        || vec![MediaId(root_id.0)],
        |members| members.as_ref().clone(),
    );
    let mut mime_values = std::collections::BTreeSet::new();
    for chunk in media_ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT file.mime
             FROM media_item media
             JOIN media_file file ON file.file_id = media.file_id
             WHERE media.media_id IN ({placeholders})"
        );
        let values = chunk.iter().map(|media_id| media_id.0).collect::<Vec<_>>();
        let rows = transaction
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(values), |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mime_values.extend(rows);
    }
    for mime in mime_values {
        Arc::make_mut(&mut snapshot.mime)
            .entry(mime.clone())
            .or_default()
            .insert(root_id.0);
        Arc::make_mut(&mut snapshot.mime_family)
            .entry(
                mime.split_once('/')
                    .map_or(mime.as_str(), |value| value.0)
                    .to_owned(),
            )
            .or_default()
            .insert(root_id.0);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_media_root_projection(
    snapshot: &mut ProjectionSnapshot,
    root_id: RootId,
    facts: &CoverFacts,
    imported_at: i64,
    captured_at: Option<i64>,
    modified_at: i64,
    tag_count: u64,
    folder_count: u64,
    has_notes: bool,
    has_urls: bool,
) {
    let id = root_id.0;
    Arc::make_mut(&mut snapshot.root_kinds)
        .entry(RootKind::Media)
        .or_default()
        .insert(id);
    Arc::make_mut(&mut snapshot.mime)
        .entry(facts.mime.clone())
        .or_default()
        .insert(id);
    Arc::make_mut(&mut snapshot.mime_family)
        .entry(
            facts
                .mime
                .split_once('/')
                .map_or(facts.mime.as_str(), |value| value.0)
                .to_owned(),
        )
        .or_default()
        .insert(id);
    for color in &facts.palette {
        Arc::make_mut(&mut snapshot.color_cells)
            .entry(color_cell(color))
            .or_default()
            .insert(id);
    }
    Arc::make_mut(&mut snapshot.cover_palettes).insert(root_id.0, Arc::new(facts.palette.clone()));
    Arc::make_mut(&mut snapshot.tag_count).insert(id, tag_count);
    Arc::make_mut(&mut snapshot.folder_count).insert(id, folder_count);
    Arc::make_mut(&mut snapshot.total_bytes).insert(id, facts.size_bytes);
    Arc::make_mut(&mut snapshot.media_count).insert(id, 1);
    Arc::make_mut(&mut snapshot.imported_at).insert(id, imported_at.max(0) as u64);
    if let Some(value) = captured_at {
        Arc::make_mut(&mut snapshot.captured_at).insert(id, value.max(0) as u64);
    }
    Arc::make_mut(&mut snapshot.modified_at).insert(id, modified_at.max(0) as u64);
    if let Some(value) = facts.width {
        Arc::make_mut(&mut snapshot.width).insert(id, value as u64);
    }
    if let Some(value) = facts.height {
        Arc::make_mut(&mut snapshot.height).insert(id, value as u64);
    }
    if let Some(value) = facts.duration_ms {
        Arc::make_mut(&mut snapshot.duration).insert(id, value);
    }
    if has_notes {
        Arc::make_mut(&mut snapshot.notes_present).insert(id);
    }
    if has_urls {
        Arc::make_mut(&mut snapshot.urls_present).insert(id);
    }
    if facts.mime.starts_with("image/") {
        Arc::make_mut(&mut snapshot.roots_with_images).insert(id);
    }
}
