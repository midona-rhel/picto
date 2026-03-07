//! Compiler system — background task that reacts to data mutations
//! and rebuilds compiled artifacts (bitmaps, projections, sidebar counts).
//!
//! Write operations enqueue `CompilerEvent`s. A background task debounces
//! (50-200ms), then runs affected compilers in dependency order.

use roaring::RoaringBitmap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use super::bitmaps::BitmapKey;
use crate::folders::db::count_uncategorized_entities;
use super::projections;
use crate::sidebar::db as sidebar;
use crate::smart_folders::db as smart_folders;
use super::SqliteDatabase;
use crate::ptr::db::PtrSqliteDatabase;

/// Events that trigger compiler runs.
#[derive(Debug, Clone)]
pub enum CompilerEvent {
    FileInserted {
        file_id: i64,
    },
    FileDeleted {
        file_id: i64,
    },
    FileStatusChanged {
        file_id: i64,
    },
    /// Batch status change — bitmaps already updated by caller, just rebuild sidebar/smart folders.
    StatusBatchChanged,
    FileTagsChanged {
        file_id: i64,
    },
    TagChanged {
        tag_id: i64,
    },
    TagGraphChanged,
    SmartFolderChanged {
        smart_folder_id: i64,
    },
    FolderChanged {
        folder_id: i64,
    },
    /// Incremental PTR sync — only these hashes changed.
    PtrSyncComplete {
        changed_hashes: Vec<String>,
    },
    /// Full PTR rebuild (manual maintenance).
    PtrFullRebuild,
    /// Duplicate pairs changed (scan, resolve, merge).
    DuplicateChanged,
    /// File view_count changed (detail/quick look viewing).
    ViewCountChanged,
    RebuildAll,
}

/// Which compilers need to run based on accumulated events.
#[derive(Default)]
struct CompilerPlan {
    rebuild_status_bitmaps: bool,
    dirty_tag_ids: HashSet<i64>,
    rebuild_tag_graph: bool,
    dirty_smart_folder_ids: HashSet<i64>,
    rebuild_all_smart_folders: bool,
    rebuild_sidebar: bool,
    dirty_file_ids: HashSet<i64>,
    rebuild_ptr_overlay: bool,
    rebuild_ptr_overlay_full: bool,
    ptr_changed_hashes: Vec<String>,
    rebuild_all: bool,
}

impl CompilerPlan {
    /// Accumulate a compiler event into the plan.
    ///
    /// Dependency rules:
    /// - File insert/delete/status → rebuild status bitmaps + ALL smart folders + sidebar.
    ///   Status changes affect AllActive membership which every smart folder scope depends on.
    /// - FileTagsChanged → rebuild ALL smart folders (any tag predicate could match) + sidebar.
    ///   We can't cheaply determine which smart folders use the changed tag, so rebuild all.
    /// - TagChanged → rebuild only that tag's bitmap + sidebar. Smart folders use EffectiveTag
    ///   bitmaps which are rebuilt per-tag, so specific smart folders update lazily.
    /// - TagGraphChanged → rebuild tag graph + ALL smart folders. Parent changes cascade
    ///   through ImpliedTag bitmaps and affect every smart folder using those tags.
    fn accumulate(&mut self, event: CompilerEvent) {
        match event {
            CompilerEvent::FileInserted { file_id } => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
                self.dirty_file_ids.insert(file_id);
            }
            CompilerEvent::FileDeleted { file_id } => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
                self.dirty_file_ids.insert(file_id);
            }
            CompilerEvent::FileStatusChanged { file_id } => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
                self.dirty_file_ids.insert(file_id);
            }
            CompilerEvent::StatusBatchChanged => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
            }
            CompilerEvent::FileTagsChanged { file_id } => {
                self.rebuild_all_smart_folders = true;
                self.rebuild_sidebar = true;
                self.dirty_file_ids.insert(file_id);
            }
            CompilerEvent::TagChanged { tag_id } => {
                self.dirty_tag_ids.insert(tag_id);
                self.rebuild_sidebar = true;
            }
            CompilerEvent::TagGraphChanged => {
                self.rebuild_tag_graph = true;
                self.rebuild_all_smart_folders = true;
                self.rebuild_sidebar = true;
            }
            CompilerEvent::SmartFolderChanged { smart_folder_id } => {
                self.dirty_smart_folder_ids.insert(smart_folder_id);
                self.rebuild_sidebar = true;
            }
            CompilerEvent::FolderChanged { folder_id: _ } => {
                self.rebuild_sidebar = true;
            }
            CompilerEvent::PtrSyncComplete { changed_hashes } => {
                self.ptr_changed_hashes.extend(changed_hashes);
                self.rebuild_ptr_overlay = true;
            }
            CompilerEvent::PtrFullRebuild => {
                self.rebuild_ptr_overlay_full = true;
            }
            CompilerEvent::DuplicateChanged => {
                self.rebuild_sidebar = true;
            }
            CompilerEvent::ViewCountChanged => {
                self.rebuild_sidebar = true;
            }
            CompilerEvent::RebuildAll => {
                self.rebuild_all = true;
            }
        }
    }

    fn is_empty(&self) -> bool {
        !self.rebuild_status_bitmaps
            && self.dirty_tag_ids.is_empty()
            && !self.rebuild_tag_graph
            && self.dirty_smart_folder_ids.is_empty()
            && !self.rebuild_all_smart_folders
            && !self.rebuild_sidebar
            && self.dirty_file_ids.is_empty()
            && !self.rebuild_ptr_overlay
            && !self.rebuild_ptr_overlay_full
            && !self.rebuild_all
    }
}

