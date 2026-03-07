//! DDL and migration runner for the library SQLite database.

use rusqlite::Connection;

pub fn apply_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(PRAGMA_SQL)
}

pub const CURRENT_VERSION: i64 = 25;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(LIBRARY_DDL)?;
    seed_manifest(conn)?;
    seed_artifact_manifest(conn)?;
    Ok(())
}

pub fn get_schema_version(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    let version: i64 = conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
        row.get(0)
    })?;
    Ok(Some(version))
}

pub fn run_migrations(conn: &Connection, from_version: i64) -> rusqlite::Result<()> {
    if from_version < 2 {
        // V2: Add display_order column to smart_folder for drag-reorder persistence.
        if !has_column(conn, "smart_folder", "display_order")? {
            conn.execute_batch("ALTER TABLE smart_folder ADD COLUMN display_order INTEGER")?;
        }
    }
    if from_version < 3 {
        // V3: Add schedule column to subscription for automatic scheduling.
        // (Kept for migration path — V4 removes it from subscription and puts it on flow.)
        if !has_column(conn, "subscription", "schedule")? {
            conn.execute_batch(
                "ALTER TABLE subscription ADD COLUMN schedule TEXT NOT NULL DEFAULT 'manual'",
            )?;
        }
    }
    if from_version < 4 {
        // V4: Add flow table, flow_id FK on subscription, migrate orphaned subscriptions.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS flow (
                flow_id    INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                schedule   TEXT NOT NULL DEFAULT 'manual',
                created_at TEXT NOT NULL
            )",
        )?;
        if !has_column(conn, "subscription", "flow_id")? {
            conn.execute_batch(
                "ALTER TABLE subscription ADD COLUMN flow_id INTEGER REFERENCES flow(flow_id) ON DELETE CASCADE",
            )?;
        }
        // Migrate: create a flow for each orphaned subscription (subscriptions without a flow_id)
        let mut stmt = conn.prepare(
            "SELECT subscription_id, name, schedule FROM subscription WHERE flow_id IS NULL",
        )?;
        let orphans: Vec<(i64, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get::<_, String>(2)
                        .unwrap_or_else(|_| "manual".to_string()),
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (sub_id, name, schedule) in orphans {
            conn.execute(
                "INSERT INTO flow (name, schedule, created_at) VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![name, schedule],
            )?;
            let flow_id = conn.last_insert_rowid();
            conn.execute(
                "UPDATE subscription SET flow_id = ?1 WHERE subscription_id = ?2",
                rusqlite::params![flow_id, sub_id],
            )?;
        }
    }
    if from_version < 5 {
        // V5: Add last_viewed_at column for recently viewed tracking
        if !has_column(conn, "file", "last_viewed_at")? {
            conn.execute_batch("ALTER TABLE file ADD COLUMN last_viewed_at TEXT")?;
        }
        // Create index for last_viewed_at queries
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_file_last_viewed ON file(last_viewed_at) WHERE last_viewed_at IS NOT NULL"
        )?;
    }
    if from_version < 6 {
        // V6: Composite indexes for grid pagination (eliminates temp B-tree sorts)
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_file_status_imported ON file(status, imported_at DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_viewed   ON file(status, last_viewed_at DESC, file_id DESC);"
        )?;
        // Update query planner statistics so the new indexes are used immediately
        conn.execute_batch("ANALYZE file")?;
    }
    if from_version < 7 {
        // V7: Duplicate pair-first rearchitecture — decision metadata + title provenance
        if !has_column(conn, "duplicate", "decision_at")? {
            conn.execute_batch(
                "ALTER TABLE duplicate ADD COLUMN decision_at TEXT;
                 ALTER TABLE duplicate ADD COLUMN decision_source TEXT;
                 ALTER TABLE duplicate ADD COLUMN decision_reason TEXT;
                 ALTER TABLE duplicate ADD COLUMN winner_file_id INTEGER;
                 ALTER TABLE duplicate ADD COLUMN loser_file_id INTEGER;",
            )?;
        }
        if !has_column(conn, "file", "name_source")? {
            conn.execute_batch(
                "ALTER TABLE file ADD COLUMN name_source TEXT NOT NULL DEFAULT 'unknown'",
            )?;
        }
    }
    if from_version < 8 {
        // V8: Additional composite indexes for grid pagination by rating, size, view_count, name
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_file_status_rating    ON file(status, rating DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_size      ON file(status, size DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_viewcount ON file(status, view_count DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_name      ON file(status, name COLLATE NOCASE, file_id);"
        )?;
        conn.execute_batch("ANALYZE file")?;
    }
    if from_version < 10 {
        // V10: First-class media entities + collections foundation.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS media_entity (
                 entity_id    INTEGER PRIMARY KEY,
                 kind         TEXT NOT NULL CHECK(kind IN ('single','collection')),
                 name         TEXT,
                 description  TEXT NOT NULL DEFAULT '',
                 status       INTEGER NOT NULL DEFAULT 1,
                 rating       INTEGER,
                 created_at   TEXT,
                 updated_at   TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_media_entity_kind ON media_entity(kind);
             CREATE INDEX IF NOT EXISTS idx_media_entity_updated ON media_entity(updated_at);

             CREATE TABLE IF NOT EXISTS entity_file (
                 entity_id INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 file_id   INTEGER NOT NULL UNIQUE REFERENCES file(file_id) ON DELETE CASCADE
             );

             CREATE TABLE IF NOT EXISTS collection_member (
                 collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 member_entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 ordinal              INTEGER NOT NULL,
                 PRIMARY KEY (collection_entity_id, member_entity_id)
             );
             CREATE INDEX IF NOT EXISTS idx_collection_member_order
                 ON collection_member(collection_entity_id, ordinal);

             CREATE TABLE IF NOT EXISTS collection_tag (
                 collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 tag                  TEXT NOT NULL,
                 PRIMARY KEY (collection_entity_id, tag)
             );
             CREATE INDEX IF NOT EXISTS idx_collection_tag_tag ON collection_tag(tag COLLATE NOCASE);",
        )?;

        // Backfill: each legacy file is mirrored as a 'single' media entity.
        conn.execute_batch(
            "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             SELECT
                f.file_id,
                'single',
                f.name,
                '',
                f.status,
                f.rating,
                COALESCE(f.imported_at, CURRENT_TIMESTAMP),
                COALESCE(f.imported_at, CURRENT_TIMESTAMP)
             FROM file f;

             INSERT OR IGNORE INTO entity_file (entity_id, file_id)
             SELECT file_id, file_id FROM file;",
        )?;
    }
    if from_version < 11 {
        // V11: Collection source provenance URLs.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS collection_source_url (
                 collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 url                  TEXT NOT NULL,
                 PRIMARY KEY (collection_entity_id, url)
             );
             CREATE INDEX IF NOT EXISTS idx_collection_source_url
                 ON collection_source_url(collection_entity_id);",
        )?;
    }
    if from_version < 12 {
        // V12: Entity metadata/tag projections.
        // Needed for entity-backed metadata reads and tag compiler paths.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entity_tag_raw (
                 entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 tag_id    INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
                 source    TEXT NOT NULL DEFAULT 'local',
                 PRIMARY KEY (entity_id, tag_id)
             );
             CREATE INDEX IF NOT EXISTS idx_etr_tag ON entity_tag_raw(tag_id, entity_id);

             CREATE TABLE IF NOT EXISTS entity_tag_implied (
                 entity_id INTEGER NOT NULL,
                 tag_id    INTEGER NOT NULL,
                 PRIMARY KEY (entity_id, tag_id)
             );
             CREATE INDEX IF NOT EXISTS idx_eti_tag ON entity_tag_implied(tag_id, entity_id);

             CREATE TABLE IF NOT EXISTS entity_metadata_projection (
                 entity_id     INTEGER PRIMARY KEY,
                 epoch         INTEGER NOT NULL,
                 resolved_json TEXT NOT NULL,
                 parents_json  TEXT NOT NULL
             );",
        )?;

        // Ensure single-file entities exist for all files so entity-tag/projection
        // rows can resolve foreign keys after this migration.
        conn.execute_batch(
            "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             SELECT
                f.file_id,
                'single',
                f.name,
                '',
                f.status,
                f.rating,
                COALESCE(f.imported_at, CURRENT_TIMESTAMP),
                COALESCE(f.imported_at, CURRENT_TIMESTAMP)
             FROM file f;

             INSERT OR IGNORE INTO entity_file (entity_id, file_id)
             SELECT file_id, file_id FROM file;",
        )?;
    }
    if from_version < 13 {
        // V13: Complete entity-era relational tables for upgraded libraries.
        // Some pre-V13 databases were already marked current while still missing
        // folder/subscription entity link tables.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS folder_entity (
                 folder_id     INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
                 entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 position_rank INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (folder_id, entity_id)
             );
             CREATE INDEX IF NOT EXISTS idx_fe_rank ON folder_entity(folder_id, position_rank);

             CREATE TABLE IF NOT EXISTS subscription_entity (
                 subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
                 entity_id       INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 PRIMARY KEY (subscription_id, entity_id)
             );",
        )?;

        // Guard: keep single entity/file ownership complete for all legacy rows.
        conn.execute_batch(
            "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             SELECT
                f.file_id,
                'single',
                f.name,
                '',
                f.status,
                f.rating,
                COALESCE(f.imported_at, CURRENT_TIMESTAMP),
                COALESCE(f.imported_at, CURRENT_TIMESTAMP)
             FROM file f;

             INSERT OR IGNORE INTO entity_file (entity_id, file_id)
             SELECT file_id, file_id FROM file;",
        )?;
    }
    if from_version < 14 {
        // V14: Backfill entity link tables from legacy file link tables.
        if table_exists(conn, "folder_file")? {
            conn.execute_batch(
                "INSERT OR IGNORE INTO folder_entity (folder_id, entity_id, position_rank)
                 SELECT ff.folder_id, ef.entity_id, ff.position_rank
                 FROM folder_file ff
                 INNER JOIN entity_file ef ON ef.file_id = ff.file_id;",
            )?;
        }
        if table_exists(conn, "subscription_file")? {
            conn.execute_batch(
                "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id)
                 SELECT sf.subscription_id, ef.entity_id
                 FROM subscription_file sf
                INNER JOIN entity_file ef ON ef.file_id = sf.file_id;",
            )?;
        }
    }
    if from_version < 15 {
        // V15: Keep single-entity status in sync with file.status for upgraded libraries.
        conn.execute_batch(
            "UPDATE media_entity
             SET status = (
                    SELECT f.status
                    FROM entity_file ef
                    INNER JOIN file f ON f.file_id = ef.file_id
                    WHERE ef.entity_id = media_entity.entity_id
                    LIMIT 1
                 ),
                 updated_at = CURRENT_TIMESTAMP
             WHERE kind = 'single'
               AND EXISTS (
                    SELECT 1
                    FROM entity_file ef
                    INNER JOIN file f ON f.file_id = ef.file_id
                    WHERE ef.entity_id = media_entity.entity_id
                      AND COALESCE(media_entity.status, -1) <> COALESCE(f.status, -1)
               );",
        )?;
    }
    if from_version < 16 {
        // V16: Parent-based collection membership on media_entity.
        if !has_column(conn, "media_entity", "parent_collection_id")? {
            conn.execute_batch(
                "ALTER TABLE media_entity ADD COLUMN parent_collection_id INTEGER REFERENCES media_entity(entity_id) ON DELETE SET NULL",
            )?;
        }
        if !has_column(conn, "media_entity", "collection_ordinal")? {
            conn.execute_batch("ALTER TABLE media_entity ADD COLUMN collection_ordinal INTEGER")?;
        }

        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_media_entity_parent ON media_entity(parent_collection_id);
             CREATE INDEX IF NOT EXISTS idx_media_entity_parent_ord ON media_entity(parent_collection_id, collection_ordinal, entity_id);",
        )?;

        // Backfill from legacy collection_member if present.
        if table_exists(conn, "collection_member")? {
            conn.execute_batch(
                "UPDATE media_entity
                 SET parent_collection_id = (
                        SELECT cm.collection_entity_id
                        FROM collection_member cm
                        WHERE cm.member_entity_id = media_entity.entity_id
                        LIMIT 1
                     ),
                     collection_ordinal = (
                        SELECT cm.ordinal
                        FROM collection_member cm
                        WHERE cm.member_entity_id = media_entity.entity_id
                        LIMIT 1
                     )
                 WHERE kind = 'single'
                   AND parent_collection_id IS NULL
                   AND EXISTS (
                        SELECT 1
                        FROM collection_member cm
                        WHERE cm.member_entity_id = media_entity.entity_id
                   );",
            )?;
        }

        // Normalize invalid states.
        conn.execute_batch(
            "UPDATE media_entity
             SET parent_collection_id = NULL, collection_ordinal = NULL
             WHERE kind = 'collection';

             UPDATE media_entity
             SET collection_ordinal = NULL
             WHERE parent_collection_id IS NULL;",
        )?;

        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS trg_media_entity_parent_validate_insert
             BEFORE INSERT ON media_entity
             BEGIN
                 SELECT RAISE(ABORT, 'media_entity: collections cannot belong to a collection')
                 WHERE NEW.kind = 'collection' AND NEW.parent_collection_id IS NOT NULL;

                 SELECT RAISE(ABORT, 'media_entity: only singles can belong to a collection')
                 WHERE NEW.kind != 'single' AND NEW.parent_collection_id IS NOT NULL;

                 SELECT RAISE(ABORT, 'media_entity: parent must be kind=collection')
                 WHERE NEW.parent_collection_id IS NOT NULL
                   AND COALESCE(
                       (SELECT kind FROM media_entity WHERE entity_id = NEW.parent_collection_id),
                       ''
                   ) != 'collection';

                 SELECT RAISE(ABORT, 'media_entity: collection_ordinal requires parent_collection_id')
                 WHERE NEW.collection_ordinal IS NOT NULL AND NEW.parent_collection_id IS NULL;
             END;

             CREATE TRIGGER IF NOT EXISTS trg_media_entity_parent_validate_update
             BEFORE UPDATE OF kind, parent_collection_id, collection_ordinal ON media_entity
             BEGIN
                 SELECT RAISE(ABORT, 'media_entity: collections cannot belong to a collection')
                 WHERE NEW.kind = 'collection' AND NEW.parent_collection_id IS NOT NULL;

                 SELECT RAISE(ABORT, 'media_entity: only singles can belong to a collection')
                 WHERE NEW.kind != 'single' AND NEW.parent_collection_id IS NOT NULL;

                 SELECT RAISE(ABORT, 'media_entity: parent must be kind=collection')
                 WHERE NEW.parent_collection_id IS NOT NULL
                   AND COALESCE(
                       (SELECT kind FROM media_entity WHERE entity_id = NEW.parent_collection_id),
                       ''
                   ) != 'collection';

                 SELECT RAISE(ABORT, 'media_entity: collection_ordinal requires parent_collection_id')
                 WHERE NEW.collection_ordinal IS NOT NULL AND NEW.parent_collection_id IS NULL;
             END;",
        )?;
    }
    if from_version < 17 {
        // V17: Backfill entity_tag_raw, entity_tag_implied, entity_metadata_projection
        // from legacy file-keyed tables (PBI-157 renames). Also add pagination index.
        if table_exists(conn, "file_tag_raw")? {
            conn.execute_batch(
                "INSERT OR IGNORE INTO entity_tag_raw (entity_id, tag_id, source)
                 SELECT ef.entity_id, ftr.tag_id, ftr.source
                 FROM file_tag_raw ftr
                 INNER JOIN entity_file ef ON ef.file_id = ftr.file_id;",
            )?;
        }
        if table_exists(conn, "file_tag_implied")? {
            conn.execute_batch(
                "INSERT OR IGNORE INTO entity_tag_implied (entity_id, tag_id)
                 SELECT ef.entity_id, fti.tag_id
                 FROM file_tag_implied fti
                 INNER JOIN entity_file ef ON ef.file_id = fti.file_id;",
            )?;
        }
        if table_exists(conn, "file_metadata_projection")? {
            conn.execute_batch(
                "INSERT OR IGNORE INTO entity_metadata_projection (entity_id, epoch, resolved_json, parents_json)
                 SELECT ef.entity_id, fmp.epoch, fmp.resolved_json, fmp.parents_json
                 FROM file_metadata_projection fmp
                 INNER JOIN entity_file ef ON ef.file_id = fmp.file_id;",
            )?;
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_media_entity_status_entity_id ON media_entity(status, entity_id DESC);",
        )?;
    }
    if from_version < 18 {
        // V18: Add display settings columns to view_pref for per-scope tile display options.
        for col in &[
            "show_name",
            "show_resolution",
            "show_extension",
            "show_label",
        ] {
            if !has_column(conn, "view_pref", col)? {
                conn.execute_batch(&format!("ALTER TABLE view_pref ADD COLUMN {} INTEGER", col))?;
            }
        }
        if !has_column(conn, "view_pref", "thumbnail_fit")? {
            conn.execute_batch("ALTER TABLE view_pref ADD COLUMN thumbnail_fit TEXT")?;
        }
    }
    if from_version < 19 {
        // V19: Allow multiple entities to reference the same file (for duplicate repointing).
        // Drop the UNIQUE constraint on entity_file.file_id by recreating the table.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entity_file_new (
                 entity_id INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 file_id   INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE
             );
             INSERT OR IGNORE INTO entity_file_new (entity_id, file_id)
             SELECT entity_id, file_id FROM entity_file;
             DROP TABLE IF EXISTS entity_file;
             ALTER TABLE entity_file_new RENAME TO entity_file;
             CREATE INDEX IF NOT EXISTS idx_entity_file_file_id ON entity_file(file_id);",
        )?;
    }
    if from_version < 20 {
        // V20: Persist credential health status for subscription auth diagnostics.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS credential_health (
                 site_category   TEXT PRIMARY KEY,
                 health_status   TEXT NOT NULL,
                 last_checked_at TEXT NOT NULL,
                 last_error      TEXT
             );",
        )?;
    }
    if from_version < 25 {
        // V25: Folder auto-tags.
        if !has_column(conn, "folder", "auto_tags")? {
            conn.execute_batch(
                "ALTER TABLE folder ADD COLUMN auto_tags TEXT NOT NULL DEFAULT '[]'",
            )?;
        }
    }
    // V21: Denormalized collection cover/count/size for fast grid queries.
    // Run unconditionally with has_column guards — V22 was deployed before V21,
    // so some databases have version >= 21 but lack these columns.
    let needs_v21 = !has_column(conn, "media_entity", "cover_file_id")?;
    if needs_v21 {
        conn.execute_batch(
            "ALTER TABLE media_entity ADD COLUMN cover_file_id INTEGER REFERENCES file(file_id) ON DELETE SET NULL",
        )?;
        conn.execute_batch(
            "ALTER TABLE media_entity ADD COLUMN cached_item_count INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute_batch(
            "ALTER TABLE media_entity ADD COLUMN cached_total_size_bytes INTEGER NOT NULL DEFAULT 0",
        )?;
        // Backfill existing collections.
        conn.execute_batch(
            "UPDATE media_entity
             SET cover_file_id = (
                 SELECT ef2.file_id
                 FROM media_entity me_member
                 JOIN entity_file ef2 ON ef2.entity_id = me_member.entity_id
                 WHERE me_member.kind = 'single'
                   AND me_member.parent_collection_id = media_entity.entity_id
                 ORDER BY COALESCE(me_member.collection_ordinal, 9223372036854775807) ASC,
                          me_member.entity_id ASC
                 LIMIT 1
             ),
             cached_item_count = (
                 SELECT COUNT(*)
                 FROM media_entity me_member
                 WHERE me_member.kind = 'single'
                   AND me_member.parent_collection_id = media_entity.entity_id
             ),
             cached_total_size_bytes = (
                 SELECT COALESCE(SUM(f2.size), 0)
                 FROM media_entity me_member
                 JOIN entity_file ef2 ON ef2.entity_id = me_member.entity_id
                 JOIN file f2 ON f2.file_id = ef2.file_id
                 WHERE me_member.kind = 'single'
                   AND me_member.parent_collection_id = media_entity.entity_id
             )
             WHERE kind = 'collection'",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_media_entity_cover ON media_entity(cover_file_id) WHERE cover_file_id IS NOT NULL",
        )?;
    }
    if from_version < 22 {
        // V22: Persist subscription post -> collection mapping so incremental
        // runs can append to existing collections without rediscovering by hash.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subscription_post_collection (
                 subscription_id     INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
                 site_id             TEXT NOT NULL,
                 post_id             TEXT NOT NULL,
                 collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
                 created_at          TEXT NOT NULL,
                 updated_at          TEXT NOT NULL,
                 PRIMARY KEY (subscription_id, site_id, post_id)
             );
             CREATE INDEX IF NOT EXISTS idx_spc_collection ON subscription_post_collection(collection_entity_id);",
        )?;
    }
    if from_version < 24 {
        // V24: Repair illegal collection -> entity_file links from older builds.
        repair_collection_entity_file_links(conn)?;
    }
    conn.execute("UPDATE schema_version SET version = ?1", [CURRENT_VERSION])?;
    Ok(())
}

