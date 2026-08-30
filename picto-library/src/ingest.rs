use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{params, OptionalExtension, Transaction};
use uuid::Uuid;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::LibraryDatabase;
use crate::fts;
use crate::model::{FolderId, MediaId, PreparedImport, RootId, RootKind, TagId};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{color_cell, ProjectionSnapshot};
use crate::{LibraryError, Result};

// On the supported release host, 48 tag-rich roots stays below one frame while
// retaining nearly all of the transaction amortization from a 64-item batch.
pub const MAX_INGEST_BATCH: usize = 48;

pub(crate) struct IngestResult {
    pub root_id: RootId,
    pub created_root: bool,
    pub snapshot: ProjectionSnapshot,
    pub resources: Vec<String>,
    pub bitmap_keys: Vec<BitmapKey>,
    pub folder_ids: Vec<FolderId>,
    pub affected_roots: RoaringBitmap,
}

pub(crate) fn insert_one(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    input: &PreparedImport,
    reuse_exact_root: bool,
    reuse_identity: bool,
) -> Result<IngestResult> {
    if reuse_identity {
        if let Some(existing) = existing_import(transaction, &snapshot, input, reuse_exact_root)? {
            let mut affected_roots =
                refresh_existing_file(transaction, &mut snapshot, existing.media_id, input)?;
            let tag_roots = if existing.exact_hash_match {
                exact_hash_owners(transaction, &snapshot, &input.facts.content_hash)?
            } else {
                RoaringBitmap::from_iter([existing.root_id.0])
            };
            let mut bitmap_keys = Vec::new();
            for name in &input.tags {
                let tag_id = if let Some(tag_id) = snapshot.tag_ids_by_name.get(name).copied() {
                    tag_id
                } else {
                    let tag_id = ensure_tag(transaction, name)?;
                    Arc::make_mut(&mut snapshot.tag_ids_by_name).insert(name.clone(), tag_id);
                    tag_id
                };
                let added_roots = {
                    let roots = Arc::make_mut(&mut snapshot.tags).entry(tag_id).or_default();
                    tag_roots
                        .iter()
                        .filter(|root_id| roots.insert(*root_id))
                        .collect::<RoaringBitmap>()
                };
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
            snapshot.revision = revision;
            return Ok(IngestResult {
                root_id: existing.root_id,
                created_root: false,
                snapshot,
                resources: vec!["roots".into(), "tags".into()],
                bitmap_keys,
                folder_ids: Vec::new(),
                affected_roots,
            });
        }
    }
    if transaction
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM deletion_tombstone WHERE stable_key = ?1)")?
        .query_row([&input.stable_key], |row| row.get::<_, bool>(0))?
    {
        return Err(LibraryError::ImportDeleted);
    }

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
    transaction
        .prepare_cached(
            "INSERT INTO library_root
             (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
              captured_at_ms, modified_at_ms, media_count, total_size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, 1, ?8)",
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
    for name in &input.tags {
        let tag_id = if let Some(tag_id) = snapshot.tag_ids_by_name.get(name).copied() {
            tag_id
        } else {
            let tag_id = ensure_tag(transaction, name)?;
            Arc::make_mut(&mut snapshot.tag_ids_by_name).insert(name.clone(), tag_id);
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

    let mut assigned_folders = 0u64;
    for folder_id in &input.folders {
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
                assigned_tags += 1;
                bitmap_keys.push(BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                });
            }
        }
        let folder_orders = Arc::make_mut(&mut snapshot.folder_orders);
        let order = folder_orders
            .entry(*folder_id)
            .or_insert_with(|| Arc::new(Vec::new()));
        let order = Arc::make_mut(order);
        if !order.contains(&root_id) {
            order.push(root_id);
            assigned_folders += 1;
        }
        Arc::make_mut(&mut snapshot.folders)
            .entry(*folder_id)
            .or_default()
            .insert(root_id.0);
    }

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
    Arc::make_mut(&mut snapshot.imported_at).insert(root_id.0, input.imported_at_ms.max(0) as u64);
    if let Some(value) = input.captured_at_ms {
        Arc::make_mut(&mut snapshot.captured_at).insert(root_id.0, value.max(0) as u64);
    }
    Arc::make_mut(&mut snapshot.modified_at).insert(root_id.0, input.imported_at_ms.max(0) as u64);
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
    if !reuse_exact_root && reuse_identity {
        bitmap_keys.extend(inherit_standalone_tags(
            transaction,
            &mut snapshot,
            root_id,
            &input.facts.content_hash,
        )?);
    }

    let affected_roots = RoaringBitmap::from_iter([root_id.0]);

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
    })
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

