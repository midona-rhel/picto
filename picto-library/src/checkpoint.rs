use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{FolderId, LabColor, Lifecycle, MediaId, Rating, RootId, RootKind, TagId};
use crate::predicate::ViewQuerySpec;
use crate::projection::{NumericIndex, ProjectionSnapshot, ShardedIdMap, SharedBitmap};
use crate::schema::SCHEMA_FINGERPRINT;
use crate::{LibraryError, Result};

pub const PROJECTION_IMPLEMENTATION_HASH: &str = "greenfield-projection-v7";

#[derive(Serialize, Deserialize)]
struct CheckpointData {
    lifecycle: std::collections::HashMap<Lifecycle, SharedBitmap>,
    ratings: std::collections::HashMap<Rating, SharedBitmap>,
    tags: std::collections::HashMap<TagId, SharedBitmap>,
    tag_ids_by_name: std::collections::HashMap<String, TagId>,
    folder_orders: std::collections::HashMap<FolderId, std::sync::Arc<Vec<RootId>>>,
    folders: std::collections::HashMap<FolderId, SharedBitmap>,
    collection_orders: std::collections::HashMap<RootId, std::sync::Arc<Vec<MediaId>>>,
    media_owner: ShardedIdMap<RootId>,
    image_media: roaring::RoaringBitmap,
    roots_with_images: roaring::RoaringBitmap,
    root_kinds: std::collections::HashMap<RootKind, SharedBitmap>,
    mime: std::collections::HashMap<String, SharedBitmap>,
    mime_family: std::collections::HashMap<String, SharedBitmap>,
    color_cells: std::collections::HashMap<u32, SharedBitmap>,
    cover_palettes: ShardedIdMap<std::sync::Arc<Vec<LabColor>>>,
    tag_count: NumericIndex,
    folder_count: NumericIndex,
    total_bytes: NumericIndex,
    media_count: NumericIndex,
    width: NumericIndex,
    height: NumericIndex,
    duration: NumericIndex,
    imported_at: NumericIndex,
    captured_at: NumericIndex,
    modified_at: NumericIndex,
    notes_present: roaring::RoaringBitmap,
    urls_present: roaring::RoaringBitmap,
    smart_results: std::collections::HashMap<u32, SharedBitmap>,
    smart_queries: std::collections::HashMap<u32, ViewQuerySpec>,
}

pub fn encode(snapshot: &ProjectionSnapshot) -> Result<Vec<u8>> {
    let data = CheckpointData {
        lifecycle: (*snapshot.lifecycle).clone(),
        ratings: (*snapshot.ratings).clone(),
        tags: (*snapshot.tags).clone(),
        tag_ids_by_name: (*snapshot.tag_ids_by_name).clone(),
        folder_orders: (*snapshot.folder_orders).clone(),
        folders: (*snapshot.folders).clone(),
        collection_orders: (*snapshot.collection_orders).clone(),
        media_owner: (*snapshot.media_owner).clone(),
        image_media: (*snapshot.image_media).clone(),
        roots_with_images: (*snapshot.roots_with_images).clone(),
        root_kinds: (*snapshot.root_kinds).clone(),
        mime: (*snapshot.mime).clone(),
        mime_family: (*snapshot.mime_family).clone(),
        color_cells: (*snapshot.color_cells).clone(),
        cover_palettes: (*snapshot.cover_palettes).clone(),
        tag_count: (*snapshot.tag_count).clone(),
        folder_count: (*snapshot.folder_count).clone(),
        total_bytes: (*snapshot.total_bytes).clone(),
        media_count: (*snapshot.media_count).clone(),
        width: (*snapshot.width).clone(),
        height: (*snapshot.height).clone(),
        duration: (*snapshot.duration).clone(),
        imported_at: (*snapshot.imported_at).clone(),
        captured_at: (*snapshot.captured_at).clone(),
        modified_at: (*snapshot.modified_at).clone(),
        notes_present: (*snapshot.notes_present).clone(),
        urls_present: (*snapshot.urls_present).clone(),
        smart_results: (*snapshot.smart_results).clone(),
        smart_queries: (*snapshot.smart_queries).clone(),
    };
    bincode::serialize(&data).map_err(|error| LibraryError::Checkpoint(error.to_string()))
}