/// Reconcile schema drift for databases that may already report CURRENT_VERSION
/// but are missing tables/columns introduced in newer builds.
pub fn reconcile_schema(conn: &Connection) -> rusqlite::Result<()> {
    // Some legacy DBs still have `subscription.site_plugin_id` but not `site_id`.
    if table_exists(conn, "subscription")? && !has_column(conn, "subscription", "site_id")? {
        conn.execute_batch("ALTER TABLE subscription ADD COLUMN site_id TEXT")?;
        if has_column(conn, "subscription", "site_plugin_id")? {
            conn.execute_batch(
                "UPDATE subscription
                 SET site_id = site_plugin_id
                 WHERE site_id IS NULL OR TRIM(site_id) = ''",
            )?;
        } else {
            conn.execute_batch(
                "UPDATE subscription
                 SET site_id = 'unknown'
                 WHERE site_id IS NULL OR TRIM(site_id) = ''",
            )?;
        }
        tracing::warn!(
            "Reconciled subscription schema: added subscription.site_id and backfilled values"
        );
    }

    // Older libraries can miss subscription_query columns that newer code
    // reads unconditionally.
    if table_exists(conn, "subscription_query")? {
        if !has_column(conn, "subscription_query", "display_name")? {
            conn.execute_batch("ALTER TABLE subscription_query ADD COLUMN display_name TEXT")?;
            tracing::warn!("Reconciled subscription_query schema: added display_name");
        }
        if !has_column(conn, "subscription_query", "paused")? {
            conn.execute_batch(
                "ALTER TABLE subscription_query ADD COLUMN paused INTEGER NOT NULL DEFAULT 0",
            )?;
            tracing::warn!("Reconciled subscription_query schema: added paused");
        }
        if !has_column(conn, "subscription_query", "last_check_time")? {
            conn.execute_batch("ALTER TABLE subscription_query ADD COLUMN last_check_time TEXT")?;
            tracing::warn!("Reconciled subscription_query schema: added last_check_time");
        }
        if !has_column(conn, "subscription_query", "files_found")? {
            conn.execute_batch(
                "ALTER TABLE subscription_query ADD COLUMN files_found INTEGER NOT NULL DEFAULT 0",
            )?;
            tracing::warn!("Reconciled subscription_query schema: added files_found");
        }
        if !has_column(conn, "subscription_query", "completed_initial_run")? {
            conn.execute_batch(
                "ALTER TABLE subscription_query ADD COLUMN completed_initial_run INTEGER NOT NULL DEFAULT 0",
            )?;
            tracing::warn!("Reconciled subscription_query schema: added completed_initial_run");
        }
        if !has_column(conn, "subscription_query", "resume_cursor")? {
            conn.execute_batch("ALTER TABLE subscription_query ADD COLUMN resume_cursor TEXT")?;
            tracing::warn!("Reconciled subscription_query schema: added resume_cursor");
        }
        if !has_column(conn, "subscription_query", "resume_strategy")? {
            conn.execute_batch("ALTER TABLE subscription_query ADD COLUMN resume_strategy TEXT")?;
            tracing::warn!("Reconciled subscription_query schema: added resume_strategy");
        }
    }

    // Credential-domain table is required by subscription settings/runtime.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credential_domain (
             site_category   TEXT PRIMARY KEY,
             credential_type TEXT NOT NULL,
             display_name    TEXT,
             created_at      TEXT NOT NULL
         );",
    )?;

    // Ensure health table exists even on older/partial schemas.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credential_health (
             site_category   TEXT PRIMARY KEY,
             health_status   TEXT NOT NULL,
             last_checked_at TEXT NOT NULL,
             last_error      TEXT
         );",
    )?;

    if table_exists(conn, "folder")? && !has_column(conn, "folder", "auto_tags")? {
        conn.execute_batch(
            "ALTER TABLE folder ADD COLUMN auto_tags TEXT NOT NULL DEFAULT '[]'",
        )?;
        tracing::warn!("Reconciled folder schema: added auto_tags");
    }

    // Data reconciliation: some upgraded builds may have illegal collection rows
    // still linked through entity_file, which corrupts collection tile rendering.
    repair_collection_entity_file_links(conn)?;

    Ok(())
}

