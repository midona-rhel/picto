use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{params, OptionalExtension, Transaction};
use uuid::Uuid;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::LibraryDatabase;
use crate::fts;
use crate::model::{FolderId, MediaId, PreparedImport, Rating, RootId, RootKind, TagId};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{color_cell, ProjectionSnapshot};
use crate::{LibraryError, Result};

// One batch imports at most 48 items. On the computer that releases are
// measured on, a batch of 48 items with many tags is written in less time
// than one frame takes, and it costs nearly as little for each item as a
// batch of 64 does.
pub const MAX_INGEST_BATCH: usize = 48;

pub(crate) struct IngestResult {
    pub root_id: RootId,
    pub created_root: bool,
    pub snapshot: ProjectionSnapshot,
    pub resources: Vec<String>,
    pub bitmap_keys: Vec<BitmapKey>,
    pub folder_ids: Vec<FolderId>,
    pub affected_roots: RoaringBitmap,
    /// The tags that this import added to a root that already existed.
    pub added_tags: Vec<(RootId, TagId)>,
    /// The folders that a merge of another library put a root into that
    /// already existed.
    pub placed: Vec<(RootId, FolderId)>,
    /// The rating that a merge of another library gave a root that already
    /// existed and had no rating.
    pub rated: Option<(RootId, Rating)>,
    /// The name, the notes and the source links of a root that already
    /// existed, before and after this import, when the import changed one
    /// of them. It is filled in for a task and in a library that records
    /// items.
    pub text: Option<crate::history::SemanticChange>,
}

/// What an import gives an item that the library already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Held {
    /// The item gets the import's tags and source. It gets the import's
    /// notes when it has none, and the import's name when its own name is a
    /// generated one and the import's is not.
    Refresh,
    /// The import is an item of another library that is merged into this
    /// one. The item keeps its name, its dates and its status. It gets the
    /// import's tags, folders and source links, the import's notes when it
    /// has none, and the import's rating when it has none.
    ///
    /// When the library does not hold the item yet, the new item gets
    /// `modified_at_ms`, the time of the item's last change in the other
    /// library, as the time of its last change. An ordinary import gives a
    /// new item the time of its import.
    Merge { modified_at_ms: i64 },
    /// The import is an item that another device of this library imported
    /// and that cloud sync merges. The item that the library holds gets
    /// nothing. When it is held under another stable key, the import's key
    /// becomes an alias of it. A new item is created as an ordinary import
    /// creates it, and no entry is recorded for it.
    Shared,
}

