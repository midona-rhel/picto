//! Event-driven compiler loop.
//! Receives structured change signals from write operations and
//! schedules the appropriate projection rebuilds.

use rusqlite::{params, Connection};

use super::bitmaps::{BitmapKey, BitmapStore};

/// What needs to be rebuilt after a batch of changes.
#[derive(Debug, Default)]
pub struct CompilerPlan {
    pub rebuild_status: bool,
    pub rebuild_all_tags: bool,
    pub dirty_tag_ids: Vec<i64>,
    pub rebuild_tag_graph: bool,
    pub rebuild_all_smart_folders: bool,
    pub dirty_smart_folder_ids: Vec<i64>,
    pub rebuild_sidebar: bool,
    pub rebuild_all: bool,
}

impl CompilerPlan {
    pub fn is_empty(&self) -> bool {
        !self.rebuild_status
            && !self.rebuild_all_tags
            && self.dirty_tag_ids.is_empty()
            && !self.rebuild_tag_graph
            && !self.rebuild_all_smart_folders
            && self.dirty_smart_folder_ids.is_empty()
            && !self.rebuild_sidebar
            && !self.rebuild_all
    }
}

/// Execute a compiler plan against the database and bitmap store.
pub fn execute_plan(conn: &Connection, bitmaps: &BitmapStore, plan: &CompilerPlan) {
    if plan.rebuild_all || plan.rebuild_status {
        super::tags::compile_status_bitmaps(conn, bitmaps);
    }

    if plan.rebuild_all || plan.rebuild_all_tags {
        super::tags::compile_all_tag_bitmaps(conn, bitmaps);
    } else {
        for tag_id in &plan.dirty_tag_ids {
            super::tags::compile_tag_bitmap(conn, bitmaps, *tag_id);
        }
    }

    if plan.rebuild_all || plan.rebuild_tag_graph {
        super::tags::compile_implied_tags(conn, bitmaps);
    }

    if plan.rebuild_all || plan.rebuild_all_tags || plan.rebuild_tag_graph || plan.rebuild_status {
        super::tags::compile_tagged_bitmap(conn, bitmaps);
    }

    if plan.rebuild_all || plan.rebuild_all_smart_folders {
        super::smart_folders::compile_all_smart_folders(conn, bitmaps);
    } else {
        for sf_id in &plan.dirty_smart_folder_ids {
            super::smart_folders::compile_smart_folder(conn, bitmaps, *sf_id);
        }
    }

    if plan.rebuild_all || plan.rebuild_sidebar {
        super::sidebar::compile_sidebar(conn, bitmaps);
    }

    // Update cached total_size_bytes on folder and smart_folder tables
    // whenever relevant projections are rebuilt.
    let needs_size_update = plan.rebuild_all
        || plan.rebuild_sidebar
        || plan.rebuild_all_smart_folders
        || !plan.dirty_smart_folder_ids.is_empty();
    if needs_size_update {
        update_cached_scope_sizes(conn, bitmaps);
    }
}

/// Update the cached `total_size_bytes` column on `folder` and `smart_folder`
/// tables. For folders this is a single batch UPDATE using folder_member joins.
/// For smart folders we read each bitmap and compute the SUM via a temp table
/// of entity IDs per smart folder.
fn update_cached_scope_sizes(conn: &Connection, bitmaps: &BitmapStore) {
    // Folders — single batch statement, no per-folder loop.
    let folder_result = conn.execute_batch(
        "UPDATE folder SET total_size_bytes = COALESCE((
            SELECT SUM(COALESCE(mf.size_bytes, me.total_size_bytes, 0))
            FROM folder_member fm
            JOIN media_entity me ON me.entity_id = fm.entity_id
            LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
            LEFT JOIN media_file mf ON mf.file_id = sme.file_id
            WHERE fm.folder_id = folder.folder_id
        ), 0)",
    );
    if let Err(e) = folder_result {
        tracing::warn!(error = %e, "Failed to update folder total_size_bytes");
    }

    // Smart folders — read bitmap entity IDs, insert into temp table, compute SUM.
    let sf_ids: Vec<i64> = conn
        .prepare("SELECT smart_folder_id FROM smart_folder")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    if sf_ids.is_empty() {
        return;
    }

    let _ = conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _sf_entity (sf_id INTEGER NOT NULL, entity_id INTEGER NOT NULL)",
    );
    let _ = conn.execute("DELETE FROM temp._sf_entity", []);

    {
        let mut insert_stmt =
            match conn.prepare("INSERT INTO temp._sf_entity (sf_id, entity_id) VALUES (?1, ?2)") {
                Ok(stmt) => stmt,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to prepare smart folder size temp insert");
                    return;
                }
            };

        for sf_id in &sf_ids {
            let bitmap = bitmaps.get(&BitmapKey::SmartFolder(*sf_id));
            for eid in bitmap.iter() {
                let _ = insert_stmt.execute(params![sf_id, eid as i64]);
            }
        }
    }

    let sf_update_result = conn.execute_batch(
        "UPDATE smart_folder SET total_size_bytes = COALESCE((
            SELECT SUM(COALESCE(mf.size_bytes, me.total_size_bytes, 0))
            FROM temp._sf_entity sfe
            JOIN media_entity me ON me.entity_id = sfe.entity_id
            LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
            LEFT JOIN media_file mf ON mf.file_id = sme.file_id
            WHERE sfe.sf_id = smart_folder.smart_folder_id
        ), 0)",
    );
    if let Err(e) = sf_update_result {
        tracing::warn!(error = %e, "Failed to update smart_folder total_size_bytes");
    }

    let _ = conn.execute_batch("DROP TABLE IF EXISTS temp._sf_entity");
}

/// Full rebuild of all projections from authoritative data.
pub fn full_rebuild(conn: &Connection, bitmaps: &BitmapStore) {
    execute_plan(
        conn,
        bitmaps,
        &CompilerPlan {
            rebuild_all: true,
            rebuild_sidebar: true,
            ..Default::default()
        },
    );
}
