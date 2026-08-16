use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::LibraryDatabase;
use crate::ingest_queue::IngestQueueCounts;
use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::types::{SubscriptionInfo, SubscriptionQueryInfo};

use super::archive::{
    clear_subscription_archive_entries_at_root, subscription_query_archive_prefix,
};
use super::runtime_db::{
    add_query_progress, add_subscription_entity, cancel_pending_subscription_jobs_for_subscription,
    count_active_subscription_query_jobs, create_subscription_query_run, create_subscription_run,
    enqueue_subscription_query_job, finalize_subscription_query_run_if_terminal,
    finalize_subscription_run_if_terminal, finalize_subscription_run_status,
    finish_subscription_query_job, lease_subscription_query_job,
    list_queued_subscription_query_jobs, list_retryable_subscription_download_attempts,
    list_running_subscription_run_ids, list_subscription_download_attempts_page,
    list_subscription_issues, list_subscription_issues_page, list_subscription_query_jobs_for_run,
    list_subscription_query_runs, list_subscription_retry_targets, list_subscription_runs,
    mark_subscription_download_attempt_retrying, record_subscription_query_source_completion,
    requeue_interrupted_subscription_query_job, reschedule_subscription_query_job,
    reset_subscription_query_state, reset_subscription_state,
    resolve_subscription_download_attempt, resolve_subscription_issues,
    set_query_completed_initial_run, set_query_resume_state, set_query_terminal_state,
    set_subscription_issue_next_retry, upsert_subscription_download_attempt,
    upsert_subscription_issue, upsert_subscription_post_member,
};
use super::types::{
    OwnedSubscriptionDownloadAttemptUpsert, OwnedSubscriptionPostMemberUpsert, Subscription,
    SubscriptionDownloadAttemptRecord, SubscriptionDownloadAttemptUpsert, SubscriptionIssueRecord,
    SubscriptionPostMemberUpsert, SubscriptionQuery, SubscriptionQueryJob,
    SubscriptionQueryRunCompletion, SubscriptionQueryRunRecord, SubscriptionRunRecord,
};

#[derive(Debug, Clone)]
struct CanonicalSubscriptionRow {
    subscription_id: i64,
    name: String,
    schedule: String,
    paused: bool,
    initial_post_limit: i64,
    periodic_post_limit: i64,
    date_added: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentQueryRunProgress {
    pub posts_processed: usize,
    pub files_downloaded: usize,
    pub files_skipped: usize,
    pub metadata_validated: usize,
    pub metadata_invalid: usize,
    pub current_posts_processed: usize,
    pub current_files_downloaded: usize,
    pub current_files_skipped: usize,
    pub current_metadata_validated: usize,
    pub current_metadata_invalid: usize,
}

#[derive(Debug, Clone)]
struct CanonicalQueryRow {
    query_id: i64,
    subscription_id: i64,
    site_id: String,
    query_kind: String,
    query_text: String,
    display_name: Option<String>,
    notes: Option<String>,
    paused: bool,
    last_check_time: Option<String>,
    files_found: i64,
    posts_found: i64,
    completed_initial_run: bool,
    resume_cursor: Option<String>,
    resume_strategy: Option<String>,
    last_success_at: Option<String>,
    last_failure_at: Option<String>,
    last_failure_kind: Option<String>,
    last_failure_message: Option<String>,
}

pub struct SubscriptionRuntimeService<'a> {
    db: &'a LibraryDatabase,
    library_root: PathBuf,
}

pub async fn link_subscription_entity(
    db: &LibraryDatabase,
    subscription_id: i64,
    entity_hash: &str,
) -> Result<bool, String> {
    let entity_hash = entity_hash.to_string();
    db.with_write(move |conn| {
        let entity_id: i64 = conn.query_row(
            "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
            [&entity_hash],
            |row| row.get(0),
        )?;
        add_subscription_entity(conn, subscription_id, entity_id)
    })
}

#[derive(Debug, Clone)]
pub struct RunnableSubscription {
    pub subscription: Subscription,
    pub queries: Vec<SubscriptionQuery>,
}

#[derive(Debug, Clone)]
pub struct RunnableQuery {
    pub subscription: Subscription,
    pub query: SubscriptionQuery,
}

#[derive(Debug, Clone)]
pub struct ScheduledSubscription {
    pub subscription_id: i64,
    pub name: String,
    pub schedule: String,
    pub last_scheduled_success_at: Option<String>,
}

impl<'a> SubscriptionRuntimeService<'a> {
    pub fn new(db: &'a LibraryDatabase, library_root: &Path) -> Self {
        Self {
            db,
            library_root: library_root.to_path_buf(),
        }
    }

