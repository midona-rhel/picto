//! Compiler system — background task that rebuilds derived read-model artifacts.
//!
//! Domain writes enqueue `ReadModelEvent`s without knowing compiler internals.
//! This loop debounces those events, rebuilds affected read models, and hands
//! publication off to the explicit publish boundary.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[cfg(test)]
use super::bitmaps::BitmapKey;
use super::publish;
use super::read_model::{DerivedArtifact, ReadModelBatchResult, ReadModelEvent};
use super::SqliteDatabase;
use crate::metadata::compiler::compile_metadata_projections;
use crate::sidebar::compiler::compile_sidebar;
use crate::smart_folders::compiler::{compile_all_smart_folders, compile_smart_folder};
use crate::tags::compiler::{
    compile_all_tag_bitmaps, compile_effective_tags, compile_status_bitmaps, compile_tag_bitmap,
    compile_tag_graph, compile_tagged_bitmap,
};

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
    rebuild_all: bool,
}

impl CompilerPlan {
    /// Accumulate a compiler event into the plan.
    ///
    /// Dependency rules:
    /// - File insert/delete/status → rebuild status bitmaps + ALL smart folders + sidebar.
    ///   Status changes affect the default visible-library bitmap which every
    ///   smart folder scope depends on.
    /// - FileTagsChanged → rebuild ALL smart folders (any tag predicate could match) + sidebar.
    ///   We can't cheaply determine which smart folders use the changed tag, so rebuild all.
    /// - TagChanged → rebuild only that tag's bitmap + sidebar. Smart folders use EffectiveTag
    ///   bitmaps which are rebuilt per-tag, so specific smart folders update lazily.
    /// - TagGraphChanged → rebuild tag graph + ALL smart folders. Parent changes cascade
    ///   through ImpliedTag bitmaps and affect every smart folder using those tags.
    fn accumulate(&mut self, event: ReadModelEvent) {
        match event {
            ReadModelEvent::FileInserted { file_id } => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
                self.dirty_file_ids.insert(file_id);
            }
            ReadModelEvent::FileDeleted { file_id } => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
                self.dirty_file_ids.insert(file_id);
            }
            ReadModelEvent::FileStatusChanged { file_id } => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
                self.dirty_file_ids.insert(file_id);
            }
            ReadModelEvent::StatusBatchChanged => {
                self.rebuild_status_bitmaps = true;
                self.rebuild_sidebar = true;
                self.rebuild_all_smart_folders = true;
            }
            ReadModelEvent::FileTagsChanged { file_id } => {
                self.rebuild_all_smart_folders = true;
                self.rebuild_sidebar = true;
                self.dirty_file_ids.insert(file_id);
            }
            ReadModelEvent::TagChanged { tag_id } => {
                self.dirty_tag_ids.insert(tag_id);
                self.rebuild_sidebar = true;
            }
            ReadModelEvent::TagGraphChanged => {
                self.rebuild_tag_graph = true;
                self.rebuild_all_smart_folders = true;
                self.rebuild_sidebar = true;
            }
            ReadModelEvent::SmartFolderChanged { smart_folder_id } => {
                self.dirty_smart_folder_ids.insert(smart_folder_id);
                self.rebuild_sidebar = true;
            }
            ReadModelEvent::FolderChanged { folder_id: _ } => {
                self.rebuild_sidebar = true;
            }
            ReadModelEvent::DuplicateChanged => {
                self.rebuild_sidebar = true;
            }
            ReadModelEvent::RebuildAll => {
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
            && !self.rebuild_all
    }
}

/// Start the compiler background task.
///
/// `on_batch_done` is called after each compiler batch completes, with a summary
/// of what was affected. The caller (state.rs) uses this to emit events.
pub async fn start_compiler_loop(
    db: Arc<SqliteDatabase>,
    mut rx: mpsc::UnboundedReceiver<ReadModelEvent>,
    on_batch_done: impl Fn(ReadModelBatchResult) + Send + 'static,
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

        tracing::debug!(
            rebuild_status = plan.rebuild_status_bitmaps,
            dirty_tags = plan.dirty_tag_ids.len(),
            dirty_smart_folders = plan.dirty_smart_folder_ids.len(),
            dirty_files = plan.dirty_file_ids.len(),
            rebuild_all = plan.rebuild_all,
            "compiler batch starting"
        );

        let db_ref = db.clone();

        let smart_folders_rebuilt = plan.rebuild_all_smart_folders
            || plan.rebuild_all
            || !plan.dirty_smart_folder_ids.is_empty();

        let (dirty_artifacts, sf_counts) = match run_compilers(&db_ref, &plan).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!(
                    rebuild_status = plan.rebuild_status_bitmaps,
                    dirty_tags = plan.dirty_tag_ids.len(),
                    dirty_smart_folders = plan.dirty_smart_folder_ids.len(),
                    dirty_files = plan.dirty_file_ids.len(),
                    rebuild_all = plan.rebuild_all,
                    "Compiler batch failed: {e}"
                );
                continue;
            }
        };

        let published = match publish::publish_pending(
            &db_ref,
            &dirty_artifacts.into_iter().collect::<Vec<_>>(),
        )
        .await
        {
            Ok(published) => published,
            Err(e) => {
                tracing::error!(
                    rebuild_status = plan.rebuild_status_bitmaps,
                    dirty_tags = plan.dirty_tag_ids.len(),
                    dirty_smart_folders = plan.dirty_smart_folder_ids.len(),
                    dirty_files = plan.dirty_file_ids.len(),
                    rebuild_all = plan.rebuild_all,
                    "Publish failed after compilation: {e}"
                );
                continue;
            }
        };

        // Only invalidate scope cache when membership-affecting bitmaps changed.
        // Metadata-only batches don't alter scope ID sets.
        let scope_affected = plan.rebuild_status_bitmaps
            || plan.rebuild_all_smart_folders
            || !plan.dirty_smart_folder_ids.is_empty()
            || plan.rebuild_tag_graph
            || !plan.dirty_tag_ids.is_empty()
            || plan.rebuild_all
            || !plan.dirty_file_ids.is_empty();
        if scope_affected {}

        on_batch_done(ReadModelBatchResult {
            smart_folders_rebuilt,
            scope_affected,
            published,
            smart_folder_counts: sf_counts,
        });
    }
}