/// Import one file. One file is one media item: an import whose source item,
/// stable key or file the library already holds resolves to that item
/// instead of creating another, and `held` says what that item gets from the
/// import. Collection imports combine overlapping collections, including
/// their tags.
pub(crate) fn insert_one(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    input: &PreparedImport,
    task: Option<crate::TaskId>,
    held: Held,
) -> Result<IngestResult> {
    if let Some(existing) = existing_import(transaction, &snapshot, input)? {
        if held == Held::Shared {
            transaction
                .prepare_cached(
                    "INSERT OR IGNORE INTO library_item_alias(stable_key, local_id)
                     SELECT ?1, ?2
                     WHERE NOT EXISTS (SELECT 1 FROM library_item WHERE stable_key = ?1)",
                )?
                .execute(params![input.stable_key, existing.media_id.0])?;
            snapshot.revision = revision;
            return Ok(IngestResult {
                root_id: existing.root_id,
                created_root: false,
                snapshot,
                resources: Vec::new(),
                bitmap_keys: Vec::new(),
                folder_ids: Vec::new(),
                affected_roots: RoaringBitmap::new(),
                added_tags: Vec::new(),
                placed: Vec::new(),
                rated: None,
                text: None,
            });
        }
        let recording = crate::mutation::records_items(transaction)?;
        let before = (task.is_some() || recording)
            .then(|| reused_text(transaction, existing))
            .transpose()?;
        let mut affected_roots =
            refresh_existing_file(transaction, &mut snapshot, existing.media_id, input, held)?;
        let mut text = None;
        if let Some((root_before, media_before)) = before {
            let (root_after, media_after) = reused_text(transaction, existing)?;
            use crate::mutation::tasks::{record_task_undo, TaskUndo};
            if root_before != root_after {
                let (before, after) = (Arc::new(root_before), Arc::new(root_after));
                if let Some(task) = task {
                    record_task_undo(transaction, task, TaskUndo::RootText { before: before.clone(), after: after.clone() })?;
                }
                text = Some(crate::history::SemanticChange::RootText { before, after });
            }
            if let (Some(task), true) = (task, media_before != media_after) {
                record_task_undo(transaction, task, TaskUndo::MediaText { media_id: existing.media_id, before: media_before, after: media_after })?;
            }
        }
        let tag_roots = RoaringBitmap::from_iter([existing.root_id.0]);
        let mut bitmap_keys = Vec::new();
        let mut added_tags = Vec::new();
        for raw_name in &input.tags {
            let name = canonical_tag_name(raw_name)?;
            let tag_id = if let Some(tag_id) = snapshot.tag_ids_by_name.get(&name).copied() {
                tag_id
            } else {
                let tag_id = ensure_tag(transaction, &name)?;
                Arc::make_mut(&mut snapshot.tag_ids_by_name).insert(name, tag_id);
                tag_id
            };
            let added_roots = {
                let roots = Arc::make_mut(&mut snapshot.tags).entry(tag_id).or_default();
                tag_roots
                    .iter()
                    .filter(|root_id| roots.insert(*root_id))
                    .collect::<RoaringBitmap>()
            };
            added_tags.extend(added_roots.iter().map(|root| (RootId(root), tag_id)));
            if !added_roots.is_empty() {
                bitmap_keys.push(BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                });
                let counts = Arc::make_mut(&mut snapshot.tag_count);
                for root_id in added_roots {
                    counts.insert(
                        root_id,
                        counts.value(root_id).unwrap_or(0).saturating_add(1),
                    );
                    affected_roots.insert(root_id);
                }
            }
        }
        if let Some(source) = &input.source_identity {
            transaction
                .prepare_cached(
                    "INSERT INTO source_provenance
                 (source_key, source_item_key, media_id, source_text)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(source_key, source_item_key, media_id) DO UPDATE SET
                 source_text = excluded.source_text",
                )?
                .execute(params![
                    source.source_key,
                    source.source_item_key,
                    existing.media_id.0,
                    source.source_text
                ])?;
        }
        let mut resources = vec!["roots".to_owned(), "tags".to_owned()];
        let mut placed = Vec::new();
        let mut rated = None;
        if matches!(held, Held::Merge { .. }) {
            let root_id = existing.root_id;
            let (folders, auto_tags) =
                place_in_folders(transaction, &mut snapshot, root_id, &input.folders)?;
            if !folders.is_empty() {
                let counts = Arc::make_mut(&mut snapshot.folder_count);
                counts.insert(
                    root_id.0,
                    counts.value(root_id.0).unwrap_or(0) + folders.len() as u64,
                );
                resources.extend(["folders".to_owned(), "sidebar".to_owned()]);
                affected_roots.insert(root_id.0);
            }
            if !auto_tags.is_empty() {
                let counts = Arc::make_mut(&mut snapshot.tag_count);
                counts.insert(
                    root_id.0,
                    counts.value(root_id.0).unwrap_or(0) + auto_tags.len() as u64,
                );
                affected_roots.insert(root_id.0);
            }
            for tag_id in auto_tags {
                bitmap_keys.push(BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                });
                added_tags.push((root_id, tag_id));
            }
            placed.extend(folders.into_iter().map(|folder_id| (root_id, folder_id)));
            if input.rating != Rating::Unrated && snapshot.rating(Rating::Unrated).contains(root_id.0)
            {
                let ratings = Arc::make_mut(&mut snapshot.ratings);
                ratings.entry(Rating::Unrated).or_default().remove(root_id.0);
                ratings.entry(input.rating).or_default().insert(root_id.0);
                for rating in [Rating::Unrated, input.rating] {
                    bitmap_keys.push(BitmapKey {
                        domain: BitmapDomain::Rating,
                        key_id: rating.bitmap_key(),
                    });
                }
                resources.push("ratings".to_owned());
                affected_roots.insert(root_id.0);
                rated = Some((root_id, input.rating));
            }
        }
        snapshot.revision = revision;
        return Ok(IngestResult {
            root_id: existing.root_id,
            created_root: false,
            snapshot,
            resources,
            bitmap_keys,
            folder_ids: placed.iter().map(|(_, folder_id)| *folder_id).collect(),
            affected_roots,
            added_tags,
            placed,
            rated,
            text,
        });
    }
    if let Some(deleted_at_ms) = transaction
        .prepare_cached("SELECT deleted_at_ms FROM deletion_tombstone WHERE stable_key = ?1")?
        .query_row([&input.stable_key], |row| row.get::<_, i64>(0))
        .optional()?
    {
        // A merged import is refused by every tombstone: the merge removed
        // the tombstone that the item's entry is later than, so a tombstone
        // that is here now is a later deletion.
        if input.imported_at_ms <= deleted_at_ms || held == Held::Shared {
            return Err(LibraryError::ImportDeleted);
        }
        // The import is later than the deletion, so the tombstone goes, and
        // the other devices learn that the item was added again.
        crate::mutation::remove_item_tombstones(
            transaction,
            std::slice::from_ref(&input.stable_key),
        )?;
    }

    // The file is imported again, so its original in the cloud folder is no
    // longer waiting to be removed. Originals are stored there under their
    // content hash, and the cloud worker finds the one that is still there
    // and does not upload the file a second time.
    transaction
        .prepare_cached(
            "DELETE FROM cloud_deleted_original
             WHERE file_name = ?1 OR file_name LIKE ?1 || '.%'",
        )?
        .execute([&input.facts.content_hash])?;

    let existing_file = transaction
        .prepare_cached(
            "SELECT file_id, perceptual_hash IS NOT NULL
             FROM media_file WHERE content_hash = ?1",
        )?
        .query_row([&input.facts.content_hash], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, bool>(1)?))
        })
        .optional()?;
    let (file_id, physical_has_perceptual_hash) =
        if let Some((file_id, has_hash)) = existing_file {
            transaction.prepare_cached(
            "UPDATE media_file SET file_path = ?2 WHERE file_id = ?1 AND file_path IS NOT ?2",
        )?.execute(params![file_id, input.file_path])?;
            (file_id, has_hash)
        } else {
            let file_id = LibraryDatabase::allocate_id(transaction)?;
            transaction
                .prepare_cached(
                    "INSERT INTO media_file
                 (file_id, content_hash, file_path, mime, size_bytes, width, height,
                  duration_ms, frame_count, perceptual_hash, palette_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )?
                .execute(params![
                    file_id,
                    input.facts.content_hash,
                    input.file_path,
                    input.facts.mime,
                    sqlite_i64(input.facts.size_bytes, "file size")?,
                    input.facts.width,
                    input.facts.height,
                    input
                        .facts
                        .duration_ms
                        .map(|value| sqlite_i64(value, "duration"))
                        .transpose()?,
                    input.facts.frame_count,
                    input.facts.perceptual_hash,
                    serde_json::to_string(&input.facts.palette)?,
                ])?;
            transaction
                .prepare_cached(
                    "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
                     SELECT ?1, 'queued', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     FROM cloud_state WHERE singleton = 1 AND provider IS NOT NULL",
                )?
                .execute([&input.facts.content_hash])?;
            (file_id, input.facts.perceptual_hash.is_some())
        };

    let root_id = RootId(LibraryDatabase::allocate_id(transaction)?);
    let media_id = MediaId(root_id.0);
    transaction
        .prepare_cached(
            "INSERT INTO library_item(local_id, stable_key, item_kind) VALUES (?1, ?2, 1)",
        )?
        .execute(params![root_id.0, input.stable_key])?;
    transaction
        .prepare_cached(
            "INSERT INTO media_item(media_id, media_name, media_notes, file_id)
         VALUES (?1, ?2, ?3, ?4)",
        )?
        .execute(params![media_id.0, input.media_name, input.notes, file_id])?;
    enqueue_file_work(
        transaction,
        file_id,
        &input.facts.content_hash,
        "thumbnail",
        input.imported_at_ms,
    )?;
    if input.facts.palette.is_empty() {
        enqueue_file_work(
            transaction,
            file_id,
            &input.facts.content_hash,
            "dominant_colors",
            input.imported_at_ms,
        )?;
    }
    if !physical_has_perceptual_hash {
        enqueue_file_work(
            transaction,
            file_id,
            &input.facts.content_hash,
            "perceptual_hash",
            input.imported_at_ms,
        )?;
    }
    let modified_at_ms = match held {
        Held::Merge { modified_at_ms } => modified_at_ms,
        Held::Refresh | Held::Shared => input.imported_at_ms,
    };
    transaction
        .prepare_cached(
            "INSERT INTO library_root
             (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
              captured_at_ms, modified_at_ms, media_count, total_size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?9, 1, ?8)",
        )?
        .execute(params![
            root_id.0,
            input.media_name,
            input.notes,
            serde_json::to_string(&input.source_urls)?,
            media_id.0,
            input.imported_at_ms,
            input.captured_at_ms,
            sqlite_i64(input.facts.size_bytes, "root size")?,
            modified_at_ms,
        ])?;
    if let Some(source) = &input.source_identity {
        transaction
            .prepare_cached(
                "INSERT INTO source_provenance
                 (source_key, source_item_key, media_id, source_text)
             VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![
                source.source_key,
                source.source_item_key,
                media_id.0,
                source.source_text
            ])?;
    }

    let lifecycle_key = BitmapKey {
        domain: BitmapDomain::Lifecycle,
        key_id: input.lifecycle.bitmap_key(),
    };
    let lifecycle = Arc::make_mut(&mut snapshot.lifecycle);
    lifecycle
        .entry(input.lifecycle)
        .or_default()
        .insert(root_id.0);

    let rating_key = BitmapKey {
        domain: BitmapDomain::Rating,
        key_id: input.rating.bitmap_key(),
    };
    let ratings = Arc::make_mut(&mut snapshot.ratings);
    ratings.entry(input.rating).or_default().insert(root_id.0);

    let mut assigned_tags = 0u64;
    let mut bitmap_keys = vec![lifecycle_key, rating_key];
    for raw_name in &input.tags {
        let name = canonical_tag_name(raw_name)?;
        let tag_id = if let Some(tag_id) = snapshot.tag_ids_by_name.get(&name).copied() {
            tag_id
        } else {
            let tag_id = ensure_tag(transaction, &name)?;
            Arc::make_mut(&mut snapshot.tag_ids_by_name).insert(name, tag_id);
            tag_id
        };
        let tags = Arc::make_mut(&mut snapshot.tags);
        let members = tags.entry(tag_id).or_default();
        if members.insert(root_id.0) {
            assigned_tags += 1;
        }
        bitmap_keys.push(BitmapKey {
            domain: BitmapDomain::Tag,
            key_id: tag_id.0,
        });
    }

    let (placed, auto_tags) = place_in_folders(transaction, &mut snapshot, root_id, &input.folders)?;
    let assigned_folders = placed.len() as u64;
    assigned_tags += auto_tags.len() as u64;
    bitmap_keys.extend(auto_tags.into_iter().map(|tag_id| BitmapKey {
        domain: BitmapDomain::Tag,
        key_id: tag_id.0,
    }));

    Arc::make_mut(&mut snapshot.root_kinds)
        .entry(RootKind::Media)
        .or_default()
        .insert(root_id.0);
    Arc::make_mut(&mut snapshot.mime)
        .entry(input.facts.mime.clone())
        .or_default()
        .insert(root_id.0);
    Arc::make_mut(&mut snapshot.mime_family)
        .entry(mime_family(&input.facts.mime).to_owned())
        .or_default()
        .insert(root_id.0);
    for color in &input.facts.palette {
        Arc::make_mut(&mut snapshot.color_cells)
            .entry(color_cell(color))
            .or_default()
            .insert(root_id.0);
    }
    Arc::make_mut(&mut snapshot.cover_palettes)
        .insert(root_id.0, Arc::new(input.facts.palette.clone()));
    let owners = Arc::make_mut(&mut snapshot.media_owner);
    owners.insert(media_id.0, root_id);
    if input.facts.mime.starts_with("image/") {
        Arc::make_mut(&mut snapshot.image_media).insert(media_id.0);
        Arc::make_mut(&mut snapshot.roots_with_images).insert(root_id.0);
    }

    Arc::make_mut(&mut snapshot.tag_count).insert(root_id.0, assigned_tags);
    Arc::make_mut(&mut snapshot.folder_count).insert(root_id.0, assigned_folders);
    Arc::make_mut(&mut snapshot.total_bytes).insert(root_id.0, input.facts.size_bytes);
    Arc::make_mut(&mut snapshot.media_count).insert(root_id.0, 1);
    Arc::make_mut(&mut snapshot.imported_at).insert(root_id.0, input.imported_at_ms);
    if let Some(value) = input.captured_at_ms {
        Arc::make_mut(&mut snapshot.captured_at).insert(root_id.0, value);
    }
    Arc::make_mut(&mut snapshot.modified_at).insert(root_id.0, modified_at_ms.max(0) as u64);
    if let Some(value) = input.facts.width {
        Arc::make_mut(&mut snapshot.width).insert(root_id.0, value as u64);
    }
    if let Some(value) = input.facts.height {
        Arc::make_mut(&mut snapshot.height).insert(root_id.0, value as u64);
    }
    if let Some(value) = input.facts.duration_ms {
        Arc::make_mut(&mut snapshot.duration).insert(root_id.0, value);
    }
    if !input.source_urls.is_empty() {
        Arc::make_mut(&mut snapshot.urls_present).insert(root_id.0);
    }
    if input
        .notes
        .as_deref()
        .is_some_and(|notes| !notes.is_empty())
    {
        Arc::make_mut(&mut snapshot.notes_present).insert(root_id.0);
    }
    let affected_roots = RoaringBitmap::from_iter([root_id.0]);

    Arc::make_mut(&mut snapshot.name_index).insert(root_id.0, &input.media_name);
    fts::mark_one(transaction, root_id, input.imported_at_ms)?;
    snapshot.revision = revision;
    Ok(IngestResult {
        root_id,
        created_root: true,
        snapshot,
        resources: vec![
            "roots".into(),
            format!("lifecycle:{:?}", input.lifecycle).to_lowercase(),
            "sidebar".into(),
            "tags".into(),
            "folders".into(),
        ],
        bitmap_keys,
        folder_ids: input.folders.clone(),
        affected_roots,
        added_tags: Vec::new(),
        placed: Vec::new(),
        rated: None,
        text: None,
    })
}