    pub async fn set_subscription_schedule(
        &self,
        id: String,
        schedule: String,
    ) -> Result<(), String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        validate_schedule(&schedule)?;
        let device_id = self.db.device_id().to_string();
        self.db.with_write(|conn| {
            let uuid: Option<String> = conn
                .query_row(
                    "SELECT uuid FROM subscription WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )
                .optional()?;
            conn.execute(
                "UPDATE subscription SET schedule = ?1 WHERE subscription_id = ?2",
                params![schedule, subscription_id],
            )?;
            if let Some(uuid) = uuid {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "subscription_updated",
                    &uuid,
                    &serde_json::json!({ "schedule": schedule }),
                )?;
            }
            Ok(())
        })
    }

    pub async fn get_subscriptions(&self) -> Result<Vec<SubscriptionInfo>, String> {
        let subs = self
            .db
            .with_read(list_subscriptions_with_counts_canonical)?;
        let queries = self.db.with_read(list_all_queries_canonical)?;
        let mut queries_map: HashMap<i64, Vec<SubscriptionQueryInfo>> = HashMap::new();
        for query in queries {
            queries_map
                .entry(query.subscription_id)
                .or_default()
                .push(query_info_from_row(query));
        }
        Ok(subs
            .into_iter()
            .map(|(sub, total_files)| {
                subscription_info_from_row(
                    sub.clone(),
                    total_files,
                    queries_map.remove(&sub.subscription_id).unwrap_or_default(),
                )
            })
            .collect())
    }

    pub async fn list_scheduled_subscriptions(&self) -> Result<Vec<ScheduledSubscription>, String> {
        self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT s.subscription_id, s.name, s.schedule,
                        MAX(CASE
                            WHEN sr.status = 'succeeded'
                             AND EXISTS (
                                 SELECT 1
                                 FROM subscription_query_job job
                                 WHERE job.run_id = sr.run_id
                                   AND job.requested_by = 'scheduled'
                             )
                            THEN sr.finished_at
                        END)
                 FROM subscription s
                 LEFT JOIN subscription_run sr ON sr.subscription_id = s.subscription_id
                 WHERE s.paused = 0
                   AND s.schedule != 'manual'
                   AND EXISTS (
                       SELECT 1
                       FROM subscription_query q
                       WHERE q.subscription_id = s.subscription_id AND q.paused = 0
                   )
                 GROUP BY s.subscription_id, s.name, s.schedule
                 ORDER BY s.subscription_id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(ScheduledSubscription {
                    subscription_id: row.get(0)?,
                    name: row.get(1)?,
                    schedule: row.get(2)?,
                    last_scheduled_success_at: row.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    pub async fn create_subscription(
        &self,
        name: String,
        initial_post_limit: Option<u32>,
        periodic_post_limit: Option<u32>,
    ) -> Result<SubscriptionInfo, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let initial_post_limit = initial_post_limit.unwrap_or(100);
        let periodic_post_limit = periodic_post_limit.unwrap_or(50);
        let uuid = crate::oplog::new_uuid();
        let device_id = self.db.device_id().to_string();
        let subscription_id = self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO subscription
                    (name, schedule, paused, initial_post_limit,
                     periodic_post_limit, uuid, date_added)
                 VALUES (?1, 'daily', 0, ?2, ?3, ?4, ?5)",
                params![
                    name,
                    initial_post_limit as i64,
                    periodic_post_limit as i64,
                    uuid,
                    now
                ],
            )?;
            let subscription_id = conn.last_insert_rowid();
            crate::oplog::record_op(
                conn,
                &device_id,
                "subscription_created",
                &uuid,
                &serde_json::json!({
                    "name": name,
                    "schedule": "daily",
                    "paused": false,
                    "initial_post_limit": initial_post_limit,
                    "periodic_post_limit": periodic_post_limit,
                    "date_added": now,
                }),
            )?;
            Ok(subscription_id)
        })?;

        Ok(SubscriptionInfo {
            id: subscription_id.to_string(),
            name,
            schedule: "daily".to_string(),
            paused: false,
            initial_post_limit,
            periodic_post_limit,
            created_at: now,
            total_files: 0,
            queries: vec![],
        })
    }

    pub async fn delete_subscription(&self, id: String) -> Result<usize, String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        let device_id = self.db.device_id().to_string();
        self.db.with_write(|conn| {
            let subscription_uuid: Option<String> = conn
                .query_row(
                    "SELECT uuid FROM subscription WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(subscription_uuid) = subscription_uuid else {
                return Ok(0);
            };

            let mut query_uuids = Vec::new();
            let mut stmt = conn.prepare_cached(
                "SELECT uuid FROM subscription_query
                 WHERE subscription_id = ?1
                 ORDER BY query_id",
            )?;
            let rows = stmt.query_map([subscription_id], |row| row.get::<_, String>(0))?;
            for row in rows {
                query_uuids.push(row?);
            }
            for query_uuid in query_uuids {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "subscription_query_deleted",
                    &query_uuid,
                    &serde_json::json!({}),
                )?;
            }
            conn.execute(
                "DELETE FROM subscription WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            crate::oplog::record_op(
                conn,
                &device_id,
                "subscription_deleted",
                &subscription_uuid,
                &serde_json::json!({}),
            )?;
            Ok(1)
        })
    }

    pub async fn pause_subscription(&self, id: String, paused: bool) -> Result<(), String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        let device_id = self.db.device_id().to_string();
        self.db.with_write(|conn| {
            let uuid: Option<String> = conn
                .query_row(
                    "SELECT uuid FROM subscription WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )
                .optional()?;
            conn.execute(
                "UPDATE subscription SET paused = ?1 WHERE subscription_id = ?2",
                params![paused as i64, subscription_id],
            )?;
            if let Some(uuid) = uuid {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "subscription_updated",
                    &uuid,
                    &serde_json::json!({ "paused": paused }),
                )?;
            }
            Ok(())
        })
    }

    pub async fn rename_subscription(&self, id: String, name: String) -> Result<(), String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        let device_id = self.db.device_id().to_string();
        self.db.with_write(|conn| {
            let uuid: Option<String> = conn
                .query_row(
                    "SELECT uuid FROM subscription WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )
                .optional()?;
            conn.execute(
                "UPDATE subscription SET name = ?1 WHERE subscription_id = ?2",
                params![trimmed, subscription_id],
            )?;
            if let Some(uuid) = uuid {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "subscription_updated",
                    &uuid,
                    &serde_json::json!({ "name": trimmed }),
                )?;
            }
            Ok(())
        })
    }

    pub async fn get_subscription_covers(
        &self,
    ) -> Result<Vec<crate::subscriptions::types::SubscriptionCoverRecord>, String> {
        self.db.with_read(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT se.subscription_id, me.entity_hash
                 FROM subscription_entity se
                 JOIN media_entity me ON me.entity_id = se.entity_id
                 WHERE se.entity_id = (
                     SELECT MAX(se2.entity_id) FROM subscription_entity se2
                     WHERE se2.subscription_id = se.subscription_id
                 )",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(crate::subscriptions::types::SubscriptionCoverRecord {
                    subscription_id: row.get::<_, i64>(0)?.to_string(),
                    entity_hash: row.get(1)?,
                })
            })?;
            rows.collect()
        })
    }

    pub async fn add_subscription_query(
        &self,
        subscription_id: String,
        site_id: String,
        query_kind: Option<String>,
        query_text: String,
        notes: Option<String>,
    ) -> Result<SubscriptionQueryInfo, String> {
        let subscription_id: i64 = subscription_id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {subscription_id}"))?;
        if crate::subscriptions::gallery_dl_runner::site_by_id(&site_id).is_none() {
            return Err(format!("Unknown site: {site_id}"));
        }
        let canonical_site_id = site_id.clone();
        let resolved_query_kind = crate::subscriptions::source_adapter::resolve_query_kind(
            &site_id,
            query_kind.as_deref(),
        );
        crate::subscriptions::source_adapter::validate_query_kind(&site_id, &resolved_query_kind)?;
        // "@handle" or a pasted profile URL becomes the bare token the site's
        // URL template expects; the raw input survives as the display name.
        let normalized_query = crate::subscriptions::source_adapter::normalize_query_text(
            &site_id,
            &resolved_query_kind,
            &query_text,
        );
        crate::subscriptions::source_adapter::validate_query_text(&site_id, &normalized_query)?;
        let query_uuid = crate::oplog::new_uuid();
        let device_id = self.db.device_id().to_string();
        let query_id = self.db.with_write(|conn| {
            let subscription_uuid: String = conn.query_row(
                "SELECT uuid FROM subscription WHERE subscription_id = ?1",
                [subscription_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO subscription_query
                    (subscription_id, uuid, site_id, query_kind, query_text, display_name, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    subscription_id,
                    query_uuid,
                    canonical_site_id,
                    resolved_query_kind,
                    normalized_query,
                    query_text.trim(),
                    notes
                ],
            )?;
            let query_id = conn.last_insert_rowid();
            crate::oplog::record_op(
                conn,
                &device_id,
                "subscription_query_created",
                &query_uuid,
                &serde_json::json!({
                    "subscription_uuid": subscription_uuid,
                    "site_id": canonical_site_id,
                    "query_kind": resolved_query_kind,
                    "query_text": normalized_query,
                    "display_name": query_text.trim(),
                    "notes": notes,
                    "paused": false,
                }),
            )?;
            Ok(query_id)
        })?;

        Ok(SubscriptionQueryInfo {
            id: query_id.to_string(),
            site_id: canonical_site_id,
            query_kind: resolved_query_kind,
            query_text: normalized_query,
            display_name: Some(query_text.trim().to_string()),
            notes,
            paused: false,
            last_check_time: None,
            files_found: 0,
            posts_found: 0,
            completed_initial_run: false,
            resume_cursor: None,
            resume_strategy: None,
            last_success_at: None,
            last_failure_at: None,
            last_failure_kind: None,
            last_failure_message: None,
        })
    }

    pub async fn edit_subscription_query(
        &self,
        id: i64,
        site_id: String,
        query_kind: Option<String>,
        query_text: String,
        display_name: Option<String>,
        notes: Option<String>,
    ) -> Result<(), String> {
        if crate::subscriptions::gallery_dl_runner::site_by_id(&site_id).is_none() {
            return Err(format!("Unknown site: {site_id}"));
        }
        let canonical_site_id = site_id.clone();
        let resolved_query_kind = crate::subscriptions::source_adapter::resolve_query_kind(
            &site_id,
            query_kind.as_deref(),
        );
        crate::subscriptions::source_adapter::validate_query_kind(&site_id, &resolved_query_kind)?;
        let normalized_query = crate::subscriptions::source_adapter::normalize_query_text(
            &site_id,
            &resolved_query_kind,
            &query_text,
        );
        crate::subscriptions::source_adapter::validate_query_text(&site_id, &normalized_query)?;
        let device_id = self.db.device_id().to_string();
        enum EditQueryOutcome {
            Running,
            Missing,
            Updated(Option<String>),
        }
        let archive_prefix = match self.db.with_write(|conn| {
            let active_jobs: i64 = conn.query_row(
                "SELECT COUNT(*) FROM subscription_query_job
                 WHERE query_id = ?1 AND status IN ('queued', 'running')",
                [id],
                |row| row.get(0),
            )?;
            if active_jobs > 0 {
                return Ok(EditQueryOutcome::Running);
            }

            let query_identity: Option<(String, String, i64, String, String, String)> = conn
                .query_row(
                    "SELECT q.uuid, s.uuid, q.subscription_id,
                            q.site_id, q.query_kind, q.query_text
                     FROM subscription_query q
                     JOIN subscription s ON s.subscription_id = q.subscription_id
                     WHERE q.query_id = ?1",
                    [id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .optional()?;
            let Some((
                query_uuid,
                subscription_uuid,
                subscription_id,
                old_site_id,
                old_query_kind,
                old_query_text,
            )) = query_identity
            else {
                return Ok(EditQueryOutcome::Missing);
            };
            let source_changed = old_site_id != canonical_site_id
                || old_query_kind != resolved_query_kind
                || old_query_text != normalized_query;
            conn.execute(
                "UPDATE subscription_query
                 SET site_id = ?1, query_kind = ?2, query_text = ?3, display_name = ?4, notes = ?5
                 WHERE query_id = ?6",
                params![
                    canonical_site_id,
                    resolved_query_kind,
                    normalized_query,
                    display_name,
                    notes,
                    id
                ],
            )?;
            if source_changed {
                reset_subscription_query_state(conn, id)?;
            }
            crate::oplog::record_op(
                conn,
                &device_id,
                "subscription_query_updated",
                &query_uuid,
                &serde_json::json!({
                    "subscription_uuid": subscription_uuid,
                    "site_id": canonical_site_id,
                    "query_kind": resolved_query_kind,
                    "query_text": normalized_query,
                    "display_name": display_name,
                    "notes": notes,
                }),
            )?;
            Ok(EditQueryOutcome::Updated(source_changed.then(|| {
                subscription_query_archive_prefix(subscription_id, id)
            })))
        })? {
            EditQueryOutcome::Running => {
                return Err("Cannot edit a subscription query while it is running".to_string())
            }
            EditQueryOutcome::Missing => return Err(format!("Query {id} not found")),
            EditQueryOutcome::Updated(archive_prefix) => archive_prefix,
        };
        if let Some(archive_prefix) = archive_prefix {
            clear_subscription_archive_entries_at_root(
                self.library_root.as_path(),
                &[archive_prefix],
            )
            .await?;
        }
        Ok(())
    }

    pub async fn delete_subscription_query(&self, id: String) -> Result<(), String> {
        let query_id: i64 = id.parse().map_err(|_| format!("Invalid query id: {id}"))?;
        let device_id = self.db.device_id().to_string();
        self.db.with_write(|conn| {
            let query_uuid: Option<String> = conn
                .query_row(
                    "SELECT uuid FROM subscription_query WHERE query_id = ?1",
                    [query_id],
                    |row| row.get(0),
                )
                .optional()?;
            conn.execute(
                "DELETE FROM subscription_query WHERE query_id = ?1",
                [query_id],
            )?;
            if let Some(query_uuid) = query_uuid {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "subscription_query_deleted",
                    &query_uuid,
                    &serde_json::json!({}),
                )?;
            }
            Ok(())
        })
    }

    pub async fn pause_subscription_query(&self, id: String, paused: bool) -> Result<(), String> {
        let query_id: i64 = id.parse().map_err(|_| format!("Invalid query id: {id}"))?;
        let device_id = self.db.device_id().to_string();
        self.db.with_write(|conn| {
            let query_identity: Option<(String, String)> = conn
                .query_row(
                    "SELECT q.uuid, s.uuid
                     FROM subscription_query q
                     JOIN subscription s ON s.subscription_id = q.subscription_id
                     WHERE q.query_id = ?1",
                    [query_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            conn.execute(
                "UPDATE subscription_query SET paused = ?1 WHERE query_id = ?2",
                params![paused as i64, query_id],
            )?;
            if let Some((query_uuid, subscription_uuid)) = query_identity {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "subscription_query_updated",
                    &query_uuid,
                    &serde_json::json!({
                        "subscription_uuid": subscription_uuid,
                        "paused": paused,
                    }),
                )?;
            }
            Ok(())
        })
    }

    pub async fn reset_subscription(&self, id: String) -> Result<(), String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        let query_ids = self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT query_id FROM subscription_query WHERE subscription_id = ?1",
            )?;
            let rows = stmt.query_map([subscription_id], |row| row.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let mut archive_prefixes: Vec<String> = query_ids
            .iter()
            .map(|query_id| subscription_query_archive_prefix(subscription_id, *query_id))
            .collect();
        archive_prefixes.push(format!("picto_s{subscription_id}_q"));

        self.db
            .with_write(|conn| reset_subscription_state(conn, subscription_id).map(|_| ()))?;
        clear_subscription_archive_entries_at_root(self.library_root.as_path(), &archive_prefixes)
            .await?;
        Ok(())
    }

    pub async fn reset_subscription_query(&self, id: String) -> Result<(), String> {
        let query_id: i64 = id.parse().map_err(|_| format!("Invalid query id: {id}"))?;
        let query = self
            .db
            .with_read(|conn| get_subscription_query_canonical(conn, query_id))?
            .ok_or_else(|| format!("Query {id} not found"))?;
        let archive_prefix = subscription_query_archive_prefix(query.subscription_id, query_id);
        self.db
            .with_write(|conn| reset_subscription_query_state(conn, query_id).map(|_| ()))?;
        clear_subscription_archive_entries_at_root(self.library_root.as_path(), &[archive_prefix])
            .await?;
        Ok(())
    }

    pub async fn get_subscription(
        &self,
        subscription_id: i64,
    ) -> Result<Option<Subscription>, String> {
        self.db.with_read(|conn| {
            get_subscription_canonical(conn, subscription_id)
                .map(|row| row.map(subscription_from_row))
        })
    }

    pub async fn get_subscription_query(
        &self,
        query_id: i64,
    ) -> Result<Option<SubscriptionQuery>, String> {
        self.db.with_read(|conn| {
            get_subscription_query_canonical(conn, query_id).map(|row| row.map(query_from_row))
        })
    }

    pub async fn get_subscription_queries(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<SubscriptionQuery>, String> {
        self.db.with_read(|conn| {
            list_subscription_queries_canonical(conn, subscription_id)
                .map(|rows| rows.into_iter().map(query_from_row).collect::<Vec<_>>())
        })
    }

    pub async fn get_runnable_subscription(
        &self,
        subscription_id: i64,
    ) -> Result<Option<RunnableSubscription>, String> {
        let Some(subscription) = self.get_subscription(subscription_id).await? else {
            return Ok(None);
        };
        let queries = self.get_subscription_queries(subscription_id).await?;
        Ok(Some(RunnableSubscription {
            subscription,
            queries,
        }))
    }

    pub async fn get_runnable_query(
        &self,
        subscription_id: i64,
        query_id: i64,
    ) -> Result<Option<RunnableQuery>, String> {
        let Some(bundle) = self.get_runnable_subscription(subscription_id).await? else {
            return Ok(None);
        };
        let Some(query) = bundle
            .queries
            .iter()
            .find(|query| query.query_id == query_id)
            .cloned()
        else {
            return Ok(None);
        };
        Ok(Some(RunnableQuery {
            subscription: bundle.subscription,
            query,
        }))
    }

    pub async fn create_subscription_run(&self, subscription_id: i64) -> Result<i64, String> {
        self.db
            .with_write(move |conn| create_subscription_run(conn, subscription_id))
    }

    /// Startup repair: finalize runtime rows orphaned by a previous process
    /// and fix stored query kinds that no longer validate for their site.
    /// Must run before the site-runner worker starts.
    pub async fn reconcile_subscription_runtime_state(
        &self,
    ) -> Result<super::runtime_db::SubscriptionReconcileReport, String> {
        let mut report = self
            .db
            .with_write(super::runtime_db::reconcile_stale_subscription_runtime)?;

        let kinds: Vec<(i64, String, String)> = self.db.with_read(|conn| {
            let mut stmt =
                conn.prepare("SELECT query_id, site_id, query_kind FROM subscription_query")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect()
        })?;
        for (query_id, site_id, kind) in kinds {
            // Queries for removed sites are left untouched and fail clearly at run time.
            if crate::subscriptions::gallery_dl_runner::site_by_id(&site_id).is_none() {
                continue;
            }
            if crate::subscriptions::source_adapter::validate_query_kind(&site_id, &kind).is_err() {
                let repaired =
                    crate::subscriptions::source_adapter::infer_query_kind(&site_id).to_string();
                self.db.with_write(move |conn| {
                    conn.execute(
                        "UPDATE subscription_query SET query_kind = ?1 WHERE query_id = ?2",
                        params![repaired, query_id],
                    )?;
                    Ok(())
                })?;
                report.query_kinds_repaired += 1;
            }
        }
        Ok(report)
    }

    pub async fn finalize_subscription_run_status(
        &self,
        run_id: i64,
        status: &str,
        failure_kind: Option<String>,
        error_message: Option<String>,
    ) -> Result<(), String> {
        let status = status.to_string();
        self.db.with_write(move |conn| {
            finalize_subscription_run_status(
                conn,
                run_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
            )
        })
    }

    pub async fn finalize_subscription_run_if_terminal(
        &self,
        run_id: i64,
    ) -> Result<Option<SubscriptionRunRecord>, String> {
        self.db
            .with_write(move |conn| finalize_subscription_run_if_terminal(conn, run_id))
    }

    pub async fn create_subscription_query_run(
        &self,
        run_id: Option<i64>,
        subscription_id: i64,
        query_id: i64,
    ) -> Result<i64, String> {
        self.db.with_write(move |conn| {
            create_subscription_query_run(conn, run_id, subscription_id, query_id)
        })
    }

    pub async fn list_unsettled_subscription_query_run_ids(&self) -> Result<Vec<i64>, String> {
        self.db.with_read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT query_run_id FROM subscription_query_run
                 WHERE status LIKE 'settling_%'
                 ORDER BY query_run_id",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect()
        })
    }

    pub async fn record_subscription_query_source_completion(
        &self,
        query_run_id: i64,
        completion: SubscriptionQueryRunCompletion,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            record_subscription_query_source_completion(conn, query_run_id, &completion)
        })
    }

    pub async fn finalize_subscription_query_run_if_terminal(
        &self,
        query_run_id: i64,
    ) -> Result<Option<SubscriptionQueryRunRecord>, String> {
        self.db
            .with_write(move |conn| finalize_subscription_query_run_if_terminal(conn, query_run_id))
    }

    pub async fn add_query_progress(
        &self,
        query_id: i64,
        last_check_time: Option<String>,
        files_found_delta: i64,
        posts_found_delta: i64,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            add_query_progress(
                conn,
                query_id,
                last_check_time.as_deref(),
                files_found_delta,
                posts_found_delta,
            )
        })
    }

    pub async fn set_query_resume_state(
        &self,
        query_id: i64,
        resume_cursor: Option<String>,
        resume_strategy: Option<String>,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            set_query_resume_state(
                conn,
                query_id,
                resume_cursor.as_deref(),
                resume_strategy.as_deref(),
            )
        })
    }

    pub async fn enqueue_subscription_query_job(
        &self,
        run_id: Option<i64>,
        subscription_id: i64,
        query_id: i64,
        site_id: &str,
        job_kind: &str,
        requested_by: &str,
        post_id: Option<&str>,
    ) -> Result<(i64, bool), String> {
        let site_id = site_id.to_string();
        let job_kind = job_kind.to_string();
        let requested_by = requested_by.to_string();
        let post_id = post_id.map(str::to_string);
        self.db.with_write(move |conn| {
            enqueue_subscription_query_job(
                conn,
                run_id,
                subscription_id,
                query_id,
                &site_id,
                &job_kind,
                &requested_by,
                post_id.as_deref(),
            )
        })
    }

    pub async fn finalize_open_runs_for_subscription(
        &self,
        subscription_id: i64,
        status: &str,
        failure_kind: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<usize, String> {
        let status = status.to_string();
        let failure_kind = failure_kind.map(str::to_string);
        let error_message = error_message.map(str::to_string);
        self.db.with_write(move |conn| {
            super::runtime_db::finalize_open_runs_for_subscription(
                conn,
                subscription_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
            )
        })
    }

    pub async fn list_queued_subscription_query_jobs(
        &self,
        limit: i64,
    ) -> Result<Vec<SubscriptionQueryJob>, String> {
        self.db
            .with_read(move |conn| list_queued_subscription_query_jobs(conn, limit))
    }

    pub async fn list_subscription_query_jobs_for_run(
        &self,
        run_id: i64,
    ) -> Result<Vec<SubscriptionQueryJob>, String> {
        self.db
            .with_read(move |conn| list_subscription_query_jobs_for_run(conn, run_id))
    }

    pub async fn lease_subscription_query_job(
        &self,
        job_id: i64,
    ) -> Result<Option<SubscriptionQueryJob>, String> {
        self.db
            .with_write(move |conn| lease_subscription_query_job(conn, job_id))
    }

    pub async fn finish_subscription_query_job(
        &self,
        job_id: i64,
        status: &str,
        failure_kind: Option<String>,
        error_message: Option<String>,
    ) -> Result<(), String> {
        let status = status.to_string();
        self.db.with_write(move |conn| {
            finish_subscription_query_job(
                conn,
                job_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
            )
        })
    }

    pub async fn reschedule_subscription_query_job(
        &self,
        job_id: i64,
        available_at: String,
        failure_kind: String,
        error_message: Option<String>,
    ) -> Result<bool, String> {
        self.db.with_write(move |conn| {
            reschedule_subscription_query_job(
                conn,
                job_id,
                &available_at,
                &failure_kind,
                error_message.as_deref(),
            )
        })
    }

    pub async fn requeue_interrupted_subscription_query_job(
        &self,
        job_id: i64,
    ) -> Result<bool, String> {
        self.db
            .with_write(move |conn| requeue_interrupted_subscription_query_job(conn, job_id))
    }

    pub async fn set_subscription_issue_next_retry(
        &self,
        subscription_id: i64,
        query_id: i64,
        failure_kind: FailureKind,
        next_retry_at: String,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            set_subscription_issue_next_retry(
                conn,
                subscription_id,
                query_id,
                failure_kind,
                &next_retry_at,
            )
        })
    }

    pub async fn cancel_pending_subscription_jobs_for_subscription(
        &self,
        subscription_id: i64,
    ) -> Result<usize, String> {
        self.db.with_write(move |conn| {
            cancel_pending_subscription_jobs_for_subscription(conn, subscription_id)
        })
    }

    pub async fn count_active_subscription_query_jobs(
        &self,
        subscription_id: i64,
    ) -> Result<i64, String> {
        self.db
            .with_read(move |conn| count_active_subscription_query_jobs(conn, subscription_id))
    }

    pub async fn set_query_completed_initial_run(
        &self,
        query_id: i64,
        completed: bool,
    ) -> Result<(), String> {
        self.db
            .with_write(move |conn| set_query_completed_initial_run(conn, query_id, completed))
    }

    pub async fn set_query_terminal_state(
        &self,
        query_id: i64,
        last_success_at: Option<String>,
        last_failure_at: Option<String>,
        last_failure_kind: Option<String>,
        last_failure_message: Option<String>,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            set_query_terminal_state(
                conn,
                query_id,
                last_success_at.as_deref(),
                last_failure_at.as_deref(),
                last_failure_kind.as_deref(),
                last_failure_message.as_deref(),
            )
        })
    }

    pub async fn upsert_subscription_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
        message: &str,
        detail: Option<&str>,
    ) -> Result<Option<i64>, String> {
        let message = message.to_string();
        let detail = detail.map(ToOwned::to_owned);
        self.db.with_write(move |conn| {
            upsert_subscription_issue(
                conn,
                subscription_id,
                query_id,
                failure_kind,
                &message,
                detail.as_deref(),
            )
        })
    }

    pub async fn resolve_subscription_issues(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            resolve_subscription_issues(conn, subscription_id, query_id, failure_kind)
        })
    }

    pub async fn upsert_subscription_download_attempt(
        &self,
        input: OwnedSubscriptionDownloadAttemptUpsert,
    ) -> Result<i64, String> {
        self.db.with_write(move |conn| {
            upsert_subscription_download_attempt(
                conn,
                SubscriptionDownloadAttemptUpsert {
                    subscription_id: input.subscription_id,
                    query_id: input.query_id,
                    query_run_id: input.query_run_id,
                    item_key: &input.item_key,
                    site_category: input.site_category.as_deref(),
                    post_id: input.post_id.as_deref(),
                    page_num: input.page_num,
                    canonical_post_url: input.canonical_post_url.as_deref(),
                    media_url: input.media_url.as_deref(),
                    retry_url: input.retry_url.as_deref(),
                    failure_kind: input.failure_kind.as_deref(),
                    last_error: input.last_error.as_deref(),
                    next_retry_at: input.next_retry_at.as_deref(),
                },
            )
        })
    }

    pub async fn resolve_subscription_download_attempt(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        item_key: &str,
    ) -> Result<(), String> {
        let item_key = item_key.to_string();
        self.db.with_write(move |conn| {
            resolve_subscription_download_attempt(conn, subscription_id, query_id, &item_key)
        })
    }

    pub async fn mark_subscription_download_attempt_retrying(
        &self,
        attempt_id: i64,
    ) -> Result<(), String> {
        self.db
            .with_write(move |conn| mark_subscription_download_attempt_retrying(conn, attempt_id))
    }

    pub async fn list_retryable_subscription_download_attempts(
        &self,
        subscription_id: i64,
        query_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionDownloadAttemptRecord>, String> {
        self.db.with_read(move |conn| {
            list_retryable_subscription_download_attempts(conn, subscription_id, query_id, limit)
        })
    }

    pub async fn find_unresolved_subscription_post_attempts(
        &self,
        subscription_id: i64,
        query_id: i64,
        post_id: &str,
    ) -> Result<Vec<SubscriptionDownloadAttemptRecord>, String> {
        let post_id = post_id.to_string();
        self.db.with_read(move |conn| {
            super::runtime_db::find_unresolved_subscription_post_attempts(
                conn,
                subscription_id,
                query_id,
                &post_id,
            )
        })
    }

    pub async fn list_subscription_retry_targets(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<crate::subscriptions::types::SubscriptionRetryTarget>, String> {
        self.db
            .with_read(move |conn| list_subscription_retry_targets(conn, subscription_id))
    }

    pub async fn upsert_subscription_post_member(
        &self,
        input: OwnedSubscriptionPostMemberUpsert,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            upsert_subscription_post_member(
                conn,
                SubscriptionPostMemberUpsert {
                    subscription_id: input.subscription_id,
                    site_id: &input.site_id,
                    post_id: &input.post_id,
                    item_key: &input.item_key,
                    page_num: input.page_num,
                    canonical_post_url: input.canonical_post_url.as_deref(),
                    media_url: input.media_url.as_deref(),
                    entity_id: input.entity_id,
                    status: &input.status,
                },
            )
        })
    }

    pub async fn add_subscription_entity(
        &self,
        subscription_id: i64,
        entity_hash: &str,
    ) -> Result<bool, String> {
        link_subscription_entity(self.db, subscription_id, entity_hash).await
    }

    pub async fn list_subscription_runs(
        &self,
        subscription_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionRunRecord>, String> {
        self.db
            .with_read(|conn| list_subscription_runs(conn, subscription_id, limit))
    }

    pub async fn list_running_subscription_run_ids(&self) -> Result<Vec<i64>, String> {
        self.db.with_read(list_running_subscription_run_ids)
    }

    pub async fn list_subscription_query_runs(
        &self,
        query_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionQueryRunRecord>, String> {
        self.db
            .with_read(|conn| list_subscription_query_runs(conn, query_id, limit))
    }

    pub async fn list_subscription_issues(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SubscriptionIssueRecord>, String> {
        self.db
            .with_read(|conn| list_subscription_issues(conn, subscription_id, query_id, limit))
    }

    pub async fn list_subscription_issues_page(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<crate::subscriptions::types::SubscriptionIssuePage, String> {
        self.db.with_read(move |conn| {
            list_subscription_issues_page(conn, subscription_id, query_id, cursor, limit)
        })
    }

    pub async fn list_subscription_download_attempts_page(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<crate::subscriptions::types::SubscriptionDownloadAttemptPage, String> {
        self.db.with_read(move |conn| {
            list_subscription_download_attempts_page(conn, subscription_id, query_id, cursor, limit)
        })
    }

    pub async fn count_current_ingest_queue(
        &self,
        query_id: i64,
    ) -> Result<IngestQueueCounts, String> {
        self.db.with_read(|conn| {
            conn.query_row(
                "WITH current_query_run AS (
                     SELECT query_run_id, run_id, started_at
                     FROM subscription_query_run
                     WHERE query_id = ?1
                     ORDER BY query_run_id DESC
                     LIMIT 1
                 ), current_job AS (
                     SELECT job.queued_at
                     FROM subscription_query_job job
                     JOIN current_query_run current
                       ON job.query_id = ?1
                      AND job.run_id IS current.run_id
                      AND job.queued_at <= current.started_at
                     ORDER BY job.queued_at DESC, job.job_id DESC
                     LIMIT 1
                 ), target_query_runs AS (
                     SELECT qr.query_run_id
                     FROM subscription_query_run qr
                     JOIN current_query_run current
                       ON (current.run_id IS NOT NULL AND qr.run_id = current.run_id)
                       OR (
                           current.run_id IS NULL
                           AND qr.run_id IS NULL
                           AND qr.query_id = ?1
                           AND qr.started_at >= COALESCE(
                               (SELECT queued_at FROM current_job),
                               current.started_at
                           )
                       )
                     WHERE qr.query_id = ?1
                 )
                 SELECT
                     COALESCE(SUM(CASE WHEN i.status = 'pending' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'running' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'complete' AND i.result_kind = 'imported' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'complete' AND i.result_kind = 'reused' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'failed' THEN 1 ELSE 0 END), 0)
                 FROM ingest_queue_item i
                 JOIN ingest_queue q ON q.queue_id = i.queue_id
                 WHERE q.query_run_id IN (SELECT query_run_id FROM target_query_runs)",
                [query_id],
                |row| {
                    Ok(IngestQueueCounts {
                        queued: row.get::<_, i64>(0)? as usize,
                        ingesting: row.get::<_, i64>(1)? as usize,
                        ingested: row.get::<_, i64>(2)? as usize,
                        reused: row.get::<_, i64>(3)? as usize,
                        failed: row.get::<_, i64>(4)? as usize,
                    })
                },
            )
        })
    }

    pub async fn count_current_query_run_progress(
        &self,
        query_id: i64,
    ) -> Result<CurrentQueryRunProgress, String> {
        self.db.with_read(move |conn| {
            conn.query_row(
                "WITH current_query_run AS (
                     SELECT query_run_id, run_id, started_at
                     FROM subscription_query_run
                     WHERE query_id = ?1
                     ORDER BY query_run_id DESC
                     LIMIT 1
                 ), current_job AS (
                     SELECT job.queued_at
                     FROM subscription_query_job job
                     JOIN current_query_run current
                       ON job.query_id = ?1
                      AND job.run_id IS current.run_id
                      AND job.queued_at <= current.started_at
                     ORDER BY job.queued_at DESC, job.job_id DESC
                     LIMIT 1
                 ), target_query_runs AS (
                     SELECT qr.query_run_id
                     FROM subscription_query_run qr
                     JOIN current_query_run current
                       ON (current.run_id IS NOT NULL AND qr.run_id = current.run_id)
                       OR (
                           current.run_id IS NULL
                           AND qr.run_id IS NULL
                           AND qr.query_id = ?1
                           AND qr.started_at >= COALESCE(
                               (SELECT queued_at FROM current_job),
                               current.started_at
                           )
                       )
                     WHERE qr.query_id = ?1
                 )
                 SELECT COALESCE(SUM(qr.posts_processed), 0),
                        COALESCE(SUM(qr.files_downloaded), 0),
                        COALESCE(SUM(qr.files_skipped), 0),
                        COALESCE(SUM(qr.metadata_validated), 0),
                        COALESCE(SUM(qr.metadata_invalid), 0),
                        COALESCE(MAX(CASE WHEN qr.query_run_id = current.query_run_id THEN qr.posts_processed END), 0),
                        COALESCE(MAX(CASE WHEN qr.query_run_id = current.query_run_id THEN qr.files_downloaded END), 0),
                        COALESCE(MAX(CASE WHEN qr.query_run_id = current.query_run_id THEN qr.files_skipped END), 0),
                        COALESCE(MAX(CASE WHEN qr.query_run_id = current.query_run_id THEN qr.metadata_validated END), 0),
                        COALESCE(MAX(CASE WHEN qr.query_run_id = current.query_run_id THEN qr.metadata_invalid END), 0)
                 FROM subscription_query_run qr
                 CROSS JOIN current_query_run current
                 WHERE qr.query_run_id IN (SELECT query_run_id FROM target_query_runs)",
                [query_id],
                |row| {
                    Ok(CurrentQueryRunProgress {
                        posts_processed: row.get::<_, i64>(0)?.max(0) as usize,
                        files_downloaded: row.get::<_, i64>(1)?.max(0) as usize,
                        files_skipped: row.get::<_, i64>(2)?.max(0) as usize,
                        metadata_validated: row.get::<_, i64>(3)?.max(0) as usize,
                        metadata_invalid: row.get::<_, i64>(4)?.max(0) as usize,
                        current_posts_processed: row.get::<_, i64>(5)?.max(0) as usize,
                        current_files_downloaded: row.get::<_, i64>(6)?.max(0) as usize,
                        current_files_skipped: row.get::<_, i64>(7)?.max(0) as usize,
                        current_metadata_validated: row.get::<_, i64>(8)?.max(0) as usize,
                        current_metadata_invalid: row.get::<_, i64>(9)?.max(0) as usize,
                    })
                },
            )
        })
    }
}

fn subscription_info_from_row(
    sub: CanonicalSubscriptionRow,
    total_files: i64,
    queries: Vec<SubscriptionQueryInfo>,
) -> SubscriptionInfo {
    SubscriptionInfo {
        id: sub.subscription_id.to_string(),
        name: sub.name,
        schedule: sub.schedule,
        paused: sub.paused,
        initial_post_limit: sub.initial_post_limit as u32,
        periodic_post_limit: sub.periodic_post_limit as u32,
        created_at: sub.date_added,
        total_files: total_files as u64,
        queries,
    }
}

fn subscription_from_row(sub: CanonicalSubscriptionRow) -> Subscription {
    Subscription {
        subscription_id: sub.subscription_id,
        name: sub.name,
        schedule: sub.schedule,
        paused: sub.paused,
        initial_post_limit: sub.initial_post_limit,
        periodic_post_limit: sub.periodic_post_limit,
        created_at: sub.date_added,
    }
}

fn query_info_from_row(query: CanonicalQueryRow) -> SubscriptionQueryInfo {
    SubscriptionQueryInfo {
        id: query.query_id.to_string(),
        site_id: query.site_id,
        query_kind: query.query_kind,
        query_text: query.query_text.clone(),
        display_name: query.display_name.or(Some(query.query_text)),
        notes: query.notes,
        paused: query.paused,
        last_check_time: query.last_check_time,
        files_found: query.files_found as u64,
        posts_found: query.posts_found as u64,
        completed_initial_run: query.completed_initial_run,
        resume_cursor: query.resume_cursor,
        resume_strategy: query.resume_strategy,
        last_success_at: query.last_success_at,
        last_failure_at: query.last_failure_at,
        last_failure_kind: query.last_failure_kind,
        last_failure_message: query.last_failure_message,
    }
}

fn query_from_row(query: CanonicalQueryRow) -> SubscriptionQuery {
    SubscriptionQuery {
        query_id: query.query_id,
        subscription_id: query.subscription_id,
        site_id: query.site_id,
        query_kind: query.query_kind,
        query_text: query.query_text.clone(),
        display_name: query.display_name.or(Some(query.query_text)),
        notes: query.notes,
        paused: query.paused,
        last_check_time: query.last_check_time,
        files_found: query.files_found,
        posts_found: query.posts_found,
        completed_initial_run: query.completed_initial_run,
        resume_cursor: query.resume_cursor,
        resume_strategy: query.resume_strategy,
        last_success_at: query.last_success_at,
        last_failure_at: query.last_failure_at,
        last_failure_kind: query.last_failure_kind,
        last_failure_message: query.last_failure_message,
    }
}

fn list_subscriptions_with_counts_canonical(
    conn: &Connection,
) -> rusqlite::Result<Vec<(CanonicalSubscriptionRow, i64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             s.subscription_id,
             s.name,
             s.schedule,
             s.paused,
             s.initial_post_limit,
             s.periodic_post_limit,
             s.date_added,
             COALESCE(fc.cnt, 0)
         FROM subscription s
         LEFT JOIN (
             SELECT subscription_id, COUNT(*) AS cnt
             FROM subscription_entity
             GROUP BY subscription_id
         ) fc ON fc.subscription_id = s.subscription_id
         ORDER BY s.name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            CanonicalSubscriptionRow {
                subscription_id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                paused: row.get::<_, i64>(3)? != 0,
                initial_post_limit: row.get(4)?,
                periodic_post_limit: row.get(5)?,
                date_added: row.get(6)?,
            },
            row.get(7)?,
        ))
    })?;
    rows.collect()
}

