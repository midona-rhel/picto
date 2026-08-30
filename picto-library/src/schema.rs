use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::{LibraryError, Result};

pub const SCHEMA_GENERATION: u32 = 1;
pub const SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-08-30-native-sources";

const PREVIOUS_SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-08-29-fts-substring";

const SCHEMA: &str = include_str!("schema_v1.sql");

pub fn create(connection: &mut Connection) -> Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")?;
    let result = (|| {
        connection.execute_batch(SCHEMA)?;
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

pub fn migrate_for_open(connection: &mut Connection, database_path: &Path) -> Result<()> {
    let (generation, fingerprint, _) = metadata(connection)?;
    if generation == SCHEMA_GENERATION && fingerprint == SCHEMA_FINGERPRINT {
        return Ok(());
    }
    if generation != SCHEMA_GENERATION || fingerprint != PREVIOUS_SCHEMA_FINGERPRINT {
        return Err(LibraryError::Incompatible(format!(
            "no migration to generation {SCHEMA_GENERATION} ({SCHEMA_FINGERPRINT}) from generation {generation} ({fingerprint})"
        )));
    }

    let backup_path = migration_backup_path(database_path);
    connection.execute("VACUUM INTO ?1", [backup_path.to_string_lossy().as_ref()])?;

    connection.execute_batch("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE;")?;
    let result = (|| {
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

    use super::{PREVIOUS_SCHEMA_FINGERPRINT, SCHEMA_FINGERPRINT};
    use crate::{LibraryDatabase, LibraryError};

    #[test]
    fn open_migrates_the_previous_alpha_schema_and_keeps_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        drop(LibraryDatabase::create(&database_path).unwrap());

        let connection = Connection::open(&database_path).unwrap();
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
                [PREVIOUS_SCHEMA_FINGERPRINT],
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
            PREVIOUS_SCHEMA_FINGERPRINT
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
}
