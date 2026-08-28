//! Direct-site account persistence for the replacement backend.
//!
//! SQLite stores only display metadata and health. Authentication secrets stay
//! in the operating-system credential store.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};
use crate::credential_store::{CredentialType, SiteCredential};
use crate::store::Store;
use crate::subscriptions::gallery_dl_runner::{site_by_id, SiteEntry, SITES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CredentialRecord {
    pub site_id: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CredentialHealthRecord {
    pub site_id: String,
    pub status: String,
    pub checked_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SetCredentialInput {
    pub site_id: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub cookies: Option<HashMap<String, String>>,
    pub headers: Option<HashMap<String, String>>,
    pub oauth_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SourceCatalogEntry {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub credential_owner_site_id: String,
    pub example_query: String,
    pub supports_query: bool,
    pub supports_account: bool,
    pub auth_required_for_full_access: bool,
    pub auth_strictly_required: bool,
    pub credential_types: Vec<String>,
    pub oauth_provider: Option<String>,
}

pub fn sources() -> Vec<SourceCatalogEntry> {
    SITES
        .iter()
        .map(|site| SourceCatalogEntry {
            id: site.id.to_string(),
            name: site.name.to_string(),
            domain: site.domain.to_string(),
            credential_owner_site_id: site.credential_owner_site_id.to_string(),
            example_query: site.example_query.to_string(),
            supports_query: site.supports_query,
            supports_account: site.supports_account,
            auth_required_for_full_access: site.auth_required_for_full_access,
            auth_strictly_required: site.auth_strictly_required,
            credential_types: site
                .credential_types
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            oauth_provider: site.oauth_provider.map(str::to_string),
        })
        .collect()
}

pub fn list_credentials(store: &Store) -> Result<Vec<CredentialRecord>, String> {
    store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT site_id, credential_type, display_name, created_at
             FROM credential ORDER BY site_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(CredentialRecord {
                    site_id: row.get(0)?,
                    credential_type: row.get(1)?,
                    display_name: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect();
        records
    })
}

pub fn list_library_credentials(
    application: &crate::library_application::LibraryApplication,
) -> Result<Vec<CredentialRecord>, String> {
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let mut statement = connection.prepare(
                    "SELECT site_id, credential_type, display_name, created_at
                     FROM credential ORDER BY site_id",
                )?;
                let records = statement
                    .query_map([], |row| {
                        Ok(CredentialRecord {
                            site_id: row.get(0)?,
                            credential_type: row.get(1)?,
                            display_name: row.get(2)?,
                            created_at: row.get(3)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(picto_library::LibraryError::from)?;
                Ok(records)
            },
        )
        .map_err(|error| error.to_string())
}

pub fn list_health(store: &Store) -> Result<Vec<CredentialHealthRecord>, String> {
    store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT site_id, status, checked_at, last_error
             FROM credential_health ORDER BY site_id",
        )?;
        let records = statement
            .query_map([], |row| {
                Ok(CredentialHealthRecord {
                    site_id: row.get(0)?,
                    status: row.get(1)?,
                    checked_at: row.get(2)?,
                    last_error: row.get(3)?,
                })
            })?
            .collect();
        records
    })
}

pub fn list_library_health(
    application: &crate::library_application::LibraryApplication,
) -> Result<Vec<CredentialHealthRecord>, String> {
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let mut statement = connection.prepare(
                    "SELECT site_id, status, checked_at, last_error
                     FROM credential_health ORDER BY site_id",
                )?;
                let records = statement
                    .query_map([], |row| {
                        Ok(CredentialHealthRecord {
                            site_id: row.get(0)?,
                            status: row.get(1)?,
                            checked_at: row.get(2)?,
                            last_error: row.get(3)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(picto_library::LibraryError::from)?;
                Ok(records)
            },
        )
        .map_err(|error| error.to_string())
}

pub fn set_library_credential(
    application: &crate::library_application::LibraryApplication,
    input: SetCredentialInput,
    now: &str,
) -> Result<picto_library::MutationReceipt, String> {
    let owner = credential_owner(&input.site_id)?;
    let credential_type = CredentialType::from_str(&input.credential_type)
        .ok_or_else(|| format!("Unsupported credential type: {}", input.credential_type))?;
    if !owner.credential_types.contains(&credential_type.as_str()) {
        return Err(format!(
            "{} does not accept {} credentials",
            owner.name,
            credential_type.as_str()
        ));
    }
    let credential = SiteCredential {
        site_category: owner.id.to_string(),
        credential_type,
        username: input.username,
        password: input.password,
        cookies: input.cookies,
        headers: input.headers,
        oauth_token: input.oauth_token,
    };
    validate_secret(&credential)?;
    crate::credential_store::set_credential(&credential)?;

    let site_id = owner.id.to_string();
    let credential_type = credential_type.as_str().to_string();
    let display_name = clean_display_name(input.display_name);
    let now = now.to_owned();
    let (_, receipt) = application
        .library()
        .auxiliary_write(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["subscriptions".to_string(), "settings".to_string()],
            [],
            move |transaction, _| {
                transaction.execute(
                    "INSERT INTO credential (site_id, credential_type, display_name, created_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(site_id) DO UPDATE SET
                         credential_type = excluded.credential_type,
                         display_name = excluded.display_name,
                         created_at = excluded.created_at",
                    params![site_id, credential_type, display_name, now],
                )?;
                transaction.execute(
                    "INSERT INTO credential_health (site_id, status, checked_at, last_error)
                     VALUES (?1, 'unknown', NULL, NULL)
                     ON CONFLICT(site_id) DO UPDATE SET
                         status = 'unknown', checked_at = NULL, last_error = NULL",
                    [&site_id],
                )?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(receipt)
}

pub fn delete_library_credential(
    application: &crate::library_application::LibraryApplication,
    site_id: &str,
) -> Result<picto_library::MutationReceipt, String> {
    let owner = credential_owner(site_id)?;
    crate::credential_store::delete_credential(owner.id)?;
    let owner_id = owner.id.to_owned();
    let (_, receipt) = application
        .library()
        .auxiliary_write(
            picto_library::database::WorkPriority::ForegroundMutation,
            ["subscriptions".to_string(), "settings".to_string()],
            [],
            move |transaction, _| {
                transaction.execute("DELETE FROM credential WHERE site_id = ?1", [owner_id])?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(receipt)
}

pub fn set_credential(
    application: &Application,
    input: SetCredentialInput,
    now: &str,
) -> Result<MutationReceipt, String> {
    let owner = credential_owner(&input.site_id)?;
    let credential_type = CredentialType::from_str(&input.credential_type)
        .ok_or_else(|| format!("Unsupported credential type: {}", input.credential_type))?;
    if !owner.credential_types.contains(&credential_type.as_str()) {
        return Err(format!(
            "{} does not accept {} credentials",
            owner.name,
            credential_type.as_str()
        ));
    }

    let credential = SiteCredential {
        site_category: owner.id.to_string(),
        credential_type,
        username: input.username,
        password: input.password,
        cookies: input.cookies,
        headers: input.headers,
        oauth_token: input.oauth_token,
    };
    validate_secret(&credential)?;
    crate::credential_store::set_credential(&credential)?;

    let site_id = owner.id.to_string();
    let credential_type = credential_type.as_str().to_string();
    let display_name = clean_display_name(input.display_name);
    let (_, revision) = application.store().transaction(|transaction| {
        transaction.execute(
            "INSERT INTO credential (site_id, credential_type, display_name, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(site_id) DO UPDATE SET
                 credential_type = excluded.credential_type,
                 display_name = excluded.display_name,
                 created_at = excluded.created_at",
            params![site_id, credential_type, display_name, now],
        )?;
        transaction.execute(
            "INSERT INTO credential_health (site_id, status, checked_at, last_error)
             VALUES (?1, 'unknown', NULL, NULL)
             ON CONFLICT(site_id) DO UPDATE SET
                 status = 'unknown', checked_at = NULL, last_error = NULL",
            [&site_id],
        )?;
        Ok(())
    })?;
    Ok(auth_receipt(revision))
}

pub fn delete_credential(
    application: &Application,
    site_id: &str,
) -> Result<MutationReceipt, String> {
    let owner = credential_owner(site_id)?;
    crate::credential_store::delete_credential(owner.id)?;
    let (_, revision, _) = application.store().transaction_if_changed(|transaction| {
        let changed =
            transaction.execute("DELETE FROM credential WHERE site_id = ?1", [owner.id])?;
        Ok(((), changed != 0))
    })?;
    Ok(auth_receipt(revision))
}

pub fn mark_run_success(store: &Store, site_id: &str, now: &str) -> Result<(), String> {
    set_existing_health(store, site_id, "valid", now, None)
}

pub fn mark_auth_failure(
    store: &Store,
    site_id: &str,
    now: &str,
    error: &str,
) -> Result<(), String> {
    set_existing_health(store, site_id, "unauthorized", now, Some(error))
}

fn set_existing_health(
    store: &Store,
    site_id: &str,
    status: &str,
    now: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let owner = credential_owner(site_id)?;
    store.transaction_if_changed(|transaction| {
        let exists = transaction
            .query_row(
                "SELECT 1 FROM credential WHERE site_id = ?1",
                [owner.id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(((), false));
        }
        let current = transaction
            .query_row(
                "SELECT status, checked_at, last_error FROM credential_health WHERE site_id = ?1",
                [owner.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let next = (
            status.to_string(),
            Some(now.to_string()),
            error.map(str::to_string),
        );
        if current.as_ref() == Some(&next) {
            return Ok(((), false));
        }
        transaction.execute(
            "INSERT INTO credential_health (site_id, status, checked_at, last_error)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(site_id) DO UPDATE SET
                 status = excluded.status,
                 checked_at = excluded.checked_at,
                 last_error = excluded.last_error",
            params![owner.id, status, now, error],
        )?;
        Ok(((), true))
    })?;
    Ok(())
}

fn credential_owner(site_id: &str) -> Result<&'static SiteEntry, String> {
    let source = site_by_id(site_id).ok_or_else(|| format!("Unknown source: {site_id}"))?;
    let owner = site_by_id(source.credential_owner_site_id).ok_or_else(|| {
        format!(
            "Unknown credential owner: {}",
            source.credential_owner_site_id
        )
    })?;
    if owner.id != owner.credential_owner_site_id {
        return Err(format!("Invalid credential owner: {}", owner.id));
    }
    Ok(owner)
}

fn validate_secret(credential: &SiteCredential) -> Result<(), String> {
    let valid = match credential.credential_type {
        CredentialType::ApiKey => credential
            .password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        CredentialType::Cookies => credential
            .cookies
            .as_ref()
            .is_some_and(|cookies| !cookies.is_empty()),
        CredentialType::OAuthToken => credential
            .oauth_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
    };
    valid.then_some(()).ok_or_else(|| {
        format!(
            "Captured {} credential is empty",
            credential.credential_type.as_str()
        )
    })
}

fn clean_display_name(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn auth_receipt(revision: u64) -> MutationReceipt {
    MutationReceipt {
        revision,
        resources: vec![
            resources::SUBSCRIPTIONS.to_string(),
            resources::SETTINGS.to_string(),
        ],
        item_ids: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;

    fn application() -> (tempfile::TempDir, Application) {
        let root = tempdir().unwrap();
        let store = Arc::new(Store::open(root.path()).unwrap());
        (root, Application::new(store))
    }

    fn insert_metadata(application: &Application, site_id: &str) {
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO credential (site_id, credential_type, display_name, created_at)
                     VALUES (?1, 'cookies', 'Test account', '2026-01-01T00:00:00Z')",
                    [site_id],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn source_aliases_resolve_to_one_credential_owner() {
        assert_eq!(credential_owner("pixivuser").unwrap().id, "pixiv");
        assert_eq!(credential_owner("pixiv").unwrap().id, "pixiv");
        assert!(credential_owner("artstation").is_err());
    }

    #[test]
    fn credential_snapshot_never_contains_secrets() {
        let (_root, application) = application();
        insert_metadata(&application, "patreon");
        let records = list_credentials(application.store()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].display_name.as_deref(), Some("Test account"));
        let json = serde_json::to_value(&records).unwrap();
        let record = json[0].as_object().unwrap();
        assert!(!record.contains_key("username"));
        assert!(!record.contains_key("password"));
        assert!(!record.contains_key("cookies"));
        assert!(!record.contains_key("oauth_token"));
    }

    #[test]
    fn run_health_is_persisted_only_for_connected_accounts() {
        let (_root, application) = application();
        mark_auth_failure(
            application.store(),
            "patreon",
            "2026-01-01T00:00:00Z",
            "login rejected",
        )
        .unwrap();
        assert!(list_health(application.store()).unwrap().is_empty());

        insert_metadata(&application, "patreon");
        mark_auth_failure(
            application.store(),
            "patreon",
            "2026-01-01T00:01:00Z",
            "login rejected",
        )
        .unwrap();
        assert_eq!(
            list_health(application.store()).unwrap()[0].status,
            "unauthorized"
        );

        mark_run_success(application.store(), "patreon", "2026-01-01T00:02:00Z").unwrap();
        let health = list_health(application.store()).unwrap();
        assert_eq!(health[0].status, "valid");
        assert!(health[0].last_error.is_none());
    }
}