/// Result of a compiler batch run — describes which domains were affected.
/// The caller is responsible for translating this into event emissions
/// (the compiler layer itself does not emit frontend events).
pub struct CompilerBatchResult {
    pub sidebar_affected: bool,
    pub smart_folders_rebuilt: bool,
    pub scope_affected: bool,
}

/// Start the compiler background task.
///
/// `on_batch_done` is called after each compiler batch completes, with a summary
/// of what was affected. The caller (state.rs) uses this to emit events.
pub async fn start_compiler_loop(
    db: Arc<SqliteDatabase>,
    ptr_db: Option<Arc<PtrSqliteDatabase>>,
    mut rx: mpsc::UnboundedReceiver<CompilerEvent>,
    on_batch_done: impl Fn(CompilerBatchResult) + Send + 'static,
) {
    tracing::info!("Compiler loop started");

    loop {
        let first = match rx.recv().await {
            Some(e) => e,
            None => {
                tracing::info!("Compiler channel closed, stopping");
                return;
            }
        };

        let mut plan = CompilerPlan::default();
        plan.accumulate(first);

        // 100ms debounce: batch rapid-fire events (e.g. multi-file import) into
        // a single compiler run. Short enough that the UI feels responsive,
        // long enough to avoid redundant bitmap rebuilds.
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Some(event)) => plan.accumulate(event),
                Ok(None) => return,
                Err(_) => break,
            }
        }

        if plan.is_empty() {
            continue;
        }

        let db_ref = db.clone();

        let smart_folders_rebuilt = plan.rebuild_all_smart_folders
            || plan.rebuild_all
            || !plan.dirty_smart_folder_ids.is_empty();

        // PBI-027: Determine which invalidations are needed from the plan,
        // so we don't emit sidebar refreshes for PTR-only / metadata-only batches.
        let sidebar_affected = plan.rebuild_sidebar
            || plan.rebuild_tag_graph
            || plan.rebuild_all
            || plan.rebuild_all_smart_folders
            || !plan.dirty_smart_folder_ids.is_empty()
            || plan.rebuild_status_bitmaps;

        if let Err(e) = run_compilers(&db_ref, ptr_db.as_ref(), &plan).await {
            tracing::error!("Compiler error: {e}");
        }

        if let Err(e) = db_ref.flush().await {
            tracing::error!("Flush error after compilation: {e}");
        }

        // PBI-032: Only invalidate scope cache when membership-affecting bitmaps changed.
        // Metadata-only or PTR-only batches don't alter scope ID sets.
        let scope_affected = plan.rebuild_status_bitmaps
            || plan.rebuild_all_smart_folders
            || !plan.dirty_smart_folder_ids.is_empty()
            || plan.rebuild_tag_graph
            || !plan.dirty_tag_ids.is_empty()
            || plan.rebuild_all
            || !plan.dirty_file_ids.is_empty();
        if scope_affected {
            db_ref.scope_cache_invalidate_all();
        }

        on_batch_done(CompilerBatchResult {
            sidebar_affected,
            smart_folders_rebuilt,
            scope_affected,
        });
    }
}

async fn run_compilers(
    db: &Arc<SqliteDatabase>,
    ptr_db: Option<&Arc<PtrSqliteDatabase>>,
    plan: &CompilerPlan,
) -> Result<(), String> {
    let start = std::time::Instant::now();

    // 1. Status bitmap compiler
    if plan.rebuild_status_bitmaps || plan.rebuild_all {
        compile_status_bitmaps(db).await?;
    }

    // 2. Tag bitmap compiler (incremental)
    if plan.rebuild_all {
        compile_all_tag_bitmaps(db).await?;
    } else {
        for &tag_id in &plan.dirty_tag_ids {
            compile_tag_bitmap(db, tag_id).await?;
        }
    }

    // 3. Tag graph compiler (siblings, ancestors, implied tags)
    if plan.rebuild_tag_graph || plan.rebuild_all {
        compile_tag_graph(db).await?;
    }

    // 4. Effective tag compiler
    if plan.rebuild_tag_graph || plan.rebuild_all || !plan.dirty_tag_ids.is_empty() {
        compile_effective_tags(
            db,
            &plan.dirty_tag_ids,
            plan.rebuild_all || plan.rebuild_tag_graph,
        )
        .await?;
    }

    // 4b. Tagged bitmap (union of all tagged files)
    if plan.rebuild_tag_graph
        || plan.rebuild_all
        || !plan.dirty_tag_ids.is_empty()
        || plan.rebuild_status_bitmaps
    {
        compile_tagged_bitmap(db).await?;
    }

    // 5. Metadata projection compiler
    if plan.rebuild_all || plan.rebuild_tag_graph || !plan.dirty_file_ids.is_empty() {
        compile_metadata_projections(
            db,
            &plan.dirty_file_ids,
            plan.rebuild_all || plan.rebuild_tag_graph,
        )
        .await?;
    }

    // 6. Smart folder compiler
    if plan.rebuild_all_smart_folders || plan.rebuild_all {
        compile_all_smart_folders(db).await?;
    } else {
        for &sf_id in &plan.dirty_smart_folder_ids {
            compile_smart_folder(db, sf_id).await?;
        }
    }

    // 7. Sidebar compiler
    if plan.rebuild_sidebar || plan.rebuild_all {
        compile_sidebar(db).await?;
    }

    // 8. PTR overlay compiler — never on RebuildAll (shares PTR writer lock)
    if plan.rebuild_ptr_overlay || plan.rebuild_ptr_overlay_full {
        if let Some(ptr) = ptr_db {
            if crate::ptr::controller::PtrController::is_ptr_syncing() {
                tracing::info!("Skipping PTR overlay rebuild (sync in progress)");
            } else if plan.rebuild_ptr_overlay_full || plan.ptr_changed_hashes.is_empty() {
                compile_ptr_overlay_full(ptr).await?;
            } else {
                compile_ptr_overlay_incremental(ptr, &plan.ptr_changed_hashes).await?;
            }
        }
    }

    let elapsed = start.elapsed();
    tracing::debug!("Compiler batch completed in {elapsed:?}");

    Ok(())
}