/// Puts `root_id` at the end of each of `folders` that it is not in yet, and
/// gives it the tags that those folders give their items. Returns the folders
/// that the root was put into and the tags that it did not have before. The
/// caller adds both to the root's counts and stores the changed folder
/// orders and tag bitmaps.
fn place_in_folders(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    root_id: RootId,
    folders: &[FolderId],
) -> Result<(Vec<FolderId>, Vec<TagId>)> {
    let mut placed = Vec::new();
    let mut added_tags = Vec::new();
    for folder_id in folders {
        if snapshot
            .folders
            .get(folder_id)
            .is_some_and(|roots| roots.contains(root_id.0))
        {
            continue;
        }
        let auto_tags = folder_auto_tags(transaction, *folder_id)?;
        for tag_id in auto_tags {
            let tag_id = TagId(tag_id);
            let exists = transaction
                .prepare_cached("SELECT EXISTS(SELECT 1 FROM tag_definition WHERE tag_id = ?1)")?
                .query_row([tag_id.0], |row| row.get::<_, bool>(0))?;
            if !exists {
                return Err(LibraryError::InvalidState(format!(
                    "folder {} references missing auto-tag {}",
                    folder_id.0, tag_id.0
                )));
            }
            if Arc::make_mut(&mut snapshot.tags)
                .entry(tag_id)
                .or_default()
                .insert(root_id.0)
            {
                added_tags.push(tag_id);
            }
        }
        let folder_orders = Arc::make_mut(&mut snapshot.folder_orders);
        let order = folder_orders
            .entry(*folder_id)
            .or_insert_with(|| Arc::new(Vec::new()));
        Arc::make_mut(order).push(root_id);
        Arc::make_mut(&mut snapshot.folders)
            .entry(*folder_id)
            .or_default()
            .insert(root_id.0);
        placed.push(*folder_id);
    }
    Ok((placed, added_tags))
}

