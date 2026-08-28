use rusqlite::Connection;

use crate::{LibraryError, Result};

pub const SCHEMA_GENERATION: u32 = 1;
pub const SCHEMA_FINGERPRINT: &str = "picto-library-schema-1-2026-08-28-canonical-ingest";

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
    let (generation, fingerprint, revision) =
        row.map_err(|_| LibraryError::Incompatible("missing schema-generation-1 metadata".into()))?;
    if generation != SCHEMA_GENERATION || fingerprint != SCHEMA_FINGERPRINT {
        return Err(LibraryError::Incompatible(format!(
            "expected generation {SCHEMA_GENERATION} ({SCHEMA_FINGERPRINT}), found {generation} ({fingerprint})"
        )));
    }
    Ok(revision)
}
