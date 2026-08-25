use std::io::Cursor;

use rusqlite::session::{invert_strm, ConflictAction, ConflictType, Session};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::{schema, Store};

const HISTORY_LIMIT: i64 = 100;

// Only canonical user-owned state belongs in application history. Runtime,
// subscription, credential, queue, revision, FTS, and history tables are
// deliberately absent.
const UNDOABLE_TABLES: &[&str] = &[
    "media_file",
    "library_item",
    "library_root",
    "media_asset",
    "collection_member",
    "media_view",
    "tag",
    "media_tag",
    "tag_alias",
    "tag_implication",
    "folder",
    "folder_item",
    "smart_folder",
    "duplicate",
    "file_color",
    "view_pref",
    "setting",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryDescriptor {
    pub command: String,
    pub label: String,
    pub resources: Vec<String>,
    pub item_ids: Vec<i64>,
    pub reload_projections: bool,
}

impl HistoryDescriptor {
    pub fn new(
        command: impl Into<String>,
        label: impl Into<String>,
        resources: Vec<String>,
        item_ids: Vec<i64>,
    ) -> Self {
        Self {
            command: command.into(),
            label: label.into(),
            resources,
            item_ids,
            reload_projections: false,
        }
    }

    pub fn rebuilding_projections(mut self) -> Self {
        self.reload_projections = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntrySummary {
    pub entry_id: i64,
    pub command: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryState {
    pub undo: Option<HistoryEntrySummary>,
    pub redo: Option<HistoryEntrySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMutation {
    pub entry: HistoryEntrySummary,
    pub resources: Vec<String>,
    pub item_ids: Vec<i64>,
    pub revision: u64,
    pub state: HistoryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDirection {
    Undo,
    Redo,
}

struct StoredEntry {
    summary: HistoryEntrySummary,
    changeset: Vec<u8>,
    resources: Vec<String>,
    item_ids: Vec<i64>,
    reload_projections: bool,
}

impl Store {
    pub fn undoable_transaction_settled<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D)>,
        settle: impl FnOnce(&Connection, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>), String> {
        let (value, revision, history, _) = self.undoable_transaction_inner(
            descriptor,
            |transaction| operation(transaction).map(|(value, delta)| (value, delta, true)),
            settle,
        )?;
        Ok((value, revision, history))
    }

    pub fn undoable_transaction_if_changed_settled<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&Connection, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        self.undoable_transaction_inner(descriptor, operation, settle)
    }

    fn undoable_transaction_inner<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&Connection, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
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
        crate::cloud::capture::begin_explicit_capture(&transaction)
            .map_err(|error| error.to_string())?;

        let mut session = Session::new(&transaction).map_err(|error| error.to_string())?;
        for table in UNDOABLE_TABLES {
            session
                .attach(Some(table))
                .map_err(|error| error.to_string())?;
        }

        let (value, delta, changed) = operation(&transaction).map_err(|error| error.to_string())?;
        let revision = if changed {
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };

        let changeset = if !changed || session.is_empty() {
            None
        } else {
            let mut changeset = Vec::new();
            session
                .changeset_strm(&mut changeset)
                .map_err(|error| error.to_string())?;
            Some(changeset)
        };
        drop(session);

        if changed {
            crate::cloud::capture::finish_explicit_capture(&transaction, changeset.as_deref())
                .map_err(|error| error.to_string())?;
        }

        let history = if let Some(changeset) = changeset {
            transaction
                .execute("DELETE FROM history_entry WHERE applied = 0", [])
                .map_err(|error| error.to_string())?;
            let resources_json =
                serde_json::to_string(&descriptor.resources).map_err(|error| error.to_string())?;
            let item_ids_json =
                serde_json::to_string(&descriptor.item_ids).map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT INTO history_entry
                         (command, label, forward_changeset, resources_json, item_ids_json,
                          reload_projections, applied, byte_size, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)",
                    params![
                        descriptor.command,
                        descriptor.label,
                        changeset,
                        resources_json,
                        item_ids_json,
                        i64::from(descriptor.reload_projections),
                        changeset.len() as i64,
                        chrono::Utc::now().to_rfc3339(),
                    ],
                )
                .map_err(|error| error.to_string())?;
            let entry_id = transaction.last_insert_rowid();
            transaction
                .execute(
                    "DELETE FROM history_entry
                     WHERE entry_id IN (
                         SELECT entry_id FROM history_entry
                         ORDER BY entry_id DESC LIMIT -1 OFFSET ?1
                     )",
                    [HISTORY_LIMIT],
                )
                .map_err(|error| error.to_string())?;
            Some(HistoryEntrySummary {
                entry_id,
                command: descriptor.command,
                label: descriptor.label,
            })
        } else {
            None
        };

        transaction.commit().map_err(|error| error.to_string())?;
        if changed {
            settle(&connection, delta)?;
        }
        Ok((value, revision, history, changed))
    }

    pub fn history_state(&self) -> Result<HistoryState, String> {
        let _guard = self
            .consistency
            .read()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        query_state(&connection).map_err(|error| error.to_string())
    }

    pub fn apply_history(
        &self,
        direction: HistoryDirection,
        settle: impl FnOnce(&Connection, bool) -> Result<(), String>,
    ) -> Result<HistoryMutation, String> {
        let _guard = self
            .consistency
            .write()
            .map_err(|_| "Store consistency lock poisoned".to_string())?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        // Changesets contain both parent and child rows. Foreign-key actions
        // must not cascade midway through applying the same changeset (for
        // example deleting a newly-created tag before its media_tag row).
        // SQLite's session extension expects FK enforcement to be disabled
        // while applying, followed by an explicit integrity check.
        let foreign_keys_enabled: bool = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if foreign_keys_enabled {
            connection
                .pragma_update(None, "foreign_keys", false)
                .map_err(|error| error.to_string())?;
        }
        let applied = (|| -> Result<(StoredEntry, u64), String> {
            let transaction = connection
                .transaction()
                .map_err(|error| error.to_string())?;
            let cloud_capture = crate::cloud::capture::SemanticCapture::start(&transaction)
                .map_err(|error| error.to_string())?;
            let entry = load_entry(&transaction, direction)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| match direction {
                    HistoryDirection::Undo => "Nothing to undo".to_string(),
                    HistoryDirection::Redo => "Nothing to redo".to_string(),
                })?;

            let mut input = Cursor::new(&entry.changeset);
            let apply_result = match direction {
                HistoryDirection::Undo => {
                    let mut inverse = Vec::new();
                    invert_strm(&mut input, &mut inverse).map_err(|error| error.to_string())?;
                    transaction.apply_strm(
                        &mut Cursor::new(inverse),
                        None::<fn(&str) -> bool>,
                        history_conflict_action,
                    )
                }
                HistoryDirection::Redo => transaction.apply_strm(
                    &mut input,
                    None::<fn(&str) -> bool>,
                    history_conflict_action,
                ),
            };
            apply_result
                .map_err(|error| format!("History conflicts with newer library data: {error}"))?;
            if transaction
                .prepare("PRAGMA foreign_key_check")
                .and_then(|mut statement| statement.exists([]))
                .map_err(|error| error.to_string())?
            {
                return Err("History would violate library relationships".to_string());
            }

            transaction
                .execute(
                    "UPDATE history_entry SET applied = ?1 WHERE entry_id = ?2",
                    params![
                        i64::from(matches!(direction, HistoryDirection::Redo)),
                        entry.summary.entry_id
                    ],
                )
                .map_err(|error| error.to_string())?;
            cloud_capture
                .finish(&transaction)
                .map_err(|error| error.to_string())?;
            let revision =
                schema::increment_revision(&transaction).map_err(|error| error.to_string())?;
            transaction.commit().map_err(|error| error.to_string())?;
            Ok((entry, revision))
        })();
        let restore_foreign_keys = if foreign_keys_enabled {
            connection
                .pragma_update(None, "foreign_keys", true)
                .map_err(|error| error.to_string())
        } else {
            Ok(())
        };
        let (entry, revision) = applied?;
        restore_foreign_keys?;
        settle(&connection, entry.reload_projections)?;
        let state = query_state(&connection).map_err(|error| error.to_string())?;

        Ok(HistoryMutation {
            entry: entry.summary,
            resources: entry.resources,
            item_ids: entry.item_ids,
            revision,
            state,
        })
    }
}

fn history_conflict_action(
    _conflict: ConflictType,
    _: rusqlite::session::ChangesetItem,
) -> ConflictAction {
    ConflictAction::SQLITE_CHANGESET_ABORT
}

fn load_entry(
    connection: &Connection,
    direction: HistoryDirection,
) -> rusqlite::Result<Option<StoredEntry>> {
    let (applied, order) = match direction {
        HistoryDirection::Undo => (1, "DESC"),
        HistoryDirection::Redo => (0, "ASC"),
    };
    let sql = format!(
        "SELECT entry_id, command, label, forward_changeset, resources_json, item_ids_json,
                reload_projections
         FROM history_entry WHERE applied = ?1 ORDER BY entry_id {order} LIMIT 1"
    );
    connection
        .query_row(&sql, [applied], |row| {
            let resources_json: String = row.get(4)?;
            let item_ids_json: String = row.get(5)?;
            Ok(StoredEntry {
                summary: HistoryEntrySummary {
                    entry_id: row.get(0)?,
                    command: row.get(1)?,
                    label: row.get(2)?,
                },
                changeset: row.get(3)?,
                resources: serde_json::from_str(&resources_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        resources_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                item_ids: serde_json::from_str(&item_ids_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        item_ids_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                reload_projections: row.get::<_, i64>(6)? != 0,
            })
        })
        .optional()
}

fn query_state(connection: &Connection) -> rusqlite::Result<HistoryState> {
    Ok(HistoryState {
        undo: query_summary(connection, 1, "DESC")?,
        redo: query_summary(connection, 0, "ASC")?,
    })
}

fn query_summary(
    connection: &Connection,
    applied: i64,
    order: &str,
) -> rusqlite::Result<Option<HistoryEntrySummary>> {
    let sql = format!(
        "SELECT entry_id, command, label FROM history_entry
         WHERE applied = ?1 ORDER BY entry_id {order} LIMIT 1"
    );
    connection
        .query_row(&sql, [applied], |row| {
            Ok(HistoryEntrySummary {
                entry_id: row.get(0)?,
                command: row.get(1)?,
                label: row.get(2)?,
            })
        })
        .optional()
}

#[cfg(test)]
mod tests {
    use super::{HistoryDescriptor, HistoryDirection};
    use crate::store::Store;

    fn rename_descriptor(item_id: i64) -> HistoryDescriptor {
        HistoryDescriptor::new(
            "items.rename",
            "Rename item",
            vec!["library".to_string(), format!("item:{item_id}")],
            vec![item_id],
        )
    }

    fn media_fixture(store: &Store) -> i64 {
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES ('history-hash', 'image/png', 10, 'now')",
                    [],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES ('history-item', 'media', 'now', 'now')",
                    [],
                )?;
                let item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, name, imported_at, updated_at)
                     VALUES (?1, ?2, 'Before', 'now', 'now')",
                    rusqlite::params![item_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                    [item_id],
                )?;
                Ok(item_id)
            })
            .unwrap()
            .0
    }

    fn name(store: &Store, item_id: i64) -> String {
        store
            .read(|connection| {
                connection.query_row(
                    "SELECT name FROM media_asset WHERE item_id = ?1",
                    [item_id],
                    |row| row.get(0),
                )
            })
            .unwrap()
    }

    #[test]
    fn captures_undoes_and_redoes_one_transaction() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);

        let (_, _, entry) = store
            .undoable_transaction_settled(
                rename_descriptor(item_id),
                |transaction| {
                    transaction.execute(
                        "UPDATE media_asset SET name = 'After' WHERE item_id = ?1",
                        [item_id],
                    )?;
                    Ok(((), ()))
                },
                |_, ()| Ok(()),
            )
            .unwrap();
        assert_eq!(entry.unwrap().label, "Rename item");
        assert_eq!(name(&store, item_id), "After");
        assert_eq!(
            store.history_state().unwrap().undo.unwrap().label,
            "Rename item"
        );

        let undone = store
            .apply_history(HistoryDirection::Undo, |_, _| Ok(()))
            .unwrap();
        assert_eq!(undone.item_ids, vec![item_id]);
        assert_eq!(name(&store, item_id), "Before");
        assert!(undone.state.undo.is_none());
        assert_eq!(undone.state.redo.unwrap().label, "Rename item");

        let redone = store
            .apply_history(HistoryDirection::Redo, |_, _| Ok(()))
            .unwrap();
        assert_eq!(name(&store, item_id), "After");
        assert!(redone.state.redo.is_none());
    }

    #[test]
    fn aborts_conflicting_undo_without_moving_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);
        store
            .undoable_transaction_settled(
                rename_descriptor(item_id),
                |transaction| {
                    transaction.execute(
                        "UPDATE media_asset SET name = 'After' WHERE item_id = ?1",
                        [item_id],
                    )?;
                    Ok(((), ()))
                },
                |_, ()| Ok(()),
            )
            .unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET name = 'Background' WHERE item_id = ?1",
                    [item_id],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(store
            .apply_history(HistoryDirection::Undo, |_, _| Ok(()))
            .is_err());
        assert_eq!(name(&store, item_id), "Background");
        assert!(store.history_state().unwrap().undo.is_some());
        assert!(store.history_state().unwrap().redo.is_none());
    }

    #[test]
    fn restores_cascaded_folder_children() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let (parent_id, child_id) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, created_at, updated_at)
                     VALUES ('parent', 'Parent', 'now', 'now')",
                    [],
                )?;
                let parent_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, parent_id, created_at, updated_at)
                     VALUES ('child', 'Child', ?1, 'now', 'now')",
                    [parent_id],
                )?;
                Ok((parent_id, transaction.last_insert_rowid()))
            })
            .unwrap()
            .0;

        store
            .undoable_transaction_settled(
                HistoryDescriptor::new(
                    "folders.delete",
                    "Delete folder",
                    vec!["folders".to_string()],
                    vec![],
                ),
                |transaction| {
                    transaction.execute("DELETE FROM folder WHERE folder_id = ?1", [parent_id])?;
                    Ok(((), ()))
                },
                |_, ()| Ok(()),
            )
            .unwrap();
        let remaining = store
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM folder", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .unwrap();
        assert_eq!(remaining, 0);

        store
            .apply_history(HistoryDirection::Undo, |_, _| Ok(()))
            .unwrap();
        let restored = store
            .read(|connection| {
                connection.query_row(
                    "SELECT parent_id FROM folder WHERE folder_id = ?1",
                    [child_id],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(restored, parent_id);
    }

    #[test]
    fn no_op_writes_do_not_create_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);

        let (_, _, entry, changed) = store
            .undoable_transaction_if_changed_settled(
                rename_descriptor(item_id),
                |_| Ok(((), (), false)),
                |_, ()| Ok(()),
            )
            .unwrap();

        assert!(!changed);
        assert!(entry.is_none());
        assert_eq!(
            store.history_state().unwrap(),
            super::HistoryState::default()
        );
    }

    #[test]
    fn a_new_write_discards_the_redo_branch() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);

        for value in ["First", "Second"] {
            store
                .undoable_transaction_settled(
                    rename_descriptor(item_id),
                    |transaction| {
                        transaction.execute(
                            "UPDATE media_asset SET name = ?1 WHERE item_id = ?2",
                            rusqlite::params![value, item_id],
                        )?;
                        Ok(((), ()))
                    },
                    |_, ()| Ok(()),
                )
                .unwrap();
        }
        store
            .apply_history(HistoryDirection::Undo, |_, _| Ok(()))
            .unwrap();
        assert!(store.history_state().unwrap().redo.is_some());

        store
            .undoable_transaction_settled(
                rename_descriptor(item_id),
                |transaction| {
                    transaction.execute(
                        "UPDATE media_asset SET name = 'Third' WHERE item_id = ?1",
                        [item_id],
                    )?;
                    Ok(((), ()))
                },
                |_, ()| Ok(()),
            )
            .unwrap();

        assert_eq!(name(&store, item_id), "Third");
        assert!(store.history_state().unwrap().redo.is_none());
    }
}