fn enqueue_file_work(
    transaction: &Transaction<'_>,
    file_id: u32,
    content_hash: &str,
    work_type: &str,
    now_ms: i64,
) -> Result<()> {
    let available_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .ok_or_else(|| LibraryError::InvalidInput("ingest timestamp is outside range".into()))?
        .to_rfc3339();
    let kind = match work_type {
        "thumbnail" => crate::model::MediaWorkKind::Thumbnail,
        "dominant_colors" => crate::model::MediaWorkKind::DominantColors,
        "perceptual_hash" => crate::model::MediaWorkKind::PerceptualHash,
        _ => {
            return Err(LibraryError::InvalidState(format!(
                "unsupported ingest work kind {work_type}"
            )))
        }
    };
    transaction
        .prepare_cached(
            "INSERT INTO work_item
             (file_id, file_hash, work_type, status, priority, attempt_count,
              available_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', ?5, 0, ?4, ?4, ?4)
         ON CONFLICT DO UPDATE SET
             status = 'pending', priority = excluded.priority, attempt_count = 0,
             available_at = excluded.available_at, last_error = NULL,
             updated_at = excluded.updated_at
         WHERE work_item.status = 'failed'",
        )?
        .execute(params![
            file_id,
            content_hash,
            work_type,
            available_at,
            kind.priority()
        ])?;
    Ok(())
}

pub(crate) fn enqueue_ai_tag_roots(
    transaction: &Transaction<'_>,
    root_ids: impl IntoIterator<Item = RootId>,
    now_ms: i64,
) -> Result<()> {
    let available_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .ok_or_else(|| LibraryError::InvalidInput("ingest timestamp is outside range".into()))?
        .to_rfc3339();
    let mut unique = root_ids
        .into_iter()
        .map(|root_id| root_id.0)
        .collect::<Vec<_>>();
    unique.sort_unstable();
    unique.dedup();
    let mut insert = transaction.prepare_cached(
        "INSERT INTO work_item
             (root_id, work_type, status, priority, attempt_count,
              available_at, created_at, updated_at)
         VALUES (?1, 'ai_tag', 'pending', ?2, 0, ?3, ?3, ?3)
         ON CONFLICT(root_id, work_type) WHERE root_id IS NOT NULL
         DO NOTHING",
    )?;
    for root_id in unique {
        insert.execute(params![
            root_id,
            crate::model::MediaWorkKind::AiTag.priority(),
            available_at
        ])?;
    }
    Ok(())
}

type ReusedText = (Vec<crate::history::RootTextState>, (String, Option<String>));

fn reused_text(transaction: &Transaction<'_>, existing: ExistingImport) -> Result<ReusedText> {
    let roots = crate::mutation::load_root_text(transaction, &RoaringBitmap::from_iter([existing.root_id.0]))?;
    let media = transaction.query_row("SELECT media_name, media_notes FROM media_item WHERE media_id = ?1", [existing.media_id.0], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok((roots, media))

}

fn refresh_existing_file(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    media_id: MediaId,
    input: &PreparedImport,
    held: Held,
) -> Result<RoaringBitmap> {
    let (file_id, has_palette, has_perceptual_hash) = transaction.query_row(
        "SELECT file.file_id, file.palette_json != '[]', file.perceptual_hash IS NOT NULL
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id
         WHERE media.media_id = ?1",
        [media_id.0],
        |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, bool>(2)?,
            ))
        },
    )?;
    transaction.execute(
        "UPDATE media_file SET file_path = ?2 WHERE file_id = ?1 AND file_path IS NOT ?2",
        params![file_id, input.file_path],
    )?;
    let mut affected_roots = RoaringBitmap::new();
    let owner = snapshot
        .media_owner
        .get(media_id.0)
        .copied()
        .unwrap_or(RootId(media_id.0));
    let standalone = owner.0 == media_id.0
        && snapshot
            .root_kinds
            .get(&RootKind::Media)
            .is_some_and(|roots| roots.contains(owner.0));
    let (current_name, current_note) = transaction.query_row(
        "SELECT media_name, media_notes FROM media_item WHERE media_id = ?1",
        [media_id.0],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;
    let current_root_name = standalone
        .then(|| {
            transaction.query_row(
                "SELECT name FROM library_root WHERE root_id = ?1",
                [owner.0],
                |row| row.get::<_, String>(0),
            )
        })
        .transpose()?;
    let incoming_note = input
        .notes
        .as_deref()
        .filter(|note| !note.trim().is_empty());
    let baseline_name = current_root_name
        .as_deref()
        .map_or(current_name.as_str(), |root_name| {
            crate::name_quality::preferred(&current_name, root_name)
        });
    let preferred_name = crate::name_quality::preferred(baseline_name, &input.media_name);
    // An item that is merged in from another library never renames the item
    // that this library holds.
    let replace_name = held == Held::Refresh && preferred_name != current_name;
    let replace_root_name = held == Held::Refresh
        && current_root_name
            .as_deref()
            .is_some_and(|root_name| root_name != preferred_name);
    let replace_note = current_note
        .as_deref()
        .is_none_or(|note| note.trim().is_empty())
        && incoming_note.is_some();
    if replace_name || replace_note {
        transaction.execute(
            "UPDATE media_item
             SET media_name = CASE WHEN ?2 THEN ?3 ELSE media_name END,
                 media_notes = CASE WHEN ?4 THEN ?5 ELSE media_notes END
             WHERE media_id = ?1",
            params![
                media_id.0,
                replace_name,
                preferred_name,
                replace_note,
                incoming_note
            ],
        )?;
    }

    affected_roots.insert(owner.0);
    if standalone {
        if replace_root_name {
            transaction.execute(
                "UPDATE library_root SET name = ?2 WHERE root_id = ?1",
                params![owner.0, preferred_name],
            )?;
            Arc::make_mut(&mut snapshot.name_index).insert(owner.0, preferred_name);
        }
        if replace_note {
            transaction.execute(
                "UPDATE library_root SET notes = ?2 WHERE root_id = ?1",
                params![owner.0, incoming_note],
            )?;
            Arc::make_mut(&mut snapshot.notes_present).insert(owner.0);
        }
        if replace_name || replace_root_name || replace_note {
            fts::mark_one(transaction, owner, input.imported_at_ms)?;
        }
    }
    if matches!(held, Held::Merge { .. }) && !input.source_urls.is_empty() {
        let stored = transaction.query_row(
            "SELECT source_urls_json FROM library_root WHERE root_id = ?1",
            [owner.0],
            |row| row.get::<_, String>(0),
        )?;
        let mut urls = serde_json::from_str::<Vec<String>>(&stored)?;
        let held_before = urls.len();
        for url in &input.source_urls {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        if urls.len() != held_before {
            transaction.execute(
                "UPDATE library_root SET source_urls_json = ?2 WHERE root_id = ?1",
                params![owner.0, serde_json::to_string(&urls)?],
            )?;
            Arc::make_mut(&mut snapshot.urls_present).insert(owner.0);
            fts::mark_one(transaction, owner, input.imported_at_ms)?;
        }
    }
    enqueue_file_work(
        transaction,
        file_id,
        &input.facts.content_hash,
        "thumbnail",
        input.imported_at_ms,
    )?;
    if !has_palette {
        enqueue_file_work(
            transaction,
            file_id,
            &input.facts.content_hash,
            "dominant_colors",
            input.imported_at_ms,
        )?;
    }
    if !has_perceptual_hash {
        enqueue_file_work(
            transaction,
            file_id,
            &input.facts.content_hash,
            "perceptual_hash",
            input.imported_at_ms,
        )?;
    }
    Ok(affected_roots)
}

#[derive(Debug, Clone, Copy)]
struct ExistingImport {
    media_id: MediaId,
    root_id: RootId,
}

/// Returns the media item that an import resolves to when the library
/// already has it. The item is looked for by its stable key, or by the alias
/// that a merged-away item left, then by its source item, and then by its
/// file. Each file has at most one media item, so a file names exactly one.
fn existing_import(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    input: &PreparedImport,
) -> Result<Option<ExistingImport>> {
    let stable_media = transaction
        .prepare_cached(
            "SELECT local_id FROM library_item WHERE stable_key = ?1 AND item_kind = 1
             UNION ALL
             SELECT alias.local_id FROM library_item_alias alias
             JOIN library_item item ON item.local_id = alias.local_id AND item.item_kind = 1
             WHERE alias.stable_key = ?1
             LIMIT 1",
        )?
        .query_row([&input.stable_key], |row| row.get::<_, u32>(0))
        .optional()?;
    let source_media = input
        .source_identity
        .as_ref()
        .map(|source| {
            transaction
                .prepare_cached(
                    "SELECT media_id FROM source_provenance
                     WHERE source_key = ?1 AND source_item_key = ?2
                     ORDER BY media_id LIMIT 1",
                )?
                .query_row(params![source.source_key, source.source_item_key], |row| {
                    row.get::<_, u32>(0)
                })
                .optional()
        })
        .transpose()?
        .flatten();
    if stable_media.is_some() && source_media.is_some() && stable_media != source_media {
        return Err(LibraryError::InvalidState(
            "stable and source identities resolve to different media".into(),
        ));
    }
    let media_id = if let Some(media_id) = stable_media.or(source_media) {
        media_id
    } else if let Some(media_id) = transaction
        .prepare_cached(
            "SELECT media.media_id
             FROM media_file file
             JOIN media_item media ON media.file_id = file.file_id
             WHERE file.content_hash = ?1
             ORDER BY media.media_id
             LIMIT 1",
        )?
        .query_row([&input.facts.content_hash], |row| row.get::<_, u32>(0))
        .optional()?
    {
        media_id
    } else {
        return Ok(None);
    };
    Ok(Some(ExistingImport {
        media_id: MediaId(media_id),
        root_id: snapshot
            .media_owner
            .get(media_id)
            .copied()
            .unwrap_or(RootId(media_id)),
    }))
}

pub(crate) fn persist_touched(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &ProjectionSnapshot,
    bitmap_keys: impl IntoIterator<Item = BitmapKey>,
    folder_ids: impl IntoIterator<Item = FolderId>,
    root_ids: impl IntoIterator<Item = RootId>,
) -> Result<()> {
    let high_bits = root_ids
        .into_iter()
        .map(|root_id| (root_id.0 >> 16) as u16)
        .collect::<std::collections::BTreeSet<_>>();
    for key in bitmap_keys {
        let values = match key.domain {
            BitmapDomain::Lifecycle => snapshot
                .lifecycle
                .iter()
                .find_map(|(value, roots)| (value.bitmap_key() == key.key_id).then_some(roots)),
            BitmapDomain::Rating => snapshot
                .ratings
                .iter()
                .find_map(|(value, roots)| (value.bitmap_key() == key.key_id).then_some(roots)),
            BitmapDomain::Tag => snapshot.tags.get(&TagId(key.key_id)),
        }
        .ok_or_else(|| {
            LibraryError::InvalidState(format!(
                "ingest changed missing bitmap {:?}/{}",
                key.domain, key.key_id
            ))
        })?;
        bitmap::replace_shards(
            transaction,
            revision,
            key,
            values,
            high_bits.iter().copied(),
        )?;
    }
    for folder_id in folder_ids {
        let values = snapshot.folder_orders.get(&folder_id).ok_or_else(|| {
            LibraryError::InvalidState(format!("ingest changed missing folder {}", folder_id.0))
        })?;
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Folder,
            folder_id.0,
            &values.iter().map(|root| root.0).collect::<Vec<_>>(),
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_tag(transaction: &Transaction<'_>, name: &str) -> Result<TagId> {
    let name = canonical_tag_name(name)?;
    let (namespace, subname) = name.split_once(':').unwrap_or(("", name.as_str()));
    if subname.trim().is_empty() {
        return Err(LibraryError::InvalidInput("tag name is empty".into()));
    }
    let namespace_id = ensure_namespace(transaction, namespace)?;
    if let Some(id) = transaction
        .query_row(
            "SELECT tag_id FROM tag_definition WHERE namespace_id = ?1 AND subname = ?2",
            params![namespace_id, subname],
            |row| row.get::<_, u32>(0),
        )
        .optional()?
    {
        return Ok(TagId(id));
    }
    let tag_id = LibraryDatabase::allocate_id(transaction)?;
    let namespace = if namespace.eq_ignore_ascii_case("general") {
        ""
    } else {
        namespace
    };
    let stable_key = free_key(
        transaction,
        "tag_definition",
        name_key("tag", &[namespace, subname]),
    )?;
    transaction.execute(
        "INSERT INTO tag_definition(tag_id, stable_key, namespace_id, subname)
         VALUES (?1, ?2, ?3, ?4)",
        params![tag_id, stable_key, namespace_id, subname],
    )?;
    Ok(TagId(tag_id))
}

/// Returns the stable key of a new tag or tag namespace with the name
/// `parts`: `kind`, a colon and the first 32 hexadecimal digits of the
/// SHA-256 of the parts. Two devices of a library that create the same name
/// give it the same key, so cloud sync takes them for one tag.
pub(crate) fn name_key(kind: &str, parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let digest = hasher.finalize();
    let digits = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{kind}:{digits}")
}

/// Returns `key` when no row of `table` has it as its stable key, and a
/// random key otherwise. A key made from a name is taken when the row that
/// was created under that name was renamed since.
pub(crate) fn free_key(transaction: &Transaction<'_>, table: &str, key: String) -> Result<String> {
    let taken = transaction.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE stable_key = ?1)"),
        [&key],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(if taken {
        Uuid::new_v4().to_string()
    } else {
        key
    })
}

pub(crate) fn canonical_tag_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(LibraryError::InvalidInput("tag name is empty".into()));
    }
    match name.split_once(':') {
        Some((namespace, subname)) if namespace.trim().eq_ignore_ascii_case("general") => {
            let subname = subname.trim();
            if subname.is_empty() {
                return Err(LibraryError::InvalidInput("tag name is empty".into()));
            }
            Ok(subname.to_owned())
        }
        _ => Ok(name.to_owned()),
    }
}

