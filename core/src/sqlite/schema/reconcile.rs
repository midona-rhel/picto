//! Drift repair for databases that already report the current schema version.

use rusqlite::Connection;

use super::support::{
    has_column, repair_collection_entity_file_links, seed_artifact_manifest, seed_manifest,
    table_exists,
};
/// Reconcile schema drift for databases that may already report CURRENT_VERSION
/// but are missing tables/columns introduced in newer builds.
pub fn reconcile_schema(conn: &Connection) -> rusqlite::Result<()> {
    // Legacy tag-graph drift:
    // - older builds used `tag_sibling` / `tag_parent`
    // - some partial schemas can miss `tag_alias` / `tag_implication` entirely
    if table_exists(conn, "tag_sibling")? {
        if !table_exists(conn, "tag_alias")? {
            conn.execute_batch("ALTER TABLE tag_sibling RENAME TO tag_alias")?;
            tracing::warn!("Reconciled tag schema: renamed legacy tag_sibling table to tag_alias");
        } else {
            conn.execute_batch("DROP TABLE IF EXISTS tag_sibling")?;
            tracing::warn!("Reconciled tag schema: dropped stale legacy tag_sibling table");
        }
    }

    if table_exists(conn, "tag_parent")? {
        if !table_exists(conn, "tag_implication")? {
            conn.execute_batch("ALTER TABLE tag_parent RENAME TO tag_implication")?;
            tracing::warn!(
                "Reconciled tag schema: renamed legacy tag_parent table to tag_implication"
            );
        } else {
            conn.execute_batch("DROP TABLE IF EXISTS tag_parent")?;
            tracing::warn!("Reconciled tag schema: dropped stale legacy tag_parent table");
        }
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tag_alias (
             from_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             to_tag_id   INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             source      TEXT NOT NULL DEFAULT 'manual',
             PRIMARY KEY (from_tag_id, source)
         );
         CREATE TABLE IF NOT EXISTS tag_implication (
             child_tag_id  INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             parent_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             source        TEXT NOT NULL DEFAULT 'manual',
             PRIMARY KEY (child_tag_id, parent_tag_id, source)
         );",
    )?;

    // Legacy subscription-group drift:
    // - older builds used `flow` / `flow_id`
    // - some partial schemas can miss `subscription_group` entirely
    if table_exists(conn, "flow")? {
        if !table_exists(conn, "subscription_group")? {
            conn.execute_batch("ALTER TABLE flow RENAME TO subscription_group")?;
            if has_column(conn, "subscription_group", "flow_id")?
                && !has_column(conn, "subscription_group", "group_id")?
            {
                conn.execute_batch(
                    "ALTER TABLE subscription_group RENAME COLUMN flow_id TO group_id",
                )?;
            }
            tracing::warn!(
                "Reconciled subscription schema: renamed legacy flow table to subscription_group"
            );
        } else {
            conn.execute_batch("DROP TABLE IF EXISTS flow")?;
            tracing::warn!("Reconciled subscription schema: dropped stale legacy flow table");
        }
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS subscription_group (
             group_id   INTEGER PRIMARY KEY,
             name       TEXT NOT NULL,
             schedule   TEXT NOT NULL DEFAULT 'manual',
             created_at TEXT NOT NULL
         );",
    )?;

    if table_exists(conn, "subscription")?
        && has_column(conn, "subscription", "flow_id")?
        && !has_column(conn, "subscription", "group_id")?
    {
        conn.execute_batch("ALTER TABLE subscription RENAME COLUMN flow_id TO group_id")?;
        tracing::warn!("Reconciled subscription schema: renamed subscription.flow_id to group_id");
    }

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
    if table_exists(conn, "credential_domain")? {
        if !has_column(conn, "credential_domain", "display_name")? {
            conn.execute_batch("ALTER TABLE credential_domain ADD COLUMN display_name TEXT")?;
            tracing::warn!("Reconciled credential schema: added credential_domain.display_name");
        }
        if !has_column(conn, "credential_domain", "created_at")? {
            conn.execute_batch(
                "ALTER TABLE credential_domain ADD COLUMN created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
            )?;
            tracing::warn!("Reconciled credential schema: added credential_domain.created_at");
        }
    }

    // Ensure health table exists even on older/partial schemas.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credential_health (
             site_category   TEXT PRIMARY KEY,
             health_status   TEXT NOT NULL,
             last_checked_at TEXT NOT NULL,
             last_error      TEXT
         );",
    )?;
    if table_exists(conn, "credential_health")? {
        if !has_column(conn, "credential_health", "last_checked_at")? {
            conn.execute_batch(
                "ALTER TABLE credential_health ADD COLUMN last_checked_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
            )?;
            tracing::warn!("Reconciled credential schema: added credential_health.last_checked_at");
        }
        if !has_column(conn, "credential_health", "last_error")? {
            conn.execute_batch("ALTER TABLE credential_health ADD COLUMN last_error TEXT")?;
            tracing::warn!("Reconciled credential schema: added credential_health.last_error");
        }
    }

    // Current-version libraries can still be partially migrated if an older
    // build advanced schema_version without landing every derived/read-model
    // table. Recreate the current canonical tables here so compilers and
    // sidebar reads do not fail later with missing-table errors.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tag_ancestor (
             tag_id      INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             ancestor_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             depth       INTEGER NOT NULL,
             PRIMARY KEY (tag_id, ancestor_id)
         );
         CREATE INDEX IF NOT EXISTS idx_ta_ancestor ON tag_ancestor(ancestor_id, tag_id);

         CREATE TABLE IF NOT EXISTS tag_display (
             tag_id      INTEGER PRIMARY KEY REFERENCES tag(tag_id) ON DELETE CASCADE,
             display_ns  TEXT NOT NULL,
             display_st  TEXT NOT NULL
         );

         CREATE TABLE IF NOT EXISTS entity_tag_implied (
             entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
             tag_id    INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
             PRIMARY KEY (entity_id, tag_id)
         );
         CREATE INDEX IF NOT EXISTS idx_eti_tag ON entity_tag_implied(tag_id, entity_id);

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

         CREATE TABLE IF NOT EXISTS entity_metadata_projection (
             entity_id     INTEGER PRIMARY KEY,
             epoch         INTEGER NOT NULL,
             resolved_json TEXT NOT NULL,
             parents_json  TEXT NOT NULL
         );

         CREATE TABLE IF NOT EXISTS manifest (
             key   TEXT PRIMARY KEY,
             epoch INTEGER NOT NULL DEFAULT 0
         );

         CREATE TABLE IF NOT EXISTS artifact_manifest_meta (
             id             INTEGER PRIMARY KEY CHECK (id = 1),
             manifest_epoch INTEGER NOT NULL DEFAULT 0,
             updated_at     TEXT
         );

         CREATE TABLE IF NOT EXISTS artifact_manifest_entry (
             manifest_epoch       INTEGER NOT NULL,
             artifact_name        TEXT NOT NULL,
             artifact_version     INTEGER NOT NULL,
             built_from_truth_seq INTEGER NOT NULL DEFAULT 0,
             payload_json         TEXT NOT NULL DEFAULT '{}',
             PRIMARY KEY (manifest_epoch, artifact_name)
         );

         CREATE TABLE IF NOT EXISTS kv_settings (
             key   TEXT PRIMARY KEY,
             value TEXT
         );",
    )?;
    seed_manifest(conn)?;
    seed_artifact_manifest(conn)?;

    if table_exists(conn, "folder")? && !has_column(conn, "folder", "auto_tags")? {
        conn.execute_batch("ALTER TABLE folder ADD COLUMN auto_tags TEXT NOT NULL DEFAULT '[]'")?;
        tracing::warn!("Reconciled folder schema: added auto_tags");
    }
    if table_exists(conn, "folder")? && !has_column(conn, "folder", "watch_path")? {
        conn.execute_batch("ALTER TABLE folder ADD COLUMN watch_path TEXT")?;
        tracing::warn!("Reconciled folder schema: added watch_path");
    }
    if table_exists(conn, "folder")? && !has_column(conn, "folder", "watch_enabled")? {
        conn.execute_batch(
            "ALTER TABLE folder ADD COLUMN watch_enabled INTEGER NOT NULL DEFAULT 0",
        )?;
        tracing::warn!("Reconciled folder schema: added watch_enabled");
    }
    if table_exists(conn, "folder")? && !has_column(conn, "folder", "watch_subfolders")? {
        conn.execute_batch(
            "ALTER TABLE folder ADD COLUMN watch_subfolders INTEGER NOT NULL DEFAULT 0",
        )?;
        tracing::warn!("Reconciled folder schema: added watch_subfolders");
    }
    if table_exists(conn, "folder")? && !has_column(conn, "folder", "watch_import_status_mode")? {
        conn.execute_batch(
            "ALTER TABLE folder ADD COLUMN watch_import_status_mode TEXT NOT NULL DEFAULT 'inherit'",
        )?;
        tracing::warn!("Reconciled folder schema: added watch_import_status_mode");
    }
    if table_exists(conn, "folder")? {
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_folder_watch_path
             ON folder(watch_path)
             WHERE watch_path IS NOT NULL",
        )?;
    }
    if table_exists(conn, "smart_folder")? && !has_column(conn, "smart_folder", "parent_id")? {
        conn.execute_batch(
            "ALTER TABLE smart_folder ADD COLUMN parent_id INTEGER REFERENCES smart_folder(smart_folder_id) ON DELETE SET NULL",
        )?;
        tracing::warn!("Reconciled smart_folder schema: added parent_id");
    }
    if table_exists(conn, "smart_folder")? {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_smart_folder_parent_order
             ON smart_folder(parent_id, COALESCE(display_order, smart_folder_id), smart_folder_id)",
        )?;
    }

    if table_exists(conn, "subscription")? && !has_column(conn, "subscription", "auto_collections")?
    {
        conn.execute_batch(
            "ALTER TABLE subscription ADD COLUMN auto_collections INTEGER NOT NULL DEFAULT 1",
        )?;
        tracing::warn!("Reconciled subscription schema: added auto_collections");
    }

    if table_exists(conn, "subscription_query")?
        && !has_column(conn, "subscription_query", "posts_found")?
    {
        conn.execute_batch(
            "ALTER TABLE subscription_query ADD COLUMN posts_found INTEGER NOT NULL DEFAULT 0",
        )?;
        tracing::warn!("Reconciled subscription_query schema: added posts_found");
    }

    // Download queue tables (PBI-539)
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

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deferred_work (
             work_id        INTEGER PRIMARY KEY,
             hash           TEXT NOT NULL,
             work_type      TEXT NOT NULL CHECK(work_type IN ('thumbnail', 'dominant_colors', 'phash')),
             status         TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'running')),
             attempt_count  INTEGER NOT NULL DEFAULT 0,
             available_at   TEXT NOT NULL,
             last_error     TEXT,
             created_at     TEXT NOT NULL,
             updated_at     TEXT NOT NULL,
             UNIQUE(hash, work_type)
         );
         CREATE INDEX IF NOT EXISTS idx_deferred_work_ready
             ON deferred_work(status, available_at, work_id);",
    )?;

    // Data reconciliation: some upgraded builds may have illegal collection rows
    // still linked through entity_file, which corrupts collection tile rendering.
    repair_collection_entity_file_links(conn)?;

    // Ensure collection created_at reflects the actual library import time
    // (earliest member file.imported_at), not content origin dates from gallery-dl.
    // Runs every startup to guard against any code path that might corrupt it.
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

    Ok(())
}
