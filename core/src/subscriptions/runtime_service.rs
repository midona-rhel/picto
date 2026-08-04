use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::LibraryDatabase;
use crate::ingest_queue::IngestQueueCounts;
use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::types::{
    RunningSubscriptions, SubscriptionGroupInfo, SubscriptionInfo, SubscriptionQueryInfo,
};

use super::archive::{
    clear_subscription_archive_entries_at_root, subscription_query_archive_prefix,
};
use super::runtime_db::{
    accumulate_subscription_run_counters, add_subscription_entity,
    cancel_pending_subscription_jobs_for_subscription, count_active_subscription_query_jobs,
    create_subscription_query_run, create_subscription_run, enqueue_subscription_query_job,
    finalize_subscription_run_status, find_unresolved_subscription_download_attempts,
    finish_subscription_query_job, finish_subscription_query_run, finish_subscription_run,
    get_subscription_post_collection, lease_subscription_query_job,
    list_queued_subscription_query_jobs, list_retryable_subscription_download_attempts,
    list_subscription_download_attempts, list_subscription_issues, list_subscription_post_members,
    list_subscription_query_jobs_for_run, list_subscription_query_runs, list_subscription_runs,
    mark_subscription_download_attempt_retrying, reset_subscription_query_state,
    reset_subscription_state, resolve_subscription_download_attempt, resolve_subscription_issues,
    set_query_completed_initial_run, set_query_resume_state, set_query_terminal_state,
    update_query_progress, upsert_subscription_download_attempt, upsert_subscription_issue,
    upsert_subscription_post_collection, upsert_subscription_post_member,
};
use super::types::{
    OwnedSubscriptionDownloadAttemptUpsert, OwnedSubscriptionPostMemberUpsert, Subscription,
    SubscriptionDownloadAttemptRecord, SubscriptionDownloadAttemptUpsert, SubscriptionGroup,
    SubscriptionIssueRecord, SubscriptionPostMemberRecord, SubscriptionPostMemberUpsert,
    SubscriptionQuery, SubscriptionQueryJob, SubscriptionQueryRunRecord, SubscriptionRunRecord,
};

#[derive(Debug, Clone)]
struct CanonicalSubscriptionRow {
    subscription_id: i64,
    name: String,
    paused: bool,
    group_id: Option<i64>,
    initial_post_limit: i64,
    periodic_post_limit: i64,
    auto_collections: bool,
    date_added: String,
}

#[derive(Debug, Clone)]
struct CanonicalGroupRow {
    group_id: i64,
    name: String,
    schedule: String,
    paused: bool,
    date_added: String,
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

#[derive(Debug, Clone)]
pub struct RunnableSubscription {
    pub subscription: Subscription,
    pub group_name: Option<String>,
    pub queries: Vec<SubscriptionQuery>,
}

#[derive(Debug, Clone)]
pub struct RunnableQuery {
    pub subscription: Subscription,
    pub group_name: Option<String>,
    pub query: SubscriptionQuery,
}

impl<'a> SubscriptionRuntimeService<'a> {
    pub fn new(db: &'a LibraryDatabase, library_root: &Path) -> Self {
        Self {
            db,
            library_root: library_root.to_path_buf(),
        }
    }

    pub async fn get_groups(&self) -> Result<Vec<SubscriptionGroupInfo>, String> {
        let groups = self.db.with_read(list_groups_canonical)?;
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

        let mut subs_by_group: HashMap<Option<i64>, Vec<SubscriptionInfo>> = HashMap::new();
        let mut totals_by_group: HashMap<i64, u64> = HashMap::new();
        for (sub, total_files) in subs {
            let subscription_id = sub.subscription_id;
            if let Some(group_id) = sub.group_id {
                totals_by_group
                    .entry(group_id)
                    .and_modify(|total| *total += total_files as u64)
                    .or_insert(total_files as u64);
            }
            subs_by_group
                .entry(sub.group_id)
                .or_default()
                .push(subscription_info_from_row(
                    sub,
                    total_files,
                    queries_map.remove(&subscription_id).unwrap_or_default(),
                ));
        }

        Ok(groups
            .into_iter()
            .map(|group| SubscriptionGroupInfo {
                id: group.group_id.to_string(),
                name: group.name,
                schedule: group.schedule,
                paused: group.paused,
                created_at: group.date_added,
                total_files: totals_by_group.get(&group.group_id).copied().unwrap_or(0),
                subscriptions: subs_by_group
                    .remove(&Some(group.group_id))
                    .unwrap_or_default(),
            })
            .collect())
    }

