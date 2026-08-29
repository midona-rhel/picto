//! Direct-site account persistence for the replacement backend.
//!
//! SQLite stores only display metadata and health. Authentication secrets stay
//! in the operating-system credential store.

use std::collections::HashMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::credential_store::{CredentialType, SiteCredential};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_aliases_resolve_to_one_credential_owner() {
        assert_eq!(credential_owner("pixivuser").unwrap().id, "pixiv");
        assert_eq!(credential_owner("pixiv").unwrap().id, "pixiv");
        assert!(credential_owner("artstation").is_err());
    }

    #[test]
    fn credential_snapshot_never_contains_secrets() {
        let root = tempfile::tempdir().unwrap();
        let application =
            crate::library_application::LibraryApplication::create(root.path()).unwrap();
        application
            .library()
            .auxiliary_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                ["settings".to_string()],
                [],
                |transaction, _| {
                    transaction.execute(
                        "INSERT INTO credential
                             (site_id, credential_type, display_name, created_at)
                         VALUES ('patreon', 'cookies', 'Test account',
                                 '2026-01-01T00:00:00Z')",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        let records = list_library_credentials(&application).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].display_name.as_deref(), Some("Test account"));
        let json = serde_json::to_value(&records).unwrap();
        let record = json[0].as_object().unwrap();
        assert!(!record.contains_key("username"));
        assert!(!record.contains_key("password"));
        assert!(!record.contains_key("cookies"));
        assert!(!record.contains_key("oauth_token"));
    }
}