pub(crate) fn ensure_namespace(transaction: &Transaction<'_>, name: &str) -> Result<u32> {
    let name = if name.eq_ignore_ascii_case("general") {
        ""
    } else {
        name
    };
    let namespace_id = if let Some(id) = transaction
        .query_row(
            "SELECT namespace_id FROM tag_namespace WHERE display_name = ?1",
            [name],
            |row| row.get::<_, u32>(0),
        )
        .optional()?
    {
        id
    } else {
        let id = LibraryDatabase::allocate_id(transaction)?;
        let stable_key = free_key(transaction, "tag_namespace", name_key("namespace", &[name]))?;
        transaction.execute(
            "INSERT INTO tag_namespace(namespace_id, stable_key, display_name)
             VALUES (?1, ?2, ?3)",
            params![id, stable_key, name],
        )?;
        id
    };
    Ok(namespace_id)
}

pub(crate) fn folder_auto_tags(
    transaction: &Transaction<'_>,
    folder_id: FolderId,
) -> Result<roaring::RoaringBitmap> {
    let payload = transaction
        .query_row(
            "SELECT auto_tag_ids FROM folder_definition WHERE folder_id = ?1",
            [folder_id.0],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or_else(|| LibraryError::NotFound(format!("folder {}", folder_id.0)))?;
    if payload.is_empty() {
        return Ok(roaring::RoaringBitmap::new());
    }
    roaring::RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(payload)).map_err(Into::into)
}

