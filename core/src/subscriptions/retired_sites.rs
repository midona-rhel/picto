//! Sources Picto no longer supports.
//!
//! A retired source has no provider, site entry or sign-in window any more,
//! but a library can still hold subscriptions and a saved sign-in for it.
//! When a library opens, those subscriptions are paused with a note saying
//! the source is gone, and the saved sign-ins are removed (from the library
//! and the system keychain). Items the source already imported are library
//! items like any other and are never touched.

use crate::library_application::LibraryApplication;

/// Retired sources: site id and the name users knew them by.
pub const RETIRED_SITES: &[(&str, &str)] = &[
    ("konachan", "Konachan"),
    ("sankaku", "Sankaku"),
    ("idolcomplex", "Idol Complex"),
    ("baraag", "Baraag"),
];

/// The failure kind a retired source's queries carry.
const RETIRED_FAILURE_KIND: &str = "invalid_query";

pub fn retired_site_name(site_id: &str) -> Option<&'static str> {
    RETIRED_SITES
        .iter()
        .find(|(id, _)| *id == site_id)
        .map(|(_, name)| *name)
}

/// What a retired source's queries say, in the run log and the
/// subscription view.
pub fn retired_site_message(name: &str) -> String {
    format!("{name} is no longer supported. Items it already imported stay in your library.")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetiredSitesSettled {
    pub paused_queries: usize,
    pub removed_credentials: usize,
}

/// Pause retired sources' queries with a note and remove their saved
/// sign-ins. Changes nothing once that is done, so it runs at every open.
pub fn settle_retired_sites(application: &LibraryApplication) -> Result<RetiredSitesSettled, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let settled = application
        .library()
        .auxiliary_write_if_changed(
            picto_library::database::WorkPriority::CorrectnessRecovery,
            ["subscriptions".to_string(), "settings".to_string()],
            [],
            |transaction, _| {
                let mut settled = RetiredSitesSettled::default();
                let mut removed = Vec::new();
                for (site_id, name) in RETIRED_SITES {
                    let message = retired_site_message(name);
                    settled.paused_queries += transaction.execute(
                        "UPDATE subscription_query
                         SET paused = 1, last_failure_at = ?3, last_failure_kind = ?4,
                             last_failure_message = ?2
                         WHERE site_id = ?1
                           AND NOT (paused = 1 AND last_failure_message IS ?2)",
                        rusqlite::params![site_id, message, now, RETIRED_FAILURE_KIND],
                    )?;
                    if transaction.execute("DELETE FROM credential WHERE site_id = ?1", [site_id])? != 0 {
                        removed.push(*site_id);
                    }
                }
                settled.removed_credentials = removed.len();
                Ok((settled != RetiredSitesSettled::default()).then_some((settled, removed)))
            },
        )
        .map_err(|error| error.to_string())?;
    let Some(((settled, removed), _)) = settled else {
        return Ok(RetiredSitesSettled::default());
    };
    for site_id in removed {
        // The library row is gone either way; a secret the keychain no longer
        // holds is nothing to remove.
        if let Err(error) = crate::credential_store::delete_credential(site_id) {
            tracing::warn!(site_id, %error, "Could not remove a retired source's saved sign-in");
        }
    }
    Ok(settled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_state(application: &LibraryApplication, site_id: &str) -> (bool, Option<String>) {
        application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok(connection.query_row(
                        "SELECT paused, last_failure_message FROM subscription_query WHERE site_id = ?1",
                        [site_id],
                        |row| Ok((row.get::<_, bool>(0)?, row.get(1)?)),
                    )?)
                },
            )
            .unwrap()
    }

    #[test]
    fn retired_sources_are_paused_with_a_note_and_lose_their_sign_ins_once() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        application
            .library()
            .auxiliary_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                ["subscriptions".to_string()],
                [],
                |transaction, _| {
                    transaction.execute_batch(
                        "INSERT INTO subscription (subscription_id, subscription_key, name, created_at)
                         VALUES (1, 'subscription', 'Mixed', '2026-10-01T00:00:00Z');
                         INSERT INTO subscription_query
                             (query_key, subscription_id, site_id, domain_key, query_kind, query_text)
                         VALUES ('retired', 1, 'sankaku', 'sankaku.app', 'search', 'solo'),
                                ('kept', 1, 'yandere', 'yande.re', 'search', 'landscape');
                         INSERT INTO credential (site_id, credential_type, created_at)
                         VALUES ('sankaku', 'cookies', '2026-10-01T00:00:00Z'),
                                ('yandere', 'cookies', '2026-10-01T00:00:00Z');",
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(
            settle_retired_sites(&application).unwrap(),
            RetiredSitesSettled {
                paused_queries: 1,
                removed_credentials: 1,
            }
        );
        assert_eq!(
            query_state(&application, "sankaku"),
            (true, Some(retired_site_message("Sankaku")))
        );
        assert_eq!(query_state(&application, "yandere"), (false, None));
        let credentials = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok(connection
                        .prepare("SELECT site_id FROM credential ORDER BY site_id")?
                        .query_map([], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?)
                },
            )
            .unwrap();
        assert_eq!(credentials, vec!["yandere".to_string()]);

        // Once is enough.
        assert_eq!(
            settle_retired_sites(&application).unwrap(),
            RetiredSitesSettled::default()
        );
    }

    #[test]
    fn retired_sources_have_no_provider_or_site_entry() {
        let registry = picto_sources::ProviderRegistry::native();
        for (site_id, _) in RETIRED_SITES {
            assert!(registry.get(site_id).is_none(), "{site_id} still has a provider");
            assert!(
                crate::subscriptions::sites::site_by_id(site_id).is_none(),
                "{site_id} is still offered"
            );
        }
    }
}
