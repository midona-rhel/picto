//! Read-only operational diagnostics derived from durable worker state.

use rusqlite::OptionalExtension;
use serde::Serialize;

use crate::app::Application;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerDiagnostic {
    pub id: String,
    pub label: String,
    pub state: &'static str,
    pub detail: String,
    pub active: i64,
    pub queued: i64,
    pub attention: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub captured_at: String,
    pub workers: Vec<WorkerDiagnostic>,
}

pub fn snapshot(application: &Application) -> Result<DiagnosticsSnapshot, String> {
    let workers = application.store().read(|connection| {
        let ingest = connection.query_row(
            "SELECT
                 COALESCE(SUM(status = 'running'), 0),
                 COALESCE(SUM(status = 'pending'), 0),
                 COALESCE(SUM(status = 'failed'), 0)
             FROM ingest_job",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let ingest_activity = connection
            .query_row(
                "SELECT source_kind FROM ingest_job WHERE status = 'running' ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let work = connection.query_row(
            "SELECT
                 COALESCE(SUM(status = 'running'), 0),
                 COALESCE(SUM(status = 'pending'), 0),
                 COALESCE(SUM(status = 'pending' AND last_error IS NOT NULL), 0)
             FROM work_item",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let work_activity = connection
            .query_row(
                "SELECT work_type FROM work_item WHERE status = 'running' ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let work_noun = {
            let mut statement = connection.prepare(
                "SELECT work_type, COUNT(*)
                 FROM work_item
                 WHERE status = 'pending'
                 GROUP BY work_type
                 ORDER BY work_type",
            )?;
            let kinds = statement
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if kinds.len() == 1 {
                format!("{} jobs", work_kind_noun(&kinds[0].0))
            } else {
                "media jobs".to_string()
            }
        };
        let subscriptions = connection.query_row(
            "SELECT
                 COALESCE(SUM(status = 'running'), 0),
                 COALESCE(SUM(status = 'pending'), 0),
                 COALESCE(SUM(status = 'failed'), 0)
             FROM subscription_run_query",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let subscription_activity = connection
            .query_row(
                "SELECT q.site_id
                 FROM subscription_run_query rq
                 JOIN subscription_query q ON q.query_id = rq.query_id
                 WHERE rq.status = 'running'
                 ORDER BY rq.started_at DESC
                 LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let scheduled_runs: i64 = connection.query_row(
            "SELECT COUNT(*) FROM subscription_run WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(vec![
            worker("ingest", "Ingest", ingest, "media awaiting publication", ingest_activity.as_deref()),
            worker(
                "derivatives",
                "Media processing",
                work,
                &work_noun,
                work_activity.as_deref().map(work_label),
            ),
            worker(
                "subscriptions",
                "Subscriptions",
                subscriptions,
                "source queries across four slots",
                subscription_activity.as_deref(),
            ),
            WorkerDiagnostic {
                id: "scheduler".into(),
                label: "Subscription scheduler".into(),
                state: if scheduled_runs > 0 { "working" } else { "waiting" },
                detail: if scheduled_runs > 0 {
                    format!("{scheduled_runs} run(s) ready")
                } else {
                    "Waiting for scheduled runs".into()
                },
                active: 0,
                queued: scheduled_runs,
                attention: 0,
            },
            WorkerDiagnostic {
                id: "folder-watches".into(),
                label: "Folder watches".into(),
                state: "waiting",
                detail: "Checks watched folders every 30 seconds".into(),
                active: 0,
                queued: 0,
                attention: 0,
            },
        ])
    })?;
    Ok(DiagnosticsSnapshot {
        captured_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        workers,
    })
}

fn worker(
    id: &str,
    label: &str,
    (active, queued, attention): (i64, i64, i64),
    noun: &str,
    activity: Option<&str>,
) -> WorkerDiagnostic {
    let state = if attention > 0 {
        "attention"
    } else if active > 0 || queued > 0 {
        "working"
    } else {
        "waiting"
    };
    let detail = if active > 0 {
        match activity {
            Some(activity) => format!("{activity} · {active} active · {queued} queued"),
            None => format!("{active} active · {queued} queued"),
        }
    } else if queued > 0 {
        format!("{queued} {noun}")
    } else {
        "Idle".into()
    };
    WorkerDiagnostic {
        id: id.into(),
        label: label.into(),
        state,
        detail,
        active,
        queued,
        attention,
    }
}

fn work_label(work_type: &str) -> &str {
    match work_type {
        "thumbnail" => "Generating thumbnails",
        "dominant_colors" => "Extracting colors",
        "perceptual_hash" => "Calculating pHash",
        "blob_delete" => "Deleting blobs",
        "ai_tag" => "AI tagging",
        other => other,
    }
}

fn work_kind_noun(work_type: &str) -> &str {
    match work_type {
        "thumbnail" => "thumbnail",
        "dominant_colors" => "color-analysis",
        "perceptual_hash" => "pHash",
        "blob_delete" => "blob-deletion",
        "ai_tag" => "AI-tagging",
        _ => "media",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;

    #[test]
    fn snapshot_reports_durable_queue_activity() {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO ingest_job (
                         job_key, source_kind, source_path, payload_json, lifecycle, status,
                         attempt_count, available_at, created_at, updated_at
                     ) VALUES ('active', 'manual', '/image.jpg', '{}', 'inbox', 'running', 0, 'now', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO work_item (
                         file_hash, work_type, status, attempt_count, available_at,
                         created_at, updated_at
                     ) VALUES ('hash', 'thumbnail', 'running', 0, 'now', 'now', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let diagnostics = snapshot(&application).unwrap();
        assert_eq!(diagnostics.workers.len(), 5);
        assert_eq!(diagnostics.workers[0].state, "working");
        assert!(diagnostics.workers[0].detail.starts_with("manual"));
        assert_eq!(
            diagnostics.workers[1].detail,
            "Generating thumbnails · 1 active · 0 queued"
        );
    }
}
