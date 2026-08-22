//! SQLite ownership for the replacement backend.
//!
//! The store exposes direct read and transaction boundaries. Domain behavior
//! stays in application modules rather than growing another database facade.

pub mod schema;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use rusqlite::{Connection, OpenFlags, Transaction};

pub const DATABASE_FILE: &str = "library.sqlite";

pub struct Store {
    path: PathBuf,
    writer: Mutex<Connection>,
    consistency: RwLock<()>,
}

impl Store {
    pub fn open(library_root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(library_root)
            .map_err(|error| format!("Failed to create library directory: {error}"))?;
        let path = library_root.join(DATABASE_FILE);
        let existed = path.exists();
        let mut writer = open_connection(&path, false)?;

        if existed {
            schema::validate(&writer)?;
        } else {
            schema::create(&mut writer)?;
        }

        Ok(Self {
            path,
            writer: Mutex::new(writer),
            consistency: RwLock::new(()),
        })
    }

    pub fn read<T>(
        &self,
        operation: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        let _guard = self
            .consistency
            .read()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let connection = open_connection(&self.path, true)?;
        operation(&connection).map_err(|error| error.to_string())
    }

    pub fn read_result<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .consistency
            .read()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let connection = open_connection(&self.path, true)?;
        operation(&connection)
    }

    pub fn transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<(T, u64), String> {
        let _guard = self
            .consistency
            .write()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let value = operation(&transaction).map_err(|error| error.to_string())?;
        let revision =
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok((value, revision))
    }

    pub fn transaction_if_changed<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, bool)>,
    ) -> Result<(T, u64, bool), String> {
        let _guard = self
            .consistency
            .write()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let (value, changed) = operation(&transaction).map_err(|error| error.to_string())?;
        let revision = if changed {
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };
        transaction.commit().map_err(|error| error.to_string())?;
        Ok((value, revision, changed))
    }

    /// Commit SQLite and settle derived state before any Store reader can
    /// observe the new revision. `settle` must recover its projection from
    /// `connection` if an incremental update fails.
    pub fn transaction_settled<T, D>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D)>,
        settle: impl FnOnce(&Connection, D) -> Result<(), String>,
    ) -> Result<(T, u64), String> {
        let _guard = self
            .consistency
            .write()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let (value, delta) = operation(&transaction).map_err(|error| error.to_string())?;
        let revision =
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        settle(&connection, delta)?;
        Ok((value, revision))
    }

    pub fn transaction_if_changed_settled<T, D>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&Connection, D) -> Result<(), String>,
    ) -> Result<(T, u64, bool), String> {
        let _guard = self
            .consistency
            .write()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let (value, delta, changed) = operation(&transaction).map_err(|error| error.to_string())?;
        let revision = if changed {
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };
        transaction.commit().map_err(|error| error.to_string())?;
        if changed {
            settle(&connection, delta)?;
        }
        Ok((value, revision, changed))
    }

    pub fn revision(&self) -> Result<u64, String> {
        self.read(schema::revision)
    }
}

fn open_connection(path: &Path, read_only: bool) -> Result<Connection, String> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|error| format!("Failed to configure SQLite: {error}"))?;
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn transaction_commits_one_revision() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        assert_eq!(store.revision().unwrap(), 0);

        let (_, revision) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES ('item-a', 'media', 'now', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(revision, 1);
        assert_eq!(store.revision().unwrap(), 1);
    }
}
