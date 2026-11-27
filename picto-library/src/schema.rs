use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::query_request::{OpenCancellation, OpenStage};
use crate::{LibraryError, Result};

pub const SCHEMA_GENERATION: u32 = 1;
pub const SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-11-27-shared-watch-paths";
const NO_CLOUD_SYNC_PREDECESSOR: &str = "picto-library-schema-1-2026-11-25-no-cloud-sync";
const KEPT_DELETED_FILES_PREDECESSOR: &str = "picto-library-schema-1-2026-11-23-kept-deleted-files";
const TOMBSTONE_ENTRIES_PREDECESSOR: &str = "picto-library-schema-1-2026-10-13-task-rollback-aliases";
const COMBINED_IMPORT_PREDECESSOR: &str = "picto-library-schema-1-2026-10-13-media-aliases-import-tasks";
const TASK_ROLLBACK_PREDECESSOR: &str = "picto-library-schema-1-2026-10-13-task-rollback";
const MEDIA_ALIASES_PREDECESSOR: &str = "picto-library-schema-1-2026-10-12-media-aliases";
const IMPORT_TASKS_PREDECESSOR: &str = "picto-library-schema-1-2026-10-12-import-tasks";
const NO_CLOUD_CLOCKS_PREDECESSOR: &str = "picto-library-schema-1-2026-10-10-no-cloud-clocks";
const CHUNKED_ORDERS_PREDECESSOR: &str = "picto-library-schema-1-2026-10-10-chunked-orders";
const WATCH_LIFECYCLE_PREDECESSOR: &str = "picto-library-schema-1-2026-10-10-watch-lifecycle";
const SOURCE_ROOTS_PREDECESSOR: &str = "picto-library-schema-1-2026-09-09-source-created-roots";
const RESIDENT_PREDECESSOR: &str = "picto-library-schema-1-2026-09-07-resident-browsing";
const ROWID_PREDECESSOR: &str = "picto-library-schema-1-2026-09-07-summary-page-rowids";
const KEYS_PREDECESSOR: &str = "picto-library-schema-1-2026-09-06-persisted-name-keys";
const NODE_PREDECESSOR: &str = "picto-library-schema-1-2026-09-06-persisted-names";
const NAME_PREDECESSOR: &str = "picto-library-schema-1-2026-09-06-persisted-text";
const TEXT_PREDECESSOR: &str = "picto-library-schema-1-2026-09-06-browsing-pages";

const DRAWING_PREDECESSOR: &str = "picto-library-schema-1-2026-08-31-duplicate-folder-names";
const PREVIOUS_SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-08-30-native-sources";
const LEGACY_SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-08-29-fts-substring";

const SCHEMA: &str = include_str!("schema_v1.sql");
/// The tables of the cloud sync that earlier schemas had. The migration drops
/// each of them that exists, and with it its indexes.
const CLOUD_SYNC_TABLES: [&str; 18] = [
    "cloud_state",
    "cloud_device_frontier",
    "shared_entry",
    "entry_recording",
    "pending_entry",
    "merge_touched",
    "wanted_file",
    "kept_deleted_file",
    "cloud_deleted_original",
    "cloud_quarantine",
    "cloud_snapshot",
    "cloud_blob_state",
    "cloud_journal",
    "cloud_tombstone",
    "cloud_outbox",
    "cloud_applied_mutation",
    "cloud_field_clock",
    "cloud_membership_clock",
];
const TASK_SCHEMA: &str = include_str!("task_schema.sql");
const ACCELERATOR_INDEXES: &str = include_str!("accelerator_indexes.sql");
const ALIAS_SCHEMA: &str = include_str!("alias_schema.sql");
/// The index that allows one media item for each file. A new library is
/// created with it. An existing library gets it once the items that share a
/// file are merged (`Library::merge_shared_files`).
pub(crate) const ONE_ITEM_PER_FILE: &str =
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_media_item_one_per_file ON media_item(file_id)";

/// Creates every accelerator index that is missing. It changes nothing when
/// all of them exist. It runs when a library is created and when a library
/// is opened.
pub fn ensure_accelerator_indexes(connection: &Connection) -> Result<()> {
    connection.execute_batch(ACCELERATOR_INDEXES)?;
    Ok(())
}

pub fn create(connection: &mut Connection) -> Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")?;
    let result = (|| {
        connection.execute_batch(SCHEMA)?;
        connection.execute_batch(TASK_SCHEMA)?;
        connection.execute_batch(ALIAS_SCHEMA)?;
        connection.execute_batch(ONE_ITEM_PER_FILE)?;
        connection.execute_batch(include_str!("browsing_schema.sql"))?;
        connection.execute_batch(ACCELERATOR_INDEXES)?;
        connection.execute(
            "INSERT INTO library_meta
                 (singleton, schema_generation, schema_fingerprint, revision, next_local_id)
             VALUES (1, ?1, ?2, 1, 1)",
            rusqlite::params![SCHEMA_GENERATION, SCHEMA_FINGERPRINT],
        )?;
        Ok::<_, LibraryError>(())
    })();
    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

pub fn validate(connection: &Connection) -> Result<u64> {
    let (generation, fingerprint, revision) = metadata(connection)?;
    if generation != SCHEMA_GENERATION || fingerprint != SCHEMA_FINGERPRINT {
        return Err(LibraryError::Incompatible(format!(
            "expected generation {SCHEMA_GENERATION} ({SCHEMA_FINGERPRINT}), found {generation} ({fingerprint})"
        )));
    }
    Ok(revision)
}

/// Interrupts the migration's statements once its open is cancelled. The
/// handler is removed when the migration ends, because the connection goes
/// on to be the library's writer, and ordinary writes must not be
/// interrupted by it.
struct MigrationInterrupt<'a>(&'a Connection);

impl<'a> MigrationInterrupt<'a> {
    fn install(connection: &'a Connection, flag: Arc<AtomicU64>) -> Result<Self> {
        connection.progress_handler(1_000, Some(move || flag.load(Ordering::Relaxed) != 0))?;
        Ok(Self(connection))
    }
}

impl Drop for MigrationInterrupt<'_> {
    fn drop(&mut self) {
        let _ = self.0.progress_handler(0, None::<fn() -> bool>);
    }
}

