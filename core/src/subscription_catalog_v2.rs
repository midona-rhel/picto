//! Product-facing subscription definitions and persisted status reads.

use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};
use crate::subscriptions::gallery_dl_runner::{build_url, site_by_id};
use crate::subscriptions::source_adapter::{
    infer_query_kind, normalize_query_text, validate_query_text,
};
use crate::subscriptions_v2::{self, CreatedRun};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct NewSubscriptionQuery {
    pub site_id: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub group_posts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct NewSubscription {
    pub name: String,
    pub schedule: String,
    #[ts(type = "number | null")]
    pub initial_post_limit: Option<i64>,
    #[ts(type = "number | null")]
    pub periodic_post_limit: Option<i64>,
    pub queries: Vec<NewSubscriptionQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionQueryView {
    #[ts(type = "number")]
    pub query_id: i64,
    pub site_id: String,
    pub query_kind: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub group_posts: bool,
    pub paused: bool,
    pub initial_run_complete: bool,
    pub source_history_complete: bool,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_message: Option<String>,
    #[ts(type = "number")]
    pub post_count: i64,
    #[ts(type = "number")]
    pub media_count: i64,
    #[ts(type = "number")]
    pub successful_run_count: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionProgress {
    #[ts(type = "number")]
    pub posts_traversed: i64,
    #[ts(type = "number")]
    pub posts_added: i64,
    #[ts(type = "number")]
    pub discovered: i64,
    #[ts(type = "number")]
    pub downloaded: i64,
    #[ts(type = "number")]
    pub ingested: i64,
    #[ts(type = "number")]
    pub failed: i64,
    #[ts(type = "number")]
    pub deleted: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionDestinationPolicy {
    #[serde(default)]
    #[ts(type = "number[]")]
    pub target_folder_ids: Vec<i64>,
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub target_folder_id: Option<i64>,
    pub automatic_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionView {
    #[ts(type = "number")]
    pub subscription_id: i64,
    pub name: String,
    pub schedule: String,
    pub paused: bool,
    #[ts(type = "number | null")]
    pub initial_post_limit: Option<i64>,
    #[ts(type = "number | null")]
    pub periodic_post_limit: Option<i64>,
    pub next_run_at: Option<String>,
    pub status: Option<String>,
    #[ts(type = "number | null")]
    pub active_run_id: Option<i64>,
    #[ts(type = "number")]
    pub media_count: i64,
    #[ts(type = "number")]
    pub open_issue_count: i64,
    pub cover_file_hash: Option<String>,
    #[ts(type = "number")]
    pub cover_focus_x: i64,
    #[ts(type = "number")]
    pub cover_focus_y: i64,
    #[ts(type = "number")]
    pub cover_zoom_percent: i64,
    pub progress: SubscriptionProgress,
    pub destination: SubscriptionDestinationPolicy,
    pub queries: Vec<SubscriptionQueryView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionCoverCandidate {
    #[ts(type = "number")]
    pub media_item_id: i64,
    pub file_hash: String,
    pub name: Option<String>,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionCoverCandidateCursor {
    pub imported_at: String,
    #[ts(type = "number")]
    pub media_item_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionCoverCandidatePage {
    pub candidates: Vec<SubscriptionCoverCandidate>,
    pub next_cursor: Option<SubscriptionCoverCandidateCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionCoverSelection {
    #[ts(type = "number")]
    pub media_item_id: i64,
    #[ts(type = "number")]
    pub focus_x: i64,
    #[ts(type = "number")]
    pub focus_y: i64,
    #[ts(type = "number")]
    pub zoom_percent: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionList {
    pub subscriptions: Vec<SubscriptionView>,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn list(application: &Application) -> Result<SubscriptionList, String> {
    application.store().read_snapshot(|connection| {
        let rows = connection
            .prepare(
                "SELECT s.subscription_id, s.name, s.schedule, s.paused,
                        s.initial_post_limit, s.periodic_post_limit, s.next_run_at,
                        active.run_id, COALESCE(active.status, latest.status),
                        COUNT(DISTINCT CASE WHEN si.state = 'ingested' THEN si.media_item_id END),
                        (SELECT COUNT(*) FROM subscription_issue issue
                         WHERE issue.subscription_id = s.subscription_id
                           AND issue.status = 'open'),
                        (
                            SELECT mf.file_hash
                            FROM subscription_source_post cover_ssp
                            JOIN source_item cover_si
                              ON cover_si.source_post_id = cover_ssp.source_post_id
                            JOIN media_asset cover_ma
                              ON cover_ma.item_id = cover_si.media_item_id
                            JOIN media_file mf ON mf.file_id = cover_ma.file_id
                            LEFT JOIN collection_member cover_cm
                              ON cover_cm.media_item_id = cover_ma.item_id
                            JOIN library_root cover_root
                              ON cover_root.item_id = COALESCE(cover_cm.collection_id, cover_ma.item_id)
                            WHERE cover_ssp.subscription_id = s.subscription_id
                              AND cover_si.state = 'ingested'
                              AND cover_root.lifecycle = 'active'
                              AND mf.mime_type LIKE 'image/%'
                            ORDER BY cover_ma.imported_at DESC, cover_ma.item_id DESC
                            LIMIT 1
                        )
                 FROM subscription s
                 LEFT JOIN subscription_source_post ssp
                   ON ssp.subscription_id = s.subscription_id
                 LEFT JOIN source_item si ON si.source_post_id = ssp.source_post_id
                 LEFT JOIN subscription_run active
                   ON active.subscription_id = s.subscription_id
                  AND active.status IN ('pending', 'running')
                 LEFT JOIN subscription_run latest ON latest.run_id = (
                     SELECT sr.run_id FROM subscription_run sr
                     WHERE sr.subscription_id = s.subscription_id
                     ORDER BY sr.run_id DESC LIMIT 1
                 )
                 GROUP BY s.subscription_id, active.run_id, active.status, latest.status
                 ORDER BY s.name, s.subscription_id",
            )?
            .query_map([], |row| {
                Ok(SubscriptionView {
                    subscription_id: row.get(0)?,
                    name: row.get(1)?,
                    schedule: row.get(2)?,
                    paused: row.get(3)?,
                    initial_post_limit: row.get(4)?,
                    periodic_post_limit: row.get(5)?,
                    next_run_at: row.get(6)?,
                    active_run_id: row.get(7)?,
                    status: row.get(8)?,
                    media_count: row.get(9)?,
                    open_issue_count: row.get(10)?,
                    cover_file_hash: row.get(11)?,
                    cover_focus_x: 500,
                    cover_focus_y: 500,
                    cover_zoom_percent: 100,
                    progress: SubscriptionProgress::default(),
                    destination: SubscriptionDestinationPolicy::default(),
                    queries: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut subscriptions = Vec::with_capacity(rows.len());
        for mut subscription in rows {
            if let Some((selection, file_hash)) = subscription_cover_from_connection(
                connection,
                subscription.subscription_id,
            )? {
                subscription.cover_file_hash = Some(file_hash);
                subscription.cover_focus_x = selection.focus_x;
                subscription.cover_focus_y = selection.focus_y;
                subscription.cover_zoom_percent = selection.zoom_percent;
            }
            subscription.destination =
                subscription_destination_from_connection(connection, subscription.subscription_id)?;
            subscription.queries = query_views(connection, subscription.subscription_id)?;
            if let Some(run_id) = subscription.active_run_id {
                subscription.progress = connection.query_row(
                    "SELECT
                         (SELECT COUNT(DISTINCT seen.source_post_id)
                          FROM subscription_source_post seen
                          JOIN subscription_run_query seen_query
                            ON seen_query.query_id = seen.query_id
                          WHERE seen_query.run_id = ?1
                            AND seen.last_seen_run_id = ?1),
                         COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                             THEN si.source_post_id END),
                         COUNT(DISTINCT rsi.source_item_id),
                         COALESCE(SUM(CASE WHEN si.state IN ('downloaded', 'ingested') THEN 1 ELSE 0 END), 0),
                         COALESCE(SUM(CASE WHEN si.state = 'ingested' THEN 1 ELSE 0 END), 0),
                         COALESCE(SUM(CASE WHEN si.state = 'failed' THEN 1 ELSE 0 END), 0),
                         COALESCE(SUM(CASE WHEN si.state = 'deleted' THEN 1 ELSE 0 END), 0)
                     FROM subscription_run_query srq
                     LEFT JOIN subscription_run_source_item rsi
                       ON rsi.run_query_id = srq.run_query_id
                     LEFT JOIN source_item si ON si.source_item_id = rsi.source_item_id
                     WHERE srq.run_id = ?1",
                    [run_id],
                    |row| {
                        Ok(SubscriptionProgress {
                            posts_traversed: row.get(0)?,
                            posts_added: row.get(1)?,
                            discovered: row.get(2)?,
                            downloaded: row.get(3)?,
                            ingested: row.get(4)?,
                            failed: row.get(5)?,
                            deleted: row.get(6)?,
                        })
                    },
                )?;
            }
            subscriptions.push(subscription);
        }
        Ok(SubscriptionList {
            subscriptions,
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

impl Application {
    pub fn set_subscription_cover(
        &self,
        subscription_id: i64,
        selection: &SubscriptionCoverSelection,
    ) -> Result<MutationReceipt, String> {
        validate_cover_selection(selection)?;
        let value = serde_json::to_string(selection).map_err(|error| error.to_string())?;
        let key = cover_setting_key(subscription_id);
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            require_subscription(transaction, subscription_id)?;
            require_subscription_cover_candidate(
                transaction,
                subscription_id,
                selection.media_item_id,
            )?;
            let previous: Option<String> = transaction
                .query_row(
                    "SELECT value_json FROM setting WHERE key = ?1",
                    [&key],
                    |row| row.get(0),
                )
                .optional()?;
            if previous.as_deref() == Some(value.as_str()) {
                return Ok(((), false));
            }
            transaction.execute(
                "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                params![key, value],
            )?;
            Ok(((), true))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn set_subscription_destination(
        &self,
        subscription_id: i64,
        policy: &SubscriptionDestinationPolicy,
    ) -> Result<MutationReceipt, String> {
        let policy = normalize_destination_policy(policy)?;
        let value = serde_json::to_string(&policy).map_err(|error| error.to_string())?;
        let key = destination_setting_key(subscription_id);
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            require_subscription(transaction, subscription_id)?;
            for &folder_id in &policy.target_folder_ids {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM folder WHERE folder_id = ?1)",
                    [folder_id],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(invalid("destination folder does not exist"));
                }
            }
            let previous: Option<String> = transaction
                .query_row(
                    "SELECT value_json FROM setting WHERE key = ?1",
                    [&key],
                    |row| row.get(0),
                )
                .optional()?;
            if previous.as_deref() == Some(value.as_str()) {
                return Ok(((), false));
            }
            transaction.execute(
                "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                params![key, value],
            )?;
            Ok(((), true))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn create_subscription_definition(
        &self,
        input: &NewSubscription,
        now: &str,
    ) -> Result<(i64, MutationReceipt), String> {
        validate_subscription(input)?;
        let queries = input
            .queries
            .iter()
            .map(prepare_query)
            .collect::<Result<Vec<_>, _>>()?;
        let next_run_at = subscriptions_v2::next_schedule_at(&input.schedule, now)?;
        let (subscription_id, revision) = self.store().transaction(|transaction| {
            transaction.execute(
                "INSERT INTO subscription (
                     subscription_key, name, schedule, paused, initial_post_limit,
                     periodic_post_limit, next_run_at, created_at
                 ) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
                params![
                    new_key("subscription"),
                    input.name.trim(),
                    input.schedule,
                    input.initial_post_limit,
                    input.periodic_post_limit,
                    next_run_at,
                    now,
                ],
            )?;
            let subscription_id = transaction.last_insert_rowid();
            let mut query_ids = Vec::with_capacity(queries.len());
            for query in &queries {
                query_ids.push(insert_query(transaction, subscription_id, query)?);
            }
            crate::cloud::capture::record_subscription_created(
                transaction,
                subscription_id,
                &query_ids,
            )?;
            Ok(subscription_id)
        })?;
        Ok((subscription_id, subscription_receipt(revision)))
    }

    pub fn add_subscription_query(
        &self,
        subscription_id: i64,
        query: &NewSubscriptionQuery,
    ) -> Result<(i64, MutationReceipt), String> {
        let query = prepare_query(query)?;
        let (query_id, revision) = self.store().transaction(|transaction| {
            require_subscription(transaction, subscription_id)?;
            let query_id = insert_query(transaction, subscription_id, &query)?;
            crate::cloud::capture::record_subscription_query_upsert(
                transaction,
                query_id,
                &[
                    "subscription",
                    "site_id",
                    "domain_key",
                    "query_kind",
                    "query_text",
                    "display_name",
                    "notes",
                    "group_posts",
                    "paused",
                ],
            )?;
            Ok(query_id)
        })?;
        Ok((query_id, subscription_receipt(revision)))
    }

    pub fn update_subscription_query(
        &self,
        query_id: i64,
        query: &NewSubscriptionQuery,
    ) -> Result<MutationReceipt, String> {
        let query = prepare_query(query)?;
        let (_, revision) = self.store().transaction(|transaction| {
            reject_active_query_edit(transaction, query_id)?;
            let changed = transaction.execute(
                "UPDATE subscription_query
                 SET site_id = ?1, domain_key = ?2, query_kind = ?3, query_text = ?4,
                     display_name = ?5, notes = ?6, resume_cursor = NULL,
                     initial_run_complete = 0, last_failure_at = NULL,
                     last_failure_kind = NULL, last_failure_message = NULL
                 WHERE query_id = ?7",
                params![
                    query.site_id,
                    query.domain_key,
                    query.query_kind,
                    query.query_text,
                    query.display_name,
                    query.notes,
                    query_id,
                ],
            )?;
            if changed != 1 {
                return Err(invalid("subscription query does not exist"));
            }
            crate::cloud::capture::record_subscription_query_upsert(
                transaction,
                query_id,
                &[
                    "site_id",
                    "domain_key",
                    "query_kind",
                    "query_text",
                    "display_name",
                    "notes",
                ],
            )?;
            Ok(())
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn pause_subscription_query(
        &self,
        query_id: i64,
        paused: bool,
    ) -> Result<MutationReceipt, String> {
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            let changed = transaction.execute(
                "UPDATE subscription_query SET paused = ?1
                 WHERE query_id = ?2 AND paused != ?1",
                params![paused, query_id],
            )?;
            if changed == 0 {
                require_query(transaction, query_id)?;
            } else {
                crate::cloud::capture::record_subscription_query_upsert(
                    transaction,
                    query_id,
                    &["paused"],
                )?;
            }
            Ok(((), changed != 0))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn set_subscription_query_grouping(
        &self,
        query_id: i64,
        group_posts: bool,
    ) -> Result<MutationReceipt, String> {
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            reject_active_query_edit(transaction, query_id)?;
            let changed = transaction.execute(
                "UPDATE subscription_query SET group_posts = ?1
                 WHERE query_id = ?2 AND group_posts != ?1",
                params![group_posts, query_id],
            )?;
            if changed == 0 {
                require_query(transaction, query_id)?;
            } else {
                crate::cloud::capture::record_subscription_query_upsert(
                    transaction,
                    query_id,
                    &["group_posts"],
                )?;
            }
            Ok(((), changed != 0))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn delete_subscription_query(&self, query_id: i64) -> Result<MutationReceipt, String> {
        let (_, revision) = self.store().transaction(|transaction| {
            reject_active_query_edit(transaction, query_id)?;
            let query_key: String = transaction.query_row(
                "SELECT query_key FROM subscription_query WHERE query_id = ?1",
                [query_id],
                |row| row.get(0),
            )?;
            let changed = transaction.execute(
                "DELETE FROM subscription_query WHERE query_id = ?1",
                [query_id],
            )?;
            if changed != 1 {
                return Err(invalid("subscription query does not exist"));
            }
            crate::cloud::capture::record_subscription_query_delete(transaction, query_key)?;
            Ok(())
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn rename_subscription(
        &self,
        subscription_id: i64,
        name: &str,
    ) -> Result<MutationReceipt, String> {
        if name.trim().is_empty() {
            return Err("Subscription name is required".to_string());
        }
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            let previous: String = transaction
                .query_row(
                    "SELECT name FROM subscription WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| invalid("subscription does not exist"))?;
            let name = name.trim();
            if previous == name {
                return Ok(((), false));
            }
            transaction.execute(
                "UPDATE subscription SET name = ?1 WHERE subscription_id = ?2",
                params![name, subscription_id],
            )?;
            crate::cloud::capture::record_subscription_upsert(
                transaction,
                subscription_id,
                &["name"],
            )?;
            Ok(((), true))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn pause_subscription(
        &self,
        subscription_id: i64,
        paused: bool,
    ) -> Result<MutationReceipt, String> {
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            require_subscription(transaction, subscription_id)?;
            let changed = transaction.execute(
                "UPDATE subscription SET paused = ?1
                 WHERE subscription_id = ?2 AND paused != ?1",
                params![paused, subscription_id],
            )?;
            if changed != 0 {
                crate::cloud::capture::record_subscription_upsert(
                    transaction,
                    subscription_id,
                    &["paused"],
                )?;
            }
            Ok(((), changed != 0))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn set_subscription_schedule(
        &self,
        subscription_id: i64,
        schedule: &str,
        now: &str,
    ) -> Result<MutationReceipt, String> {
        let next_run_at = subscriptions_v2::next_schedule_at(schedule, now)?;
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            let previous: (String, Option<String>) = transaction
                .query_row(
                    "SELECT schedule, next_run_at FROM subscription WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| invalid("subscription does not exist"))?;
            if previous == (schedule.to_string(), next_run_at.clone()) {
                return Ok(((), false));
            }
            transaction.execute(
                "UPDATE subscription SET schedule = ?1, next_run_at = ?2
                 WHERE subscription_id = ?3",
                params![schedule, next_run_at, subscription_id],
            )?;
            crate::cloud::capture::record_subscription_upsert(
                transaction,
                subscription_id,
                &["schedule"],
            )?;
            Ok(((), true))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn set_subscription_posts_per_run(
        &self,
        subscription_id: i64,
        posts_per_run: i64,
    ) -> Result<MutationReceipt, String> {
        if !(1..=10_000).contains(&posts_per_run) {
            return Err("Posts per run must be between 1 and 10,000".to_string());
        }
        let (_, revision, _) = self.store().transaction_if_changed(|transaction| {
            reject_active_subscription_edit(transaction, subscription_id)?;
            let changed = transaction.execute(
                "UPDATE subscription
                 SET initial_post_limit = ?1, periodic_post_limit = ?1
                 WHERE subscription_id = ?2
                   AND (initial_post_limit IS NOT ?1 OR periodic_post_limit IS NOT ?1)",
                params![posts_per_run, subscription_id],
            )?;
            if changed == 0 {
                require_subscription(transaction, subscription_id)?;
            } else {
                crate::cloud::capture::record_subscription_upsert(
                    transaction,
                    subscription_id,
                    &["initial_post_limit", "periodic_post_limit"],
                )?;
            }
            Ok(((), changed != 0))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn delete_subscription(&self, subscription_id: i64) -> Result<MutationReceipt, String> {
        self.store()
            .read(|connection| require_subscription(connection, subscription_id))?;
        crate::onlyfans_source_v2::clear_subscription_state(
            self.store().library_root(),
            subscription_id,
        )?;
        let (_, revision) = self.store().transaction(|transaction| {
            let subscription_key: String = transaction.query_row(
                "SELECT subscription_key FROM subscription WHERE subscription_id = ?1",
                [subscription_id],
                |row| row.get(0),
            )?;
            let changed = transaction.execute(
                "DELETE FROM subscription WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            if changed != 1 {
                return Err(invalid("subscription does not exist"));
            }
            transaction.execute(
                "DELETE FROM setting WHERE key = ?1",
                [destination_setting_key(subscription_id)],
            )?;
            transaction.execute(
                "DELETE FROM setting WHERE key = ?1",
                [cover_setting_key(subscription_id)],
            )?;
            crate::cloud::capture::record_subscription_delete(transaction, subscription_key)?;
            Ok(())
        })?;
        Ok(subscription_receipt(revision))
    }

    pub async fn reset_subscription(
        &self,
        subscription_id: i64,
    ) -> Result<MutationReceipt, String> {
        self.store().read(|connection| {
            require_subscription(connection, subscription_id)?;
            reject_active_subscription_edit(connection, subscription_id)
        })?;

        crate::subscriptions::archive::clear_subscription_archive_entries_at_root(
            self.store().library_root(),
            subscription_id,
        )
        .await?;
        crate::onlyfans_source_v2::clear_subscription_state(
            self.store().library_root(),
            subscription_id,
        )?;

        let (_, revision) = self.store().transaction(|transaction| {
            require_subscription(transaction, subscription_id)?;
            reject_active_subscription_edit(transaction, subscription_id)?;
            // Reset is the explicit user override for source tombstones. A
            // normal retry can never resurrect deliberately deleted media.
            transaction.execute(
                "DELETE FROM ingest_job
                 WHERE source_item_id IN (
                     SELECT si.source_item_id
                     FROM subscription_source_post ssp
                     CROSS JOIN source_item si ON si.source_post_id = ssp.source_post_id
                     WHERE ssp.subscription_id = ?1 AND si.media_item_id IS NULL
                     UNION
                     SELECT rsi.source_item_id
                     FROM subscription_run_source_item rsi
                     JOIN subscription_run_query srq
                       ON srq.run_query_id = rsi.run_query_id
                     JOIN subscription_run sr ON sr.run_id = srq.run_id
                     JOIN source_item si ON si.source_item_id = rsi.source_item_id
                     WHERE sr.subscription_id = ?1 AND si.media_item_id IS NULL
                 )",
                [subscription_id],
            )?;
            transaction.execute(
                "UPDATE source_item
                 SET state = 'pending', last_error = NULL, updated_at = datetime('now')
                 WHERE media_item_id IS NULL AND source_item_id IN (
                     SELECT si.source_item_id
                     FROM subscription_source_post ssp
                     JOIN source_item si ON si.source_post_id = ssp.source_post_id
                     WHERE ssp.subscription_id = ?1
                     UNION
                     SELECT rsi.source_item_id
                     FROM subscription_run_source_item rsi
                     JOIN subscription_run_query srq
                       ON srq.run_query_id = rsi.run_query_id
                     JOIN subscription_run sr ON sr.run_id = srq.run_id
                     WHERE sr.subscription_id = ?1
                 )",
                [subscription_id],
            )?;
            crate::cloud::capture::record_subscription_source_item_restores(
                transaction,
                subscription_id,
            )?;
            transaction.execute(
                "UPDATE subscription_query
                 SET resume_cursor = NULL, initial_run_complete = 0,
                     last_success_at = NULL, last_failure_at = NULL,
                     last_failure_kind = NULL, last_failure_message = NULL
                 WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            transaction.execute(
                "DELETE FROM subscription_issue WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            crate::cloud::capture::record_subscription_source_posts_removed(
                transaction,
                subscription_id,
            )?;
            transaction.execute(
                "DELETE FROM subscription_source_post WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            transaction.execute(
                "DELETE FROM subscription_run WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            Ok(())
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn request_subscription_run(
        &self,
        subscription_id: i64,
        now: &str,
    ) -> Result<(CreatedRun, MutationReceipt), String> {
        let (run, revision) = subscriptions_v2::create_run_with_revision(
            self.store(),
            subscription_id,
            "manual",
            now,
        )?;
        Ok((run, subscription_receipt(revision)))
    }

    pub fn cancel_subscription_run(
        &self,
        subscription_id: i64,
        now: &str,
    ) -> Result<MutationReceipt, String> {
        let revision =
            subscriptions_v2::cancel_subscription_run(self.store(), subscription_id, now)?;
        Ok(subscription_receipt(revision))
    }
}

fn destination_setting_key(subscription_id: i64) -> String {
    format!("subscription.{subscription_id}.destination")
}

fn cover_setting_key(subscription_id: i64) -> String {
    format!("subscription.{subscription_id}.cover")
}

pub fn subscription_cover_candidates(
    application: &Application,
    subscription_id: i64,
    cursor: Option<&SubscriptionCoverCandidateCursor>,
    limit: i64,
) -> Result<SubscriptionCoverCandidatePage, String> {
    const MAX_LIMIT: i64 = 200;
    let limit = limit.clamp(1, MAX_LIMIT);
    application.store().read(|connection| {
        require_subscription(connection, subscription_id)?;
        let cursor_imported_at = cursor.map(|value| value.imported_at.as_str());
        let cursor_media_item_id = cursor.map(|value| value.media_item_id);
        let mut rows = connection
            .prepare(
                "WITH candidates AS (
                     SELECT ma.item_id AS media_item_id, mf.file_hash, ma.name,
                            mf.pixel_width, mf.pixel_height,
                            ma.imported_at
                     FROM subscription_source_post ssp
                     JOIN source_item si ON si.source_post_id = ssp.source_post_id
                     JOIN media_asset ma ON ma.item_id = si.media_item_id
                     JOIN media_file mf ON mf.file_id = ma.file_id
                     LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
                     JOIN library_root root
                       ON root.item_id = COALESCE(cm.collection_id, ma.item_id)
                     WHERE ssp.subscription_id = ?1
                       -- Keep the subscription lookup outermost. Unary plus prevents SQLite from
                       -- scanning the global state index before applying the subscription scope.
                       AND +si.state = 'ingested'
                       AND root.lifecycle = 'active'
                       AND mf.mime_type LIKE 'image/%'
                     GROUP BY ma.item_id, mf.file_hash, ma.name,
                              mf.pixel_width, mf.pixel_height, ma.imported_at
                 )
                 SELECT media_item_id, file_hash, name, pixel_width, pixel_height, imported_at
                 FROM candidates
                 WHERE ?2 IS NULL
                    OR imported_at < ?2
                    OR (imported_at = ?2 AND media_item_id < ?3)
                 ORDER BY imported_at DESC, media_item_id DESC
                 LIMIT ?4",
            )?
            .query_map(
                params![
                    subscription_id,
                    cursor_imported_at,
                    cursor_media_item_id,
                    limit + 1,
                ],
                |row| {
                    Ok((
                        SubscriptionCoverCandidate {
                            media_item_id: row.get(0)?,
                            file_hash: row.get(1)?,
                            name: row.get(2)?,
                            pixel_width: row.get(3)?,
                            pixel_height: row.get(4)?,
                        },
                        row.get::<_, String>(5)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_cursor = (rows.len() as i64 > limit).then(|| {
            let (candidate, imported_at) = &rows[limit as usize - 1];
            SubscriptionCoverCandidateCursor {
                imported_at: imported_at.clone(),
                media_item_id: candidate.media_item_id,
            }
        });
        rows.truncate(limit as usize);
        Ok(SubscriptionCoverCandidatePage {
            candidates: rows.into_iter().map(|(candidate, _)| candidate).collect(),
            next_cursor,
        })
    })
}

fn validate_cover_selection(selection: &SubscriptionCoverSelection) -> Result<(), String> {
    if !(0..=1000).contains(&selection.focus_x)
        || !(0..=1000).contains(&selection.focus_y)
        || !(100..=300).contains(&selection.zoom_percent)
    {
        return Err("invalid subscription cover crop".to_string());
    }
    Ok(())
}

fn require_subscription_cover_candidate(
    connection: &rusqlite::Connection,
    subscription_id: i64,
    media_item_id: i64,
) -> rusqlite::Result<String> {
    find_subscription_cover_candidate(connection, subscription_id, media_item_id)?
        .ok_or_else(|| invalid("cover media is not active in this subscription"))
}

fn find_subscription_cover_candidate(
    connection: &rusqlite::Connection,
    subscription_id: i64,
    media_item_id: i64,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT mf.file_hash
             FROM subscription_source_post ssp
             JOIN source_item si ON si.source_post_id = ssp.source_post_id
             JOIN media_asset ma ON ma.item_id = si.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             LEFT JOIN collection_member cm ON cm.media_item_id = ma.item_id
             JOIN library_root root
               ON root.item_id = COALESCE(cm.collection_id, ma.item_id)
             WHERE ssp.subscription_id = ?1
               AND ma.item_id = ?2
               AND si.state = 'ingested'
               AND root.lifecycle = 'active'
             LIMIT 1",
            params![subscription_id, media_item_id],
            |row| row.get(0),
        )
        .optional()
}

fn subscription_cover_from_connection(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> rusqlite::Result<Option<(SubscriptionCoverSelection, String)>> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [cover_setting_key(subscription_id)],
            |row| row.get(0),
        )
        .optional()?;
    let Some(value) = value else {
        return Ok(None);
    };
    let selection: SubscriptionCoverSelection = serde_json::from_str(&value)
        .map_err(|error| invalid(format!("invalid subscription cover: {error}")))?;
    validate_cover_selection(&selection).map_err(invalid)?;
    let file_hash =
        find_subscription_cover_candidate(connection, subscription_id, selection.media_item_id)?;
    Ok(file_hash.map(|file_hash| (selection, file_hash)))
}

pub fn subscription_destination(
    application: &Application,
    subscription_id: i64,
) -> Result<SubscriptionDestinationPolicy, String> {
    application
        .store()
        .read(|connection| subscription_destination_from_connection(connection, subscription_id))
}

fn subscription_destination_from_connection(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> rusqlite::Result<SubscriptionDestinationPolicy> {
    require_subscription(connection, subscription_id)?;
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [destination_setting_key(subscription_id)],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|json| {
            let policy = serde_json::from_str(&json)
                .map_err(|error| invalid(format!("invalid subscription destination: {error}")))?;
            normalize_destination_policy(&policy).map_err(invalid)
        })
        .transpose()
        .map(|policy| policy.unwrap_or_default())
}

fn normalize_destination_policy(
    policy: &SubscriptionDestinationPolicy,
) -> Result<SubscriptionDestinationPolicy, String> {
    let mut automatic_tags = Vec::new();
    for tag in &policy.automatic_tags {
        let tag = tag.trim();
        if tag.is_empty() || automatic_tags.iter().any(|current| current == tag) {
            continue;
        }
        crate::tag_name_v2::parse_local(tag)
            .map_err(|error| format!("invalid automatic tag '{tag}': {error}"))?;
        automatic_tags.push(tag.to_string());
    }
    let mut target_folder_ids = policy.target_folder_ids.clone();
    if let Some(folder_id) = policy.target_folder_id {
        if !target_folder_ids.contains(&folder_id) {
            target_folder_ids.push(folder_id);
        }
    }
    target_folder_ids.sort_unstable();
    target_folder_ids.dedup();
    Ok(SubscriptionDestinationPolicy {
        target_folder_ids,
        target_folder_id: None,
        automatic_tags,
    })
}

fn query_views(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryView>> {
    connection
        .prepare(
            "SELECT q.query_id, q.site_id, q.query_kind, q.query_text,
                    q.display_name, q.notes, q.group_posts, q.paused, q.initial_run_complete,
                    COALESCE(q.resume_cursor = '', 0),
                    q.last_success_at, q.last_failure_at, q.last_failure_kind,
                    q.last_failure_message,
                    COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                        THEN ssp.source_post_id END),
                    COUNT(DISTINCT CASE WHEN si.state = 'ingested' THEN si.media_item_id END),
                    (SELECT COUNT(*) FROM subscription_run_query completed
                     WHERE completed.query_id = q.query_id
                       AND completed.status = 'succeeded')
             FROM subscription_query q
             LEFT JOIN subscription_source_post ssp ON ssp.query_id = q.query_id
             LEFT JOIN source_item si ON si.source_post_id = ssp.source_post_id
             WHERE q.subscription_id = ?1
             GROUP BY q.query_id ORDER BY q.query_id",
        )?
        .query_map([subscription_id], |row| {
            Ok(SubscriptionQueryView {
                query_id: row.get(0)?,
                site_id: row.get(1)?,
                query_kind: row.get(2)?,
                query_text: row.get(3)?,
                display_name: row.get(4)?,
                notes: row.get(5)?,
                group_posts: row.get(6)?,
                paused: row.get(7)?,
                initial_run_complete: row.get(8)?,
                source_history_complete: row.get(9)?,
                last_success_at: row.get(10)?,
                last_failure_at: row.get(11)?,
                last_failure_kind: row.get(12)?,
                last_failure_message: row.get(13)?,
                post_count: row.get(14)?,
                media_count: row.get(15)?,
                successful_run_count: row.get(16)?,
            })
        })?
        .collect()
}

struct PreparedQuery {
    site_id: String,
    domain_key: String,
    query_kind: String,
    query_text: String,
    display_name: Option<String>,
    notes: Option<String>,
    group_posts: bool,
}

fn insert_query(
    transaction: &rusqlite::Transaction<'_>,
    subscription_id: i64,
    query: &PreparedQuery,
) -> rusqlite::Result<i64> {
    transaction.execute(
        "INSERT INTO subscription_query (
             query_key, subscription_id, site_id, domain_key, query_kind,
             query_text, display_name, notes, group_posts
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            new_key("query"),
            subscription_id,
            query.site_id,
            query.domain_key,
            query.query_kind,
            query.query_text.trim(),
            query.display_name,
            query.notes,
            query.group_posts,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn validate_subscription(input: &NewSubscription) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("Subscription name is required".to_string());
    }
    subscriptions_v2::next_schedule_at(&input.schedule, "2026-01-01T00:00:00Z")?;
    Ok(())
}

fn prepare_query(query: &NewSubscriptionQuery) -> Result<PreparedQuery, String> {
    let site_id = query.site_id.trim();
    let query_kind = infer_query_kind(site_id);
    let query_text = normalize_query_text(site_id, query_kind, &query.query_text);
    validate_query_text(site_id, &query_text)?;
    build_url(site_id, &query_text)
        .ok_or_else(|| format!("Invalid {site_id} subscription query"))?;
    let site = site_by_id(site_id).ok_or_else(|| format!("Unknown site: {site_id}"))?;
    Ok(PreparedQuery {
        site_id: site.id.to_string(),
        domain_key: site.domain.to_string(),
        query_kind: query_kind.to_string(),
        query_text,
        display_name: optional_text(query.display_name.as_deref()),
        notes: optional_text(query.notes.as_deref()),
        group_posts: query.group_posts,
    })
}

fn optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn require_subscription(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> rusqlite::Result<()> {
    connection
        .query_row(
            "SELECT 1 FROM subscription WHERE subscription_id = ?1",
            [subscription_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| invalid("subscription does not exist"))
}

fn require_query(connection: &rusqlite::Connection, query_id: i64) -> rusqlite::Result<()> {
    connection
        .query_row(
            "SELECT 1 FROM subscription_query WHERE query_id = ?1",
            [query_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| invalid("subscription query does not exist"))
}

fn reject_active_query_edit(
    connection: &rusqlite::Connection,
    query_id: i64,
) -> rusqlite::Result<()> {
    let active: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM subscription_run_query srq
             JOIN subscription_run sr ON sr.run_id = srq.run_id
             WHERE srq.query_id = ?1 AND sr.status IN ('pending', 'running')
         )",
        [query_id],
        |row| row.get(0),
    )?;
    if active {
        return Err(invalid("stop the subscription before editing its query"));
    }
    Ok(())
}

fn reject_active_subscription_edit(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> rusqlite::Result<()> {
    let active: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
         )",
        [subscription_id],
        |row| row.get(0),
    )?;
    if active {
        return Err(invalid("stop the subscription before resetting it"));
    }
    Ok(())
}

fn subscription_receipt(revision: u64) -> MutationReceipt {
    MutationReceipt {
        revision,
        resources: vec![
            resources::SUBSCRIPTIONS.to_string(),
            resources::TASKS.to_string(),
        ],
        item_ids: Vec::new(),
    }
}

fn new_key(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}:{}", hex::encode(bytes))
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

    fn input() -> NewSubscription {
        NewSubscription {
            name: "Artist".into(),
            schedule: "daily".into(),
            initial_post_limit: Some(100),
            periodic_post_limit: Some(20),
            queries: vec![NewSubscriptionQuery {
                site_id: "pixivuser".into(),
                query_text: "42".into(),
                display_name: Some("Artist".into()),
                notes: None,
                group_posts: true,
            }],
        }
    }

    #[test]
    fn create_is_atomic_and_list_reads_one_persisted_view() {
        let (_directory, application) = fixture();
        let (subscription_id, receipt) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(receipt.revision, 1);

        let catalog = list(&application).unwrap();
        assert_eq!(catalog.subscriptions.len(), 1);
        let subscription = &catalog.subscriptions[0];
        assert_eq!(subscription.subscription_id, subscription_id);
        assert_eq!(subscription.queries.len(), 1);
        assert_eq!(subscription.queries[0].query_text, "42");
        assert_eq!(subscription.queries[0].successful_run_count, 0);
        assert!(!subscription.queries[0].source_history_complete);
        let domain: String = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT domain_key FROM subscription_query WHERE query_id = ?1",
                    [subscription.queries[0].query_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(domain, "pixiv.net");
        assert_eq!(
            subscription.next_run_at.as_deref(),
            Some("2026-01-02T00:00:00+00:00")
        );

        let query_id = subscription.queries[0].query_id;
        for run_id in 1..=2 {
            application
                .store()
                .transaction(|transaction| {
                    transaction.execute(
                        "INSERT INTO subscription_run (
                             run_id, subscription_id, requested_by, status,
                             started_at, finished_at, created_at
                         ) VALUES (?1, ?2, 'manual', 'succeeded', 'now', 'now', 'now')",
                        params![run_id, subscription_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO subscription_run_query (
                             run_id, query_id, status, available_at,
                             started_at, finished_at
                         ) VALUES (?1, ?2, 'succeeded', 'now', 'now', 'now')",
                        params![run_id, query_id],
                    )?;
                    if run_id == 2 {
                        transaction.execute(
                            "UPDATE subscription_query SET resume_cursor = '' WHERE query_id = ?1",
                            [query_id],
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
            let catalog = list(&application).unwrap();
            let query = &catalog.subscriptions[0].queries[0];
            assert_eq!(query.successful_run_count, run_id);
            assert_eq!(query.source_history_complete, run_id == 2);
        }
    }

    #[test]
    fn deleting_subscription_keeps_imported_media_and_source_provenance() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('pixiv', 'post', 'now', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        application.delete_subscription(subscription_id).unwrap();
        let source_posts: i64 = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM source_post", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(source_posts, 1);
    }

    #[test]
    fn custom_cover_only_accepts_subscription_media_in_all() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let query_id = list(&application).unwrap().subscriptions[0].queries[0].query_id;
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file (
                         file_id, file_hash, mime_type, size_bytes,
                         pixel_width, pixel_height, created_at
                     ) VALUES (1, 'cover-hash', 'image/jpeg', 10, 800, 600, 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (1, 'media:1', 'media', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO media_asset (
                         item_id, file_id, name, imported_at, updated_at
                     ) VALUES (1, 1, 'Cover', 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (1, 'active')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO source_post (
                         source_post_id, site_id, post_key, root_item_id, created_at, updated_at
                     ) VALUES (1, 'pixiv', 'post', 1, 'now', 'now')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO subscription_source_post (
                         subscription_id, query_id, source_post_id
                     ) VALUES (?1, ?2, 1)",
                    params![subscription_id, query_id],
                )?;
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, media_item_id,
                         state, created_at, updated_at
                     ) VALUES (1, 'image', 0, 1, 'ingested', 'now', 'now')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let page = subscription_cover_candidates(&application, subscription_id, None, 200).unwrap();
        assert_eq!(page.candidates.len(), 1);
        assert_eq!(page.candidates[0].file_hash, "cover-hash");
        assert!(page.next_cursor.is_none());

        application
            .set_subscription_cover(
                subscription_id,
                &SubscriptionCoverSelection {
                    media_item_id: 1,
                    focus_x: 250,
                    focus_y: 750,
                    zoom_percent: 160,
                },
            )
            .unwrap();
        let subscription = &list(&application).unwrap().subscriptions[0];
        assert_eq!(subscription.cover_file_hash.as_deref(), Some("cover-hash"));
        assert_eq!(subscription.cover_focus_x, 250);
        assert_eq!(subscription.cover_focus_y, 750);
        assert_eq!(subscription.cover_zoom_percent, 160);

        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE library_root SET lifecycle = 'inbox' WHERE item_id = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(
            subscription_cover_candidates(&application, subscription_id, None, 200)
                .unwrap()
                .candidates
                .is_empty()
        );
        assert!(list(&application).unwrap().subscriptions[0]
            .cover_file_hash
            .is_none());
    }

    #[test]
    fn automatic_cover_uses_latest_image_import_without_bookkeeping_churn() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let query_id = list(&application).unwrap().subscriptions[0].queries[0].query_id;
        application
            .store()
            .transaction(|transaction| {
                for (item_id, imported_at, mime_type) in [
                    (1_i64, "2026-01-01T00:00:01Z", "image/jpeg"),
                    (2_i64, "2026-01-01T00:00:03Z", "image/jpeg"),
                    (3_i64, "2026-01-01T00:00:03Z", "image/jpeg"),
                    (4_i64, "2026-01-01T00:00:04Z", "video/mp4"),
                ] {
                    transaction.execute(
                        "INSERT INTO media_file (
                             file_id, file_hash, mime_type, size_bytes, created_at
                         ) VALUES (?1, ?2, ?3, 10, ?4)",
                        params![item_id, format!("hash-{item_id}"), mime_type, imported_at],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_item (
                             item_id, item_key, kind, created_at, updated_at
                         ) VALUES (?1, ?2, 'media', ?3, ?3)",
                        params![item_id, format!("media:{item_id}"), imported_at],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_asset (
                             item_id, file_id, name, imported_at, updated_at
                         ) VALUES (?1, ?1, ?2, ?3, ?3)",
                        params![item_id, format!("Cover {item_id}"), imported_at],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                        [item_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO source_post (
                             source_post_id, site_id, post_key, root_item_id,
                             created_at, updated_at
                         ) VALUES (?1, 'pixiv', ?2, ?1, ?3, ?3)",
                        params![item_id, format!("post-{item_id}"), imported_at],
                    )?;
                    transaction.execute(
                        "INSERT INTO subscription_source_post (
                             subscription_id, query_id, source_post_id
                         ) VALUES (?1, ?2, ?3)",
                        params![subscription_id, query_id, item_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO source_item (
                             source_post_id, item_key, position, media_item_id,
                             state, created_at, updated_at
                         ) VALUES (?1, 'image', 0, ?1, 'ingested', ?2, ?2)",
                        params![item_id, imported_at],
                    )?;
                }
                // Retraversal and state bookkeeping must not make an old image the cover.
                transaction.execute(
                    "UPDATE source_item SET updated_at = '2099-01-01T00:00:00Z'
                     WHERE source_post_id = 1",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(
            list(&application).unwrap().subscriptions[0]
                .cover_file_hash
                .as_deref(),
            Some("hash-3")
        );
        let first = subscription_cover_candidates(&application, subscription_id, None, 2).unwrap();
        assert_eq!(
            first
                .candidates
                .iter()
                .map(|candidate| candidate.media_item_id)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        let cursor = first.next_cursor.expect("first page cursor");
        assert_eq!(cursor.imported_at, "2026-01-01T00:00:03Z");
        assert_eq!(cursor.media_item_id, 2);

        let second =
            subscription_cover_candidates(&application, subscription_id, Some(&cursor), 2).unwrap();
        assert_eq!(
            second
                .candidates
                .iter()
                .map(|candidate| candidate.media_item_id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert!(second.next_cursor.is_none());
    }

    #[tokio::test]
    async fn reset_forgets_sync_history_without_deleting_source_provenance() {
        let (directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let query_id = list(&application).unwrap().subscriptions[0].queries[0].query_id;
        let (run, _) = application
            .request_subscription_run(subscription_id, "2026-01-01T00:00:01Z")
            .unwrap();

        assert!(application
            .reset_subscription(subscription_id)
            .await
            .is_err());
        application
            .cancel_subscription_run(subscription_id, "2026-01-01T00:00:02Z")
            .unwrap();

        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE subscription_query
                     SET resume_cursor = 'page-2', initial_run_complete = 1,
                         last_success_at = 'now', last_failure_at = 'now',
                         last_failure_kind = 'network', last_failure_message = 'failed'
                     WHERE query_id = ?1",
                    [query_id],
                )?;
                transaction.execute(
                    "INSERT INTO source_post (site_id, post_key, created_at, updated_at)
                     VALUES ('pixiv', 'post', 'now', 'now')",
                    [],
                )?;
                let source_post_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO source_item (
                         source_post_id, item_key, position, state, last_error,
                         created_at, updated_at
                     ) VALUES (?1, 'image', 0, 'deleted', 'deleted by user', 'now', 'now')",
                    [source_post_id],
                )?;
                let source_item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO ingest_job (
                         job_key, source_kind, source_path, source_item_id, payload_json,
                         lifecycle, delete_after_ingest, status, available_at,
                         created_at, updated_at
                     ) VALUES (
                         'subscription:pixiv:post:image', 'subscription', '/tmp/missing',
                         ?1, '{}', 'inbox', 1, 'succeeded', 'now', 'now', 'now'
                     )",
                    [source_item_id],
                )?;
                let run_query_id: i64 = transaction.query_row(
                    "SELECT run_query_id FROM subscription_run_query
                     WHERE run_id = ?1 AND query_id = ?2",
                    params![run.run_id, query_id],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO subscription_run_source_item (run_query_id, source_item_id)
                     VALUES (?1, ?2)",
                    params![run_query_id, source_item_id],
                )?;
                transaction.execute(
                    "INSERT INTO subscription_issue (
                         issue_key, subscription_id, query_id, issue_kind, message,
                         status, first_seen_at, last_seen_at
                     ) VALUES ('issue', ?1, ?2, 'network', 'failed', 'open', 'now', 'now')",
                    params![subscription_id, query_id],
                )?;
                Ok(())
            })
            .unwrap();

        let archive_path = directory.path().join("gdl-archive.sqlite3");
        let archive = rusqlite::Connection::open(&archive_path).unwrap();
        archive
            .execute_batch("CREATE TABLE archive (entry TEXT PRIMARY KEY);")
            .unwrap();
        archive
            .execute(
                "INSERT INTO archive (entry) VALUES (?1)",
                [format!("picto_s{subscription_id}_q{query_id}_post")],
            )
            .unwrap();
        archive
            .execute(
                "INSERT INTO archive (entry) VALUES ('picto_s999_q1_post')",
                [],
            )
            .unwrap();
        drop(archive);

        application
            .reset_subscription(subscription_id)
            .await
            .unwrap();

        application
            .store()
            .read(|connection| {
                let query: (Option<String>, bool, Option<String>, Option<String>) = connection
                    .query_row(
                        "SELECT resume_cursor, initial_run_complete,
                                last_success_at, last_failure_at
                         FROM subscription_query WHERE query_id = ?1",
                        [query_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )?;
                assert_eq!(query, (None, false, None, None));
                for table in [
                    "subscription_run",
                    "subscription_issue",
                    "subscription_source_post",
                ] {
                    let count: i64 = connection.query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE subscription_id = ?1"),
                        [subscription_id],
                        |row| row.get(0),
                    )?;
                    assert_eq!(count, 0, "{table} should be reset");
                }
                let source_posts: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM source_post", [], |row| row.get(0))?;
                assert_eq!(source_posts, 1);
                let source_item: (String, Option<String>) = connection.query_row(
                    "SELECT state, last_error FROM source_item",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(source_item, ("pending".to_string(), None));
                let ingest_jobs: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM ingest_job", [], |row| row.get(0))?;
                assert_eq!(ingest_jobs, 0);
                Ok(())
            })
            .unwrap();

        let archive = rusqlite::Connection::open(archive_path).unwrap();
        let entries = archive
            .prepare("SELECT entry FROM archive ORDER BY entry")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(entries, vec!["picto_s999_q1_post".to_string()]);
    }

    #[test]
    fn run_request_is_idempotent_and_cancelled_from_persisted_state() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let (first, _) = application
            .request_subscription_run(subscription_id, "2026-01-01T00:00:01Z")
            .unwrap();
        let (again, _) = application
            .request_subscription_run(subscription_id, "2026-01-01T00:00:02Z")
            .unwrap();
        assert_eq!(first.run_id, again.run_id);
        assert!(!again.created);

        application
            .cancel_subscription_run(subscription_id, "2026-01-01T00:00:03Z")
            .unwrap();
        assert_eq!(
            list(&application).unwrap().subscriptions[0]
                .status
                .as_deref(),
            Some("cancelled")
        );
    }

    #[test]
    fn query_edits_are_normalized_and_the_last_query_can_be_removed() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let query_id = list(&application).unwrap().subscriptions[0].queries[0].query_id;

        application
            .update_subscription_query(
                query_id,
                &NewSubscriptionQuery {
                    site_id: "tumblr".into(),
                    query_text: "@NASA".into(),
                    display_name: Some(" NASA ".into()),
                    notes: Some(" ".into()),
                    group_posts: true,
                },
            )
            .unwrap();
        let query = &list(&application).unwrap().subscriptions[0].queries[0];
        assert_eq!(query.query_text, "NASA");
        assert_eq!(query.display_name.as_deref(), Some("NASA"));
        assert_eq!(query.notes, None);
        assert!(query.group_posts);
        application
            .set_subscription_query_grouping(query_id, false)
            .unwrap();
        assert!(!list(&application).unwrap().subscriptions[0].queries[0].group_posts);
        application.delete_subscription_query(query_id).unwrap();
        assert!(list(&application).unwrap().subscriptions[0]
            .queries
            .is_empty());
        application
            .add_subscription_query(
                subscription_id,
                &NewSubscriptionQuery {
                    site_id: "e621".into(),
                    query_text: "canine".into(),
                    display_name: None,
                    notes: None,
                    group_posts: true,
                },
            )
            .unwrap();
        assert_eq!(
            list(&application).unwrap().subscriptions[0].queries[0].query_kind,
            "search"
        );
    }

    #[test]
    fn posts_per_run_is_one_subscription_setting() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();

        application
            .set_subscription_posts_per_run(subscription_id, 25)
            .unwrap();

        let subscription = &list(&application).unwrap().subscriptions[0];
        assert_eq!(subscription.initial_post_limit, Some(25));
        assert_eq!(subscription.periodic_post_limit, Some(25));
        assert!(application
            .set_subscription_posts_per_run(subscription_id, 0)
            .unwrap_err()
            .contains("between 1 and 10,000"));
    }

    #[test]
    fn subscription_can_be_created_before_sources_are_added() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(
                &NewSubscription {
                    name: "Later".into(),
                    schedule: "manual".into(),
                    initial_post_limit: None,
                    periodic_post_limit: None,
                    queries: Vec::new(),
                },
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        let subscription = &list(&application).unwrap().subscriptions[0];
        assert_eq!(subscription.subscription_id, subscription_id);
        assert!(subscription.queries.is_empty());
    }

    #[test]
    fn destination_policy_is_persisted_and_returned_with_subscription() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let folder_id = application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder (folder_key, name, created_at, updated_at)
                     VALUES ('downloads', 'Downloads', 'now', 'now')",
                    [],
                )?;
                Ok(transaction.last_insert_rowid())
            })
            .unwrap()
            .0;

        application
            .set_subscription_destination(
                subscription_id,
                &SubscriptionDestinationPolicy {
                    target_folder_ids: vec![folder_id],
                    target_folder_id: None,
                    automatic_tags: vec![
                        " creator:alice ".into(),
                        "favorite".into(),
                        "favorite".into(),
                    ],
                },
            )
            .unwrap();

        assert_eq!(
            list(&application).unwrap().subscriptions[0].destination,
            SubscriptionDestinationPolicy {
                target_folder_ids: vec![folder_id],
                target_folder_id: None,
                automatic_tags: vec!["creator:alice".into(), "favorite".into()],
            }
        );
    }
}