fn get_subscription_canonical(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Option<CanonicalSubscriptionRow>> {
    conn.query_row(
        "SELECT
             subscription_id,
             name,
             schedule,
             paused,
             initial_post_limit,
             periodic_post_limit,
             date_added
         FROM subscription
         WHERE subscription_id = ?1",
        [subscription_id],
        |row| {
            Ok(CanonicalSubscriptionRow {
                subscription_id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                paused: row.get::<_, i64>(3)? != 0,
                initial_post_limit: row.get(4)?,
                periodic_post_limit: row.get(5)?,
                date_added: row.get(6)?,
            })
        },
    )
    .optional()
}

fn list_all_queries_canonical(conn: &Connection) -> rusqlite::Result<Vec<CanonicalQueryRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             query_id,
             subscription_id,
             site_id,
             query_kind,
             query_text,
             display_name,
             notes,
             paused,
             last_check_time,
             files_found,
             posts_found,
             completed_initial_run,
             resume_cursor,
             resume_strategy,
             last_success_at,
             last_failure_at,
             last_failure_kind,
             last_failure_message
         FROM subscription_query
         ORDER BY subscription_id, query_id",
    )?;
    let rows = stmt.query_map([], map_query_row_canonical)?;
    rows.collect()
}

fn list_subscription_queries_canonical(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<CanonicalQueryRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             query_id,
             subscription_id,
             site_id,
             query_kind,
             query_text,
             display_name,
             notes,
             paused,
             last_check_time,
             files_found,
             posts_found,
             completed_initial_run,
             resume_cursor,
             resume_strategy,
             last_success_at,
             last_failure_at,
             last_failure_kind,
             last_failure_message
         FROM subscription_query
         WHERE subscription_id = ?1
         ORDER BY query_id",
    )?;
    let rows = stmt.query_map([subscription_id], map_query_row_canonical)?;
    rows.collect()
}