/// Migrate a supported predecessor schema in one transaction after backing
/// the file up. A cancelled open leaves the database as it was: the backup is
/// taken outside the transaction and removed again when the migration does
/// not commit, and SQLite rolls an interrupted transaction back itself.
pub fn migrate_for_open(
    connection: &mut Connection,
    database_path: &Path,
    cancellation: &OpenCancellation,
    progress: &mut dyn FnMut(OpenStage),
) -> Result<()> {
    let connection: &Connection = connection;
    let (generation, fingerprint, _) = metadata(connection)?;
    if generation == SCHEMA_GENERATION && fingerprint == SCHEMA_FINGERPRINT {
        return Ok(());
    }
    if generation != SCHEMA_GENERATION
        || !matches!(
            fingerprint.as_str(),
            NO_CLOUD_SYNC_PREDECESSOR
                | KEPT_DELETED_FILES_PREDECESSOR
                | TOMBSTONE_ENTRIES_PREDECESSOR
                | COMBINED_IMPORT_PREDECESSOR
                | TASK_ROLLBACK_PREDECESSOR
                | MEDIA_ALIASES_PREDECESSOR
                | IMPORT_TASKS_PREDECESSOR
                | NO_CLOUD_CLOCKS_PREDECESSOR
                | CHUNKED_ORDERS_PREDECESSOR
                | WATCH_LIFECYCLE_PREDECESSOR
                | SOURCE_ROOTS_PREDECESSOR
                | RESIDENT_PREDECESSOR
                | ROWID_PREDECESSOR
                | KEYS_PREDECESSOR
                | NODE_PREDECESSOR
                | NAME_PREDECESSOR
                | TEXT_PREDECESSOR
                | DRAWING_PREDECESSOR
                | PREVIOUS_SCHEMA_FINGERPRINT
                | LEGACY_SCHEMA_FINGERPRINT
        )
    {
        return Err(LibraryError::Incompatible(format!(
            "no migration to generation {SCHEMA_GENERATION} ({SCHEMA_FINGERPRINT}) from generation {generation} ({fingerprint})"
        )));
    }

    if cancellation.is_cancelled() {
        return Err(LibraryError::OpenCancelled);
    }
    progress(OpenStage::Migrating);
    let backup_path = backup_path(database_path, "pre-migration");
    let created_backup = !backup_path.exists();
    let result = {
        let _interrupt = MigrationInterrupt::install(connection, cancellation.flag())?;
        (|| {
            backup(connection, &backup_path)?;
            if cancellation.is_cancelled() {
                return Err(LibraryError::OpenCancelled);
            }
            connection.execute_batch("PRAGMA foreign_keys = OFF; BEGIN IMMEDIATE;")?;
            if fingerprint == LEGACY_SCHEMA_FINGERPRINT {
                connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS source_post_attempt (
                attempt_id INTEGER PRIMARY KEY,
                run_query_id INTEGER NOT NULL REFERENCES subscription_run_query(run_query_id) ON DELETE CASCADE,
                source_post_id INTEGER NOT NULL REFERENCES source_post(source_post_id) ON DELETE CASCADE,
                state TEXT NOT NULL CHECK (state IN (
                    'discovered', 'downloading', 'downloaded', 'ingesting',
                    'added', 'skipped', 'failed', 'cancelled'
                )),
                terminal_reason TEXT,
                started_at TEXT NOT NULL,
                settled_at TEXT,
                UNIQUE(run_query_id, source_post_id)
            ) STRICT;
            CREATE UNIQUE INDEX IF NOT EXISTS idx_source_post_attempt_open
                ON source_post_attempt(run_query_id)
                WHERE state NOT IN ('added', 'skipped', 'failed', 'cancelled');
            CREATE TABLE IF NOT EXISTS source_file_attempt (
                file_attempt_id INTEGER PRIMARY KEY,
                attempt_id INTEGER NOT NULL REFERENCES source_post_attempt(attempt_id) ON DELETE CASCADE,
                source_item_id INTEGER NOT NULL REFERENCES source_item(source_item_id) ON DELETE CASCADE,
                content_hash TEXT,
                state TEXT NOT NULL CHECK (state IN ('discovered', 'staged', 'retained', 'duplicate', 'failed')),
                staged_path TEXT,
                bytes_staged INTEGER NOT NULL DEFAULT 0 CHECK (bytes_staged >= 0),
                error TEXT,
                UNIQUE(attempt_id, source_item_id)
            ) STRICT;
            CREATE TABLE IF NOT EXISTS source_attempt_root (
                attempt_id INTEGER NOT NULL REFERENCES source_post_attempt(attempt_id) ON DELETE CASCADE,
                root_id INTEGER REFERENCES library_root(root_id) ON DELETE SET NULL,
                root_stable_key TEXT NOT NULL,
                PRIMARY KEY(attempt_id, root_stable_key)
            ) WITHOUT ROWID, STRICT;
            CREATE INDEX IF NOT EXISTS idx_source_post_attempt_run
                ON source_post_attempt(run_query_id, attempt_id);
            CREATE INDEX IF NOT EXISTS idx_source_file_attempt_progress
                ON source_file_attempt(attempt_id, state, file_attempt_id);",
            )?;
            }
            if ![
                NO_CLOUD_SYNC_PREDECESSOR,
                KEPT_DELETED_FILES_PREDECESSOR,
                TOMBSTONE_ENTRIES_PREDECESSOR,
                COMBINED_IMPORT_PREDECESSOR,
                TASK_ROLLBACK_PREDECESSOR,
                MEDIA_ALIASES_PREDECESSOR,
                IMPORT_TASKS_PREDECESSOR,
                NO_CLOUD_CLOCKS_PREDECESSOR,
                CHUNKED_ORDERS_PREDECESSOR,
                WATCH_LIFECYCLE_PREDECESSOR,
                SOURCE_ROOTS_PREDECESSOR,
                RESIDENT_PREDECESSOR,
                DRAWING_PREDECESSOR,
                TEXT_PREDECESSOR,
                NAME_PREDECESSOR,
                NODE_PREDECESSOR,
                KEYS_PREDECESSOR,
                ROWID_PREDECESSOR,
            ]
            .contains(&fingerprint.as_str())
            {
                connection.execute_batch(
                    "ALTER TABLE folder_definition RENAME TO folder_definition_unique_names;
             CREATE TABLE folder_definition (
                 folder_id INTEGER PRIMARY KEY CHECK (folder_id BETWEEN 1 AND 4294967295),
                 stable_key TEXT NOT NULL UNIQUE,
                 parent_id INTEGER REFERENCES folder_definition(folder_id) ON DELETE RESTRICT,
                 name TEXT NOT NULL,
                 icon TEXT,
                 color TEXT,
                 notes TEXT,
                 auto_tag_ids BLOB NOT NULL DEFAULT X'',
                 cover_root_id INTEGER REFERENCES library_root(root_id) ON DELETE SET NULL,
                 watch_path TEXT UNIQUE,
                 watch_enabled INTEGER NOT NULL DEFAULT 0 CHECK (watch_enabled IN (0, 1)),
                 watch_subfolders INTEGER NOT NULL DEFAULT 0 CHECK (watch_subfolders IN (0, 1)),
                 display_order INTEGER NOT NULL
             ) STRICT;
             INSERT INTO folder_definition (
                 folder_id, stable_key, parent_id, name, icon, color, notes, auto_tag_ids,
                 cover_root_id, watch_path, watch_enabled, watch_subfolders, display_order
             )
             SELECT folder_id, stable_key, parent_id, name, icon, color, notes, auto_tag_ids,
                    cover_root_id, watch_path, watch_enabled, watch_subfolders, display_order
             FROM folder_definition_unique_names;
             DROP TABLE folder_definition_unique_names;
             CREATE INDEX idx_folder_parent_order
                 ON folder_definition(parent_id, display_order, folder_id);",
                )?;
            }
            // Every earlier schema stored some state for browsing: drawing
            // pages, text chunks or the nodes of the alphabetical name tree,
            // each with a journal and repair code of its own. That state is
            // now rebuilt in memory from the canonical rows when the library
            // is opened. So those tables are dropped here, and only the
            // journal of changed roots and its triggers are kept, defined
            // once for every earlier schema.
            if ![
                NO_CLOUD_SYNC_PREDECESSOR,
                KEPT_DELETED_FILES_PREDECESSOR,
                TOMBSTONE_ENTRIES_PREDECESSOR,
                COMBINED_IMPORT_PREDECESSOR,
                TASK_ROLLBACK_PREDECESSOR,
                MEDIA_ALIASES_PREDECESSOR,
                IMPORT_TASKS_PREDECESSOR,
                NO_CLOUD_CLOCKS_PREDECESSOR,
                RESIDENT_PREDECESSOR,
                SOURCE_ROOTS_PREDECESSOR,
                WATCH_LIFECYCLE_PREDECESSOR,
                CHUNKED_ORDERS_PREDECESSOR,
            ]
            .contains(&fingerprint.as_str())
            {
                connection.execute_batch(
                    "DROP TABLE IF EXISTS browsing_summary_page;
             DROP TABLE IF EXISTS browsing_page;
             DROP TABLE IF EXISTS browsing_chunk;
             DROP TABLE IF EXISTS browsing_domain;
             DROP TABLE IF EXISTS browsing_text_dirty;
             DROP TRIGGER IF EXISTS browsing_name_insert;
             DROP TRIGGER IF EXISTS browsing_name_delete;
             DROP TRIGGER IF EXISTS browsing_name_update;
             DROP TABLE IF EXISTS browsing_name_dirty;
             DROP TABLE IF EXISTS browsing_name_manifest;
             DROP TABLE IF EXISTS browsing_name_node;
             DROP TABLE IF EXISTS browsing_name_keys;
             DROP TRIGGER IF EXISTS browsing_root_insert;
             DROP TRIGGER IF EXISTS browsing_root_delete;
             DROP TRIGGER IF EXISTS browsing_root_update;
             DROP TRIGGER IF EXISTS browsing_media_file_update;
             DROP TRIGGER IF EXISTS browsing_cover_facts_update;",
                )?;
                connection.execute_batch(include_str!("browsing_schema.sql"))?;
                connection.execute_batch("DELETE FROM browsing_dirty_root")?;
            }
            // The migration from these predecessors changes no table that
            // the projection is built from, and it does not change the
            // revision, so a saved projection checkpoint stays valid and
            // gets the new fingerprint.
            if [
                NO_CLOUD_SYNC_PREDECESSOR,
                KEPT_DELETED_FILES_PREDECESSOR,
                TOMBSTONE_ENTRIES_PREDECESSOR,
                COMBINED_IMPORT_PREDECESSOR,
                TASK_ROLLBACK_PREDECESSOR,
                MEDIA_ALIASES_PREDECESSOR,
                IMPORT_TASKS_PREDECESSOR,
                NO_CLOUD_CLOCKS_PREDECESSOR,
                CHUNKED_ORDERS_PREDECESSOR,
                WATCH_LIFECYCLE_PREDECESSOR,
                SOURCE_ROOTS_PREDECESSOR,
                RESIDENT_PREDECESSOR,
                TEXT_PREDECESSOR,
                NAME_PREDECESSOR,
                NODE_PREDECESSOR,
                KEYS_PREDECESSOR,
                ROWID_PREDECESSOR,
            ]
            .contains(&fingerprint.as_str())
            {
                connection.execute(
                "UPDATE projection_checkpoint SET schema_fingerprint=?1 WHERE schema_fingerprint=?2",
                rusqlite::params![SCHEMA_FINGERPRINT, fingerprint],
            )?;
            }
            require_columns(
                connection,
                "source_post_attempt",
                &[
                    "attempt_id",
                    "run_query_id",
                    "source_post_id",
                    "state",
                    "terminal_reason",
                    "started_at",
                    "settled_at",
                ],
            )?;
            require_columns(
                connection,
                "source_file_attempt",
                &[
                    "file_attempt_id",
                    "attempt_id",
                    "source_item_id",
                    "content_hash",
                    "state",
                    "staged_path",
                    "bytes_staged",
                    "error",
                ],
            )?;
            require_columns(
                connection,
                "source_attempt_root",
                &["attempt_id", "root_id", "root_stable_key"],
            )?;
            // An attempt from before this column does not say whether it
            // created a root. The column is 0 for it. It is not guessed from
            // the files that the attempt retained, and no counts for
            // notifications are filled in from a guess.
            if !has_column(connection, "source_attempt_root", "created_root")? {
                connection.execute_batch(
                    "ALTER TABLE source_attempt_root ADD COLUMN created_root INTEGER NOT NULL DEFAULT 0 CHECK (created_root IN (0, 1));"
                )?;
            }
            // Watched folders imported into the Inbox before this column
            // existed, so the Inbox is the column's default.
            if !has_column(connection, "folder_definition", "watch_lifecycle")? {
                connection.execute_batch(
                    "ALTER TABLE folder_definition ADD COLUMN watch_lifecycle INTEGER NOT NULL DEFAULT 2 CHECK (watch_lifecycle IN (1, 2));"
                )?;
            }
            require_columns(connection, "source_attempt_root", &["created_root"])?;
            // Each order was stored as one blob for its owner and is now
            // stored in chunks of a fixed size. The ids and their order are
            // unchanged, so the checkpoints stay valid.
            if !has_table(connection, "ordered_membership_chunk")? {
                crate::ordering::migrate_single_blob_orders(connection)?;
            }
            // The tables of the foreground tasks are created, and each
            // queued job gets the columns that name its task.
            connection.execute_batch(TASK_SCHEMA)?;
            if !has_column(connection, "ingest_job", "task_id")? {
                connection.execute_batch(
                    "ALTER TABLE ingest_job ADD COLUMN task_id INTEGER REFERENCES task(task_id) ON DELETE SET NULL;
                     ALTER TABLE ingest_job ADD COLUMN units INTEGER NOT NULL DEFAULT 1;
                     ALTER TABLE ingest_job ADD COLUMN prepared INTEGER NOT NULL DEFAULT 1 CHECK (prepared IN (0, 1));",
                )?;
            }
            require_columns(connection, "ingest_job", &["task_id", "units", "prepared"])?;
            // Merged-away items keep their stable key as an alias. The
            // merge itself, and the one-item-per-file index after it, run
            // once the library is open (`Library::merge_shared_files`).
            connection.execute_batch(ALIAS_SCHEMA)?;
            drop_cloud_sync_tables(connection)?;
            drop_watch_path_uniqueness(connection)?;
            connection.execute(
                "UPDATE library_meta SET schema_fingerprint = ?1 WHERE singleton = 1",
                [SCHEMA_FINGERPRINT],
            )?;
            let foreign_key_error = connection
                .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                .optional()?;
            if foreign_key_error.is_some() {
                return Err(LibraryError::Incompatible(
                    "migration would leave invalid foreign keys".into(),
                ));
            }
            Ok::<_, LibraryError>(())
        })()
    };
    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT; PRAGMA foreign_keys = ON;")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK; PRAGMA foreign_keys = ON;");
            if cancellation.is_cancelled() {
                // Nothing committed, so the backup describes the file as it
                // still is; a retry takes its own. Only a file this attempt
                // created is removed.
                if created_backup {
                    let _ = std::fs::remove_file(&backup_path);
                }
                return Err(LibraryError::OpenCancelled);
            }
            Err(error)
        }
    }
}

