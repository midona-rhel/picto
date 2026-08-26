//! Persisted application and per-view preferences.

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};
use crate::store::history::HistoryDescriptor;

const APPLICATION_SETTINGS_KEY: &str = "application";
const GRID_DEFAULTS_SCOPE: &str = "grid:defaults";
pub const DEFAULT_SUBSCRIPTION_INBOX_ITEM_LIMIT: u64 = 1_000;
const MAX_SUBSCRIPTION_INBOX_ITEM_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SettingsSnapshot {
    #[ts(type = "unknown")]
    pub value: serde_json::Value,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn application_settings(application: &Application) -> Result<SettingsSnapshot, String> {
    read_value(application, "setting", "key", APPLICATION_SETTINGS_KEY)
}

pub fn subscription_inbox_item_limit(application: &Application) -> Result<u64, String> {
    let settings = application_settings(application)?;
    let Some(value) = settings.value.get("subscriptionInboxItemLimit") else {
        return Ok(DEFAULT_SUBSCRIPTION_INBOX_ITEM_LIMIT);
    };
    let limit = value.as_u64().ok_or_else(|| {
        "Application setting subscriptionInboxItemLimit must be a positive integer".to_string()
    })?;
    if !(1..=MAX_SUBSCRIPTION_INBOX_ITEM_LIMIT).contains(&limit) {
        return Err(format!(
            "Application setting subscriptionInboxItemLimit must be between 1 and {MAX_SUBSCRIPTION_INBOX_ITEM_LIMIT}"
        ));
    }
    Ok(limit)
}

pub fn subscription_inbox_is_full(application: &Application) -> Result<bool, String> {
    let limit = subscription_inbox_item_limit(application)?;
    application.store().read(|connection| {
        let count: u64 = connection.query_row(
            "SELECT COUNT(*) FROM (
                 SELECT 1 FROM library_root
                 WHERE lifecycle = 'inbox'
                 LIMIT ?1
             )",
            [limit],
            |row| row.get(0),
        )?;
        Ok(count >= limit)
    })
}

pub fn view_preferences(
    application: &Application,
    scope: &str,
) -> Result<SettingsSnapshot, String> {
    let scope = required("View preference scope", scope)?;
    read_value(application, "view_pref", "scope", &scope)
}

impl Application {
    pub fn replace_application_settings(
        &self,
        value: &serde_json::Value,
    ) -> Result<MutationReceipt, String> {
        require_object("Application settings", value)?;
        replace_value(
            self,
            "settings.replace",
            "Replace settings",
            "setting",
            "key",
            APPLICATION_SETTINGS_KEY,
            value,
        )
    }

    pub fn patch_application_settings(
        &self,
        patch: &serde_json::Value,
    ) -> Result<MutationReceipt, String> {
        patch_value(
            self,
            "settings.patch",
            "Change settings",
            "setting",
            "key",
            APPLICATION_SETTINGS_KEY,
            patch,
        )
    }

    pub fn patch_view_preferences(
        &self,
        scope: &str,
        patch: &serde_json::Value,
    ) -> Result<MutationReceipt, String> {
        let scope = required("View preference scope", scope)?;
        patch_untracked_value(self, "view_pref", "scope", &scope, patch)
    }

    pub fn reset_view_preferences(&self) -> Result<MutationReceipt, String> {
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            let removed = transaction.execute(
                "DELETE FROM view_pref WHERE scope <> ?1",
                [GRID_DEFAULTS_SCOPE],
            )?;
            Ok(((), removed > 0))
        })?;
        Ok(settings_receipt(revision))
    }
}

