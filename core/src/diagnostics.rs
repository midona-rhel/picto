//! Read-only operational diagnostics derived from durable worker state.

use rusqlite::OptionalExtension;
use serde::Serialize;

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

pub fn snapshot_library(
    application: &crate::library_application::LibraryApplication,
) -> Result<DiagnosticsSnapshot, String> {
    let workers = application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| durable_workers(connection).map_err(Into::into),
        )
        .map_err(|error| error.to_string())?;
    Ok(finish_snapshot(workers, application.ai_worker_status()))
}

fn durable_workers(connection: &rusqlite::Connection) -> rusqlite::Result<Vec<WorkerDiagnostic>> {
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
                 COALESCE(SUM(status = 'failed'
                              OR (status = 'pending' AND last_error IS NOT NULL)), 0)
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
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
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
                 (SELECT COUNT(*) FROM subscription_issue WHERE status = 'open')
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
        worker(
            "ingest",
            "Ingest",
            ingest,
            "media awaiting publication",
            ingest_activity.as_deref(),
        ),
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
            state: if scheduled_runs > 0 {
                "working"
            } else {
                "waiting"
            },
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
}

fn finish_snapshot(
    mut workers: Vec<WorkerDiagnostic>,
    ai: crate::library_application::AiWorkerStatus,
) -> DiagnosticsSnapshot {
    workers.push(WorkerDiagnostic {
        id: "ai-tagger".into(),
        label: "AI tagging".into(),
        state: if ai.active { "working" } else { "waiting" },
        detail: ai.detail,
        active: i64::from(ai.active),
        queued: 0,
        attention: 0,
    });
    DiagnosticsSnapshot {
        captured_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        workers,
    }
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
    use super::*;

    #[test]
    fn schema_one_snapshot_uses_the_canonical_scheduler_and_process_state() {
        let directory = tempfile::tempdir().unwrap();
        let application =
            crate::library_application::LibraryApplication::create(directory.path()).unwrap();
        application.set_ai_worker_status(true, "Running model · 3 roots");

        let diagnostics = snapshot_library(&application).unwrap();
        assert_eq!(diagnostics.workers.len(), 6);
        let ai = diagnostics
            .workers
            .iter()
            .find(|worker| worker.id == "ai-tagger")
            .unwrap();
        assert_eq!(ai.state, "working");
        assert_eq!(ai.detail, "Running model · 3 roots");
    }

    #[test]
    fn failed_media_work_is_reported_as_attention() {
        let directory = tempfile::tempdir().unwrap();
        let application =
            crate::library_application::LibraryApplication::create(directory.path()).unwrap();
        application
            .library()
            .database()
            .maintenance_write(
                picto_library::database::WorkPriority::Maintenance,
                |transaction| {
                    transaction.execute(
                        "INSERT INTO work_item
                             (file_hash, work_type, status, priority, attempt_count,
                              available_at, last_error, created_at, updated_at)
                         VALUES ('failed-thumbnail', 'thumbnail', 'failed', 0, 8,
                                 '2026-08-31T00:00:00Z', 'source missing',
                                 '2026-08-31T00:00:00Z', '2026-08-31T00:00:00Z')",
                        [],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        let diagnostics = snapshot_library(&application).unwrap();
        let derivatives = diagnostics
            .workers
            .iter()
            .find(|worker| worker.id == "derivatives")
            .unwrap();
        assert_eq!(derivatives.state, "attention");
        assert_eq!(derivatives.attention, 1);
    }
}