/// Drops the tables of the cloud sync (`CLOUD_SYNC_TABLES`) that the library
/// has.
///
/// A file in `kept_deleted_file` is the original of an item that a cloud sync
/// merge deleted. Its `media_file` row and the file on disk were kept for a
/// few days in case the item came back. Before the table is dropped, each of
/// these files that no media item uses is added to `blob_cleanup_queue`, so
/// the blob cleanup removes it from the disk like the file of any other
/// deleted item.
fn drop_cloud_sync_tables(connection: &Connection) -> Result<()> {
    if has_table(connection, "kept_deleted_file")? {
        connection.execute_batch(
            "INSERT INTO blob_cleanup_queue (file_id, file_path, enqueued_revision)
             SELECT file.file_id, file.file_path,
                    (SELECT revision FROM library_meta WHERE singleton = 1)
             FROM kept_deleted_file kept
             JOIN media_file file ON file.content_hash = kept.content_hash
             WHERE NOT EXISTS (
                 SELECT 1 FROM media_item media WHERE media.file_id = file.file_id
             )
             ON CONFLICT(file_id) DO NOTHING;",
        )?;
    }
    for table in CLOUD_SYNC_TABLES {
        connection.execute_batch(&format!("DROP TABLE IF EXISTS {table};"))?;
    }
    Ok(())
}

/// Rebuilds `folder_definition` without the uniqueness of `watch_path`, when
/// the table still has it. Synced folders come from several computers, so two
/// of them can name the same path; `Library::set_folder_watch` checks the path
/// when the user sets it. The rows, their ids and their order are kept, so a
/// saved projection checkpoint stays valid.
fn drop_watch_path_uniqueness(connection: &Connection) -> Result<()> {
    let definition: String = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'folder_definition'",
        [],
        |row| row.get(0),
    )?;
    if !definition.contains("watch_path TEXT UNIQUE") {
        return Ok(());
    }
    let table = SCHEMA
        .split(";\n")
        .find(|statement| statement.contains("CREATE TABLE folder_definition ("))
        .ok_or_else(|| LibraryError::InvalidState("the schema has no folder_definition".into()))?
        .replace("CREATE TABLE folder_definition (", "CREATE TABLE folder_definition_rebuilt (");
    connection.execute_batch(&format!(
        "{table};
         INSERT INTO folder_definition_rebuilt (
             folder_id, stable_key, parent_id, name, icon, color, notes, auto_tag_ids,
             cover_root_id, watch_path, watch_enabled, watch_subfolders, watch_lifecycle,
             display_order
         )
         SELECT folder_id, stable_key, parent_id, name, icon, color, notes, auto_tag_ids,
                cover_root_id, watch_path, watch_enabled, watch_subfolders, watch_lifecycle,
                display_order
         FROM folder_definition;
         DROP TABLE folder_definition;
         ALTER TABLE folder_definition_rebuilt RENAME TO folder_definition;
         CREATE INDEX idx_folder_parent_order
             ON folder_definition(parent_id, display_order, folder_id);"
    ))?;
    Ok(())
}

fn metadata(connection: &Connection) -> Result<(u32, String, u64)> {
    let row = connection.query_row(
        "SELECT schema_generation, schema_fingerprint, revision
         FROM library_meta WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
            ))
        },
    );
    row.map_err(|_| LibraryError::Incompatible("missing schema-generation-1 metadata".into()))
}

/// Returns the path of a backup of the database at `database_path`, made
/// now: the database's file name followed by `.<label>-<milliseconds since
/// 1970>.backup`, in the database's folder. The migration uses the label
/// `pre-migration` and a merge of another library uses `pre-merge`.
pub fn backup_path(database_path: &Path, label: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("library.sqlite");
    database_path.with_file_name(format!("{file_name}.{label}-{timestamp}.backup"))
}

/// Writes a copy of the whole database, as it is committed at this moment,
/// to the new file `backup_path` (`VACUUM INTO`). The connection must not be
/// inside a transaction. SQLite refuses to write over an existing file.
pub fn backup(connection: &Connection, backup_path: &Path) -> Result<()> {
    connection.execute("VACUUM INTO ?1", [backup_path.to_string_lossy().as_ref()])?;
    Ok(())
}

fn has_table(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    Ok(found)
}