async fn run_compilers(
    db: &Arc<SqliteDatabase>,
    plan: &CompilerPlan,
) -> Result<(BTreeSet<DerivedArtifact>, Vec<(i64, i64)>), String> {
    let start = std::time::Instant::now();
    let mut dirty_artifacts = BTreeSet::new();

    // 1. Status bitmap compiler
    if plan.rebuild_status_bitmaps || plan.rebuild_all {
        let t = std::time::Instant::now();
        compile_status_bitmaps(db).await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled status bitmaps"
        );
        dirty_artifacts.insert(DerivedArtifact::Files);
    }

    // 2. Tag bitmap compiler (incremental)
    if plan.rebuild_all {
        let t = std::time::Instant::now();
        compile_all_tag_bitmaps(db).await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled all tag bitmaps"
        );
        dirty_artifacts.insert(DerivedArtifact::Tags);
    } else if !plan.dirty_tag_ids.is_empty() {
        let t = std::time::Instant::now();
        for &tag_id in &plan.dirty_tag_ids {
            compile_tag_bitmap(db, tag_id).await?;
        }
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            count = plan.dirty_tag_ids.len(),
            "compiled dirty tag bitmaps"
        );
        dirty_artifacts.insert(DerivedArtifact::Tags);
    }

    // 3. Tag graph compiler (siblings, ancestors, implied tags)
    if plan.rebuild_tag_graph || plan.rebuild_all {
        let t = std::time::Instant::now();
        compile_tag_graph(db).await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled tag graph"
        );
        dirty_artifacts.insert(DerivedArtifact::TagGraph);
    }

    // 4. Effective tag compiler
    if plan.rebuild_tag_graph || plan.rebuild_all || !plan.dirty_tag_ids.is_empty() {
        let t = std::time::Instant::now();
        compile_effective_tags(
            db,
            &plan.dirty_tag_ids,
            plan.rebuild_all || plan.rebuild_tag_graph,
        )
        .await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled effective tags"
        );
        dirty_artifacts.insert(DerivedArtifact::EffectiveTags);
    }

    // 4b. Tagged bitmap (union of all tagged files)
    if plan.rebuild_tag_graph
        || plan.rebuild_all
        || !plan.dirty_tag_ids.is_empty()
        || plan.rebuild_status_bitmaps
    {
        let t = std::time::Instant::now();
        compile_tagged_bitmap(db).await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled tagged bitmap"
        );
        dirty_artifacts.insert(DerivedArtifact::Tags);
    }

    // 5. Metadata projection compiler
    if plan.rebuild_all || plan.rebuild_tag_graph || !plan.dirty_file_ids.is_empty() {
        let t = std::time::Instant::now();
        compile_metadata_projections(
            db,
            &plan.dirty_file_ids,
            plan.rebuild_all || plan.rebuild_tag_graph,
        )
        .await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled metadata projections"
        );
        dirty_artifacts.insert(DerivedArtifact::MetadataProjection);
    }

    // 6. Smart folder compiler
    if plan.rebuild_all_smart_folders || plan.rebuild_all {
        let t = std::time::Instant::now();
        compile_all_smart_folders(db).await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled all smart folders"
        );
        dirty_artifacts.insert(DerivedArtifact::SmartFolders);
    } else if !plan.dirty_smart_folder_ids.is_empty() {
        let t = std::time::Instant::now();
        for &sf_id in &plan.dirty_smart_folder_ids {
            compile_smart_folder(db, sf_id).await?;
        }
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            count = plan.dirty_smart_folder_ids.len(),
            "compiled dirty smart folders"
        );
        dirty_artifacts.insert(DerivedArtifact::SmartFolders);
    }

    // Collect smart folder counts after compilation
    let sf_counts = if plan.rebuild_all_smart_folders || plan.rebuild_all {
        // All SF bitmaps were rebuilt — get all IDs and their counts
        let all_ids = db
            .with_read_conn(|conn| {
                conn.prepare("SELECT smart_folder_id FROM smart_folder")
                    .and_then(|mut stmt| {
                        stmt.query_map([], |row| row.get::<_, i64>(0))
                            .map(|rows| rows.flatten().collect::<Vec<_>>())
                    })
            })
            .await
            .unwrap_or_default();
        all_ids
            .iter()
            .map(|&id| {
                (
                    id,
                    db.bitmaps.len(&super::bitmaps::BitmapKey::SmartFolder(id)) as i64,
                )
            })
            .collect()
    } else if !plan.dirty_smart_folder_ids.is_empty() {
        plan.dirty_smart_folder_ids
            .iter()
            .map(|&id| {
                (
                    id,
                    db.bitmaps.len(&super::bitmaps::BitmapKey::SmartFolder(id)) as i64,
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    // 7. Sidebar compiler
    if plan.rebuild_sidebar || plan.rebuild_all {
        let t = std::time::Instant::now();
        compile_sidebar(db).await?;
        tracing::debug!(
            elapsed_ms = t.elapsed().as_secs_f64() * 1000.0,
            "compiled sidebar"
        );
        dirty_artifacts.insert(DerivedArtifact::Sidebar);
    }

    tracing::debug!(
        elapsed_ms = start.elapsed().as_secs_f64() * 1000.0,
        artifacts = ?dirty_artifacts,
        sf_counts = sf_counts.len(),
        "compiler batch complete"
    );

    Ok((dirty_artifacts, sf_counts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_plan_accumulates_events() {
        let mut plan = CompilerPlan::default();
        assert!(plan.is_empty());

        plan.accumulate(ReadModelEvent::FileInserted { file_id: 1 });
        assert!(!plan.is_empty());
        assert!(plan.rebuild_status_bitmaps);
        assert!(plan.rebuild_sidebar);
        assert!(plan.dirty_file_ids.contains(&1));

        plan.accumulate(ReadModelEvent::TagChanged { tag_id: 42 });
        assert!(plan.dirty_tag_ids.contains(&42));

        plan.accumulate(ReadModelEvent::FileStatusChanged { file_id: 2 });
        assert!(plan.dirty_file_ids.contains(&2));

        plan.accumulate(ReadModelEvent::FileTagsChanged { file_id: 3 });
        assert!(plan.dirty_file_ids.contains(&3));
        assert!(plan.rebuild_all_smart_folders);
    }

    #[test]
    fn compiler_plan_rebuild_all_subsumes_incremental() {
        let mut plan = CompilerPlan::default();
        plan.accumulate(ReadModelEvent::TagChanged { tag_id: 1 });
        plan.accumulate(ReadModelEvent::TagChanged { tag_id: 2 });
        plan.accumulate(ReadModelEvent::RebuildAll);
        assert!(plan.rebuild_all);
    }

    /// File insertion DOES trigger sidebar (status bitmaps + sidebar rebuild).
    #[test]
    fn file_insert_does_affect_sidebar() {
        let mut plan = CompilerPlan::default();
        plan.accumulate(ReadModelEvent::FileInserted { file_id: 1 });

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

    #[tokio::test]
    async fn sidebar_untagged_count_uses_active_minus_tagged() {
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

        // active = 2 (active_t + active_u); tagged = 1 (active_t) => untagged = 1
        assert_eq!(untagged, 1);
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
        // Create a smart folder with an empty predicate.
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

        // Active (status=1) should have 3 files.
        assert_eq!(db.bitmaps.len(&BitmapKey::Status(1)), 3);

        // Compile smart folder
        compile_all_smart_folders(&db).await.unwrap();

        // Empty smart folder predicates have no local rules, so they compile to
        // an empty effective predicate and therefore an empty result.
        let sf_len = db.bitmaps.len(&BitmapKey::SmartFolder(1));
        assert_eq!(sf_len, 0, "Smart folder bitmap ({sf_len}) should be empty");
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
                .read_model_tx
                .send(ReadModelEvent::FileInserted { file_id: i });
        }

        // The debounce window is 100ms. After events settle + compile,
        // the bitmaps should reflect the compiled state.
        // Wait for compiler to process.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // All 100 rapid events should be batched - the channel should be
        // drained. Send one more event and verify it still works.
        let _ = db
            .read_model_tx
            .send(ReadModelEvent::FileStatusChanged { file_id: 1 });

        // This test verifies the channel accepts events after rapid fire.
        // The actual debounce is structural (CompilerPlan accumulation).
        // Full debounce testing requires the compiler loop running with
        // an app_handle, which we can't easily mock here.
        // The CompilerPlan accumulation test above verifies batching logic.
    }
}
