//! Historical schema upgrade steps, grouped behind one deterministic runner.

use rusqlite::{params, Connection};

use super::support::{has_column, repair_collection_entity_file_links, table_exists};
use super::CURRENT_VERSION;
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
    if from_version < 26 {
        // V26: Rename tag_sibling → tag_alias, tag_parent → tag_implication,
        //      flow → subscription_group, flow_id → group_id.
        if table_exists(conn, "tag_sibling")? {
            conn.execute_batch("ALTER TABLE tag_sibling RENAME TO tag_alias")?;
        }
        if table_exists(conn, "tag_parent")? {
            conn.execute_batch("ALTER TABLE tag_parent RENAME TO tag_implication")?;
        }
        if table_exists(conn, "flow")? {
            if !table_exists(conn, "subscription_group")? {
                conn.execute_batch("ALTER TABLE flow RENAME TO subscription_group")?;
                conn.execute_batch(
                    "ALTER TABLE subscription_group RENAME COLUMN flow_id TO group_id",
                )?;
            } else {
                // DDL already created subscription_group; drop stale v4-created flow table.
                conn.execute_batch("DROP TABLE IF EXISTS flow")?;
            }
        }
        if has_column(conn, "subscription", "flow_id")?
            && !has_column(conn, "subscription", "group_id")?
        {
            conn.execute_batch("ALTER TABLE subscription RENAME COLUMN flow_id TO group_id")?;
        }
    }
    if from_version < 27 && has_column(conn, "file", "blurhash")? {
        // V27: Remove legacy blurhash storage. The product now uses dominant color
        // placeholders only, so the column and all related drift can go away.
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP TABLE IF EXISTS file_fts;

             CREATE TABLE file_new (
                 file_id              INTEGER PRIMARY KEY,
                 hash                 TEXT    NOT NULL UNIQUE,
                 name                 TEXT,
                 size                 INTEGER NOT NULL,
                 mime                 TEXT    NOT NULL,
                 width                INTEGER,
                 height               INTEGER,
                 duration_ms          INTEGER,
                 num_frames           INTEGER,
                 has_audio            INTEGER NOT NULL DEFAULT 0,
                 status               INTEGER NOT NULL DEFAULT 0,
                 rating               INTEGER,
                 view_count           INTEGER NOT NULL DEFAULT 0,
                 last_viewed_at       TEXT,
                 phash                TEXT,
                 imported_at          TEXT    NOT NULL,
                 notes                TEXT,
                 source_urls_json     TEXT,
                 dominant_color_hex   TEXT,
                 dominant_palette_blob BLOB,
                 name_source          TEXT NOT NULL DEFAULT 'unknown'
             );

             INSERT INTO file_new (
                 file_id, hash, name, size, mime, width, height, duration_ms,
                 num_frames, has_audio, status, rating, view_count,
                 last_viewed_at, phash, imported_at, notes, source_urls_json,
                 dominant_color_hex, dominant_palette_blob, name_source
             )
             SELECT
                 file_id, hash, name, size, mime, width, height, duration_ms,
                 num_frames, has_audio, status, rating, view_count,
                 last_viewed_at, phash, imported_at, notes, source_urls_json,
                 dominant_color_hex, dominant_palette_blob, name_source
             FROM file;

             DROP TABLE file;
             ALTER TABLE file_new RENAME TO file;

             CREATE INDEX IF NOT EXISTS idx_file_status     ON file(status);
             CREATE INDEX IF NOT EXISTS idx_file_imported   ON file(imported_at);
             CREATE INDEX IF NOT EXISTS idx_file_size       ON file(size);
             CREATE INDEX IF NOT EXISTS idx_file_rating     ON file(rating);
             CREATE INDEX IF NOT EXISTS idx_file_view_count ON file(view_count);
             CREATE INDEX IF NOT EXISTS idx_file_phash      ON file(phash) WHERE phash IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_file_last_viewed ON file(last_viewed_at) WHERE last_viewed_at IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_file_mime       ON file(mime);
             CREATE INDEX IF NOT EXISTS idx_file_status_imported  ON file(status, imported_at DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_viewed    ON file(status, last_viewed_at DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_rating    ON file(status, rating DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_size      ON file(status, size DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_viewcount ON file(status, view_count DESC, file_id DESC);
             CREATE INDEX IF NOT EXISTS idx_file_status_name      ON file(status, name COLLATE NOCASE, file_id);

             CREATE VIRTUAL TABLE file_fts USING fts5(
                 name, notes, source_urls,
                 content='file',
                 content_rowid='file_id',
                 tokenize='unicode61'
             );
             INSERT INTO file_fts(rowid, name, notes, source_urls)
             SELECT file_id, name, notes, source_urls_json FROM file;
             PRAGMA foreign_keys = ON;",
        )?;
    }
    if from_version < 28 {
        if !has_column(conn, "folder", "watch_path")? {
            conn.execute_batch("ALTER TABLE folder ADD COLUMN watch_path TEXT")?;
        }
        if !has_column(conn, "folder", "watch_enabled")? {
            conn.execute_batch(
                "ALTER TABLE folder ADD COLUMN watch_enabled INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if !has_column(conn, "folder", "watch_subfolders")? {
            conn.execute_batch(
                "ALTER TABLE folder ADD COLUMN watch_subfolders INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if !has_column(conn, "folder", "watch_import_status_mode")? {
            conn.execute_batch(
                "ALTER TABLE folder ADD COLUMN watch_import_status_mode TEXT NOT NULL DEFAULT 'inherit'",
            )?;
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_folder_watch_path
             ON folder(watch_path)
             WHERE watch_path IS NOT NULL",
        )?;
    }
    if from_version < 29 {
        if !has_column(conn, "smart_folder", "parent_id")? {
            conn.execute_batch(
                "ALTER TABLE smart_folder ADD COLUMN parent_id INTEGER REFERENCES smart_folder(smart_folder_id) ON DELETE SET NULL",
            )?;
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_smart_folder_parent_order
             ON smart_folder(parent_id, COALESCE(display_order, smart_folder_id), smart_folder_id)",
        )?;
    }
    if from_version < 31 {
        // V30-31: Repair collection created_at timestamps.
        // sync_collection_aggregate_metadata used to overwrite created_at with
        // MIN(member.created_at) from media_entity, which was the content origin
        // date (from gallery-dl), not the library import date. This made collections
        // sort as years-old items.
        // Fix: set created_at to earliest member file.imported_at — the actual
        // time the first file in this collection was added to the library.
        conn.execute_batch(
            "UPDATE media_entity
             SET created_at = COALESCE(
                 (SELECT MIN(f_m.imported_at)
                  FROM media_entity me_m
                  JOIN entity_file ef_m ON ef_m.entity_id = me_m.entity_id
                  JOIN file f_m ON f_m.file_id = ef_m.file_id
                  WHERE me_m.kind = 'single'
                    AND me_m.parent_collection_id = media_entity.entity_id),
                 created_at)
             WHERE kind = 'collection'",
        )?;
    }
    if from_version < 32 {
        // V32: Re-repair collection created_at after ratchet code was removed.
        // Previous migration ran but sync_collection_aggregate_metadata re-corrupted
        // the values before the ratchet was removed. Same fix, re-applied.
        conn.execute_batch(
            "UPDATE media_entity
             SET created_at = COALESCE(
                 (SELECT MIN(f_m.imported_at)
                  FROM media_entity me_m
                  JOIN entity_file ef_m ON ef_m.entity_id = me_m.entity_id
                  JOIN file f_m ON f_m.file_id = ef_m.file_id
                  WHERE me_m.kind = 'single'
                    AND me_m.parent_collection_id = media_entity.entity_id),
                 created_at)
             WHERE kind = 'collection'",
        )?;
    }
    if from_version < 33 {
        // V33: Per-subscription toggle for auto-creating collections from multi-image posts.
        if !has_column(conn, "subscription", "auto_collections")? {
            conn.execute_batch(
                "ALTER TABLE subscription ADD COLUMN auto_collections INTEGER NOT NULL DEFAULT 1",
            )?;
        }
    }
    if from_version < 34 {
        // V34: Rename file_limit → post_limit to clarify semantics (limits posts, not files).
        if has_column(conn, "subscription", "initial_file_limit")? {
            conn.execute_batch(
                "ALTER TABLE subscription RENAME COLUMN initial_file_limit TO initial_post_limit;
                 ALTER TABLE subscription RENAME COLUMN periodic_file_limit TO periodic_post_limit;",
            )?;
        }
    }
    if from_version < 35 {
        // V35: Persistent download queue for interrupted collection imports.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS download_queue (
                queue_id        INTEGER PRIMARY KEY,
                subscription_id INTEGER NOT NULL,
                query_id        INTEGER,
                post_id         TEXT NOT NULL,
                category        TEXT NOT NULL,
                preferred_name  TEXT,
                expected_count  INTEGER,
                status          TEXT NOT NULL DEFAULT 'pending',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS download_queue_item (
                item_id     INTEGER PRIMARY KEY,
                queue_id    INTEGER NOT NULL REFERENCES download_queue(queue_id) ON DELETE CASCADE,
                blob_hash   TEXT,
                page_num    INTEGER,
                metadata    TEXT,
                status      TEXT NOT NULL DEFAULT 'pending',
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_dqi_queue ON download_queue_item(queue_id);",
        )?;
    }
    // ── V36: Collection entity hash identity + drop collection_tag ──
    if from_version < 36 {
        // 1. Add hash column to media_entity
        if !has_column(conn, "media_entity", "hash")? {
            conn.execute_batch("ALTER TABLE media_entity ADD COLUMN hash TEXT;")?;
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_media_entity_hash ON media_entity(hash) WHERE hash IS NOT NULL;",
        )?;

        // 2. Backfill hashes for existing collections
        {
            let mut stmt =
                conn.prepare("SELECT entity_id FROM media_entity WHERE kind = 'collection'")?;
            let ids: Vec<i64> = stmt
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut update =
                conn.prepare("UPDATE media_entity SET hash = ?1 WHERE entity_id = ?2")?;
            for id in ids {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(format!("collection:{id}").as_bytes());
                let hash = hex::encode(hasher.finalize());
                update.execute(params![hash, id])?;
            }
        }

        // 3. Migrate collection_tag → entity_tag_raw, then drop
        if table_exists(conn, "collection_tag")? {
            let mut stmt = conn.prepare(
                "SELECT collection_entity_id, tag FROM collection_tag",
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (entity_id, tag) in rows {
                if let Some((ns, st)) = crate::tags::normalize::parse_tag(&tag) {
                    let tag_id: i64 = match conn.query_row(
                        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                        params![ns, st],
                        |row| row.get(0),
                    ) {
                        Ok(id) => id,
                        Err(_) => {
                            conn.execute(
                                "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)",
                                params![ns, st],
                            )?;
                            conn.last_insert_rowid()
                        }
                    };
                    conn.execute(
                        "INSERT OR IGNORE INTO entity_tag_raw (entity_id, tag_id, source) VALUES (?1, ?2, 'local')",
                        params![entity_id, tag_id],
                    )?;
                }
            }
            conn.execute_batch("DROP TABLE IF EXISTS collection_tag;")?;
        }
    }

    conn.execute("UPDATE schema_version SET version = ?1", [CURRENT_VERSION])?;
    Ok(())
}