fn require_columns(connection: &Connection, table: &str, required: &[&str]) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if let Some(missing) = required
        .iter()
        .find(|column| !columns.iter().any(|item| item == *column))
    {
        return Err(LibraryError::Incompatible(format!(
            "migration produced an invalid {table} table: missing {missing}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{LEGACY_SCHEMA_FINGERPRINT, PREVIOUS_SCHEMA_FINGERPRINT, SCHEMA_FINGERPRINT};
    use crate::{LibraryDatabase, LibraryError};

    const OBSOLETE_BROWSING_TABLES: &str = "'browsing_summary_page','browsing_page','browsing_chunk','browsing_domain','browsing_text_dirty','browsing_name_dirty','browsing_name_manifest','browsing_name_node','browsing_name_keys'";

    #[test]
    fn subscription_outcome_indexes_are_installed_on_existing_library_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DROP INDEX idx_source_post_attempt_added;
             DROP INDEX idx_source_file_attempt_retained;
             DROP INDEX idx_subscription_run_latest;
             DROP INDEX idx_subscription_run_active;",
            )
            .unwrap();
        drop(connection);
        for _ in 0..2 {
            drop(LibraryDatabase::open(&path).unwrap());
            let connection = Connection::open(&path).unwrap();
            assert_eq!(
                count(
                    &connection,
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN
                 ('idx_source_post_attempt_added', 'idx_source_file_attempt_retained',
                  'idx_subscription_run_latest', 'idx_subscription_run_active')"
                ),
                4
            );
            assert_eq!(super::validate(&connection).unwrap(), 1);
        }
        // A change that only adds accelerator indexes does not rewrite the
        // canonical tables and is not a migration.
        assert_eq!(fingerprint_of(&path), SCHEMA_FINGERPRINT);
        assert_eq!(backups_in(directory.path()), 0);
    }

    fn remove_browsing_schema(connection: &Connection) {
        connection
            .execute_batch("ALTER TABLE source_attempt_root DROP COLUMN created_root")
            .unwrap();
        connection
            .execute_batch(
                "DROP TRIGGER browsing_root_insert;
            DROP TRIGGER browsing_root_delete; DROP TRIGGER browsing_root_update;
            DROP TRIGGER browsing_media_file_update; DROP TRIGGER browsing_cover_facts_update;
            DROP TABLE browsing_dirty_root;",
            )
            .unwrap();
    }

    /// Adds the tables for browsing that the earlier schemas stored, in the
    /// forms that the migration must drop. The migration does not read what
    /// is in them.
    fn add_obsolete_browsing_schema(connection: &Connection) {
        connection.execute_batch(
            "CREATE TABLE browsing_domain(domain TEXT PRIMARY KEY, format_version INTEGER, generation INTEGER, chunk_count INTEGER);
            CREATE TABLE browsing_chunk(domain TEXT, chunk_id INTEGER, format_version INTEGER, generation INTEGER, checksum BLOB, page_directory BLOB, PRIMARY KEY(domain, chunk_id));
            CREATE TABLE browsing_page(domain TEXT, chunk_id INTEGER, page_id INTEGER, format_version INTEGER, checksum BLOB, payload BLOB, PRIMARY KEY(domain, page_id));
            CREATE TABLE browsing_summary_page(page_id INTEGER PRIMARY KEY, format_version INTEGER, checksum BLOB, payload BLOB);
            CREATE TABLE browsing_text_dirty(chunk_id INTEGER PRIMARY KEY);
            CREATE TABLE browsing_name_node(node_id INTEGER PRIMARY KEY, checksum BLOB, payload BLOB);
            CREATE TABLE browsing_name_manifest(singleton INTEGER PRIMARY KEY, format_version INTEGER, generation INTEGER, root_node INTEGER, node_count INTEGER, key_count INTEGER);
            CREATE TABLE browsing_name_dirty(root_id INTEGER PRIMARY KEY);
            CREATE TABLE browsing_name_keys(page_id INTEGER PRIMARY KEY, checksum BLOB, payload BLOB);
            CREATE TRIGGER browsing_name_insert AFTER INSERT ON library_root BEGIN INSERT OR IGNORE INTO browsing_name_dirty VALUES (NEW.root_id); END;
            CREATE TRIGGER browsing_name_delete AFTER DELETE ON library_root BEGIN INSERT OR IGNORE INTO browsing_name_dirty VALUES (OLD.root_id); END;
            CREATE TRIGGER browsing_name_update AFTER UPDATE OF name ON library_root BEGIN INSERT OR IGNORE INTO browsing_name_dirty VALUES (NEW.root_id); END;
            INSERT INTO browsing_domain VALUES ('summary', 3, 1, 1), ('text', 1, 1, 1);
            INSERT INTO browsing_summary_page VALUES (0, 3, X'00', X'00');
            INSERT INTO browsing_page VALUES ('text', 0, 0, 1, X'00', X'00');",
        ).unwrap();
    }

    fn count(connection: &Connection, sql: &str) -> u32 {
        connection
            .query_row(sql, [], |row| row.get::<_, u32>(0))
            .unwrap()
    }

    fn assert_resident_browsing_schema(connection: &Connection) {
        assert_eq!(super::validate(connection).unwrap(), 1);
        assert_eq!(
            count(
                connection,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name IN ({OBSOLETE_BROWSING_TABLES})"
                )
            ),
            0
        );
        assert_eq!(
            count(
                connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'browsing_%'"
            ),
            5
        );
        assert_eq!(
            count(connection, "SELECT COUNT(*) FROM browsing_dirty_root"),
            0
        );
    }

    #[test]
    fn browsing_migration_preserves_canonical_rows_and_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        remove_browsing_schema(&connection);
        connection.execute_batch("INSERT INTO library_item VALUES (7,'migration-item',1);
            INSERT INTO media_file(file_id,content_hash,file_path,mime,size_bytes) VALUES(7,'hash','synthetic','image/png',123);
            INSERT INTO media_item VALUES(7,'猫',NULL,7);
            INSERT INTO library_root(root_id,name,cover_media_id,imported_at_ms,modified_at_ms,media_count,total_size_bytes)
                VALUES(7,'猫',7,-10,0,1,123);").unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint=?1",
                [super::DRAWING_PREDECESSOR],
            )
            .unwrap();
        drop(connection);
        drop(LibraryDatabase::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        assert_eq!(super::validate(&connection).unwrap(), 1);
        assert_eq!(
            connection
                .query_row("SELECT name FROM library_root WHERE root_id=7", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "猫"
        );
        assert_resident_browsing_schema(&connection);
        let backups = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "backup"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            super::metadata(&backup).unwrap().1,
            super::DRAWING_PREDECESSOR
        );
        assert_eq!(
            backup
                .query_row("SELECT name FROM library_root WHERE root_id=7", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "猫"
        );
        assert_eq!(
            backup
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='browsing_dirty_root'",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn resident_migration_adds_explicit_creation_provenance_without_guessing_old_counts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("ALTER TABLE source_attempt_root DROP COLUMN created_root")
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint=?1",
                [super::RESIDENT_PREDECESSOR],
            )
            .unwrap();
        drop(connection);
        drop(LibraryDatabase::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        super::validate(&connection).unwrap();
        let default: String = connection.query_row(
            "SELECT dflt_value FROM pragma_table_info('source_attempt_root') WHERE name='created_root'",
            [], |row| row.get(0)).unwrap();
        assert_eq!(default, "0");
        let backups = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "backup"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            super::metadata(&backup).unwrap().1,
            super::RESIDENT_PREDECESSOR
        );
        assert_eq!(count(&backup, "SELECT COUNT(*) FROM pragma_table_info('source_attempt_root') WHERE name='created_root'"), 0);
    }

    #[test]
    fn source_roots_migration_keeps_existing_watches_importing_into_the_inbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "ALTER TABLE folder_definition DROP COLUMN watch_lifecycle;
                 INSERT INTO folder_definition
                     (folder_id, stable_key, name, watch_path, watch_enabled, display_order)
                 VALUES (3, 'watched', 'Watched', '/watched', 1, 0);",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint=?1",
                [super::SOURCE_ROOTS_PREDECESSOR],
            )
            .unwrap();
        drop(connection);
        drop(LibraryDatabase::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        super::validate(&connection).unwrap();
        assert_eq!(
            count(
                &connection,
                "SELECT watch_lifecycle FROM folder_definition WHERE folder_id = 3"
            ),
            2
        );
    }

    /// The cloud sync tables of the schema `KEPT_DELETED_FILES_PREDECESSOR`,
    /// as that schema created them.
    const KEPT_DELETED_FILES_CLOUD_TABLES: &str = "
        CREATE TABLE cloud_state (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            library_id TEXT NOT NULL UNIQUE,
            device_id TEXT NOT NULL,
            provider TEXT CHECK (provider IN ('google_drive', 'dropbox')),
            account_label TEXT,
            remote_root TEXT,
            paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
            state TEXT NOT NULL DEFAULT 'disabled',
            phase TEXT NOT NULL DEFAULT 'idle',
            blocking INTEGER NOT NULL DEFAULT 0 CHECK (blocking IN (0, 1)),
            completed_units INTEGER NOT NULL DEFAULT 0,
            total_units INTEGER,
            message TEXT NOT NULL DEFAULT '',
            last_sync_at TEXT,
            last_snapshot_at TEXT,
            pending_blobs INTEGER NOT NULL DEFAULT 0,
            missing_blobs INTEGER NOT NULL DEFAULT 0,
            schema_generation INTEGER NOT NULL DEFAULT 1,
            hlc_physical_ms INTEGER NOT NULL DEFAULT 0,
            hlc_logical INTEGER NOT NULL DEFAULT 0,
            retention_json TEXT NOT NULL DEFAULT '{}'
        ) STRICT;
        CREATE TABLE cloud_device_frontier (
            device_id TEXT PRIMARY KEY,
            hlc_physical_ms INTEGER NOT NULL,
            hlc_logical INTEGER NOT NULL,
            updated_at TEXT NOT NULL
        ) WITHOUT ROWID, STRICT;
        CREATE TABLE shared_entry (
            subject_key TEXT NOT NULL,
            kind TEXT NOT NULL,
            second_key TEXT NOT NULL DEFAULT '',
            value TEXT,
            clock_ms INTEGER NOT NULL,
            clock_counter INTEGER NOT NULL,
            device_id TEXT NOT NULL,
            PRIMARY KEY(subject_key, kind, second_key)
        ) WITHOUT ROWID, STRICT;
        CREATE TABLE entry_recording (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            started_ms INTEGER NOT NULL,
            started_counter INTEGER NOT NULL,
            start_next_root INTEGER,
            start_end_root INTEGER NOT NULL,
            action_revision INTEGER NOT NULL DEFAULT 0,
            action_ms INTEGER NOT NULL DEFAULT 0,
            action_counter INTEGER NOT NULL DEFAULT 0
        ) STRICT;
        CREATE TABLE pending_entry (
            pending_id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            second_key TEXT NOT NULL DEFAULT '',
            value TEXT,
            clock_ms INTEGER NOT NULL,
            clock_counter INTEGER NOT NULL,
            root_bitmap BLOB NOT NULL
        ) STRICT;
        CREATE TABLE merge_touched (
            subject_kind TEXT NOT NULL,
            subject_key TEXT NOT NULL,
            PRIMARY KEY(subject_kind, subject_key)
        ) WITHOUT ROWID, STRICT;
        CREATE TABLE wanted_file (content_hash TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
        CREATE TABLE kept_deleted_file (
            content_hash TEXT PRIMARY KEY,
            remove_after_ms INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX idx_kept_deleted_file_remove_after ON kept_deleted_file(remove_after_ms);
        CREATE TABLE cloud_deleted_original (
            file_name TEXT PRIMARY KEY,
            deleted_at TEXT NOT NULL,
            purge_after TEXT
        ) WITHOUT ROWID, STRICT;
        CREATE TABLE cloud_quarantine (
            quarantine_id INTEGER PRIMARY KEY,
            mutation_id TEXT NOT NULL UNIQUE,
            reason TEXT NOT NULL,
            envelope_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            resolved_at TEXT
        ) STRICT;
        CREATE TABLE cloud_snapshot (
            snapshot_id TEXT PRIMARY KEY,
            frontier_json TEXT NOT NULL,
            database_sha256 TEXT NOT NULL,
            artifact_sha256 TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            verified INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
            created_at TEXT NOT NULL,
            remote_path TEXT,
            published_at TEXT
        ) WITHOUT ROWID, STRICT;
        CREATE TABLE cloud_blob_state (
            file_hash TEXT PRIMARY KEY REFERENCES media_file(content_hash) ON DELETE CASCADE,
            state TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            remote_present INTEGER NOT NULL DEFAULT 0 CHECK (remote_present IN (0, 1)),
            remote_extension TEXT,
            last_error TEXT,
            uploaded_at TEXT,
            updated_at TEXT NOT NULL
        ) WITHOUT ROWID, STRICT;
        CREATE INDEX idx_cloud_blob_state_state ON cloud_blob_state(state, remote_present);
        CREATE TABLE cloud_journal (
            journal_id INTEGER PRIMARY KEY,
            revision INTEGER NOT NULL,
            operation_kind TEXT NOT NULL,
            target_bitmap BLOB,
            payload_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            expanded_at_ms INTEGER
        ) STRICT;
        CREATE INDEX idx_cloud_pending ON cloud_journal(expanded_at_ms, journal_id);";

    /// Returns the names of the tables, indexes and triggers in the
    /// database that belong to cloud sync.
    fn cloud_sync_objects(connection: &Connection) -> Vec<String> {
        let names = super::CLOUD_SYNC_TABLES
            .iter()
            .map(|table| format!("'{table}'"))
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = connection
            .prepare(&format!(
                "SELECT name FROM sqlite_master
                 WHERE name LIKE '%cloud%' OR name IN ({names}) OR tbl_name IN ({names})"
            ))
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    /// Adds a media item `id` with its own file whose content hash is
    /// `hash`, or only the file when `item` is false.
    fn add_file(connection: &Connection, id: u32, hash: &str, item: bool) {
        connection
            .execute(
                "INSERT INTO media_file(file_id, content_hash, file_path, mime, size_bytes)
                 VALUES (?1, ?2, 'blobs/f/' || ?2, 'image/png', 10)",
                rusqlite::params![id, hash],
            )
            .unwrap();
        if item {
            connection
                .execute_batch(&format!(
                    "INSERT INTO library_item VALUES ({id}, 'item-{id}', 1);
                     INSERT INTO media_item VALUES ({id}, 'Item {id}', NULL, {id});
                     INSERT INTO library_root(root_id, name, cover_media_id, imported_at_ms,
                                              modified_at_ms, media_count, total_size_bytes)
                         VALUES ({id}, 'Item {id}', {id}, 1, 1, 1, 10);"
                ))
                .unwrap();
        }
    }

    fn queued_files(connection: &Connection) -> Vec<u32> {
        let mut statement = connection
            .prepare("SELECT file_id FROM blob_cleanup_queue ORDER BY file_id")
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, u32>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    /// A library of the schema just before this one has every cloud sync
    /// table, with rows. The migration drops them all, queues the kept files
    /// that no item uses for removal from the disk, keeps the projection
    /// checkpoint and keeps a backup of the database as it was.
    #[test]
    fn the_migration_drops_the_cloud_sync_tables_and_queues_the_kept_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(KEPT_DELETED_FILES_CLOUD_TABLES).unwrap();
        // File 1 is used by an item, file 2 is a kept file that no item
        // uses, and file 3 is the file of an item that is not kept.
        add_file(&connection, 1, "used", true);
        add_file(&connection, 2, "kept", false);
        add_file(&connection, 3, "other", true);
        connection
            .execute_batch(
                "INSERT INTO cloud_state (singleton, library_id, device_id, provider)
                     VALUES (1, 'library', 'device', 'dropbox');
                 INSERT INTO cloud_device_frontier VALUES ('device', 5, 0, 'now');
                 INSERT INTO shared_entry VALUES ('kept', 'delete', '', NULL, 5, 0, 'device');
                 INSERT INTO entry_recording (singleton, started_ms, started_counter, start_end_root)
                     VALUES (1, 1, 0, 4);
                 INSERT INTO pending_entry VALUES (1, 'rating', '', '3', 5, 0, X'00');
                 INSERT INTO merge_touched VALUES ('file', 'kept');
                 INSERT INTO wanted_file VALUES ('wanted');
                 INSERT INTO kept_deleted_file VALUES ('used', 1), ('kept', 1), ('missing', 1);
                 INSERT INTO cloud_deleted_original VALUES ('gone.png', 'now', NULL);
                 INSERT INTO cloud_quarantine VALUES (1, 'mutation', 'reason', '{}', 'now', NULL);
                 INSERT INTO cloud_snapshot (snapshot_id, frontier_json, database_sha256,
                                             artifact_sha256, size_bytes, created_at)
                     VALUES ('snapshot', '{}', 'a', 'b', 1, 'now');
                 INSERT INTO cloud_blob_state (file_hash, state, updated_at)
                     VALUES ('used', 'available', 'now');
                 INSERT INTO cloud_journal (revision, operation_kind, payload_json, created_at_ms)
                     VALUES (1, 'root.ingest', '{}', 1);
                 INSERT INTO projection_checkpoint VALUES
                     (1, 'picto-library-schema-1-2026-11-23-kept-deleted-files', 'hash', 1, X'00', X'00');",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1",
                [super::KEPT_DELETED_FILES_PREDECESSOR],
            )
            .unwrap();
        drop(connection);

        drop(LibraryDatabase::open(&path).unwrap());
        // The library opens a second time without another migration.
        drop(LibraryDatabase::open(&path).unwrap());

        let connection = Connection::open(&path).unwrap();
        super::validate(&connection).unwrap();
        assert_eq!(cloud_sync_objects(&connection), Vec::<String>::new());
        assert_eq!(queued_files(&connection), [2]);
        assert_eq!(count(&connection, "SELECT COUNT(*) FROM media_file"), 3);
        assert_eq!(count(&connection, "SELECT COUNT(*) FROM media_item"), 2);
        assert_eq!(
            connection
                .query_row(
                    "SELECT schema_fingerprint FROM projection_checkpoint",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            SCHEMA_FINGERPRINT
        );
        assert_eq!(backups_in(directory.path()), 1);
        let backup = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "backup"))
            .unwrap();
        let backup = Connection::open(backup).unwrap();
        assert_eq!(
            super::metadata(&backup).unwrap().1,
            super::KEPT_DELETED_FILES_PREDECESSOR
        );
        assert_eq!(count(&backup, "SELECT COUNT(*) FROM kept_deleted_file"), 3);
        assert_eq!(count(&backup, "SELECT COUNT(*) FROM cloud_journal"), 1);
    }

    /// A library of the schema whose watched paths were unique and that had
    /// no `tag_merge` loses that uniqueness, gets the table, keeps its folders
    /// and a backup, and opens a second time without another migration.
    #[test]
    fn the_migration_lets_two_folders_watch_one_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        let unique = super::SCHEMA
            .split(";\n")
            .find(|statement| statement.contains("CREATE TABLE folder_definition ("))
            .unwrap()
            .replace("    watch_path TEXT,\n", "    watch_path TEXT UNIQUE,\n")
            .replace("CREATE TABLE folder_definition (", "CREATE TABLE folder_definition_unique (");
        connection
            .execute_batch(&format!(
                "PRAGMA foreign_keys = OFF;
                 DROP TABLE tag_merge;
                 {unique};
                 DROP TABLE folder_definition;
                 ALTER TABLE folder_definition_unique RENAME TO folder_definition;
                 INSERT INTO folder_definition (folder_id, stable_key, parent_id, name, watch_path, display_order)
                     VALUES (1, 'top', NULL, 'Top', '/watched', 0), (2, 'child', 1, 'Child', NULL, 0);"
            ))
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1",
                [super::NO_CLOUD_SYNC_PREDECESSOR],
            )
            .unwrap();
        drop(connection);

        drop(LibraryDatabase::open(&path).unwrap());
        drop(LibraryDatabase::open(&path).unwrap());

        let connection = Connection::open(&path).unwrap();
        super::validate(&connection).unwrap();
        assert_eq!(count(&connection, "SELECT COUNT(*) FROM folder_definition WHERE parent_id = 1"), 1);
        assert_eq!(count(&connection, "SELECT COUNT(*) FROM sqlite_master WHERE name = 'tag_merge'"), 1);
        connection
            .execute("UPDATE folder_definition SET watch_path = '/watched' WHERE folder_id = 2", [])
            .unwrap();
        assert_eq!(backups_in(directory.path()), 1);
    }

    /// A library of the oldest schema that this version still migrates has
    /// the cloud sync tables of that time, which are dropped as well.
    #[test]
    fn the_oldest_supported_schema_migrates_without_cloud_sync_tables() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        remove_browsing_schema(&connection);
        add_obsolete_browsing_schema(&connection);
        connection
            .execute_batch(
                "DROP TABLE source_attempt_root;
                 DROP TABLE source_file_attempt;
                 DROP TABLE source_post_attempt;",
            )
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE cloud_state (
                     singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                     library_id TEXT NOT NULL UNIQUE,
                     device_id TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE cloud_outbox (
                     mutation_id TEXT PRIMARY KEY, payload_json TEXT NOT NULL,
                     created_at TEXT NOT NULL, published_at TEXT
                 ) WITHOUT ROWID, STRICT;
                 CREATE INDEX idx_cloud_outbox_pending
                     ON cloud_outbox(published_at, created_at, mutation_id);
                 CREATE TABLE cloud_applied_mutation (mutation_id TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_device_frontier (device_id TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_field_clock (object_key TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_membership_clock (owner_key TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_tombstone (
                     object_kind TEXT NOT NULL, object_key TEXT NOT NULL,
                     deleted_at TEXT NOT NULL, purge_after TEXT,
                     PRIMARY KEY(object_kind, object_key)
                 ) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_quarantine (quarantine_id INTEGER PRIMARY KEY) STRICT;
                 CREATE TABLE cloud_snapshot (snapshot_id TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_blob_state (
                     file_hash TEXT PRIMARY KEY REFERENCES media_file(content_hash) ON DELETE CASCADE
                 ) WITHOUT ROWID, STRICT;
                 CREATE TABLE cloud_journal (journal_id INTEGER PRIMARY KEY, revision INTEGER NOT NULL) STRICT;
                 CREATE INDEX idx_cloud_pending ON cloud_journal(revision, journal_id);
                 INSERT INTO cloud_state VALUES (1, 'library', 'device');
                 INSERT INTO cloud_outbox VALUES ('mutation', '{}', 'now', NULL);
                 INSERT INTO cloud_tombstone VALUES ('blob', 'abc.png', 'now', NULL);
                 INSERT INTO cloud_journal VALUES (1, 1);",
            )
            .unwrap();
        add_file(&connection, 7, "hash", true);
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1",
                [LEGACY_SCHEMA_FINGERPRINT],
            )
            .unwrap();
        drop(connection);

        drop(LibraryDatabase::open(&path).unwrap());
        drop(LibraryDatabase::open(&path).unwrap());

        let connection = Connection::open(&path).unwrap();
        super::validate(&connection).unwrap();
        assert_resident_browsing_schema(&connection);
        assert_eq!(cloud_sync_objects(&connection), Vec::<String>::new());
        assert_eq!(queued_files(&connection), Vec::<u32>::new());
        assert_eq!(count(&connection, "SELECT COUNT(*) FROM library_root"), 1);
        assert_eq!(backups_in(directory.path()), 1);
    }

    #[test]
    fn malformed_legacy_orders_fail_without_mutating_the_database_on_reopen() {
        for (label, cardinality, version, declared_count, ids) in [
            ("format", 2, 2, 2_u32, vec![1_u32, 2]),
            ("cardinality", 3, 1, 2, vec![1, 2]),
            ("negative cardinality", -1, 1, 2, vec![1, 2]),
            ("embedded count", 2, 1, 3, vec![1, 2]),
            ("duplicates", 3, 1, 3, vec![1, 2, 1]),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("library.sqlite");
            drop(LibraryDatabase::create(&path).unwrap());
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(
                "DROP TABLE ordered_membership_chunk;
                 CREATE TABLE ordered_membership (
                     owner_kind INTEGER NOT NULL, owner_id INTEGER NOT NULL,
                     revision INTEGER NOT NULL, cardinality INTEGER NOT NULL,
                     format_version INTEGER NOT NULL, checksum BLOB NOT NULL,
                     payload BLOB NOT NULL, PRIMARY KEY(owner_kind, owner_id)
                 ) WITHOUT ROWID, STRICT;",
            ).unwrap();
            let mut payload = declared_count.to_le_bytes().to_vec();
            ids.iter().for_each(|id| payload.extend_from_slice(&id.to_le_bytes()));
            connection.execute(
                "INSERT INTO ordered_membership VALUES (2, 9, 1, ?1, ?2, ?3, ?4)",
                rusqlite::params![cardinality, version, crate::bitmap::checksum(&payload).as_slice(), payload],
            ).unwrap();
            connection.execute("UPDATE library_meta SET schema_fingerprint=?1", [super::WATCH_LIFECYCLE_PREDECESSOR]).unwrap();
            drop(connection);

            for _ in 0..2 {
                assert!(LibraryDatabase::open(&path).is_err(), "{label} was silently migrated");
                assert_eq!(fingerprint_of(&path), super::WATCH_LIFECYCLE_PREDECESSOR);
                let connection = Connection::open(&path).unwrap();
                assert_eq!(count(&connection, "SELECT COUNT(*) FROM sqlite_master WHERE name='ordered_membership_chunk'"), 0);
                let stored = connection.query_row("SELECT cardinality, format_version, payload FROM ordered_membership", [], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Vec<u8>>(2)?))
                }).unwrap();
                assert_eq!(stored, (cardinality, version, payload.clone()));
            }
            let backups = std::fs::read_dir(directory.path()).unwrap()
                .map(|entry| entry.unwrap().path()).filter(|path| path.extension().is_some_and(|extension| extension == "backup"))
                .collect::<Vec<_>>();
            assert!(!backups.is_empty());
            for backup in backups {
                assert_eq!(fingerprint_of(&backup), super::WATCH_LIFECYCLE_PREDECESSOR);
                let connection = Connection::open(backup).unwrap();
                assert_eq!(connection.query_row("SELECT payload FROM ordered_membership", [], |row| row.get::<_, Vec<u8>>(0)).unwrap(), payload);
            }
        }
    }

    #[test]
    fn single_blob_orders_are_split_into_chunks_without_changing_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        // The predecessor kept each order as one blob: a u32 count, then ids.
        let ids = (1..=2_500_u32).rev().collect::<Vec<_>>();
        let mut payload = (ids.len() as u32).to_le_bytes().to_vec();
        ids.iter()
            .for_each(|id| payload.extend_from_slice(&id.to_le_bytes()));
        connection
            .execute_batch(
                "DROP TABLE ordered_membership_chunk;
                 CREATE TABLE ordered_membership (
                     owner_kind INTEGER NOT NULL, owner_id INTEGER NOT NULL,
                     revision INTEGER NOT NULL, cardinality INTEGER NOT NULL,
                     format_version INTEGER NOT NULL, checksum BLOB NOT NULL,
                     payload BLOB NOT NULL, PRIMARY KEY(owner_kind, owner_id)
                 ) WITHOUT ROWID, STRICT;",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO ordered_membership VALUES (2, 9, 1, ?1, 1, ?2, ?3)",
                rusqlite::params![
                    ids.len() as i64,
                    crate::bitmap::checksum(&payload).as_slice(),
                    payload
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint=?1",
                [super::WATCH_LIFECYCLE_PREDECESSOR],
            )
            .unwrap();
        drop(connection);
        drop(LibraryDatabase::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        super::validate(&connection).unwrap();
        assert_eq!(
            crate::ordering::load(&connection, crate::ordering::OrderOwnerKind::Folder, 9)
                .unwrap()
                .unwrap(),
            ids
        );
        assert_eq!(
            count(&connection, "SELECT COUNT(*) FROM ordered_membership_chunk"),
            3
        );
    }

    #[test]
    fn open_migrates_the_rowid_predecessor_and_drops_every_persisted_browsing_domain() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("ALTER TABLE source_attempt_root DROP COLUMN created_root")
            .unwrap();
        add_obsolete_browsing_schema(&connection);
        connection.execute_batch("INSERT INTO library_item VALUES (7,'migration-item',1);
            INSERT INTO media_file(file_id,content_hash,file_path,mime,size_bytes) VALUES(7,'hash','synthetic','image/png',123);
            INSERT INTO media_item VALUES(7,'猫',NULL,7);
            INSERT INTO library_root(root_id,name,cover_media_id,imported_at_ms,modified_at_ms,media_count,total_size_bytes)
                VALUES(7,'猫',7,-10,0,1,123);").unwrap();
        // The database holds a journal row that was left behind and a
        // checkpoint that names the earlier schema.
        assert_eq!(
            count(&connection, "SELECT COUNT(*) FROM browsing_dirty_root"),
            1
        );
        connection
            .execute(
                "INSERT INTO projection_checkpoint VALUES (1, ?1, 'hash', 1, X'00', X'00')",
                [super::ROWID_PREDECESSOR],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint=?1",
                [super::ROWID_PREDECESSOR],
            )
            .unwrap();
        drop(connection);
        drop(LibraryDatabase::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        assert_resident_browsing_schema(&connection);
        assert_eq!(
            connection
                .query_row("SELECT name FROM library_root WHERE root_id=7", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "猫"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT schema_fingerprint FROM projection_checkpoint",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            SCHEMA_FINGERPRINT
        );
        // The triggers that the migration defined again write a row to the
        // journal of changed roots for each change, as before.
        connection
            .execute("UPDATE library_root SET name='犬' WHERE root_id=7", [])
            .unwrap();
        assert_eq!(
            count(&connection, "SELECT COUNT(*) FROM browsing_dirty_root"),
            1
        );
    }

    #[test]
    fn combined_schema_migrates_feature_and_acceptance_predecessors_with_backup_and_reopen() {
        for predecessor in [
            super::COMBINED_IMPORT_PREDECESSOR,
            super::TASK_ROLLBACK_PREDECESSOR,
            super::MEDIA_ALIASES_PREDECESSOR,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("library.sqlite");
            let library = crate::Library::create(&path).unwrap();
            let task = library
                .create_task("import", "Paused import", "{}", true, false, 1)
                .unwrap();
            let folder = library
                .create_folder_for_task("Task folder", None, task)
                .unwrap()
                .0;
            library.set_task_paused(task, true).unwrap();
            drop(library);
            let connection = Connection::open(&path).unwrap();
            if predecessor == super::MEDIA_ALIASES_PREDECESSOR {
                connection.execute_batch(
                    "DROP TABLE task_undo; DROP TABLE task_added_tag; DROP TABLE task_created_root;
                     DROP INDEX idx_ingest_job_task; DROP INDEX idx_ingest_job_claim;
                     ALTER TABLE ingest_job DROP COLUMN task_id;
                     ALTER TABLE ingest_job DROP COLUMN units;
                     ALTER TABLE ingest_job DROP COLUMN prepared;
                     DROP TABLE task;",
                ).unwrap();
            } else if predecessor == super::COMBINED_IMPORT_PREDECESSOR {
                connection.execute_batch("DROP TABLE task_undo;").unwrap();
            } else {
                connection
                    .execute_batch(
                        "DROP TABLE library_item_alias; DROP INDEX idx_media_item_one_per_file;",
                    )
                    .unwrap();
            }
            connection
                .execute(
                    "UPDATE library_meta SET schema_fingerprint = ?1",
                    [predecessor],
                )
                .unwrap();
            drop(connection);

            let library = crate::Library::open(&path).unwrap();
            library.merge_shared_files().unwrap();
            if predecessor == super::COMBINED_IMPORT_PREDECESSOR {
                assert_eq!(library.task(task).unwrap().unwrap().state, crate::TaskState::Paused);
                assert!(library.folders().unwrap().iter().any(|row| row.folder_id == folder));
            }
            if predecessor == super::TASK_ROLLBACK_PREDECESSOR {
                assert_eq!(
                    library.task(task).unwrap().unwrap().state,
                    crate::TaskState::Paused
                );
                library.stop_task(task, true, 2).unwrap();
                library.revert_task(task, 3).unwrap();
                assert!(!library
                    .folders()
                    .unwrap()
                    .iter()
                    .any(|row| row.folder_id == folder));
            }
            drop(library);
            let connection = Connection::open(&path).unwrap();
            assert_eq!(super::metadata(&connection).unwrap().1, SCHEMA_FINGERPRINT);
            for table in ["task", "task_undo", "library_item_alias"] {
                assert!(super::has_table(&connection, table).unwrap());
            }
            super::require_columns(&connection, "ingest_job", &["task_id", "units", "prepared"])
                .unwrap();
            assert_eq!(
                count(
                    &connection,
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='idx_media_item_one_per_file'"
                ),
                1
            );
            drop(connection);
            let backups = std::fs::read_dir(directory.path())
                .unwrap()
                .filter_map(|entry| {
                    let path = entry.unwrap().path();
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .ends_with(".backup")
                        .then_some(path)
                })
                .collect::<Vec<_>>();
            assert_eq!(backups.len(), 1);
            let backup = Connection::open(&backups[0]).unwrap();
            assert_eq!(super::metadata(&backup).unwrap().1, predecessor);
            assert_eq!(
                super::has_table(&backup, "task_undo").unwrap(),
                predecessor == super::TASK_ROLLBACK_PREDECESSOR
            );
            assert_eq!(
                super::has_table(&backup, "library_item_alias").unwrap(),
                predecessor != super::TASK_ROLLBACK_PREDECESSOR
            );
            drop(crate::Library::open(&path).unwrap());
        }
    }

    #[test]
    fn import_task_rollback_migration_backs_up_and_reopens() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&database_path).unwrap());
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE task_undo;
            INSERT INTO task(task_id, kind, title, state, input_json, created_at_ms)
            VALUES (1, 'import', 'Paused', 'paused', '{}', 1);",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1",
                [super::IMPORT_TASKS_PREDECESSOR],
            )
            .unwrap();
        drop(connection);
        drop(LibraryDatabase::open(&database_path).unwrap());
        let connection = Connection::open(&database_path).unwrap();
        assert!(super::has_table(&connection, "task_undo").unwrap());
        assert_eq!(
            connection
                .query_row("SELECT state FROM task WHERE task_id=1", [], |row| row
                    .get::<_, String>(
                    0
                ))
                .unwrap(),
            "paused"
        );
        let backups = std::fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|entry| {
                let path = entry.unwrap().path();
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".pre-migration-")
                    .then_some(path)
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert!(!super::has_table(&backup, "task_undo").unwrap());
        drop(connection);
        drop(LibraryDatabase::open(&database_path).unwrap());
    }

    #[test]
    fn open_adds_import_tasks_to_the_previous_schema_and_keeps_queued_jobs() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&database_path).unwrap());

        // The predecessor had no task tables and no task columns on jobs.
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "DROP INDEX IF EXISTS idx_ingest_job_task;
                 DROP INDEX IF EXISTS idx_ingest_job_claim;
                 ALTER TABLE ingest_job DROP COLUMN task_id;
                 ALTER TABLE ingest_job DROP COLUMN units;
                 ALTER TABLE ingest_job DROP COLUMN prepared;
                 DROP TABLE task_added_tag;
                 DROP TABLE task_created_root;
                 DROP TABLE task;
                 INSERT INTO ingest_job
                     (job_key, source_kind, source_path, payload_json, status,
                      attempt_count, available_at, created_at, updated_at)
                 VALUES ('queued', 'manual', '/a.png', '{}', 'pending', 0, 'now', 'now', 'now');",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1 WHERE singleton = 1",
                [super::NO_CLOUD_CLOCKS_PREDECESSOR],
            )
            .unwrap();
        drop(connection);

        drop(LibraryDatabase::open(&database_path).unwrap());
        let migrated = Connection::open(&database_path).unwrap();
        assert_eq!(
            migrated
                .query_row(
                    "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            SCHEMA_FINGERPRINT,
        );
        // A job that was queued before the migration can still be claimed.
        // It has no task, and it counts as read already.
        assert_eq!(
            migrated
                .query_row(
                    "SELECT task_id IS NULL, units, prepared FROM ingest_job WHERE job_key = 'queued'",
                    [],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
                )
                .unwrap(),
            (true, 1, 1),
        );
        for table in ["task", "task_created_root", "task_added_tag"] {
            assert!(super::has_table(&migrated, table).unwrap(), "{table} was not created");
        }
        // Reopening the migrated file needs no further migration.
        drop(migrated);
        drop(LibraryDatabase::open(&database_path).unwrap());
    }

    #[test]
    fn open_migrates_the_immediately_previous_schema() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&database_path).unwrap());

        let connection = Connection::open(&database_path).unwrap();
        remove_browsing_schema(&connection);
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1 WHERE singleton = 1",
                [PREVIOUS_SCHEMA_FINGERPRINT],
            )
            .unwrap();
        drop(connection);

        drop(LibraryDatabase::open(&database_path).unwrap());
        let migrated = Connection::open(&database_path).unwrap();
        assert_eq!(
            migrated
                .query_row(
                    "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            SCHEMA_FINGERPRINT,
        );
        migrated
            .execute_batch(
                "INSERT INTO folder_definition
                     (folder_id, stable_key, parent_id, name, display_order)
                 VALUES (100, 'duplicate-name-1', NULL, 'Same name', 0);
                 INSERT INTO folder_definition
                     (folder_id, stable_key, parent_id, name, display_order)
                 VALUES (101, 'duplicate-name-2', NULL, 'Same name', 1);",
            )
            .unwrap();
        assert_eq!(
            migrated
                .query_row(
                    "SELECT COUNT(*) FROM pragma_index_list('folder_definition')
                     WHERE name = 'idx_folder_parent_order' AND \"unique\" = 0",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .unwrap(),
            1,
        );
    }

    #[test]
    fn open_migrates_the_previous_alpha_schema_and_keeps_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&database_path).unwrap());

        let connection = Connection::open(&database_path).unwrap();
        remove_browsing_schema(&connection);
        connection
            .execute(
                "INSERT INTO setting (key, value_json) VALUES ('kept', 'true')",
                [],
            )
            .unwrap();
        connection
            .execute_batch(
                "DROP TABLE source_attempt_root;
                 DROP TABLE source_file_attempt;
                 DROP TABLE source_post_attempt;",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = ?1 WHERE singleton = 1",
                [LEGACY_SCHEMA_FINGERPRINT],
            )
            .unwrap();
        drop(connection);

        let database = LibraryDatabase::open(&database_path).unwrap();
        let fingerprint = database
            .read(crate::database::WorkPriority::VisibleRead, |connection| {
                Ok(connection.query_row(
                    "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )?)
            })
            .unwrap();
        assert_eq!(fingerprint, SCHEMA_FINGERPRINT);
        drop(database);

        let migrated = Connection::open(&database_path).unwrap();
        assert_eq!(
            migrated
                .query_row(
                    "SELECT value_json FROM setting WHERE key = 'kept'",
                    [],
                    |row| { row.get::<_, String>(0) }
                )
                .unwrap(),
            "true"
        );
        assert_eq!(
            migrated
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name IN (
                         'source_post_attempt', 'source_file_attempt', 'source_attempt_root'
                     )",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            3
        );

        let backups = std::fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.contains(".pre-migration-") && name.ends_with(".backup")
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            backup
                .query_row(
                    "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            LEGACY_SCHEMA_FINGERPRINT
        );
    }

    /// A library whose schema fingerprint is none that this version knows,
    /// such as one that a newer version wrote, is not opened and not changed.
    #[test]
    fn open_rejects_an_unknown_schema_without_mutating_it() {
        for fingerprint in ["unknown", "picto-library-schema-1-2027-01-01-from-a-newer-version"] {
            let directory = tempfile::tempdir().unwrap();
            let database_path = directory.path().join("library.sqlite");
            drop(LibraryDatabase::create(&database_path).unwrap());
            let connection = Connection::open(&database_path).unwrap();
            connection
                .execute(
                    "UPDATE library_meta SET schema_fingerprint = ?1 WHERE singleton = 1",
                    [fingerprint],
                )
                .unwrap();
            drop(connection);

            let error = match LibraryDatabase::open(&database_path) {
                Ok(_) => panic!("{fingerprint}: the library was opened"),
                Err(error) => error,
            };
            assert!(matches!(error, LibraryError::Incompatible(_)), "{fingerprint}");
            let connection = Connection::open(&database_path).unwrap();
            assert_eq!(
                connection
                    .query_row(
                        "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap(),
                fingerprint
            );
            assert_eq!(
                std::fs::read_dir(directory.path())
                    .unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .is_some_and(|name| name.contains(".pre-migration-"))
                    })
                    .count(),
                0
            );
        }
    }

    fn predecessor_fixture(directory: &std::path::Path) -> std::path::PathBuf {
        let path = directory.join("library.sqlite");
        drop(LibraryDatabase::create(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        remove_browsing_schema(&connection);
        connection.execute_batch("INSERT INTO library_item VALUES (7,'migration-item',1);
            INSERT INTO media_file(file_id,content_hash,file_path,mime,size_bytes) VALUES(7,'hash','synthetic','image/png',123);
            INSERT INTO media_item VALUES(7,'猫',NULL,7);
            INSERT INTO library_root(root_id,name,cover_media_id,imported_at_ms,modified_at_ms,media_count,total_size_bytes)
                VALUES(7,'猫',7,-10,0,1,123);").unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint=?1",
                [super::DRAWING_PREDECESSOR],
            )
            .unwrap();
        path
    }

    fn fingerprint_of(path: &std::path::Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
    }

    fn backups_in(directory: &std::path::Path) -> usize {
        std::fs::read_dir(directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.contains(".pre-migration-"))
            })
            .count()
    }

    #[test]
    fn an_open_cancelled_before_migration_touches_neither_schema_nor_directory() {
        let directory = tempfile::tempdir().unwrap();
        let path = predecessor_fixture(directory.path());
        let handle = crate::OpenCancellation::new();
        handle.cancel();
        let mut stages = Vec::new();
        let error = LibraryDatabase::open_with(&path, &handle, &mut |stage| stages.push(stage))
            .err()
            .unwrap();
        assert!(matches!(error, LibraryError::OpenCancelled));
        assert!(stages.is_empty());
        assert_eq!(fingerprint_of(&path), super::DRAWING_PREDECESSOR);
        assert_eq!(backups_in(directory.path()), 0);
        // A fresh handle migrates normally, with exactly one backup.
        drop(LibraryDatabase::open(&path).unwrap());
        assert_eq!(fingerprint_of(&path), SCHEMA_FINGERPRINT);
        assert_eq!(backups_in(directory.path()), 1);
    }

    #[test]
    fn an_open_cancelled_during_migration_rolls_back_and_removes_its_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = predecessor_fixture(directory.path());
        let handle = crate::OpenCancellation::new();
        let cancel = handle.clone();
        let mut stages = Vec::new();
        let error = LibraryDatabase::open_with(&path, &handle, &mut |stage| {
            stages.push(stage);
            // Cancel from inside the migration: SQLite's progress handler or
            // the check after the backup catches it before anything commits.
            cancel.cancel();
        })
        .err()
        .unwrap();
        assert!(matches!(error, LibraryError::OpenCancelled));
        assert_eq!(stages, [crate::OpenStage::Migrating]);
        assert_eq!(fingerprint_of(&path), super::DRAWING_PREDECESSOR);
        assert_eq!(
            count(
                &Connection::open(&path).unwrap(),
                "SELECT COUNT(*) FROM library_root"
            ),
            1
        );
        assert_eq!(backups_in(directory.path()), 0);
        drop(LibraryDatabase::open(&path).unwrap());
        assert_eq!(fingerprint_of(&path), SCHEMA_FINGERPRINT);
        assert_eq!(backups_in(directory.path()), 1);
    }
}
