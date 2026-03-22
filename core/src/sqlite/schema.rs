//! DDL and migration runner for the library SQLite database.

use rusqlite::Connection;

#[path = "schema/ddl.rs"]
mod ddl;
#[path = "schema/migrations.rs"]
mod migrations;
#[path = "schema/reconcile.rs"]
mod reconcile;
#[path = "schema/support.rs"]
mod support;
#[cfg(test)]
#[path = "schema/tests.rs"]
mod tests;

use support::{seed_artifact_manifest, seed_manifest};

pub const CURRENT_VERSION: i64 = 36;

pub fn apply_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(ddl::PRAGMA_SQL)
}

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(ddl::LIBRARY_DDL)?;
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

pub use migrations::run_migrations;
pub use reconcile::reconcile_schema;