pub fn decode(payload: &[u8], revision: u64) -> Result<ProjectionSnapshot> {
    let data: CheckpointData = bincode::deserialize(payload)
        .map_err(|error| LibraryError::Checkpoint(error.to_string()))?;
    Ok(ProjectionSnapshot {
        revision,
        lifecycle: std::sync::Arc::new(data.lifecycle),
        ratings: std::sync::Arc::new(data.ratings),
        tags: std::sync::Arc::new(data.tags),
        tag_ids_by_name: std::sync::Arc::new(data.tag_ids_by_name),
        folder_orders: std::sync::Arc::new(data.folder_orders),
        folders: std::sync::Arc::new(data.folders),
        collection_orders: std::sync::Arc::new(data.collection_orders),
        media_owner: std::sync::Arc::new(data.media_owner),
        image_media: std::sync::Arc::new(data.image_media),
        roots_with_images: std::sync::Arc::new(data.roots_with_images),
        root_kinds: std::sync::Arc::new(data.root_kinds),
        mime: std::sync::Arc::new(data.mime),
        mime_family: std::sync::Arc::new(data.mime_family),
        color_cells: std::sync::Arc::new(data.color_cells),
        cover_palettes: std::sync::Arc::new(data.cover_palettes),
        tag_count: std::sync::Arc::new(data.tag_count),
        folder_count: std::sync::Arc::new(data.folder_count),
        total_bytes: std::sync::Arc::new(data.total_bytes),
        media_count: std::sync::Arc::new(data.media_count),
        width: std::sync::Arc::new(data.width),
        height: std::sync::Arc::new(data.height),
        duration: std::sync::Arc::new(data.duration),
        imported_at: std::sync::Arc::new(data.imported_at),
        captured_at: std::sync::Arc::new(data.captured_at),
        modified_at: std::sync::Arc::new(data.modified_at),
        notes_present: std::sync::Arc::new(data.notes_present),
        urls_present: std::sync::Arc::new(data.urls_present),
        smart_results: std::sync::Arc::new(data.smart_results),
        smart_queries: std::sync::Arc::new(data.smart_queries),
    })
}

pub fn write(connection: &Connection, revision: u64, payload: &[u8]) -> Result<()> {
    let digest: [u8; 32] = Sha256::digest(payload).into();
    connection.execute(
        "INSERT INTO projection_checkpoint
             (singleton, schema_fingerprint, implementation_hash, database_revision, checksum, payload)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(singleton) DO UPDATE SET
             schema_fingerprint = excluded.schema_fingerprint,
             implementation_hash = excluded.implementation_hash,
             database_revision = excluded.database_revision,
             checksum = excluded.checksum,
             payload = excluded.payload",
        params![
            SCHEMA_FINGERPRINT,
            PROJECTION_IMPLEMENTATION_HASH,
            revision as i64,
            digest.as_slice(),
            payload
        ],
    )?;
    Ok(())
}

pub fn read(connection: &Connection, revision: u64) -> Result<Option<Vec<u8>>> {
    let result = connection.query_row(
        "SELECT schema_fingerprint, implementation_hash, database_revision, checksum, payload
         FROM projection_checkpoint WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        },
    );
    let Ok((schema, implementation, checkpoint_revision, checksum, payload)) = result else {
        return Ok(None);
    };
    if schema != SCHEMA_FINGERPRINT
        || implementation != PROJECTION_IMPLEMENTATION_HASH
        || checkpoint_revision != revision
        || Sha256::digest(&payload).as_slice() != checksum
    {
        return Ok(None);
    }
    Ok(Some(payload))
}