    pub async fn create_group(
        &self,
        name: String,
        schedule: Option<String>,
    ) -> Result<SubscriptionGroupInfo, String> {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Group name cannot be empty".to_string());
        }
        let schedule = schedule.unwrap_or_else(|| "manual".to_string());
        validate_schedule(&schedule)?;
        let now = chrono::Utc::now().to_rfc3339();
        let group_id = self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO subscription_group (name, schedule, uuid, date_added) VALUES (?1, ?2, ?3, ?4)",
                params![trimmed, schedule, crate::oplog::new_uuid(), now],
            )?;
            Ok(conn.last_insert_rowid())
        })?;

        Ok(SubscriptionGroupInfo {
            id: group_id.to_string(),
            name: trimmed,
            schedule,
            paused: false,
            created_at: now,
            total_files: 0,
            subscriptions: vec![],
        })
    }

    pub async fn delete_group(&self, id: String) -> Result<(), String> {
        let group_id: i64 = id.parse().map_err(|_| format!("Invalid group id: {id}"))?;
        self.db.with_write(|conn| {
            conn.execute(
                "DELETE FROM subscription_group WHERE group_id = ?1",
                [group_id],
            )?;
            Ok(())
        })
    }

    pub async fn rename_group(&self, id: String, name: String) -> Result<(), String> {
        let group_id: i64 = id.parse().map_err(|_| format!("Invalid group id: {id}"))?;
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription_group SET name = ?1 WHERE group_id = ?2",
                params![trimmed, group_id],
            )?;
            Ok(())
        })
    }

    pub async fn set_group_schedule(&self, id: String, schedule: String) -> Result<(), String> {
        let group_id: i64 = id.parse().map_err(|_| format!("Invalid group id: {id}"))?;
        validate_schedule(&schedule)?;
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription_group SET schedule = ?1 WHERE group_id = ?2",
                params![schedule, group_id],
            )?;
            Ok(())
        })
    }

    pub async fn set_group_paused(&self, id: String, paused: bool) -> Result<(), String> {
        let group_id: i64 = id.parse().map_err(|_| format!("Invalid group id: {id}"))?;
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription_group SET paused = ?1 WHERE group_id = ?2",
                params![paused as i64, group_id],
            )?;
            Ok(())
        })
    }

    pub async fn is_group_paused(&self, group_id: i64) -> Result<bool, String> {
        self.db.with_read(move |conn| {
            conn.query_row(
                "SELECT paused FROM subscription_group WHERE group_id = ?1",
                [group_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|value| value != 0)
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

    pub async fn create_subscription(
        &self,
        name: String,
        group_id: Option<i64>,
        initial_post_limit: Option<u32>,
        periodic_post_limit: Option<u32>,
    ) -> Result<SubscriptionInfo, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let subscription_id = self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO subscription (name, site_id, paused, group_id, initial_post_limit, periodic_post_limit, auto_collections, uuid, date_added)
                 VALUES (?1, '', 0, ?2, ?3, ?4, 1, ?5, ?6)",
                params![
                    name,
                    group_id,
                    initial_post_limit.unwrap_or(100) as i64,
                    periodic_post_limit.unwrap_or(50) as i64,
                    crate::oplog::new_uuid(),
                    now
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })?;

        Ok(SubscriptionInfo {
            id: subscription_id.to_string(),
            name,
            paused: false,
            group_id: group_id.map(|id| id.to_string()),
            initial_post_limit: initial_post_limit.unwrap_or(100),
            periodic_post_limit: periodic_post_limit.unwrap_or(50),
            auto_collections: true,
            created_at: now,
            total_files: 0,
            queries: vec![],
        })
    }

    pub async fn delete_subscription(&self, id: String) -> Result<usize, String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        self.db.with_write(|conn| {
            conn.execute(
                "DELETE FROM subscription WHERE subscription_id = ?1",
                [subscription_id],
            )
            .map_err(Into::into)
        })
    }

    pub async fn pause_subscription(&self, id: String, paused: bool) -> Result<(), String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription SET paused = ?1 WHERE subscription_id = ?2",
                params![paused as i64, subscription_id],
            )?;
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
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription SET name = ?1 WHERE subscription_id = ?2",
                params![trimmed, subscription_id],
            )?;
            Ok(())
        })
    }

    pub async fn set_subscription_auto_collections(
        &self,
        subscription_id: i64,
        auto_collections: bool,
    ) -> Result<(), String> {
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription SET auto_collections = ?1 WHERE subscription_id = ?2",
                params![auto_collections as i64, subscription_id],
            )?;
            Ok(())
        })
    }

    /// Move a subscription into a group (or out of every group with None).
    pub async fn set_subscription_group(
        &self,
        subscription_id: i64,
        group_id: Option<i64>,
    ) -> Result<(), String> {
        if let Some(gid) = group_id {
            let exists: bool = self.db.with_read(move |conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM subscription_group WHERE group_id = ?1)",
                    params![gid],
                    |row| row.get(0),
                )
            })?;
            if !exists {
                return Err(format!("Group {gid} not found"));
            }
        }
        self.db.with_write(move |conn| {
            conn.execute(
                "UPDATE subscription SET group_id = ?1 WHERE subscription_id = ?2",
                params![group_id, subscription_id],
            )?;
            Ok(())
        })
    }

    /// Collections this subscription has created from multi-image posts.
    pub async fn list_subscription_collections(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<crate::subscriptions::types::SubscriptionCollectionRecord>, String> {
        self.db.with_read(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT me.entity_hash, me.name, me.member_count, spc.site_id, spc.post_id
                 FROM subscription_post_collection spc
                 JOIN media_entity me ON me.entity_id = spc.collection_entity_id
                 WHERE spc.subscription_id = ?1
                 ORDER BY spc.date_modified DESC",
            )?;
            let rows = stmt.query_map(params![subscription_id], |row| {
                Ok(crate::subscriptions::types::SubscriptionCollectionRecord {
                    entity_hash: row.get(0)?,
                    name: row.get(1)?,
                    member_count: row.get(2)?,
                    site_id: row.get(3)?,
                    post_id: row.get(4)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Newest downloaded file per subscription — cover images for the
    /// Following grid. One row per subscription that has files.
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
        let canonical_site_id =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(&site_id).to_string();
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
        let query_id = self.db.with_write(|conn| {
            conn.execute(
                "INSERT INTO subscription_query (subscription_id, site_id, query_kind, query_text, display_name, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    subscription_id,
                    canonical_site_id,
                    resolved_query_kind,
                    normalized_query,
                    query_text.trim(),
                    notes
                ],
            )?;
            Ok(conn.last_insert_rowid())
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
        let canonical_site_id =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(&site_id).to_string();
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
        self.db.with_write(|conn| {
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
            Ok(())
        })
    }

    pub async fn delete_subscription_query(&self, id: String) -> Result<(), String> {
        let query_id: i64 = id.parse().map_err(|_| format!("Invalid query id: {id}"))?;
        self.db.with_write(|conn| {
            conn.execute(
                "DELETE FROM subscription_query WHERE query_id = ?1",
                [query_id],
            )?;
            Ok(())
        })
    }

    pub async fn pause_subscription_query(&self, id: String, paused: bool) -> Result<(), String> {
        let query_id: i64 = id.parse().map_err(|_| format!("Invalid query id: {id}"))?;
        self.db.with_write(|conn| {
            conn.execute(
                "UPDATE subscription_query SET paused = ?1 WHERE query_id = ?2",
                params![paused as i64, query_id],
            )?;
            Ok(())
        })
    }

    pub async fn reset_subscription_checked(
        &self,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        let subscription_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {id}"))?;
        {
            let map = running_subs.lock().await;
            if map.contains_key(&id) {
                return Err(format!(
                    "Subscription {subscription_id} is running; stop it before resetting"
                ));
            }
        }

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

    pub async fn reset_subscription_query_checked(
        &self,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        let query_id: i64 = id.parse().map_err(|_| format!("Invalid query id: {id}"))?;
        let query = self
            .db
            .with_read(|conn| get_subscription_query_canonical(conn, query_id))?
            .ok_or_else(|| format!("Query {id} not found"))?;
        {
            let map = running_subs.lock().await;
            if map.contains_key(&query.subscription_id.to_string()) {
                return Err(format!(
                    "Subscription {} is running; stop it before resetting query {}",
                    query.subscription_id, query_id
                ));
            }
        }

        let archive_prefix = subscription_query_archive_prefix(query.subscription_id, query_id);
        self.db
            .with_write(|conn| reset_subscription_query_state(conn, query_id).map(|_| ()))?;
        clear_subscription_archive_entries_at_root(self.library_root.as_path(), &[archive_prefix])
            .await?;
        Ok(())
    }

    pub async fn get_group(&self, group_id: i64) -> Result<Option<SubscriptionGroup>, String> {
        self.db.with_read(|conn| {
            get_group_canonical(conn, group_id).map(|row| row.map(group_from_row))
        })
    }

    pub async fn list_subscriptions_for_group(
        &self,
        group_id: i64,
    ) -> Result<Vec<Subscription>, String> {
        self.db.with_read(|conn| {
            list_subscriptions_for_group_canonical(conn, group_id).map(|rows| {
                rows.into_iter()
                    .map(subscription_from_row)
                    .collect::<Vec<_>>()
            })
        })
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
        let group_name = match subscription.group_id {
            Some(group_id) => self.get_group(group_id).await?.map(|group| group.name),
            None => None,
        };
        Ok(Some(RunnableSubscription {
            subscription,
            group_name,
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
            group_name: bundle.group_name,
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
            // Queries for removed sites (e.g. kemono) are left untouched — they
            // fail at run time with "Unknown site", which is the right error.
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

    pub async fn finish_subscription_run(
        &self,
        run_id: i64,
        status: &str,
        failure_kind: Option<String>,
        error_message: Option<String>,
        files_downloaded: i64,
        files_skipped: i64,
        metadata_validated: i64,
        metadata_invalid: i64,
    ) -> Result<(), String> {
        let status = status.to_string();
        self.db.with_write(move |conn| {
            finish_subscription_run(
                conn,
                run_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
                files_downloaded,
                files_skipped,
                metadata_validated,
                metadata_invalid,
            )
        })
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

    pub async fn accumulate_subscription_run_counters(
        &self,
        run_id: i64,
        files_downloaded_delta: i64,
        files_skipped_delta: i64,
        metadata_validated_delta: i64,
        metadata_invalid_delta: i64,
    ) -> Result<(), String> {
        self.db.with_write(move |conn| {
            accumulate_subscription_run_counters(
                conn,
                run_id,
                files_downloaded_delta,
                files_skipped_delta,
                metadata_validated_delta,
                metadata_invalid_delta,
            )
        })
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

    pub async fn finish_subscription_query_run(
        &self,
        query_run_id: i64,
        status: &str,
        failure_kind: Option<String>,
        error_message: Option<String>,
        posts_processed: i64,
        files_downloaded: i64,
        files_skipped: i64,
    ) -> Result<(), String> {
        let status = status.to_string();
        self.db.with_write(move |conn| {
            finish_subscription_query_run(
                conn,
                query_run_id,
                &status,
                failure_kind.as_deref(),
                error_message.as_deref(),
                posts_processed,
                files_downloaded,
                files_skipped,
            )
        })
    }

    pub async fn update_query_progress(
        &self,
        query_id: i64,
        last_check_time: &str,
        files_found: i64,
        posts_found: i64,
    ) -> Result<(), String> {
        let last_check_time = last_check_time.to_string();
        self.db.with_write(move |conn| {
            update_query_progress(conn, query_id, &last_check_time, files_found, posts_found)
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

    pub async fn find_unresolved_subscription_download_attempts(
        &self,
        subscription_id: i64,
        query_id: i64,
        site_category: &str,
        post_id: &str,
    ) -> Result<Vec<SubscriptionDownloadAttemptRecord>, String> {
        let site_category = site_category.to_string();
        let post_id = post_id.to_string();
        self.db.with_read(move |conn| {
            find_unresolved_subscription_download_attempts(
                conn,
                subscription_id,
                query_id,
                &site_category,
                &post_id,
            )
        })
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
                    entity_hash: input.entity_hash.as_deref(),
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
        let entity_hash = entity_hash.to_string();
        self.db.with_write(move |conn| {
            let entity_id: i64 = conn.query_row(
                "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
                [&entity_hash],
                |row| row.get(0),
            )?;
            add_subscription_entity(conn, subscription_id, entity_id)
        })
    }

    pub async fn upsert_subscription_post_collection(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
        collection_entity_id: i64,
    ) -> Result<(), String> {
        let site_id = site_id.to_string();
        let post_id = post_id.to_string();
        self.db.with_write(move |conn| {
            upsert_subscription_post_collection(
                conn,
                subscription_id,
                &site_id,
                &post_id,
                collection_entity_id,
            )
        })
    }

    pub async fn list_subscription_post_members(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
    ) -> Result<Vec<SubscriptionPostMemberRecord>, String> {
        let site_id = site_id.to_string();
        let post_id = post_id.to_string();
        self.db.with_read(move |conn| {
            list_subscription_post_members(conn, subscription_id, &site_id, &post_id)
        })
    }

    pub async fn get_subscription_post_collection(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
    ) -> Result<Option<i64>, String> {
        let site_id = site_id.to_string();
        let post_id = post_id.to_string();
        self.db.with_read(move |conn| {
            get_subscription_post_collection(conn, subscription_id, &site_id, &post_id)
        })
    }

    pub async fn list_subscription_runs(
        &self,
        subscription_id: i64,
        limit: i64,
    ) -> Result<Vec<SubscriptionRunRecord>, String> {
        self.db
            .with_read(|conn| list_subscription_runs(conn, subscription_id, limit))
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

    pub async fn list_subscription_download_attempts(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SubscriptionDownloadAttemptRecord>, String> {
        self.db.with_read(|conn| {
            list_subscription_download_attempts(conn, subscription_id, query_id, limit)
        })
    }

    pub async fn count_subscription_ingest_queue(
        &self,
        subscription_id: i64,
    ) -> Result<IngestQueueCounts, String> {
        self.db.with_read(|conn| {
            conn.query_row(
                "SELECT
                     COALESCE(SUM(CASE WHEN i.status = 'pending' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'running' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'complete' AND i.result_kind = 'imported' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'complete' AND i.result_kind = 'reused' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN i.status = 'failed' THEN 1 ELSE 0 END), 0)
                 FROM ingest_queue_item i
                 JOIN ingest_queue q ON q.queue_id = i.queue_id
                 WHERE q.subscription_id = ?1",
                [subscription_id],
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
}

fn subscription_info_from_row(
    sub: CanonicalSubscriptionRow,
    total_files: i64,
    queries: Vec<SubscriptionQueryInfo>,
) -> SubscriptionInfo {
    SubscriptionInfo {
        id: sub.subscription_id.to_string(),
        name: sub.name,
        paused: sub.paused,
        group_id: sub.group_id.map(|id| id.to_string()),
        initial_post_limit: sub.initial_post_limit as u32,
        periodic_post_limit: sub.periodic_post_limit as u32,
        auto_collections: sub.auto_collections,
        created_at: sub.date_added,
        total_files: total_files as u64,
        queries,
    }
}

fn subscription_from_row(sub: CanonicalSubscriptionRow) -> Subscription {
    Subscription {
        subscription_id: sub.subscription_id,
        name: sub.name,
        paused: sub.paused,
        group_id: sub.group_id,
        initial_post_limit: sub.initial_post_limit,
        periodic_post_limit: sub.periodic_post_limit,
        auto_collections: sub.auto_collections,
        created_at: sub.date_added,
    }
}

fn query_info_from_row(query: CanonicalQueryRow) -> SubscriptionQueryInfo {
    SubscriptionQueryInfo {
        id: query.query_id.to_string(),
        site_id: crate::subscriptions::gallery_dl_runner::canonical_site_id(&query.site_id)
            .to_string(),
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
        site_id: crate::subscriptions::gallery_dl_runner::canonical_site_id(&query.site_id)
            .to_string(),
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

fn group_from_row(group: CanonicalGroupRow) -> SubscriptionGroup {
    SubscriptionGroup {
        group_id: group.group_id,
        name: group.name,
        schedule: group.schedule,
        created_at: group.date_added,
    }
}

fn list_groups_canonical(conn: &Connection) -> rusqlite::Result<Vec<CanonicalGroupRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT group_id, name, schedule, paused, date_added
         FROM subscription_group
         ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CanonicalGroupRow {
            group_id: row.get(0)?,
            name: row.get(1)?,
            schedule: row.get(2)?,
            paused: row.get::<_, i64>(3)? != 0,
            date_added: row.get(4)?,
        })
    })?;
    rows.collect()
}

fn list_subscriptions_with_counts_canonical(
    conn: &Connection,
) -> rusqlite::Result<Vec<(CanonicalSubscriptionRow, i64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             s.subscription_id,
             s.name,
             s.paused,
             s.group_id,
             s.initial_post_limit,
             s.periodic_post_limit,
             s.auto_collections,
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
                paused: row.get::<_, i64>(2)? != 0,
                group_id: row.get(3)?,
                initial_post_limit: row.get(4)?,
                periodic_post_limit: row.get(5)?,
                auto_collections: row.get::<_, i64>(6)? != 0,
                date_added: row.get(7)?,
            },
            row.get(8)?,
        ))
    })?;
    rows.collect()
}

fn list_subscriptions_for_group_canonical(
    conn: &Connection,
    group_id: i64,
) -> rusqlite::Result<Vec<CanonicalSubscriptionRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             subscription_id,
             name,
             paused,
             group_id,
             initial_post_limit,
             periodic_post_limit,
             auto_collections,
             date_added
         FROM subscription
         WHERE group_id = ?1
         ORDER BY name",
    )?;
    let rows = stmt.query_map([group_id], |row| {
        Ok(CanonicalSubscriptionRow {
            subscription_id: row.get(0)?,
            name: row.get(1)?,
            paused: row.get::<_, i64>(2)? != 0,
            group_id: row.get(3)?,
            initial_post_limit: row.get(4)?,
            periodic_post_limit: row.get(5)?,
            auto_collections: row.get::<_, i64>(6)? != 0,
            date_added: row.get(7)?,
        })
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
             paused,
             group_id,
             initial_post_limit,
             periodic_post_limit,
             auto_collections,
             date_added
         FROM subscription
         WHERE subscription_id = ?1",
        [subscription_id],
        |row| {
            Ok(CanonicalSubscriptionRow {
                subscription_id: row.get(0)?,
                name: row.get(1)?,
                paused: row.get::<_, i64>(2)? != 0,
                group_id: row.get(3)?,
                initial_post_limit: row.get(4)?,
                periodic_post_limit: row.get(5)?,
                auto_collections: row.get::<_, i64>(6)? != 0,
                date_added: row.get(7)?,
            })
        },
    )
    .optional()
}

fn get_group_canonical(
    conn: &Connection,
    group_id: i64,
) -> rusqlite::Result<Option<CanonicalGroupRow>> {
    conn.query_row(
        "SELECT group_id, name, schedule, paused, date_added
         FROM subscription_group
         WHERE group_id = ?1",
        [group_id],
        |row| {
            Ok(CanonicalGroupRow {
                group_id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                paused: row.get::<_, i64>(3)? != 0,
                date_added: row.get(4)?,
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