/// Repair legacy/corrupt states where a `collection` entity is still linked in
/// `entity_file`. Collections must never own a direct file row.
///
/// Strategy:
/// - move/recreate the linked file as a `single` member under the collection
/// - remove the illegal `entity_file` link from the collection entity
/// - re-sync collection aggregate metadata (cover/count/size/tag mirror)
fn repair_collection_entity_file_links(conn: &Connection) -> rusqlite::Result<()> {
    if !table_exists(conn, "media_entity")?
        || !table_exists(conn, "entity_file")?
        || !table_exists(conn, "file")?
    {
        return Ok(());
    }

    #[derive(Debug)]
    struct BadCollectionFileLink {
        collection_id: i64,
        file_id: i64,
        file_name: Option<String>,
        file_status: i64,
        file_rating: Option<i64>,
        imported_at: Option<String>,
    }

    let mut stmt = conn.prepare(
        "SELECT c.entity_id, ef.file_id, f.name, f.status, f.rating, f.imported_at
         FROM media_entity c
         JOIN entity_file ef ON ef.entity_id = c.entity_id
         JOIN file f ON f.file_id = ef.file_id
         WHERE c.kind = 'collection'
         ORDER BY c.entity_id",
    )?;
    let bad_links: Vec<BadCollectionFileLink> = stmt
        .query_map([], |row| {
            Ok(BadCollectionFileLink {
                collection_id: row.get(0)?,
                file_id: row.get(1)?,
                file_name: row.get(2)?,
                file_status: row.get(3)?,
                file_rating: row.get(4)?,
                imported_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if bad_links.is_empty() {
        return Ok(());
    }

    let mut repaired_count = 0usize;
    for link in bad_links {
        let max_ordinal: i64 = conn.query_row(
            "SELECT COALESCE(MAX(collection_ordinal), 0)
             FROM media_entity
             WHERE parent_collection_id = ?1",
            [link.collection_id],
            |row| row.get(0),
        )?;
        let next_ordinal = max_ordinal + 1;

        let existing_single = conn.query_row(
            "SELECT me.entity_id, me.parent_collection_id
             FROM entity_file ef
             JOIN media_entity me ON me.entity_id = ef.entity_id
             WHERE ef.file_id = ?1
               AND me.kind = 'single'
             LIMIT 1",
            [link.file_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        );

        match existing_single {
            Ok((single_entity_id, parent_collection_id)) => {
                if parent_collection_id.is_none() {
                    conn.execute(
                        "UPDATE media_entity
                         SET parent_collection_id = ?1,
                             collection_ordinal = ?2,
                             updated_at = CURRENT_TIMESTAMP
                         WHERE entity_id = ?3
                           AND kind = 'single'",
                        rusqlite::params![link.collection_id, next_ordinal, single_entity_id],
                    )?;
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                conn.execute(
                    "INSERT INTO media_entity (
                         kind, name, description, status, rating, created_at, updated_at,
                         parent_collection_id, collection_ordinal
                     ) VALUES (
                         'single', ?1, '', ?2, ?3, COALESCE(?4, CURRENT_TIMESTAMP), CURRENT_TIMESTAMP,
                         ?5, ?6
                     )",
                    rusqlite::params![
                        link.file_name,
                        link.file_status,
                        link.file_rating,
                        link.imported_at,
                        link.collection_id,
                        next_ordinal
                    ],
                )?;
                let new_entity_id = conn.last_insert_rowid();
                conn.execute(
                    "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (?1, ?2)",
                    rusqlite::params![new_entity_id, link.file_id],
                )?;
            }
            Err(e) => return Err(e),
        }

        conn.execute(
            "DELETE FROM entity_file WHERE entity_id = ?1",
            [link.collection_id],
        )?;
        crate::sqlite::collections::sync_collection_aggregate_metadata(conn, link.collection_id)?;
        repaired_count += 1;
    }

    if repaired_count > 0 {
        tracing::warn!(
            repaired_count,
            "Repaired collection rows with illegal direct file links"
        );
    }

    Ok(())
}

/// Check if a table has a specific column using PRAGMA table_info.
fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )
}

fn seed_manifest(conn: &Connection) -> rusqlite::Result<()> {
    let keys = [
        "global",
        "files",
        "tags",
        "tag_graph",
        "sidebar",
        "smart_folders",
        "bitmaps",
        "ptr_overlay",
    ];
    let mut stmt =
        conn.prepare_cached("INSERT OR IGNORE INTO manifest (key, epoch) VALUES (?1, 0)")?;
    for key in &keys {
        stmt.execute([key])?;
    }
    Ok(())
}

fn seed_artifact_manifest(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO artifact_manifest_meta (id, manifest_epoch, updated_at)
         VALUES (1, 0, CURRENT_TIMESTAMP)",
        [],
    )?;

    let current_epoch: i64 = conn.query_row(
        "SELECT manifest_epoch FROM artifact_manifest_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO artifact_manifest_entry
            (manifest_epoch, artifact_name, artifact_version, built_from_truth_seq, payload_json)
         VALUES (?1, ?2, 0, 0, ?3)",
    )?;

    let artifacts = [
        "global",
        "files",
        "tags",
        "tag_graph",
        "sidebar",
        "smart_folders",
        "bitmaps",
        "ptr_overlay",
    ];
    for artifact in &artifacts {
        let payload_json = if *artifact == "bitmaps" {
            r#"{"active_file":"bitmaps.bin"}"#
        } else {
            "{}"
        };
        stmt.execute(rusqlite::params![current_epoch, artifact, payload_json])?;
    }
    Ok(())
}

const PRAGMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -64000;
PRAGMA mmap_size = 268435456;
PRAGMA temp_store = MEMORY;
"#;

const LIBRARY_DDL: &str = r#"
-- ═══════════════════════════════════════════════════
-- FILES
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS file (
    file_id         INTEGER PRIMARY KEY,
    hash            TEXT    NOT NULL UNIQUE,
    name            TEXT,
    size            INTEGER NOT NULL,
    mime            TEXT    NOT NULL,
    width           INTEGER,
    height          INTEGER,
    duration_ms     INTEGER,
    num_frames      INTEGER,
    has_audio       INTEGER NOT NULL DEFAULT 0,
    blurhash        TEXT,
    status          INTEGER NOT NULL DEFAULT 0,
    rating          INTEGER,
    view_count      INTEGER NOT NULL DEFAULT 0,
    last_viewed_at  TEXT,
    phash           TEXT,
    imported_at     TEXT    NOT NULL,
    notes           TEXT,
    source_urls_json TEXT,
    dominant_color_hex TEXT,
    dominant_palette_blob BLOB,
    name_source TEXT NOT NULL DEFAULT 'unknown'
);
CREATE INDEX IF NOT EXISTS idx_file_status     ON file(status);
CREATE INDEX IF NOT EXISTS idx_file_imported   ON file(imported_at);
CREATE INDEX IF NOT EXISTS idx_file_size       ON file(size);
CREATE INDEX IF NOT EXISTS idx_file_rating     ON file(rating);
CREATE INDEX IF NOT EXISTS idx_file_view_count ON file(view_count);
CREATE INDEX IF NOT EXISTS idx_file_phash      ON file(phash) WHERE phash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_file_last_viewed ON file(last_viewed_at) WHERE last_viewed_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_file_mime       ON file(mime);
-- Composite indexes for grid pagination (status + sort column + file_id tiebreaker)
CREATE INDEX IF NOT EXISTS idx_file_status_imported  ON file(status, imported_at DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_viewed    ON file(status, last_viewed_at DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_rating    ON file(status, rating DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_size      ON file(status, size DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_viewcount ON file(status, view_count DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_name      ON file(status, name COLLATE NOCASE, file_id);

CREATE TABLE IF NOT EXISTS file_color (
    rowid   INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    hex     TEXT    NOT NULL,
    l       REAL    NOT NULL,
    a       REAL    NOT NULL,
    b       REAL    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fc_file ON file_color(file_id);
CREATE INDEX IF NOT EXISTS idx_fc_lab  ON file_color(l, a, b);

CREATE VIRTUAL TABLE IF NOT EXISTS file_color_rtree USING rtree(
    id,
    l_min, l_max,
    a_min, a_max,
    b_min, b_max
);

CREATE VIRTUAL TABLE IF NOT EXISTS file_fts USING fts5(
    name, notes, source_urls,
    content='file',
    content_rowid='file_id',
    tokenize='unicode61'
);

-- ═══════════════════════════════════════════════════
-- MEDIA ENTITIES (single + collection)
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS media_entity (
    entity_id    INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL CHECK(kind IN ('single','collection')),
    parent_collection_id INTEGER REFERENCES media_entity(entity_id) ON DELETE SET NULL,
    collection_ordinal   INTEGER,
    cover_file_id        INTEGER REFERENCES file(file_id) ON DELETE SET NULL,
    cached_item_count    INTEGER NOT NULL DEFAULT 0,
    cached_total_size_bytes INTEGER NOT NULL DEFAULT 0,
    name         TEXT,
    description  TEXT NOT NULL DEFAULT '',
    status       INTEGER NOT NULL DEFAULT 1,
    rating       INTEGER,
    created_at   TEXT,
    updated_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_media_entity_kind    ON media_entity(kind);
CREATE INDEX IF NOT EXISTS idx_media_entity_updated ON media_entity(updated_at);
CREATE INDEX IF NOT EXISTS idx_media_entity_status_entity_id ON media_entity(status, entity_id DESC);
CREATE INDEX IF NOT EXISTS idx_media_entity_parent ON media_entity(parent_collection_id);
CREATE INDEX IF NOT EXISTS idx_media_entity_parent_ord ON media_entity(parent_collection_id, collection_ordinal, entity_id);
CREATE INDEX IF NOT EXISTS idx_media_entity_cover ON media_entity(cover_file_id) WHERE cover_file_id IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS trg_media_entity_parent_validate_insert
BEFORE INSERT ON media_entity
BEGIN
    SELECT RAISE(ABORT, 'media_entity: collections cannot belong to a collection')
    WHERE NEW.kind = 'collection' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: only singles can belong to a collection')
    WHERE NEW.kind != 'single' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: parent must be kind=collection')
    WHERE NEW.parent_collection_id IS NOT NULL
      AND COALESCE(
          (SELECT kind FROM media_entity WHERE entity_id = NEW.parent_collection_id),
          ''
      ) != 'collection';

    SELECT RAISE(ABORT, 'media_entity: collection_ordinal requires parent_collection_id')
    WHERE NEW.collection_ordinal IS NOT NULL AND NEW.parent_collection_id IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS trg_media_entity_parent_validate_update
BEFORE UPDATE OF kind, parent_collection_id, collection_ordinal ON media_entity
BEGIN
    SELECT RAISE(ABORT, 'media_entity: collections cannot belong to a collection')
    WHERE NEW.kind = 'collection' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: only singles can belong to a collection')
    WHERE NEW.kind != 'single' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: parent must be kind=collection')
    WHERE NEW.parent_collection_id IS NOT NULL
      AND COALESCE(
          (SELECT kind FROM media_entity WHERE entity_id = NEW.parent_collection_id),
          ''
      ) != 'collection';

    SELECT RAISE(ABORT, 'media_entity: collection_ordinal requires parent_collection_id')
    WHERE NEW.collection_ordinal IS NOT NULL AND NEW.parent_collection_id IS NULL;
END;

CREATE TABLE IF NOT EXISTS entity_file (
    entity_id INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    file_id   INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_entity_file_file_id ON entity_file(file_id);
CREATE TRIGGER IF NOT EXISTS trg_entity_file_kind_check
BEFORE INSERT ON entity_file
BEGIN
    SELECT RAISE(ABORT, 'entity_file: entity must be kind=single')
    WHERE (SELECT kind FROM media_entity WHERE entity_id = NEW.entity_id) != 'single';
END;

CREATE TABLE IF NOT EXISTS collection_member (
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    member_entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    ordinal              INTEGER NOT NULL,
    PRIMARY KEY (collection_entity_id, member_entity_id)
);
CREATE INDEX IF NOT EXISTS idx_collection_member_order ON collection_member(collection_entity_id, ordinal);

CREATE TABLE IF NOT EXISTS collection_tag (
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag                  TEXT NOT NULL,
    PRIMARY KEY (collection_entity_id, tag)
);
CREATE INDEX IF NOT EXISTS idx_collection_tag_tag ON collection_tag(tag COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS collection_source_url (
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    url                  TEXT NOT NULL,
    PRIMARY KEY (collection_entity_id, url)
);
CREATE INDEX IF NOT EXISTS idx_collection_source_url ON collection_source_url(collection_entity_id);

-- ═══════════════════════════════════════════════════
-- TAGS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS tag (
    tag_id     INTEGER PRIMARY KEY,
    namespace  TEXT NOT NULL,
    subtag     TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    UNIQUE(namespace, subtag)
);
CREATE INDEX IF NOT EXISTS idx_tag_subtag     ON tag(subtag COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_tag_file_count ON tag(file_count) WHERE file_count > 0;

CREATE TABLE IF NOT EXISTS entity_tag_raw (
    entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag_id    INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source    TEXT NOT NULL DEFAULT 'local',
    PRIMARY KEY (entity_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_etr_tag ON entity_tag_raw(tag_id, entity_id);

CREATE TABLE IF NOT EXISTS tag_sibling (
    from_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    to_tag_id   INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source      TEXT NOT NULL,
    PRIMARY KEY (from_tag_id, source)
);

CREATE TABLE IF NOT EXISTS tag_parent (
    child_tag_id  INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    parent_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source        TEXT NOT NULL,
    PRIMARY KEY (child_tag_id, parent_tag_id, source)
);

CREATE TABLE IF NOT EXISTS tag_ancestor (
    tag_id      INTEGER NOT NULL,
    ancestor_id INTEGER NOT NULL,
    depth       INTEGER NOT NULL,
    PRIMARY KEY (tag_id, ancestor_id)
);
CREATE INDEX IF NOT EXISTS idx_ta_ancestor ON tag_ancestor(ancestor_id, tag_id);

CREATE TABLE IF NOT EXISTS tag_display (
    tag_id     INTEGER PRIMARY KEY,
    display_ns TEXT NOT NULL,
    display_st TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS entity_tag_implied (
    entity_id INTEGER NOT NULL,
    tag_id    INTEGER NOT NULL,
    PRIMARY KEY (entity_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_eti_tag ON entity_tag_implied(tag_id, entity_id);

CREATE VIRTUAL TABLE IF NOT EXISTS tag_fts USING fts5(
    namespace, subtag,
    content='tag',
    content_rowid='tag_id',
    tokenize='unicode61'
);

-- ═══════════════════════════════════════════════════
-- FOLDERS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS folder (
    folder_id  INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    parent_id  INTEGER REFERENCES folder(folder_id) ON DELETE SET NULL,
    icon       TEXT,
    color      TEXT,
    auto_tags  TEXT NOT NULL DEFAULT '[]',
    sort_order INTEGER,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE IF NOT EXISTS folder_entity (
    folder_id     INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
    entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    position_rank INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (folder_id, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_fe_rank ON folder_entity(folder_id, position_rank);

-- ═══════════════════════════════════════════════════
-- SMART FOLDERS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS smart_folder (
    smart_folder_id INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    icon            TEXT,
    color           TEXT,
    predicate_json  TEXT NOT NULL,
    sort_field      TEXT,
    sort_order      TEXT,
    display_order   INTEGER,
    created_at      TEXT,
    updated_at      TEXT
);

-- ═══════════════════════════════════════════════════
-- FLOWS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS flow (
    flow_id    INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    schedule   TEXT NOT NULL DEFAULT 'manual',
    created_at TEXT NOT NULL
);

-- ═══════════════════════════════════════════════════
-- SUBSCRIPTIONS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS subscription (
    subscription_id         INTEGER PRIMARY KEY,
    name                    TEXT NOT NULL,
    site_id                 TEXT NOT NULL,
    paused                  INTEGER NOT NULL DEFAULT 0,
    flow_id                 INTEGER REFERENCES flow(flow_id) ON DELETE CASCADE,
    initial_file_limit      INTEGER NOT NULL DEFAULT 100,
    periodic_file_limit     INTEGER NOT NULL DEFAULT 50,
    created_at              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS subscription_query (
    query_id              INTEGER PRIMARY KEY,
    subscription_id       INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_text            TEXT NOT NULL,
    display_name          TEXT,
    paused                INTEGER NOT NULL DEFAULT 0,
    last_check_time       TEXT,
    files_found           INTEGER NOT NULL DEFAULT 0,
    completed_initial_run INTEGER NOT NULL DEFAULT 0,
    resume_cursor         TEXT,
    resume_strategy       TEXT
);
CREATE INDEX IF NOT EXISTS idx_sq_sub ON subscription_query(subscription_id);

CREATE TABLE IF NOT EXISTS subscription_entity (
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    entity_id       INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    PRIMARY KEY (subscription_id, entity_id)
);

CREATE TABLE IF NOT EXISTS subscription_post_collection (
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    site_id              TEXT NOT NULL,
    post_id              TEXT NOT NULL,
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    PRIMARY KEY (subscription_id, site_id, post_id)
);
CREATE INDEX IF NOT EXISTS idx_spc_collection ON subscription_post_collection(collection_entity_id);

-- ═══════════════════════════════════════════════════
-- CREDENTIALS (domain list; actual secrets in OS keychain)
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS credential_domain (
    site_category   TEXT PRIMARY KEY,
    credential_type TEXT NOT NULL,
    display_name    TEXT,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS credential_health (
    site_category   TEXT PRIMARY KEY,
    health_status   TEXT NOT NULL,
    last_checked_at TEXT NOT NULL,
    last_error      TEXT
);

-- ═══════════════════════════════════════════════════
-- DUPLICATES
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS duplicate (
    file_id_a      INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    file_id_b      INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    distance       REAL    NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'detected',
    decision_at    TEXT,
    decision_source TEXT,
    decision_reason TEXT,
    winner_file_id INTEGER,
    loser_file_id  INTEGER,
    PRIMARY KEY (file_id_a, file_id_b),
    CHECK (file_id_a < file_id_b)
);
CREATE INDEX IF NOT EXISTS idx_dup_b      ON duplicate(file_id_b);
CREATE INDEX IF NOT EXISTS idx_dup_status ON duplicate(status);

-- ═══════════════════════════════════════════════════
-- SIDEBAR PROJECTION
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS sidebar_node (
    node_id             TEXT PRIMARY KEY,
    kind                TEXT NOT NULL,
    parent_id           TEXT,
    name                TEXT NOT NULL,
    icon                TEXT,
    color               TEXT,
    sort_order          INTEGER,
    count               INTEGER,
    freshness           TEXT NOT NULL DEFAULT 'stale',
    epoch               INTEGER NOT NULL DEFAULT 0,
    selectable          INTEGER NOT NULL DEFAULT 1,
    expanded_by_default INTEGER NOT NULL DEFAULT 0,
    meta_json           TEXT,
    updated_at          TEXT
);

-- ═══════════════════════════════════════════════════
-- METADATA PROJECTION
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS entity_metadata_projection (
    entity_id     INTEGER PRIMARY KEY,
    epoch         INTEGER NOT NULL,
    resolved_json TEXT NOT NULL,
    parents_json  TEXT NOT NULL
);

-- ═══════════════════════════════════════════════════
-- VIEW PREFERENCES
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS view_pref (
    scope           TEXT PRIMARY KEY,
    sort_field      TEXT,
    sort_dir        TEXT,
    layout          TEXT,
    tile_size       INTEGER,
    show_name       INTEGER,
    show_resolution INTEGER,
    show_extension  INTEGER,
    show_label      INTEGER,
    thumbnail_fit   TEXT
);

-- ═══════════════════════════════════════════════════
-- LARGE MUTATION TRACKING
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS mutation_action (
    action_id   INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'running',
    total       INTEGER NOT NULL DEFAULT 0,
    progress    INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    created_at  TEXT NOT NULL,
    finished_at TEXT
);

-- ═══════════════════════════════════════════════════
-- MANIFEST (global epoch tracking)
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS manifest (
    key   TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL DEFAULT 0
);

-- Global manifest snapshot metadata (V2)
CREATE TABLE IF NOT EXISTS artifact_manifest_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    manifest_epoch INTEGER NOT NULL DEFAULT 0,
    updated_at     TEXT
);

CREATE TABLE IF NOT EXISTS artifact_manifest_entry (
    manifest_epoch      INTEGER NOT NULL,
    artifact_name       TEXT NOT NULL,
    artifact_version    INTEGER NOT NULL,
    built_from_truth_seq INTEGER NOT NULL DEFAULT 0,
    payload_json        TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (manifest_epoch, artifact_name)
);

-- ═══════════════════════════════════════════════════
-- KV SETTINGS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS kv_settings (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- Schema version
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
INSERT OR IGNORE INTO schema_version (version) VALUES (25);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_init_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        // All core tables should exist
        let expected_tables = [
            "file",
            "file_color",
            "media_entity",
            "entity_file",
            "collection_member",
            "collection_tag",
            "tag",
            "entity_tag_raw",
            "tag_sibling",
            "tag_parent",
            "tag_ancestor",
            "tag_display",
            "entity_tag_implied",
            "folder",
            "folder_entity",
            "smart_folder",
            "flow",
            "subscription",
            "subscription_query",
            "subscription_entity",
            "subscription_post_collection",
            "credential_domain",
            "credential_health",
            "duplicate",
            "sidebar_node",
            "entity_metadata_projection",
            "view_pref",
            "mutation_action",
            "manifest",
            "kv_settings",
            "schema_version",
            "artifact_manifest_meta",
            "artifact_manifest_entry",
        ];

        for table in &expected_tables {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "Table '{table}' should exist after init_schema");
        }

        // FTS5 + R*Tree virtual tables
        for vt in &["file_fts", "tag_fts", "file_color_rtree"] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [vt],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                exists,
                "Virtual table '{vt}' should exist after init_schema"
            );
        }

        // Schema version should be current version
        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, Some(CURRENT_VERSION));

        // Manifest should be seeded
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM manifest", [], |row| row.get(0))
            .unwrap();
        assert!(
            count >= 8,
            "Manifest should have at least 8 seeded keys, got {count}"
        );

        // Global manifest snapshot metadata should be seeded
        let published_epoch: i64 = conn
            .query_row(
                "SELECT manifest_epoch FROM artifact_manifest_meta WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(published_epoch, 0);
    }

    #[test]
    fn schema_init_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();
        // Second init should not fail (IF NOT EXISTS)
        init_schema(&conn).unwrap();
        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, Some(CURRENT_VERSION));
    }

    /// Verify the full V1→CURRENT migration path is safe on an already-current schema.
    /// This ensures every migration step uses IF NOT EXISTS / has_column guards so
    /// re-running migrations never fails (dry-run validation).
    #[test]
    fn migrations_are_idempotent_from_v1() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        // Simulate a V1 database that needs all migrations applied.
        conn.execute("UPDATE schema_version SET version = 1", [])
            .unwrap();

        // Run full migration chain — should succeed because all steps are guarded.
        run_migrations(&conn, 1).unwrap();

        let version = get_schema_version(&conn).unwrap();
        assert_eq!(version, Some(CURRENT_VERSION));

        // Verify V4 migration artifacts (flow table + flow_id column)
        assert!(has_column(&conn, "subscription", "flow_id").unwrap());

        // Verify V5 migration artifacts
        assert!(has_column(&conn, "file", "last_viewed_at").unwrap());

        // Verify V7 migration artifacts
        assert!(has_column(&conn, "duplicate", "decision_at").unwrap());
        assert!(has_column(&conn, "file", "name_source").unwrap());

        // Verify V8 composite indexes exist
        let index_names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='file'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for expected in &[
            "idx_file_status_rating",
            "idx_file_status_size",
            "idx_file_status_viewcount",
            "idx_file_status_name",
        ] {
            assert!(
                index_names.iter().any(|n| n == expected),
                "Index {expected} should exist after V8 migration"
            );
        }

        // Verify V10 collection/entity schema artifacts.
        for table in &[
            "media_entity",
            "entity_file",
            "collection_member",
            "collection_tag",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "Table '{table}' should exist after V10 migration");
        }

        // Verify V12 entity projection/tag artifacts.
        for table in &[
            "entity_tag_raw",
            "entity_tag_implied",
            "entity_metadata_projection",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "Table '{table}' should exist after V12 migration");
        }

        // Verify V21 denormalized collection metadata columns.
        assert!(has_column(&conn, "media_entity", "cover_file_id").unwrap());
        assert!(has_column(&conn, "media_entity", "cached_item_count").unwrap());
        assert!(has_column(&conn, "media_entity", "cached_total_size_bytes").unwrap());

        // Verify V13 link tables for upgraded libraries.
        for table in &["folder_entity", "subscription_entity"] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "Table '{table}' should exist after V13 migration");
        }
    }

    #[test]
    fn v14_backfills_entity_links_from_legacy_tables() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        // Seed minimal graph: one folder, one flow+subscription, one file with single-entity mapping.
        conn.execute("INSERT INTO folder (folder_id, name) VALUES (1, 'f')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO flow (flow_id, name, schedule, created_at) VALUES (1, 'flow', 'manual', CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, site_id, paused, flow_id, initial_file_limit, periodic_file_limit, created_at)
             VALUES (1, 'sub', 'x', 0, 1, 100, 50, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO file (file_id, hash, name, size, mime, has_audio, status, view_count, imported_at)
             VALUES (100, 'h100', 'n100', 1, 'image/png', 0, 1, 0, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             VALUES (100, 'single', 'n100', '', 1, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (100, 100)",
            [],
        )
        .unwrap();

        // Legacy membership tables that older DBs still contain.
        conn.execute_batch(
            "CREATE TABLE folder_file (
                folder_id INTEGER NOT NULL,
                file_id INTEGER NOT NULL,
                position_rank INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (folder_id, file_id)
            );
            CREATE TABLE subscription_file (
                subscription_id INTEGER NOT NULL,
                file_id INTEGER NOT NULL,
                PRIMARY KEY (subscription_id, file_id)
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO folder_file (folder_id, file_id, position_rank) VALUES (1, 100, 7)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subscription_file (subscription_id, file_id) VALUES (1, 100)",
            [],
        )
        .unwrap();

        // Simulate pre-v14 schema and run upgrade.
        conn.execute("UPDATE schema_version SET version = 13", [])
            .unwrap();
        run_migrations(&conn, 13).unwrap();

        let folder_links: i64 = conn
            .query_row("SELECT COUNT(*) FROM folder_entity WHERE folder_id = 1 AND entity_id = 100 AND position_rank = 7", [], |row| row.get(0))
            .unwrap();
        assert_eq!(folder_links, 1);

        let sub_links: i64 = conn
            .query_row("SELECT COUNT(*) FROM subscription_entity WHERE subscription_id = 1 AND entity_id = 100", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sub_links, 1);
    }

    #[test]
    fn v15_syncs_single_entity_status_from_file() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO file (file_id, hash, size, mime, has_audio, status, view_count, imported_at)
             VALUES (200, 'h200', 1, 'image/png', 0, 2, 0, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             VALUES (200, 'single', '', '', 1, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (200, 200)",
            [],
        )
        .unwrap();

        conn.execute("UPDATE schema_version SET version = 14", [])
            .unwrap();
        run_migrations(&conn, 14).unwrap();

        let status: i64 = conn
            .query_row(
                "SELECT status FROM media_entity WHERE entity_id = 200",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, 2);
    }

    #[test]
    fn v16_backfills_parent_collection_membership() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO media_entity (entity_id, kind, status, created_at, updated_at)
             VALUES (900, 'collection', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media_entity (entity_id, kind, status, created_at, updated_at)
             VALUES (901, 'single', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO collection_member (collection_entity_id, member_entity_id, ordinal)
             VALUES (900, 901, 7)",
            [],
        )
        .unwrap();

        conn.execute("UPDATE schema_version SET version = 15", [])
            .unwrap();
        run_migrations(&conn, 15).unwrap();

        let parent: Option<i64> = conn
            .query_row(
                "SELECT parent_collection_id FROM media_entity WHERE entity_id = 901",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let ordinal: Option<i64> = conn
            .query_row(
                "SELECT collection_ordinal FROM media_entity WHERE entity_id = 901",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent, Some(900));
        assert_eq!(ordinal, Some(7));
    }
}
