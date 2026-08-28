use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Transaction};
use uuid::Uuid;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::LibraryDatabase;
use crate::fts;
use crate::model::{FolderId, MediaId, PreparedImport, RootId, RootKind, TagId};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{color_cell, ProjectionSnapshot};
use crate::{LibraryError, Result};

pub const MAX_INGEST_BATCH: usize = 40;

pub(crate) struct IngestResult {
    pub root_id: RootId,
    pub snapshot: ProjectionSnapshot,
    pub resources: Vec<String>,
    pub bitmap_keys: Vec<BitmapKey>,
    pub folder_ids: Vec<FolderId>,
}

pub(crate) fn insert_one(
    transaction: &Transaction<'_>,
    revision: u64,
    mut snapshot: ProjectionSnapshot,
    input: &PreparedImport,
) -> Result<IngestResult> {
    let file_id = if let Some(file_id) = transaction
        .query_row(
            "SELECT file_id FROM media_file WHERE content_hash = ?1",
            [&input.facts.content_hash],
            |row| row.get::<_, u32>(0),
        )
        .optional()?
    {
        file_id
    } else {
        let file_id = LibraryDatabase::allocate_id(transaction)?;
        transaction.execute(
            "INSERT INTO media_file
                 (file_id, content_hash, file_path, mime, size_bytes, width, height,
                  duration_ms, frame_count, perceptual_hash, palette_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
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
            ],
        )?;
        file_id
    };

    let root_id = RootId(LibraryDatabase::allocate_id(transaction)?);
    let media_id = MediaId(root_id.0);
    transaction.execute(
        "INSERT INTO library_item(local_id, stable_key, item_kind) VALUES (?1, ?2, 1)",
        params![root_id.0, input.stable_key],
    )?;
    transaction.execute(
        "INSERT INTO media_item(media_id, media_name, file_id) VALUES (?1, ?2, ?3)",
        params![media_id.0, input.media_name, file_id],
    )?;
    transaction.execute(
        "INSERT INTO library_root
             (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
              captured_at_ms, modified_at_ms, media_count, total_size_bytes)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?5, 1, ?7)",
        params![
            root_id.0,
            input.media_name,
            serde_json::to_string(&input.source_urls)?,
            media_id.0,
            input.imported_at_ms,
            input.captured_at_ms,
            sqlite_i64(input.facts.size_bytes, "root size")?,
        ],
    )?;

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
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM folder_definition WHERE folder_id = ?1)",
            [folder_id.0],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(LibraryError::InvalidInput(format!(
                "folder {} does not exist",
                folder_id.0
            )));
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
        .insert(root_id, Arc::new(input.facts.palette.clone()));
    let owners = Arc::make_mut(&mut snapshot.media_owner);
    if owners.len() <= media_id.0 as usize {
        owners.resize(media_id.0 as usize + 1, None);
    }
    owners[media_id.0 as usize] = Some(root_id);

    Arc::make_mut(&mut snapshot.tag_count).insert(root_id.0, assigned_tags);
    Arc::make_mut(&mut snapshot.folder_count).insert(root_id.0, assigned_folders);
    Arc::make_mut(&mut snapshot.total_bytes).insert(root_id.0, input.facts.size_bytes);
    Arc::make_mut(&mut snapshot.media_count).insert(root_id.0, 1);
    Arc::make_mut(&mut snapshot.imported_at).insert(root_id.0, input.imported_at_ms.max(0) as u64);
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

    fts::mark_one(transaction, root_id, input.imported_at_ms)?;
    transaction.execute(
        "INSERT INTO cloud_journal
             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
         VALUES (?1, 'root.ingest', NULL, ?2, ?3)",
        params![
            revision as i64,
            serde_json::json!({"root_id": root_id.0, "stable_key": input.stable_key}).to_string(),
            input.imported_at_ms
        ],
    )?;
    snapshot.revision = revision;
    Ok(IngestResult {
        root_id,
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
    })
}

pub(crate) fn persist_touched(
    transaction: &Transaction<'_>,
    revision: u64,
    snapshot: &ProjectionSnapshot,
    bitmap_keys: impl IntoIterator<Item = BitmapKey>,
    folder_ids: impl IntoIterator<Item = FolderId>,
) -> Result<()> {
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
        bitmap::replace(transaction, revision, key, values)?;
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
    let namespace_id = if let Some(id) = transaction
        .query_row(
            "SELECT namespace_id FROM tag_namespace WHERE display_name = ?1",
            [namespace],
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
            params![id, Uuid::new_v4().to_string(), namespace],
        )?;
        id
    };
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

fn mime_family(mime: &str) -> &str {
    mime.split_once('/').map_or(mime, |(family, _)| family)
}

fn sqlite_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        LibraryError::InvalidInput(format!("{field} exceeds SQLite's signed integer range"))
    })
}