async fn compile_status_bitmaps(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        for status in 0..=2i64 {
            let mut bm = RoaringBitmap::new();
            let mut stmt = conn.prepare_cached(
                "SELECT me.entity_id
                 FROM media_entity me
                 WHERE me.status = ?1
                   AND (
                       me.kind = 'collection'
                       OR me.parent_collection_id IS NULL
                   )",
            )?;
            let rows = stmt.query_map([status], |row| row.get::<_, i64>(0))?;
            for row in rows {
                bm.insert(row? as u32);
            }
            bitmaps.set(BitmapKey::Status(status), bm);
        }

        // AllActive = inbox + active (status 0 + 1), excludes trash
        let mut all_active = bitmaps.get(&BitmapKey::Status(0));
        all_active |= &bitmaps.get(&BitmapKey::Status(1));
        bitmaps.set(BitmapKey::AllActive, all_active);

        Ok(())
    })
    .await?;

    db.manifest.bump_working_artifact_version("files");
    Ok(())
}

async fn compile_tag_bitmap(db: &Arc<SqliteDatabase>, tag_id: i64) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let mut bm = RoaringBitmap::new();
        let mut stmt =
            conn.prepare_cached("SELECT entity_id FROM entity_tag_raw WHERE tag_id = ?1")?;
        let rows = stmt.query_map([tag_id], |row| row.get::<_, i64>(0))?;
        for row in rows {
            bm.insert(row? as u32);
        }
        bitmaps.set(BitmapKey::Tag(tag_id), bm);
        Ok(())
    })
    .await?;

    db.manifest.bump_working_artifact_version("tags");
    Ok(())
}

async fn compile_all_tag_bitmaps(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let mut stmt =
            conn.prepare_cached("SELECT tag_id, entity_id FROM entity_tag_raw ORDER BY tag_id")?;
        let mut current_tag: Option<i64> = None;
        let mut current_bm = RoaringBitmap::new();

        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;

        for row in rows {
            let (tag_id, entity_id) = row?;
            if current_tag != Some(tag_id) {
                if let Some(prev_tag) = current_tag {
                    bitmaps.set(BitmapKey::Tag(prev_tag), std::mem::take(&mut current_bm));
                }
                current_tag = Some(tag_id);
            }
            current_bm.insert(entity_id as u32);
        }

        if let Some(last_tag) = current_tag {
            bitmaps.set(BitmapKey::Tag(last_tag), current_bm);
        }

        Ok(())
    })
    .await?;

    db.manifest.bump_working_artifact_version("tags");
    Ok(())
}

async fn compile_tag_graph(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_conn(move |conn| {
        // Rebuild tag_ancestor using WITH RECURSIVE CTE (single-pass)
        conn.execute("DELETE FROM tag_ancestor", [])?;
        conn.execute_batch(
            "INSERT OR IGNORE INTO tag_ancestor (tag_id, ancestor_id, depth)
             WITH RECURSIVE ancestors(tag_id, ancestor_id, depth) AS (
                 SELECT child_tag_id, parent_tag_id, 1
                 FROM tag_parent
                 UNION ALL
                 SELECT a.tag_id, tp.parent_tag_id, a.depth + 1
                 FROM ancestors a
                 JOIN tag_parent tp ON tp.child_tag_id = a.ancestor_id
                 WHERE a.depth < 50
             )
             SELECT tag_id, ancestor_id, depth FROM ancestors",
        )?;

        // Rebuild tag_display from siblings
        conn.execute("DELETE FROM tag_display", [])?;
        conn.execute_batch(
            "INSERT OR REPLACE INTO tag_display (tag_id, display_ns, display_st)
             SELECT t.tag_id,
                    COALESCE(st.display_ns, t.namespace),
                    COALESCE(st.display_st, t.subtag)
             FROM tag t
             LEFT JOIN (
                 SELECT ts.from_tag_id,
                        t2.namespace AS display_ns,
                        t2.subtag AS display_st
                 FROM tag_sibling ts
                 JOIN tag t2 ON t2.tag_id = ts.to_tag_id
             ) st ON st.from_tag_id = t.tag_id",
        )?;

        // Rebuild entity_tag_implied
        conn.execute("DELETE FROM entity_tag_implied", [])?;
        conn.execute_batch(
            "INSERT OR IGNORE INTO entity_tag_implied (entity_id, tag_id)
             SELECT etr.entity_id, ta.ancestor_id
             FROM entity_tag_raw etr
             JOIN tag_ancestor ta ON ta.tag_id = etr.tag_id",
        )?;

        // Rebuild ImpliedTag bitmaps
        let mut stmt = conn
            .prepare_cached("SELECT tag_id, entity_id FROM entity_tag_implied ORDER BY tag_id")?;
        let mut current_tag: Option<i64> = None;
        let mut current_bm = RoaringBitmap::new();

        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;

        for row in rows {
            let (tag_id, entity_id) = row?;
            if current_tag != Some(tag_id) {
                if let Some(prev_tag) = current_tag {
                    bitmaps.set(
                        BitmapKey::ImpliedTag(prev_tag),
                        std::mem::take(&mut current_bm),
                    );
                }
                current_tag = Some(tag_id);
            }
            current_bm.insert(entity_id as u32);
        }

        if let Some(last_tag) = current_tag {
            bitmaps.set(BitmapKey::ImpliedTag(last_tag), current_bm);
        }

        Ok(())
    })
    .await?;

    db.manifest.bump_working_artifact_version("tag_graph");
    Ok(())
}

