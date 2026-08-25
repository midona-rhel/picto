use chrono::{DateTime, Duration, Utc};

use super::epoch;
use super::provider::{CloudProvider, DirectoryProvider};
use super::reconcile::{self, ReconcileMode};
use crate::app::Application;

const BLOB_BATCH_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkerState {
    pub connected: bool,
    pub initialized: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickResult {
    pub state_changed: bool,
    pub reconciled: bool,
    pub published_mutations: usize,
    pub uploaded_blobs: usize,
    pub downloaded_blobs: usize,
    pub snapshot_created: bool,
    pub snapshots_pruned: usize,
}

struct Configuration {
    provider: String,
    root: String,
    paused: bool,
    last_snapshot_at: Option<String>,
}

pub async fn tick(
    application: &Application,
    state: &mut WorkerState,
) -> Result<TickResult, String> {
    match tick_inner(application, state).await {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = set_status(application, "error", "idle", &error);
            Err(error)
        }
    }
}

async fn tick_inner(
    application: &Application,
    state: &mut WorkerState,
) -> Result<TickResult, String> {
    let Some(configuration) = configuration(application)? else {
        state.connected = false;
        state.initialized = false;
        return Ok(TickResult::default());
    };
    if configuration.paused {
        state.connected = false;
        return Ok(TickResult::default());
    }
    let provider =
        match DirectoryProvider::open_provider_root(&configuration.provider, &configuration.root) {
            Ok(provider) => provider,
            Err(error) => {
                state.connected = false;
                let changed = set_status(application, "offline", "idle", &error)?;
                return Ok(TickResult {
                    state_changed: changed,
                    ..TickResult::default()
                });
            }
        };
    if !provider.connectivity().await? {
        state.connected = false;
        let changed = set_status(
            application,
            "offline",
            "idle",
            "The installed cloud sync folder is unavailable. Local changes are retained.",
        )?;
        return Ok(TickResult {
            state_changed: changed,
            ..TickResult::default()
        });
    }

    let reconnected = state.initialized && !state.connected;
    state.connected = true;
    state.initialized = true;
    let mut result = TickResult::default();
    if reconnected || reconcile::remote_metadata_pending(application, &provider).await? {
        reconcile::reconcile(application, &provider, ReconcileMode::Reconnect).await?;
        result.reconciled = true;
        result.state_changed = true;
    } else if epoch::flush_due(application.store(), Utc::now())? {
        let flushed = epoch::flush(application.store(), &provider, false).await?;
        result.published_mutations = flushed.published_mutations;
        result.state_changed |= flushed.published_mutations > 0;
    }

    let uploaded = super::blob::upload_pending(application, &provider, BLOB_BATCH_SIZE).await?;
    let recovered = super::blob::recover_pending(application, &provider, BLOB_BATCH_SIZE).await?;
    result.uploaded_blobs = uploaded.uploaded;
    result.downloaded_blobs = recovered.downloaded;
    result.state_changed |= uploaded != Default::default() || recovered != Default::default();

    if snapshot_due(configuration.last_snapshot_at.as_deref()) {
        super::snapshot::publish(application.store(), &provider).await?;
        result.snapshot_created = true;
        result.state_changed = true;
        result.snapshots_pruned =
            super::snapshot::prune_remote(application.store(), &provider).await?;
    }
    if !result.reconciled {
        result.state_changed |= set_status(application, "idle", "idle", "")?;
    }
    Ok(result)
}

fn configuration(application: &Application) -> Result<Option<Configuration>, String> {
    application.store().read(|connection| {
        connection
            .query_row(
                "SELECT provider, remote_root, paused, last_snapshot_at
                 FROM cloud_state
                 WHERE singleton = 1 AND provider IS NOT NULL",
                [],
                |row| {
                    Ok(Configuration {
                        provider: row.get(0)?,
                        root: row.get(1)?,
                        paused: row.get::<_, i64>(2)? != 0,
                        last_snapshot_at: row.get(3)?,
                    })
                },
            )
            .optional()
    })
}

fn snapshot_due(last_snapshot_at: Option<&str>) -> bool {
    last_snapshot_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|last| {
            Utc::now().signed_duration_since(last.with_timezone(&Utc)) >= Duration::days(1)
        })
}

fn set_status(
    application: &Application,
    state: &str,
    phase: &str,
    message: &str,
) -> Result<bool, String> {
    let (_, _, changed) = application.store().transaction_if_changed(|transaction| {
        let changed = transaction.execute(
            "UPDATE cloud_state SET state = ?1, phase = ?2, blocking = 0,
                    completed_units = 0, total_units = NULL, message = ?3
             WHERE singleton = 1
               AND (state != ?1 OR phase != ?2 OR blocking != 0
                    OR completed_units != 0 OR total_units IS NOT NULL OR message != ?3)",
            rusqlite::params![state, phase, message],
        )? > 0;
        Ok(((), changed))
    })?;
    Ok(changed)
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cloud::{self, ConfigureCloudInput};
    use crate::store::Store;

    #[tokio::test]
    async fn configured_folder_publishes_a_verified_snapshot() {
        let library = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));
        cloud::configure(
            &application,
            &ConfigureCloudInput {
                provider: "dropbox".into(),
                account_label: "test".into(),
                root_path: remote.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();

        let mut state = WorkerState::default();
        let result = tick(&application, &mut state).await.unwrap();

        assert!(state.connected);
        assert!(result.snapshot_created);
        assert!(!cloud::snapshot::list_remote(
            application.store(),
            &DirectoryProvider::open_existing(remote.path()).unwrap()
        )
        .await
        .unwrap()
        .is_empty());
    }
}