fn refresh_existing_file(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    media_id: MediaId,
    input: &PreparedImport,
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
    let (current_name, current_note) = transaction.query_row(
        "SELECT media_name, media_notes FROM media_item WHERE media_id = ?1",
        [media_id.0],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;
    let incoming_note = input
        .notes
        .as_deref()
        .filter(|note| !note.trim().is_empty());
    let replace_name = should_replace_name(&current_name, &input.media_name);
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
                input.media_name,
                replace_note,
                incoming_note
            ],
        )?;
    }

    let owner = snapshot
        .media_owner
        .get(media_id.0)
        .copied()
        .unwrap_or(RootId(media_id.0));
    affected_roots.insert(owner.0);
    if owner.0 == media_id.0
        && snapshot
            .root_kinds
            .get(&RootKind::Media)
            .is_some_and(|roots| roots.contains(owner.0))
    {
        if replace_name {
            transaction.execute(
                "UPDATE library_root SET name = ?2 WHERE root_id = ?1",
                params![owner.0, input.media_name],
            )?;
        }
        if replace_note {
            transaction.execute(
                "UPDATE library_root SET notes = ?2 WHERE root_id = ?1",
                params![owner.0, incoming_note],
            )?;
            Arc::make_mut(&mut snapshot.notes_present).insert(owner.0);
        }
        if replace_name || replace_note {
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
    exact_hash_match: bool,
}

fn existing_import(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    input: &PreparedImport,
    reuse_exact_root: bool,
) -> Result<Option<ExistingImport>> {
    let stable_media = transaction
        .prepare_cached(
            "SELECT local_id FROM library_item WHERE stable_key = ?1 AND item_kind = 1",
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
    let exact_media = if reuse_exact_root {
        transaction
            .prepare_cached(
                "SELECT media.media_id
                 FROM media_item media
                 JOIN media_file file ON file.file_id = media.file_id
                 LEFT JOIN library_root root ON root.root_id = media.media_id
                 WHERE file.content_hash = ?1
                 ORDER BY root.root_id IS NULL, media.media_id
                 LIMIT 1",
            )?
            .query_row([&input.facts.content_hash], |row| row.get::<_, u32>(0))
            .optional()?
    } else {
        None
    };
    let exact_hash_match = exact_media.is_some();
    let media_id = stable_media.or(source_media).or(exact_media);
    Ok(media_id.map(|media_id| ExistingImport {
        media_id: MediaId(media_id),
        root_id: snapshot
            .media_owner
            .get(media_id)
            .copied()
            .unwrap_or(RootId(media_id)),
        exact_hash_match,
    }))
}

fn exact_hash_owners(
    transaction: &Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    content_hash: &str,
) -> Result<RoaringBitmap> {
    let mut statement = transaction.prepare_cached(
        "SELECT media.media_id
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id
         WHERE file.content_hash = ?1",
    )?;
    let media_ids = statement
        .query_map([content_hash], |row| row.get::<_, u32>(0))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    media_ids
        .into_iter()
        .map(|media_id| {
            snapshot
                .media_owner
                .get(media_id)
                .map(|owner| owner.0)
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!("media {media_id} has no owning root"))
                })
        })
        .collect()
}

fn should_replace_name(existing: &str, incoming: &str) -> bool {
    is_weak_name(existing) && !is_weak_name(incoming)
}