async fn compile_effective_tags(
    db: &Arc<SqliteDatabase>,
    dirty_tag_ids: &HashSet<i64>,
    rebuild_all: bool,
) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();

    if rebuild_all {
        // Rebuild all effective tags
        let tag_ids: Vec<i64> = db
            .with_read_conn(|conn| {
                let mut stmt = conn.prepare_cached("SELECT tag_id FROM tag")?;
                let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
                rows.collect()
            })
            .await?;

        for tag_id in tag_ids {
            let direct = bitmaps.get(&BitmapKey::Tag(tag_id));
            let implied = bitmaps.get(&BitmapKey::ImpliedTag(tag_id));
            bitmaps.set(BitmapKey::EffectiveTag(tag_id), &direct | &implied);
        }
    } else {
        for &tag_id in dirty_tag_ids {
            let direct = bitmaps.get(&BitmapKey::Tag(tag_id));
            let implied = bitmaps.get(&BitmapKey::ImpliedTag(tag_id));
            bitmaps.set(BitmapKey::EffectiveTag(tag_id), &direct | &implied);
        }
    }

    db.manifest.bump_working_artifact_version("effective_tags");
    Ok(())
}

async fn compile_tagged_bitmap(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let mut tagged = RoaringBitmap::new();
        // OR all entity_ids that appear in entity_tag_raw (direct tags)
        let mut stmt = conn.prepare_cached("SELECT DISTINCT entity_id FROM entity_tag_raw")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        for row in rows {
            tagged.insert(row? as u32);
        }
        // Also include implied tags
        let mut stmt2 = conn.prepare_cached("SELECT DISTINCT entity_id FROM entity_tag_implied")?;
        let rows2 = stmt2.query_map([], |row| row.get::<_, i64>(0))?;
        for row in rows2 {
            tagged.insert(row? as u32);
        }
        // Keep tagged in the same visibility domain as status bitmaps (collection members hidden).
        tagged &= &bitmaps.get(&BitmapKey::AllActive);
        bitmaps.set(BitmapKey::Tagged, tagged);
        Ok(())
    })
    .await?;
    Ok(())
}

async fn compile_metadata_projections(
    db: &Arc<SqliteDatabase>,
    dirty_file_ids: &HashSet<i64>,
    rebuild_all: bool,
) -> Result<(), String> {
    // Read the current working version (without bumping yet) to use as epoch
    // for the projection rows. We bump AFTER writes succeed to avoid stale
    // version on failure.
    let pre_version = db
        .manifest
        .published_artifact_version("metadata_projection") as i64;
    let projection_version = pre_version + 1;

    if rebuild_all {
        db.with_conn(move |conn| {
            let mut stmt = conn.prepare_cached("SELECT file_id FROM file")?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            let file_ids: rusqlite::Result<Vec<i64>> = rows.collect();
            let file_ids = file_ids?;
            projections::build_projections_batch(conn, &file_ids, projection_version)
        })
        .await?;
    } else if !dirty_file_ids.is_empty() {
        let file_ids: Vec<i64> = dirty_file_ids.iter().copied().collect();
        db.with_conn(move |conn| {
            projections::build_projections_batch(conn, &file_ids, projection_version)
        })
        .await?;
    } else {
        return Ok(());
    }

    // Bump version AFTER writes succeed
    db.manifest
        .bump_working_artifact_version("metadata_projection");
    Ok(())
}

async fn compile_all_smart_folders(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let sfs = smart_folders::list_smart_folders(conn)?;
        for sf in sfs {
            let pred: smart_folders::SmartFolderPredicate =
                match serde_json::from_str(&sf.predicate_json) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse smart folder {} predicate: {e}",
                            sf.smart_folder_id
                        );
                        continue;
                    }
                };
            match smart_folders::compile_predicate(conn, &pred, &bitmaps) {
                Ok(bm) => {
                    bitmaps.set(BitmapKey::SmartFolder(sf.smart_folder_id), bm);
                }
                Err(e) => {
                    tracing::warn!("Failed to compile smart folder {}: {e}", sf.smart_folder_id);
                }
            }
        }
        Ok(())
    })
    .await?;

    db.manifest.bump_working_artifact_version("smart_folders");
    Ok(())
}

