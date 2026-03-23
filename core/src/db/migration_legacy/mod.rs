//! Legacy schema migration — one-shot import from old tables to new.
//!
//! This is the ONLY module allowed to reference old table names.
//! No runtime code depends on this module after migration completes.

use rusqlite::{params, Connection};

/// Old schema version that triggers migration.
const OLD_SCHEMA_MAX_VERSION: i64 = 99;
/// New schema version after migration.
const NEW_SCHEMA_VERSION: i64 = 100;

/// Check if the database needs migration from old schema.
pub fn needs_migration(conn: &Connection) -> bool {
    let version: i64 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
        .unwrap_or(0);
    version > 0 && version <= OLD_SCHEMA_MAX_VERSION
}

/// Check if the database is already on the new schema.
pub fn is_new_schema(conn: &Connection) -> bool {
    let version: i64 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
        .unwrap_or(0);
    version >= NEW_SCHEMA_VERSION
}

/// Run the full migration from old schema to new.
/// Creates new tables, copies data, drops old tables, sets new version.
pub fn migrate(conn: &Connection) -> Result<MigrationResult, String> {
    let mut result = MigrationResult::default();

    // Create new tables (they use IF NOT EXISTS so safe to run)
    conn.execute_batch(crate::db::core::schema::LIBRARY_DDL)
        .map_err(|e| format!("Failed to create new schema: {e}"))?;

    // ── Step 1: media_file ← file ─────────────────────────────────
    let file_count = conn
        .execute(
            "INSERT OR IGNORE INTO media_file (file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio, perceptual_hash, dominant_color_hex, dominant_palette_blob, date_added)
             SELECT file_id, hash, mime, size, width, height, duration_ms, num_frames, COALESCE(has_audio, 0), phash, dominant_color_hex, dominant_palette_blob, COALESCE(imported_at, datetime('now'))
             FROM file
             WHERE hash IS NOT NULL",
            [],
        )
        .map_err(|e| format!("Failed to migrate files: {e}"))?;
    result.files_migrated = file_count;

    // ── Step 2: media_entity ← media_entity (old) ─────────────────
    // Singles
    let singles = conn
        .execute(
            "INSERT OR IGNORE INTO media_entity (entity_id, entity_hash, entity_kind, status, name, notes, rating, source_urls_json, date_created, date_added, date_modified, parent_collection_entity_id, collection_ordinal)
             SELECT me.entity_id, COALESCE(me.hash, f.hash, hex(randomblob(32))), 'single', COALESCE(me.status, 0),
                    COALESCE(me.name, f.name), me.description, COALESCE(me.rating, f.rating),
                    f.source_urls_json,
                    COALESCE(me.created_at, f.imported_at, datetime('now')),
                    COALESCE(f.imported_at, me.created_at, datetime('now')),
                    COALESCE(me.updated_at, datetime('now')),
                    me.parent_collection_id, me.collection_ordinal
             FROM media_entity me
             LEFT JOIN entity_file ef ON ef.entity_id = me.entity_id
             LEFT JOIN file f ON f.file_id = ef.file_id
             WHERE me.kind = 'single'",
            [],
        )
        .map_err(|e| format!("Failed to migrate single entities: {e}"))?;
    result.singles_migrated = singles;

    // Collections
    let collections = conn
        .execute(
            "INSERT OR IGNORE INTO media_entity (entity_id, entity_hash, entity_kind, status, name, notes, rating, date_created, date_added, date_modified, member_count, total_size_bytes, primary_member_entity_id)
             SELECT me.entity_id, COALESCE(me.hash, hex(randomblob(32))), 'collection', COALESCE(me.status, 0),
                    me.name, me.description, me.rating,
                    COALESCE(me.created_at, datetime('now')),
                    COALESCE(me.created_at, datetime('now')),
                    COALESCE(me.updated_at, datetime('now')),
                    me.cached_item_count, me.cached_total_size_bytes, me.cover_file_id
             FROM media_entity me
             WHERE me.kind = 'collection'",
            [],
        )
        .map_err(|e| format!("Failed to migrate collection entities: {e}"))?;
    result.collections_migrated = collections;

    // ── Step 3: single_media_entity ← entity_file ─────────────────
    let bridges = conn
        .execute(
            "INSERT OR IGNORE INTO single_media_entity (entity_id, file_id)
             SELECT ef.entity_id, ef.file_id
             FROM entity_file ef
             JOIN media_entity me ON me.entity_id = ef.entity_id
             WHERE me.kind = 'single'",
            [],
        )
        .map_err(|e| format!("Failed to migrate single_media_entity: {e}"))?;
    result.bridges_migrated = bridges;

    // ── Step 4: entity_tag ← entity_tag_raw ───────────────────────
    let tags = conn
        .execute(
            "INSERT OR IGNORE INTO entity_tag (entity_id, tag_id, source)
             SELECT entity_id, tag_id, source FROM entity_tag_raw",
            [],
        )
        .map_err(|e| format!("Failed to migrate entity_tag: {e}"))?;
    result.tags_migrated = tags;

    // ── Step 5: folder_member ← folder_entity ─────────────────────
    let folder_members = conn
        .execute(
            "INSERT OR IGNORE INTO folder_member (folder_id, entity_id, position_rank)
             SELECT folder_id, entity_id, position_rank FROM folder_entity",
            [],
        )
        .map_err(|e| format!("Failed to migrate folder_member: {e}"))?;
    result.folder_members_migrated = folder_members;

    // ── Step 6: Fix collection primary_member_entity_id ───────────
    // The old schema used cover_file_id (a file_id). The new schema uses
    // primary_member_entity_id (an entity_id of the first member by ordinal).
    conn.execute(
        "UPDATE media_entity SET primary_member_entity_id = (
             SELECT child.entity_id
             FROM media_entity child
             WHERE child.parent_collection_entity_id = media_entity.entity_id
             ORDER BY child.collection_ordinal ASC
             LIMIT 1
         )
         WHERE entity_kind = 'collection'",
        [],
    )
    .map_err(|e| format!("Failed to fix primary_member_entity_id: {e}"))?;

    // ── Step 7: Recompute collection aggregates ───────────────────
    conn.execute(
        "UPDATE media_entity SET
             member_count = (
                 SELECT COUNT(*) FROM media_entity child
                 WHERE child.parent_collection_entity_id = media_entity.entity_id
             ),
             total_size_bytes = (
                 SELECT COALESCE(SUM(mf.size_bytes), 0)
                 FROM media_entity child
                 JOIN single_media_entity sme ON sme.entity_id = child.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE child.parent_collection_entity_id = media_entity.entity_id
             )
         WHERE entity_kind = 'collection'",
        [],
    )
    .map_err(|e| format!("Failed to recompute collection aggregates: {e}"))?;

    // ── Step 8: Migrate file_color ────────────────────────────────
    // file_color references media_file.file_id which has same IDs as old file.file_id
    // so the FK should be valid after media_file migration.

    // ── Step 9: Migrate subscription tables ───────────────────────
    // subscription_group, subscription, subscription_query already match new schema
    // shape (same column names). subscription_entity uses entity_id FK.
    // These tables are kept as-is since the new schema matches.

    // ── Step 10: Drop old tables ──────────────────────────────────
    let old_tables = [
        "entity_file",
        "entity_tag_raw",
        "folder_entity",
        "entity_metadata_projection",
        "artifact_manifest_meta",
        "artifact_manifest_entry",
        "mutation_action",
        "collection_source_url",
        "file_fts",
    ];
    for table in &old_tables {
        let _ = conn.execute(&format!("DROP TABLE IF EXISTS {table}"), []);
    }

    // ── Step 11: Set new schema version ───────────────────────────
    conn.execute("DELETE FROM schema_version", [])
        .map_err(|e| format!("Failed to clear schema_version: {e}"))?;
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        [NEW_SCHEMA_VERSION],
    )
    .map_err(|e| format!("Failed to set new schema version: {e}"))?;

    Ok(result)
}

#[derive(Debug, Default)]
pub struct MigrationResult {
    pub files_migrated: usize,
    pub singles_migrated: usize,
    pub collections_migrated: usize,
    pub bridges_migrated: usize,
    pub tags_migrated: usize,
    pub folder_members_migrated: usize,
}

impl std::fmt::Display for MigrationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Migration complete: {} files, {} singles, {} collections, {} bridges, {} tags, {} folder members",
            self.files_migrated,
            self.singles_migrated,
            self.collections_migrated,
            self.bridges_migrated,
            self.tags_migrated,
            self.folder_members_migrated,
        )
    }
}