fn read_value(
    application: &Application,
    table: &str,
    key_column: &str,
    key: &str,
) -> Result<SettingsSnapshot, String> {
    application.store().read(|connection| {
        let sql = format!("SELECT value_json FROM {table} WHERE {key_column} = ?1");
        let value_json = connection
            .query_row(&sql, [key], |row| row.get::<_, String>(0))
            .optional()?;
        let value = value_json
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        Ok(SettingsSnapshot {
            value,
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

fn replace_value(
    application: &Application,
    command: &str,
    label: &str,
    table: &str,
    key_column: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<MutationReceipt, String> {
    let encoded = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let (_, revision, _, _) = application.undoable_transaction_if_changed(
        settings_history(command, label),
        |transaction| {
            let sql = format!("SELECT value_json FROM {table} WHERE {key_column} = ?1");
            let previous = transaction
                .query_row(&sql, [key], |row| row.get::<_, String>(0))
                .optional()?;
            if previous.as_deref() == Some(encoded.as_str()) {
                return Ok(((), (), false));
            }
            let sql = format!(
                "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
                     ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
            );
            transaction.execute(&sql, params![key, encoded])?;
            Ok(((), (), true))
        },
        |_, ()| Ok(()),
    )?;
    Ok(settings_receipt(revision))
}

fn patch_value(
    application: &Application,
    command: &str,
    label: &str,
    table: &str,
    key_column: &str,
    key: &str,
    patch: &serde_json::Value,
) -> Result<MutationReceipt, String> {
    require_object("Settings patch", patch)?;
    let (_, revision, _, _) = application.undoable_transaction_if_changed(
        settings_history(command, label),
        |transaction| {
            Ok((
                (),
                (),
                patch_stored_value(transaction, table, key_column, key, patch)?,
            ))
        },
        |_, ()| Ok(()),
    )?;
    Ok(settings_receipt(revision))
}

fn patch_untracked_value(
    application: &Application,
    table: &str,
    key_column: &str,
    key: &str,
    patch: &serde_json::Value,
) -> Result<MutationReceipt, String> {
    require_object("Settings patch", patch)?;
    let (_, revision, _) = application.store().transaction_if_changed(|transaction| {
        Ok((
            (),
            patch_stored_value(transaction, table, key_column, key, patch)?,
        ))
    })?;
    Ok(settings_receipt(revision))
}

fn patch_stored_value(
    transaction: &Transaction<'_>,
    table: &str,
    key_column: &str,
    key: &str,
    patch: &serde_json::Value,
) -> rusqlite::Result<bool> {
    let select = format!("SELECT value_json FROM {table} WHERE {key_column} = ?1");
    let previous = transaction
        .query_row(&select, [key], |row| row.get::<_, String>(0))
        .optional()?
        .map(|value| {
            serde_json::from_str::<serde_json::Value>(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?
        .unwrap_or_else(|| serde_json::json!({}));
    if !previous.is_object() {
        return Err(invalid("Stored settings must be a JSON object"));
    }
    let mut value = previous.clone();
    merge_object(&mut value, patch);
    if value == previous {
        return Ok(false);
    }
    let encoded = serde_json::to_string(&value)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let upsert = format!(
        "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
             ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
    );
    transaction.execute(&upsert, params![key, encoded])?;
    Ok(true)
}

fn settings_history(command: &str, label: &str) -> HistoryDescriptor {
    HistoryDescriptor::new(
        command,
        label,
        vec![resources::SETTINGS.to_string()],
        Vec::new(),
    )
}

fn merge_object(target: &mut serde_json::Value, patch: &serde_json::Value) {
    let target = target
        .as_object_mut()
        .expect("stored settings must be a JSON object");
    for (key, value) in patch.as_object().expect("validated patch") {
        if value.is_null() {
            target.remove(key);
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn require_object(label: &str, value: &serde_json::Value) -> Result<(), String> {
    if value.is_object() {
        Ok(())
    } else {
        Err(format!("{label} must be a JSON object"))
    }
}

fn required(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    Ok(value.to_string())
}

fn settings_receipt(revision: u64) -> MutationReceipt {
    MutationReceipt {
        revision,
        resources: vec![resources::SETTINGS.to_string()],
        item_ids: Vec::new(),
    }
}

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;

    fn fixture() -> (tempfile::TempDir, Application) {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        (directory, application)
    }

    #[test]
    fn settings_are_persisted_and_no_op_writes_keep_the_revision() {
        let (_directory, application) = fixture();
        let first = application
            .patch_application_settings(&serde_json::json!({"zoom": 1.25, "theme": "dark"}))
            .unwrap();
        assert_eq!(first.revision, 1);
        let no_op = application
            .patch_application_settings(&serde_json::json!({"zoom": 1.25}))
            .unwrap();
        assert_eq!(no_op.revision, 1);
        assert_eq!(
            application_settings(&application).unwrap().value,
            serde_json::json!({"zoom": 1.25, "theme": "dark"})
        );

        application.undo().unwrap();
        assert_eq!(
            application_settings(&application).unwrap().value,
            serde_json::json!({})
        );
    }

    #[test]
    fn null_removes_a_preference_without_affecting_other_scopes() {
        let (_directory, application) = fixture();
        application
            .patch_view_preferences(
                "system:all",
                &serde_json::json!({"size": 180, "sort": "name"}),
            )
            .unwrap();
        application
            .patch_view_preferences("system:all", &serde_json::json!({"sort": null}))
            .unwrap();

        assert_eq!(
            view_preferences(&application, "system:all").unwrap().value,
            serde_json::json!({"size": 180})
        );
        assert_eq!(
            view_preferences(&application, "system:inbox")
                .unwrap()
                .value,
            serde_json::json!({})
        );

        assert!(application.history_state().unwrap().undo.is_none());
    }

    #[test]
    fn subscription_inbox_limit_defaults_and_counts_top_level_items() {
        let (_directory, application) = fixture();
        assert_eq!(
            subscription_inbox_item_limit(&application).unwrap(),
            DEFAULT_SUBSCRIPTION_INBOX_ITEM_LIMIT
        );
        assert!(!subscription_inbox_is_full(&application).unwrap());

        application
            .patch_application_settings(&serde_json::json!({
                "subscriptionInboxItemLimit": 1
            }))
            .unwrap();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (1, 'inbox-item', 'collection', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (1, 'inbox')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(subscription_inbox_is_full(&application).unwrap());
    }

    #[test]
    fn view_preferences_do_not_replace_real_undo_history() {
        let (_directory, application) = fixture();
        application
            .patch_application_settings(&serde_json::json!({"theme": "dark"}))
            .unwrap();
        application
            .patch_view_preferences("system:all", &serde_json::json!({"size": 180}))
            .unwrap();

        application.undo().unwrap();
        assert_eq!(
            application_settings(&application).unwrap().value,
            serde_json::json!({})
        );
        assert_eq!(
            view_preferences(&application, "system:all").unwrap().value,
            serde_json::json!({"size": 180})
        );
    }

    #[test]
    fn resetting_view_preferences_preserves_only_grid_defaults() {
        let (_directory, application) = fixture();
        application
            .patch_view_preferences(
                GRID_DEFAULTS_SCOPE,
                &serde_json::json!({"view_mode": "grid"}),
            )
            .unwrap();
        application
            .patch_view_preferences("system:active", &serde_json::json!({"show_name": false}))
            .unwrap();
        application
            .patch_view_preferences("folder:9", &serde_json::json!({"sort_field": "name"}))
            .unwrap();

        application.reset_view_preferences().unwrap();

        assert_eq!(
            view_preferences(&application, GRID_DEFAULTS_SCOPE)
                .unwrap()
                .value,
            serde_json::json!({"view_mode": "grid"})
        );
        assert_eq!(
            view_preferences(&application, "system:active")
                .unwrap()
                .value,
            serde_json::json!({})
        );
        assert_eq!(
            view_preferences(&application, "folder:9").unwrap().value,
            serde_json::json!({})
        );
    }
}