fn inherit_standalone_tags(
    transaction: &Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    target: RootId,
    content_hash: &str,
) -> Result<Vec<BitmapKey>> {
    let mut statement = transaction.prepare_cached(
        "SELECT DISTINCT media.media_id
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id
         JOIN library_item item ON item.local_id = media.media_id AND item.item_kind = 1
         JOIN library_root root ON root.root_id = media.media_id
         WHERE file.content_hash = ?1 AND media.media_id != ?2",
    )?;
    let donors = statement
        .query_map(params![content_hash, target.0], |row| row.get::<_, u32>(0))?
        .collect::<std::result::Result<RoaringBitmap, rusqlite::Error>>()?;
    if donors.is_empty() {
        return Ok(Vec::new());
    }

    let inherited = snapshot
        .tags
        .iter()
        .filter_map(|(tag_id, roots)| (!roots.is_disjoint(&donors)).then_some(*tag_id))
        .collect::<Vec<_>>();
    let mut changed = Vec::new();
    let mut added = 0u64;
    for tag_id in inherited {
        if Arc::make_mut(&mut snapshot.tags)
            .entry(tag_id)
            .or_default()
            .insert(target.0)
        {
            added += 1;
            changed.push(BitmapKey {
                domain: BitmapDomain::Tag,
                key_id: tag_id.0,
            });
        }
    }
    if added != 0 {
        let counts = Arc::make_mut(&mut snapshot.tag_count);
        counts.insert(
            target.0,
            counts.value(target.0).unwrap_or(0).saturating_add(added),
        );
    }
    Ok(changed)
}

fn is_weak_name(name: &str) -> bool {
    let basename = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let stem = basename.rsplit_once('.').map_or(basename, |(stem, _)| stem);
    let compact = stem
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    if compact.is_empty() || compact.chars().all(|character| character.is_ascii_digit()) {
        return true;
    }
    if compact.len() >= 12
        && compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return true;
    }
    let alphabetic = compact
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let digits = compact.len().saturating_sub(alphabetic);
    let words = stem
        .split(|character: char| !character.is_ascii_alphabetic())
        .filter(|part| part.len() >= 3)
        .count();
    // A single word fused to an id-sized digit run ("gelbooru_14583420",
    // "post12345678") is a synthetic source name, not a human title.
    if words <= 1 && digits >= 4 {
        return true;
    }
    words == 0 || (digits >= 4 && alphabetic <= 4)
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
    let (namespace, subname) = name.split_once(':').unwrap_or(("", name));
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
    transaction.execute(
        "INSERT INTO tag_definition(tag_id, stable_key, namespace_id, subname)
         VALUES (?1, ?2, ?3, ?4)",
        params![tag_id, Uuid::new_v4().to_string(), namespace_id, subname],
    )?;
    Ok(TagId(tag_id))
}

pub(crate) fn ensure_namespace(transaction: &Transaction<'_>, name: &str) -> Result<u32> {
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
        transaction.execute(
            "INSERT INTO tag_namespace(namespace_id, stable_key, display_name)
             VALUES (?1, ?2, ?3)",
            params![id, Uuid::new_v4().to_string(), name],
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
    use super::is_weak_name;

    #[test]
    fn synthetic_source_names_are_weak_and_human_titles_are_not() {
        for weak in [
            "gelbooru_14583420",
            "post12345678",
            "14583420.png",
            "2085395535410712592_1.jpg",
            "d92be9442094b7d22424a460cd5d5296",
        ] {
            assert!(is_weak_name(weak), "{weak} should be weak");
        }
        for strong in [
            "Lupa Hairpoon",
            "Art Trade with BigDad",
            "wallpaper2",
            "commission for maythedong",
        ] {
            assert!(!is_weak_name(strong), "{strong} should be strong");
        }
    }

    /// Every synthetic shape the engine's name generators can emit must be
    /// classified weak, so a later source carrying a human title can upgrade
    /// it. Generators include `{provider}_{post_id}`, source file stems
    /// (ids/hashes), and the OnlyFans `{creator} - {date}` fallback.
    #[test]
    fn every_generated_name_shape_is_upgradeable() {
        for generated in [
            "gelbooru_14583420",
            "danbooru_12086543",
            "rule34_1263248",
            "twitter_2085395535410712592",
            "ehentai_8df0b5400a",
            "idolcomplex_1084425",
            // OnlyFans file stems ("0ig3r76odf_source", "3840x5766_<hex>")
            // never become names because the provider emits a title. Plain
            // hexadecimal stems are covered in the test above.
            "f1nn5ter - 2026-08-24",
        ] {
            assert!(is_weak_name(generated), "{generated} should be weak");
        }
        // OnlyFans post text used as a title is a human name and must never
        // be downgraded.
        assert!(!is_weak_name("custom latex catsuit..? don't mind if I"));
    }
}
