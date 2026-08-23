//! Persisted application and per-view preferences.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};

const APPLICATION_SETTINGS_KEY: &str = "application";

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
        replace_value(self, "setting", "key", APPLICATION_SETTINGS_KEY, value)
    }

    pub fn patch_application_settings(
        &self,
        patch: &serde_json::Value,
    ) -> Result<MutationReceipt, String> {
        patch_value(self, "setting", "key", APPLICATION_SETTINGS_KEY, patch)
    }

    pub fn patch_view_preferences(
        &self,
        scope: &str,
        patch: &serde_json::Value,
    ) -> Result<MutationReceipt, String> {
        let scope = required("View preference scope", scope)?;
        patch_value(self, "view_pref", "scope", &scope, patch)
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
    table: &str,
    key_column: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<MutationReceipt, String> {
    let encoded = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let (_, revision, _) = application.store().transaction_if_changed(|transaction| {
        let sql = format!("SELECT value_json FROM {table} WHERE {key_column} = ?1");
        let previous = transaction
            .query_row(&sql, [key], |row| row.get::<_, String>(0))
            .optional()?;
        if previous.as_deref() == Some(encoded.as_str()) {
            return Ok(((), false));
        }
        let sql = format!(
            "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
                 ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
        );
        transaction.execute(&sql, params![key, encoded])?;
        Ok(((), true))
    })?;
    Ok(settings_receipt(revision))
}

fn patch_value(
    application: &Application,
    table: &str,
    key_column: &str,
    key: &str,
    patch: &serde_json::Value,
) -> Result<MutationReceipt, String> {
    require_object("Settings patch", patch)?;
    let (_, revision, _) = application.store().transaction_if_changed(|transaction| {
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
            return Ok(((), false));
        }
        let encoded = serde_json::to_string(&value)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let upsert = format!(
            "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
                 ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
        );
        transaction.execute(&upsert, params![key, encoded])?;
        Ok(((), true))
    })?;
    Ok(settings_receipt(revision))
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
    }
}