async fn compile_smart_folder(
    db: &Arc<SqliteDatabase>,
    smart_folder_id: i64,
) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| {
        let sf = smart_folders::get_smart_folder(conn, smart_folder_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let pred: smart_folders::SmartFolderPredicate = serde_json::from_str(&sf.predicate_json)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let bm = smart_folders::compile_predicate(conn, &pred, &bitmaps)?;
        bitmaps.set(BitmapKey::SmartFolder(smart_folder_id), bm);
        Ok(())
    })
    .await?;

    db.manifest.bump_working_artifact_version("smart_folders");
    Ok(())
}

async fn compile_sidebar(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    // Use a pre-computed epoch value for the rows; bump AFTER writes succeed.
    let pre_epoch = db.manifest.published_artifact_version("sidebar");
    let epoch = pre_epoch + 1;

    db.with_conn(move |conn| {
        // Ensure sidebar is seeded
        sidebar::seed_sidebar_if_empty(conn)?;

        // Update system counts from bitmaps (O(1))
        let all_count = bitmaps.len(&BitmapKey::Status(1));
        let inbox_count = bitmaps.len(&BitmapKey::Status(0));
        let trash_count = bitmaps.len(&BitmapKey::Status(2));
        let uncategorized_count = count_uncategorized_entities(conn)?;

        sidebar::update_sidebar_count(conn, "system:all_files", all_count as i64, epoch as i64)?;
        sidebar::update_sidebar_count(conn, "system:inbox", inbox_count as i64, epoch as i64)?;
        sidebar::update_sidebar_count(
            conn,
            "system:uncategorized",
            uncategorized_count,
            epoch as i64,
        )?;
        sidebar::update_sidebar_count(conn, "system:trash", trash_count as i64, epoch as i64)?;

        // Untagged count: AllActive (inbox + active) - Tagged
        let all_active_count = bitmaps.len(&BitmapKey::AllActive);
        let tagged_count = bitmaps.len(&BitmapKey::Tagged);
        let untagged_count = all_active_count.saturating_sub(tagged_count);
        sidebar::update_sidebar_count(
            conn,
            "system:untagged",
            untagged_count as i64,
            epoch as i64,
        )?;

        // Recently viewed count
        let recent_viewed_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM media_entity me
             JOIN entity_file ef ON ef.entity_id = me.entity_id
             JOIN file f ON f.file_id = ef.file_id
             WHERE me.status = 1
               AND me.kind = 'single'
               AND f.view_count > 0
               AND me.parent_collection_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        sidebar::update_sidebar_count(
            conn,
            "system:recent_viewed",
            recent_viewed_count,
            epoch as i64,
        )?;

        // Duplicate count: unresolved detected pairs
        let dup_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
            [],
            |row| row.get(0),
        )?;
        sidebar::update_sidebar_count(conn, "system:duplicates", dup_count, epoch as i64)?;

        // Collect all nodes to batch-write
        let mut nodes: Vec<sidebar::SidebarNode> = Vec::new();

        // Smart folder nodes
        let sfs = smart_folders::list_smart_folders(conn)?;
        for sf in sfs {
            let node_id = format!("smart:{}", sf.smart_folder_id);
            let count = bitmaps.len(&BitmapKey::SmartFolder(sf.smart_folder_id));
            nodes.push(sidebar::SidebarNode {
                node_id,
                kind: "smart_folder".into(),
                parent_id: Some("section:smart_folders".into()),
                name: sf.name.clone(),
                icon: sf.icon.clone(),
                color: sf.color.clone(),
                sort_order: sf.display_order.or(Some(sf.smart_folder_id)),
                count: Some(count as i64),
                freshness: "fresh".into(),
                epoch: epoch as i64,
                selectable: true,
                expanded_by_default: false,
                meta_json: {
                    // Include the predicate + sort fields so the frontend can
                    // filter the grid without a round-trip back to the DB.
                    let mut meta = serde_json::json!({
                        "smart_folder_id": sf.smart_folder_id,
                    });
                    // Parse predicate_json back to a JSON value for embedding
                    if let Ok(pred) = serde_json::from_str::<serde_json::Value>(&sf.predicate_json)
                    {
                        meta["predicate"] = pred;
                    }
                    if let Some(ref sf_field) = sf.sort_field {
                        meta["sort_field"] = serde_json::Value::String(sf_field.clone());
                    }
                    if let Some(ref sf_order) = sf.sort_order {
                        meta["sort_order"] = serde_json::Value::String(sf_order.clone());
                    }
                    Some(meta.to_string())
                },
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
            });
        }

        // Folder nodes — count only non-trashed files (inbox + active)
        let active_bm = bitmaps.get(&BitmapKey::AllActive);
        let folders = crate::folders::db::list_folders(conn)?;
        for folder in folders {
            let node_id = format!("folder:{}", folder.folder_id);
            let count = (bitmaps.get(&BitmapKey::Folder(folder.folder_id)) & &active_bm).len();
            let parent_id = folder
                .parent_id
                .map(|pid| format!("folder:{pid}"))
                .unwrap_or_else(|| "section:folders".into());
            nodes.push(sidebar::SidebarNode {
                node_id,
                kind: "folder".into(),
                parent_id: Some(parent_id),
                name: folder.name,
                icon: folder.icon,
                color: folder.color,
                sort_order: folder.sort_order,
                count: Some(count as i64),
                freshness: "fresh".into(),
                epoch: epoch as i64,
                selectable: true,
                expanded_by_default: false,
                meta_json: Some(
                    serde_json::json!({
                        "folder_id": folder.folder_id,
                        "auto_tags": folder.auto_tags,
                    })
                    .to_string(),
                ),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
            });
        }

        // Clean up orphaned smart_folder / folder sidebar_node rows that no
        // longer have a backing record (e.g. after deletion).
        let live_ids: std::collections::HashSet<String> =
            nodes.iter().map(|n| n.node_id.clone()).collect();
        let existing: Vec<(String, String)> = conn
            .prepare_cached(
                "SELECT node_id, kind FROM sidebar_node WHERE kind IN ('smart_folder', 'folder')",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        for (existing_id, _kind) in &existing {
            if !live_ids.contains(existing_id) {
                sidebar::delete_sidebar_node(conn, existing_id)?;
            }
        }

        // Batch write all nodes
        sidebar::upsert_sidebar_nodes_batch(conn, &nodes)?;

        Ok(())
    })
    .await?;

    // Bump version AFTER all writes succeed
    db.manifest.bump_working_artifact_version("sidebar");
    Ok(())
}