fn get_subscription_query_canonical(
    conn: &Connection,
    query_id: i64,
) -> rusqlite::Result<Option<CanonicalQueryRow>> {
    conn.query_row(
        "SELECT
             query_id,
             subscription_id,
             site_id,
             query_kind,
             query_text,
             display_name,
             notes,
             paused,
             last_check_time,
             files_found,
             posts_found,
             completed_initial_run,
             resume_cursor,
             resume_strategy,
             last_success_at,
             last_failure_at,
             last_failure_kind,
             last_failure_message
         FROM subscription_query
         WHERE query_id = ?1",
        [query_id],
        map_query_row_canonical,
    )
    .optional()
}

fn map_query_row_canonical(row: &rusqlite::Row) -> rusqlite::Result<CanonicalQueryRow> {
    Ok(CanonicalQueryRow {
        query_id: row.get(0)?,
        subscription_id: row.get(1)?,
        site_id: row.get(2)?,
        query_kind: row.get(3)?,
        query_text: row.get(4)?,
        display_name: row.get(5)?,
        notes: row.get(6)?,
        paused: row.get::<_, i64>(7)? != 0,
        last_check_time: row.get(8)?,
        files_found: row.get(9)?,
        posts_found: row.get(10)?,
        completed_initial_run: row.get::<_, i64>(11)? != 0,
        resume_cursor: row.get(12)?,
        resume_strategy: row.get(13)?,
        last_success_at: row.get(14)?,
        last_failure_at: row.get(15)?,
        last_failure_kind: row.get(16)?,
        last_failure_message: row.get(17)?,
    })
}

fn validate_schedule(schedule: &str) -> Result<(), String> {
    match schedule {
        "manual" | "daily" | "weekly" | "monthly" => Ok(()),
        _ => Err(format!(
            "Invalid schedule: {}. Must be one of: manual, daily, weekly, monthly",
            schedule
        )),
    }
}
