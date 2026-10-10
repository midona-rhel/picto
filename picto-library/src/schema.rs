use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::query_request::{OpenCancellation, OpenStage};
use crate::{LibraryError, Result};

pub const SCHEMA_GENERATION: u32 = 1;
pub const SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-10-10-no-cloud-clocks";
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
const ACCELERATOR_INDEXES: &str = include_str!("accelerator_indexes.sql");

/// Create any missing accelerator index. Idempotent; runs at create and open.
pub fn ensure_accelerator_indexes(connection: &Connection) -> Result<()> {
    connection.execute_batch(ACCELERATOR_INDEXES)?;
    Ok(())
}

pub fn create(connection: &mut Connection) -> Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")?;
    let result = (|| {
        connection.execute_batch(SCHEMA)?;
        connection.execute_batch(include_str!("browsing_schema.sql"))?;
        connection.execute_batch(ACCELERATOR_INDEXES)?;
        connection.execute(
            "INSERT INTO library_meta
                 (singleton, schema_generation, schema_fingerprint, revision, next_local_id)
             VALUES (1, ?1, ?2, 1, 1)",
            rusqlite::params![SCHEMA_GENERATION, SCHEMA_FINGERPRINT],
        )?;
        connection.execute(
            "INSERT INTO cloud_state (singleton, library_id, device_id)
             VALUES (1, ?1, ?2)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                uuid::Uuid::new_v4().to_string()
            ],
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

/// Interrupts the migration's statements once its open is cancelled. Scoped
/// to the migration: the writer connection lives on as the library's writer
/// and must not carry the handler into ordinary writes.
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
            CHUNKED_ORDERS_PREDECESSOR
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
    let backup_path = migration_backup_path(database_path);
    let created_backup = !backup_path.exists();
    let result = {
        let _interrupt = MigrationInterrupt::install(connection, cancellation.flag())?;
        (|| {
            connection.execute("VACUUM INTO ?1", [backup_path.to_string_lossy().as_ref()])?;
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
            // Every predecessor persisted some browsing state: drawing pages,
            // text chunks or alphabetical name nodes, each with its own journal
            // and repair lifecycle. The resident model rebuilds all of it from
            // canonical rows at open, so only the dirty-root journal and its
            // triggers remain, redefined here once for every predecessor.
            if ![
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
            // These predecessors change only derived storage. Their canonical
            // data and existing checkpoint representation are unchanged.
            if [
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
            // Older attempts do not prove grid-root creation; never infer it
            // from retained files or backfill speculative notification counts.
            if !has_column(connection, "source_attempt_root", "created_root")? {
                connection.execute_batch(
                    "ALTER TABLE source_attempt_root ADD COLUMN created_root INTEGER NOT NULL DEFAULT 0 CHECK (created_root IN (0, 1));"
                )?;
            }
            // Watched folders kept importing into the Inbox; that stays the default.
            if !has_column(connection, "folder_definition", "watch_lifecycle")? {
                connection.execute_batch(
                    "ALTER TABLE folder_definition ADD COLUMN watch_lifecycle INTEGER NOT NULL DEFAULT 2 CHECK (watch_lifecycle IN (1, 2));"
                )?;
            }
            require_columns(connection, "source_attempt_root", &["created_root"])?;
            // Orders move from one blob per owner to fixed-size chunks; the
            // ids and their order are unchanged, so checkpoints stay valid.
            if !has_table(connection, "ordered_membership_chunk")? {
                crate::ordering::migrate_single_blob_orders(connection)?;
            }
            // The per-field CRDT design was replaced by snapshot sync; nothing
            // ever wrote these tables.
            connection.execute_batch(
                "DROP INDEX IF EXISTS idx_cloud_outbox_pending;
                 DROP TABLE IF EXISTS cloud_outbox;
                 DROP TABLE IF EXISTS cloud_applied_mutation;
                 DROP TABLE IF EXISTS cloud_field_clock;
                 DROP TABLE IF EXISTS cloud_membership_clock;",
            )?;
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

fn migration_backup_path(database_path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("library.sqlite");
    database_path.with_file_name(format!("{file_name}.pre-migration-{timestamp}.backup"))
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

    /// The persisted browsing state every predecessor left behind, with the
    /// shapes the migration must drop; contents never matter to it.
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
        // A stale journal row and a checkpoint stamped with the predecessor.
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
        // The redefined triggers journal canonical changes exactly as before.
        connection
            .execute("UPDATE library_root SET name='犬' WHERE root_id=7", [])
            .unwrap();
        assert_eq!(
            count(&connection, "SELECT COUNT(*) FROM browsing_dirty_root"),
            1
        );
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

    #[test]
    fn open_rejects_an_unknown_schema_without_mutating_it() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&database_path).unwrap());
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute(
                "UPDATE library_meta SET schema_fingerprint = 'unknown' WHERE singleton = 1",
                [],
            )
            .unwrap();
        drop(connection);

        let error = match LibraryDatabase::open(&database_path) {
            Ok(_) => panic!("unknown schema unexpectedly opened"),
            Err(error) => error,
        };
        assert!(matches!(error, LibraryError::Incompatible(_)));
        let connection = Connection::open(&database_path).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "unknown"
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