pub(crate) fn encode_folder_auto_tags(tags: &roaring::RoaringBitmap) -> Result<Vec<u8>> {
    bitmap::encode(tags)
}

fn mime_family(mime: &str) -> &str {
    mime.split_once('/').map_or(mime, |(family, _)| family)
}

fn sqlite_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        LibraryError::InvalidInput(format!("{field} exceeds SQLite's signed integer range"))
    })
}

#[cfg(test)]
mod weak_name_tests {
    use crate::name_quality::{quality, NameQuality};

    #[test]
    fn synthetic_source_names_are_weak_and_human_titles_are_not() {
        for weak in [
            "gelbooru_14583420",
            "post12345678",
            "14583420.png",
            "2085395535410712592_1.jpg",
            "d92be9442094b7d22424a460cd5d5296",
        ] {
            assert_ne!(
                quality(weak),
                NameQuality::Meaningful,
                "{weak} should be weak"
            );
        }
        for strong in [
            "Lupa Hairpoon",
            "Art Trade with BigDad",
            "wallpaper2",
            "commission for maythedong",
        ] {
            assert_eq!(
                quality(strong),
                NameQuality::Meaningful,
                "{strong} should be strong"
            );
        }
    }

    /// Every form of name that the application generates must be classified
    /// as weak, so that a later import of the same file with a title that a
    /// person wrote replaces it. The generated forms include
    /// `{provider}_{post_id}`, the stems of source files (ids and hashes),
    /// and the OnlyFans form `{creator} - {date}`.
    #[test]
    fn every_generated_name_shape_is_upgradeable() {
        for generated in [
            "gelbooru_14583420",
            "danbooru_12086543",
            "rule34_1263248",
            "twitter_2085395535410712592",
            "ehentai_8df0b5400a",
            // OnlyFans file stems ("0ig3r76odf_source", "3840x5766_<hex>")
            // never become names because the provider emits a title. Plain
            // hexadecimal stems are covered in the test above.
            "f1nn5ter - 2026-08-24",
        ] {
            assert_ne!(
                quality(generated),
                NameQuality::Meaningful,
                "{generated} should be weak"
            );
        }
        // The text of an OnlyFans post that is used as a title was written by
        // a person. It must never be classified as weak.
        assert_eq!(
            quality("custom latex catsuit..? don't mind if I"),
            NameQuality::Meaningful
        );
    }
}
