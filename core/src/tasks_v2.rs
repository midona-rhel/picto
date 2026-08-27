//! Persisted background-task status for renderer progress and notifications.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::Application;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct QueueCounts {
    #[ts(type = "number")]
    pub pending: i64,
    #[ts(type = "number")]
    pub running: i64,
    #[ts(type = "number")]
    pub failed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TaskIssue {
    pub source: String,
    #[ts(type = "number")]
    pub task_id: i64,
    pub kind: String,
    pub message: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TaskSnapshot {
    pub ingest: QueueCounts,
    pub background: QueueCounts,
    pub subscriptions: QueueCounts,
    pub cloud: QueueCounts,
    pub issues: Vec<TaskIssue>,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn snapshot(application: &Application) -> Result<TaskSnapshot, String> {
    application.store().read(|connection| {
        let ingest = connection.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE status = 'pending'),
                 COUNT(*) FILTER (WHERE status = 'running'),
                 COUNT(*) FILTER (WHERE status = 'failed')
             FROM ingest_job",
            [],
            counts,
        )?;
        let background = connection.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE status = 'pending'),
                 COUNT(*) FILTER (WHERE status = 'running'),
                 0
             FROM work_item",
            [],
            counts,
        )?;
        let subscriptions = connection.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE status = 'pending'),
                 COUNT(*) FILTER (WHERE status = 'running'),
                 COUNT(*) FILTER (WHERE status = 'failed')
             FROM subscription_run",
            [],
            counts,
        )?;
        let cloud = connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM cloud_outbox WHERE published_at IS NULL)
                   + (SELECT COUNT(*) FROM cloud_blob_state
                      WHERE state = 'queued' OR (state = 'available' AND remote_present = 0)),
                 (SELECT COUNT(*) FROM cloud_state
                  WHERE singleton = 1 AND state IN ('reconciling', 'syncing'))
                   + (SELECT COUNT(*) FROM cloud_blob_state WHERE state = 'downloading'),
                 (SELECT COUNT(*) FROM cloud_quarantine WHERE resolved_at IS NULL)
                   + (SELECT COUNT(*) FROM cloud_blob_state WHERE last_error IS NOT NULL)
                   + (SELECT COUNT(*) FROM cloud_state WHERE singleton = 1 AND state = 'error')",
            [],
            counts,
        )?;
        let issues = connection
            .prepare(
                "SELECT source, task_id, kind, message, updated_at FROM (
                     SELECT 'ingest' AS source, ingest_job_id AS task_id,
                            source_kind AS kind, last_error AS message,
                            updated_at
                     FROM ingest_job
                     WHERE status = 'failed' AND last_error IS NOT NULL
                     UNION ALL
                     SELECT 'background', work_id, work_type, last_error, updated_at
                     FROM work_item
                     WHERE last_error IS NOT NULL
                     UNION ALL
                     SELECT 'subscription', issue_id, issue_kind, message, last_seen_at
                     FROM subscription_issue
                     WHERE status = 'open'
                     UNION ALL
                     SELECT 'cloud', quarantine_id, 'quarantined_mutation', reason, created_at
                     FROM cloud_quarantine
                     WHERE resolved_at IS NULL
                     UNION ALL
                     SELECT 'cloud', 0, 'blob_' || state, last_error, updated_at
                     FROM cloud_blob_state
                     WHERE last_error IS NOT NULL
                     UNION ALL
                     SELECT 'cloud', 0, 'sync', message, COALESCE(last_sync_at, '')
                     FROM cloud_state
                     WHERE singleton = 1 AND state = 'error' AND message != ''
                 )
                 ORDER BY updated_at DESC, source, task_id DESC
                 LIMIT 100",
            )?
            .query_map([], |row| {
                Ok(TaskIssue {
                    source: row.get(0)?,
                    task_id: row.get(1)?,
                    kind: row.get(2)?,
                    message: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(TaskSnapshot {
            ingest,
            background,
            subscriptions,
            cloud,
            issues,
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

fn counts(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueueCounts> {
    Ok(QueueCounts {
        pending: row.get(0)?,
        running: row.get(1)?,
        failed: row.get(2)?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;

    #[test]
    fn snapshot_is_derived_only_from_persisted_queue_state() {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO ingest_job (
                         job_key, source_kind, source_path, payload_json, lifecycle,
                         status, attempt_count, available_at, last_error, created_at, updated_at
                     ) VALUES
                         ('pending', 'manual', '/a', '{}', 'inbox', 'pending', 0, 'now', NULL, 'now', 'now'),
                         ('failed', 'watch', '/b', '{}', 'inbox', 'failed', 2, 'now', 'bad file', 'now', 'later')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO work_item (
                         work_id, file_hash, work_type, status, attempt_count, available_at,
                         last_error, created_at, updated_at
                     ) VALUES (1, 'hash', 'ai_tag', 'pending', 1, 'now', 'model unavailable', 'now', 'latest')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let tasks = snapshot(&application).unwrap();
        assert_eq!(tasks.ingest.pending, 1);
        assert_eq!(tasks.ingest.failed, 1);
        assert_eq!(tasks.background.pending, 1);
        assert_eq!(tasks.cloud, QueueCounts::default());
        assert_eq!(tasks.issues.len(), 2);
        assert_eq!(tasks.issues[0].source, "background");
        assert_eq!(tasks.revision, 2);
    }
}