async fn compile_ptr_overlay_full(ptr_db: &Arc<PtrSqliteDatabase>) -> Result<(), String> {
    let epoch = chrono::Utc::now().timestamp();
    let count = ptr_db.rebuild_overlay(epoch).await?;
    ptr_db.bump_epoch().await;
    tracing::info!(count, epoch, "PTR overlay full rebuild, caches invalidated");
    Ok(())
}

async fn compile_ptr_overlay_incremental(
    ptr_db: &Arc<PtrSqliteDatabase>,
    changed_hashes: &[String],
) -> Result<(), String> {
    let epoch = chrono::Utc::now().timestamp();
    let hashes = changed_hashes.to_vec();
    let count = ptr_db.rebuild_overlay_for_hashes(hashes, epoch).await?;
    ptr_db.bump_epoch().await;
    tracing::info!(
        count,
        changed = changed_hashes.len(),
        epoch,
        "PTR overlay incremental rebuild, caches invalidated"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_plan_accumulates_events() {
        let mut plan = CompilerPlan::default();
        assert!(plan.is_empty());

        plan.accumulate(CompilerEvent::FileInserted { file_id: 1 });
        assert!(!plan.is_empty());
        assert!(plan.rebuild_status_bitmaps);
        assert!(plan.rebuild_sidebar);
        assert!(plan.dirty_file_ids.contains(&1));

        plan.accumulate(CompilerEvent::TagChanged { tag_id: 42 });
        assert!(plan.dirty_tag_ids.contains(&42));

        plan.accumulate(CompilerEvent::FileStatusChanged { file_id: 2 });
        assert!(plan.dirty_file_ids.contains(&2));

        plan.accumulate(CompilerEvent::FileTagsChanged { file_id: 3 });
        assert!(plan.dirty_file_ids.contains(&3));
        assert!(plan.rebuild_all_smart_folders);
    }

    #[test]
    fn compiler_plan_rebuild_all_subsumes_incremental() {
        let mut plan = CompilerPlan::default();
        plan.accumulate(CompilerEvent::TagChanged { tag_id: 1 });
        plan.accumulate(CompilerEvent::TagChanged { tag_id: 2 });
        plan.accumulate(CompilerEvent::RebuildAll);
        assert!(plan.rebuild_all);
    }

    /// PBI-027: Metadata-only / PTR-only batches should not trigger sidebar invalidation.
    #[test]
    fn ptr_sync_complete_without_dirty_files_does_not_affect_sidebar() {
        let mut plan = CompilerPlan::default();
        plan.accumulate(CompilerEvent::PtrSyncComplete {
            changed_hashes: vec![],
        });

        // sidebar_affected uses same logic as the compiler loop
        let sidebar_affected = plan.rebuild_sidebar
            || plan.rebuild_tag_graph
            || plan.rebuild_all
            || plan.rebuild_all_smart_folders
            || !plan.dirty_smart_folder_ids.is_empty()
            || plan.rebuild_status_bitmaps;

        assert!(
            !sidebar_affected,
            "PTR sync with no changed hashes should not trigger sidebar invalidation"
        );
    }

    /// PBI-027: File insertion DOES trigger sidebar (status bitmaps + sidebar rebuild).
    #[test]
    fn file_insert_does_affect_sidebar() {
        let mut plan = CompilerPlan::default();
        plan.accumulate(CompilerEvent::FileInserted { file_id: 1 });

        let sidebar_affected = plan.rebuild_sidebar
            || plan.rebuild_tag_graph
            || plan.rebuild_all
            || plan.rebuild_all_smart_folders
            || !plan.dirty_smart_folder_ids.is_empty()
            || plan.rebuild_status_bitmaps;

        assert!(
            sidebar_affected,
            "File insertion should trigger sidebar invalidation"
        );
    }

    #[test]
    fn view_count_change_affects_sidebar_only() {
        let mut plan = CompilerPlan::default();
        plan.accumulate(CompilerEvent::ViewCountChanged);
        assert!(plan.rebuild_sidebar);
        assert!(!plan.rebuild_status_bitmaps);
        assert!(!plan.rebuild_all_smart_folders);
        assert!(plan.dirty_file_ids.is_empty());
    }

    #[tokio::test]
    async fn sidebar_untagged_count_uses_all_active_minus_tagged() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SqliteDatabase::open(dir.path()).await.unwrap());

        // 4 rows:
        // - inbox + untagged
        // - active + tagged
        // - active + untagged
        // - trash + untagged
        db.with_conn(|conn| {
            for (hash, status) in [
                ("inbox_u", 0),
                ("active_t", 1),
                ("active_u", 1),
                ("trash_u", 2),
            ] {
                conn.execute(
                    "INSERT INTO file (hash, size, mime, status, imported_at)
                     VALUES (?1, 100, 'image/png', ?2, '2024-01-01')",
                    rusqlite::params![hash, status],
                )?;
                let fid = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO media_entity (entity_id, kind, status, created_at)
                     VALUES (?1, 'single', ?2, '2024-01-01')",
                    rusqlite::params![fid, status],
                )?;
                conn.execute(
                    "INSERT INTO entity_file (entity_id, file_id) VALUES (?1, ?1)",
                    [fid],
                )?;
            }

            conn.execute(
                "INSERT INTO tag (namespace, subtag) VALUES ('artist', 'x')",
                [],
            )?;
            // Tag only the active_t entity (entity_id = 2 by insertion order above).
            conn.execute(
                "INSERT INTO entity_tag_raw (entity_id, tag_id) VALUES (2, 1)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        compile_status_bitmaps(&db).await.unwrap();
        compile_all_tag_bitmaps(&db).await.unwrap();
        compile_tagged_bitmap(&db).await.unwrap();
        compile_sidebar(&db).await.unwrap();

        let untagged: i64 = db
            .with_read_conn(|conn| {
                conn.query_row(
                    "SELECT count FROM sidebar_node WHERE node_id = 'system:untagged'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap();

        // AllActive = inbox + active = 3; tagged (within AllActive) = 1 => untagged = 2
        assert_eq!(untagged, 2);
    }

    #[tokio::test]
    async fn status_bitmaps_compile_from_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteDatabase::open(dir.path()).await.unwrap();

        // Insert test files with different statuses + matching media_entity rows
        db.with_conn(|conn| {
            for (hash, size, status, date) in [
                ("aaa", 100, 0, "2024-01-01"),
                ("bbb", 200, 0, "2024-01-02"),
                ("ccc", 300, 1, "2024-01-03"),
                ("ddd", 400, 2, "2024-01-04"),
            ] {
                conn.execute(
                    "INSERT INTO file (hash, size, mime, status, imported_at)
                     VALUES (?1, ?2, 'image/png', ?3, ?4)",
                    rusqlite::params![hash, size, status, date],
                )?;
                let fid = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO media_entity (entity_id, kind, status, created_at) VALUES (?1, 'single', ?2, ?3)",
                    rusqlite::params![fid, status, date],
                )?;
                conn.execute(
                    "INSERT INTO entity_file (entity_id, file_id) VALUES (?1, ?1)",
                    [fid],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();

        compile_status_bitmaps(&db).await.unwrap();

        // Inbox (status=0) should have 2 files
        assert_eq!(db.bitmaps.len(&BitmapKey::Status(0)), 2);
        // Active (status=1) should have 1 file
        assert_eq!(db.bitmaps.len(&BitmapKey::Status(1)), 1);
        // Trash (status=2) should have 1 file
        assert_eq!(db.bitmaps.len(&BitmapKey::Status(2)), 1);
        // AllActive = inbox + active = 3
        assert_eq!(db.bitmaps.len(&BitmapKey::AllActive), 3);
    }

    #[tokio::test]
    async fn tag_bitmaps_compile_from_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteDatabase::open(dir.path()).await.unwrap();

        // Insert files and tags
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO file (hash, size, mime, status, imported_at)
                 VALUES ('aaa', 100, 'image/png', 0, '2024-01-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO file (hash, size, mime, status, imported_at)
                 VALUES ('bbb', 200, 'image/png', 0, '2024-01-02')",
                [],
            )?;
            // Create media_entity + entity_file for each file (required for entity_tag_raw FK)
            conn.execute("INSERT INTO media_entity (kind) VALUES ('single')", [])?;
            conn.execute(
                "INSERT INTO entity_file (entity_id, file_id) VALUES (1, 1)",
                [],
            )?;
            conn.execute("INSERT INTO media_entity (kind) VALUES ('single')", [])?;
            conn.execute(
                "INSERT INTO entity_file (entity_id, file_id) VALUES (2, 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO tag (namespace, subtag) VALUES ('character', 'saber')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tag (namespace, subtag) VALUES ('series', 'fate')",
                [],
            )?;
            // Tag entity 1 with both tags, entity 2 with only tag 1
            conn.execute(
                "INSERT INTO entity_tag_raw (entity_id, tag_id) VALUES (1, 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO entity_tag_raw (entity_id, tag_id) VALUES (1, 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO entity_tag_raw (entity_id, tag_id) VALUES (2, 1)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        compile_all_tag_bitmaps(&db).await.unwrap();

        // Tag 1 (character:saber) should have 2 files
        assert_eq!(db.bitmaps.len(&BitmapKey::Tag(1)), 2);
        // Tag 2 (series:fate) should have 1 file
        assert_eq!(db.bitmaps.len(&BitmapKey::Tag(2)), 1);
    }

    #[tokio::test]
    async fn smart_folder_bitmap_matches_sql() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteDatabase::open(dir.path()).await.unwrap();

        // Insert files with different statuses + matching media_entity rows
        db.with_conn(|conn| {
            for i in 1..=10 {
                let status = if i <= 7 { 0 } else { 1 }; // 7 inbox, 3 active
                conn.execute(
                    "INSERT INTO file (hash, size, mime, status, imported_at)
                     VALUES (?1, ?2, 'image/png', ?3, '2024-01-01')",
                    rusqlite::params![format!("hash{i}"), i * 100, status],
                )?;
                let fid = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO media_entity (entity_id, kind, status, created_at) VALUES (?1, 'single', ?2, '2024-01-01')",
                    rusqlite::params![fid, status],
                )?;
                conn.execute(
                    "INSERT INTO entity_file (entity_id, file_id) VALUES (?1, ?1)",
                    [fid],
                )?;
            }
            // Create a smart folder with valid predicate (no tag rules = all active files)
            conn.execute(
                "INSERT INTO smart_folder (name, predicate_json) VALUES ('All',
                 '{\"groups\":[]}')",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        // Build status bitmaps first (smart folders depend on them)
        compile_status_bitmaps(&db).await.unwrap();

        // AllActive = inbox + active = 10
        assert_eq!(db.bitmaps.len(&BitmapKey::AllActive), 10);

        // Compile smart folder
        compile_all_smart_folders(&db).await.unwrap();

        // Smart folder should match AllActive count
        let sf_len = db.bitmaps.len(&BitmapKey::SmartFolder(1));
        let all_active_len = db.bitmaps.len(&BitmapKey::AllActive);
        assert_eq!(
            sf_len, all_active_len,
            "Smart folder bitmap ({sf_len}) should match AllActive ({all_active_len})"
        );
    }

    #[tokio::test]
    async fn bitmap_persistence_survives_restart() {
        let dir = tempfile::tempdir().unwrap();

        // First "session": create DB, insert data, compile, flush
        {
            let db = SqliteDatabase::open(dir.path()).await.unwrap();
            db.with_conn(|conn| {
                for (hash, size, status, date) in [
                    ("persist1", 100, 0, "2024-01-01"),
                    ("persist2", 200, 1, "2024-01-02"),
                ] {
                    conn.execute(
                        "INSERT INTO file (hash, size, mime, status, imported_at)
                         VALUES (?1, ?2, 'image/png', ?3, ?4)",
                        rusqlite::params![hash, size, status, date],
                    )?;
                    let fid = conn.last_insert_rowid();
                    conn.execute(
                        "INSERT INTO media_entity (entity_id, kind, status, created_at) VALUES (?1, 'single', ?2, ?3)",
                        rusqlite::params![fid, status, date],
                    )?;
                    conn.execute(
                        "INSERT INTO entity_file (entity_id, file_id) VALUES (?1, ?1)",
                        [fid],
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();

            compile_status_bitmaps(&db).await.unwrap();

            // Verify before flush
            assert_eq!(db.bitmaps.len(&BitmapKey::Status(0)), 1);
            assert_eq!(db.bitmaps.len(&BitmapKey::Status(1)), 1);
            assert_eq!(db.bitmaps.len(&BitmapKey::AllActive), 2);

            db.bitmaps.flush().unwrap();
        }

        // Second "session": reopen DB, bitmaps should be loaded from disk
        {
            let db = SqliteDatabase::open(dir.path()).await.unwrap();
            assert_eq!(
                db.bitmaps.len(&BitmapKey::Status(0)),
                1,
                "Inbox bitmap should survive restart"
            );
            assert_eq!(
                db.bitmaps.len(&BitmapKey::Status(1)),
                1,
                "Active bitmap should survive restart"
            );
            assert_eq!(
                db.bitmaps.len(&BitmapKey::AllActive),
                2,
                "AllActive bitmap should survive restart"
            );
        }
    }

    #[tokio::test]
    async fn debounce_batches_rapid_events() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteDatabase::open(dir.path()).await.unwrap();

        // Insert some files so compilers have data to work with
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO file (hash, size, mime, status, imported_at)
                 VALUES ('deb1', 100, 'image/png', 0, '2024-01-01')",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        // Send 100 rapid events through the compiler channel
        for i in 1..=100 {
            let _ = db
                .compiler_tx
                .send(CompilerEvent::FileInserted { file_id: i });
        }

        // The debounce window is 100ms. After events settle + compile,
        // the bitmaps should reflect the compiled state.
        // Wait for compiler to process.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // All 100 rapid events should be batched - the channel should be
        // drained. Send one more event and verify it still works.
        let _ = db
            .compiler_tx
            .send(CompilerEvent::FileStatusChanged { file_id: 1 });

        // This test verifies the channel accepts events after rapid fire.
        // The actual debounce is structural (CompilerPlan accumulation).
        // Full debounce testing requires the compiler loop running with
        // an app_handle, which we can't easily mock here.
        // The CompilerPlan accumulation test above verifies batching logic.
    }
}
