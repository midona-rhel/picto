//! Product-facing subscription definitions and persisted status reads.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;

use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::blob_store::BlobStore;
use crate::library_application::LibraryApplication;
use crate::subscriptions::sites::{build_url, site_by_id};
use crate::subscriptions::source_adapter::{infer_query_kind, normalize_query_text};
use crate::subscriptions::{self, CreatedRun};

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
    pub posts_skipped: i64,
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
    pub root_count: i64,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredSubscriptionCover {
    media_item_id: i64,
    focus_x: i64,
    focus_y: i64,
    zoom_percent: i64,
    #[serde(default)]
    rendered_hash: Option<String>,
}

impl StoredSubscriptionCover {
    fn selection(&self) -> SubscriptionCoverSelection {
        SubscriptionCoverSelection {
            media_item_id: self.media_item_id,
            focus_x: self.focus_x,
            focus_y: self.focus_y,
            zoom_percent: self.zoom_percent,
        }
    }
}

#[derive(Debug, Clone)]
struct SubscriptionCoverSource {
    file_hash: String,
    file_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionList {
    pub subscriptions: Vec<SubscriptionView>,
    pub global_paused: bool,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn list_library(application: &LibraryApplication) -> Result<SubscriptionList, String> {
    application
        .library()
        .auxiliary_read_consistent(
            picto_library::database::WorkPriority::VisibleRead,
            |connection, projection| {
                let mut subscriptions = query_subscription_views(connection)?;
                let mut queries = query_views_by_subscription(connection)?;
                let (mut destinations, mut covers) = subscription_settings_by_id_library(
                    connection,
                    application.blobs(),
                    projection,
                )?;
                for subscription in &mut subscriptions {
                    subscription.queries = queries
                        .remove(&subscription.subscription_id)
                        .unwrap_or_default();
                    subscription.destination = destinations
                        .remove(&subscription.subscription_id)
                        .unwrap_or_default();
                    if let Some(Some((selection, file_hash))) =
                        covers.remove(&subscription.subscription_id)
                    {
                        subscription.cover_file_hash = Some(file_hash);
                        subscription.cover_focus_x = selection.focus_x;
                        subscription.cover_focus_y = selection.focus_y;
                        subscription.cover_zoom_percent = selection.zoom_percent;
                    }
                }
                Ok(SubscriptionList {
                    subscriptions,
                    global_paused: subscriptions_globally_paused(connection)?,
                    revision: projection.revision,
                })
            },
        )
        .map_err(|error| error.to_string())
}

fn query_subscription_views(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<Vec<SubscriptionView>> {
    connection
        .prepare(
            "WITH active_runs AS (
                 SELECT *
                 FROM subscription_run
                 WHERE status IN ('pending', 'running')
             ),
             latest_run_ids AS (
                 SELECT subscription_id, MAX(run_id) AS run_id
                 FROM subscription_run
                 GROUP BY subscription_id
             ),
             root_totals AS (
                 SELECT ssp.subscription_id,
                        COUNT(DISTINCT post.root_item_id) AS root_count
                 FROM subscription_source_post ssp
                 JOIN source_post post ON post.source_post_id = ssp.source_post_id
                 WHERE post.root_item_id IS NOT NULL
                 GROUP BY ssp.subscription_id
             ),
             issue_totals AS (
                 SELECT subscription_id, COUNT(*) AS issue_count
                 FROM subscription_issue
                 WHERE status = 'open'
                 GROUP BY subscription_id
             ),
             traversed_posts AS (
                 SELECT srq.run_id,
                        COUNT(DISTINCT attempt.attempt_id) AS posts_traversed,
                        COUNT(DISTINCT CASE WHEN attempt.state = 'skipped'
                                            THEN attempt.attempt_id END) AS posts_skipped
                 FROM subscription_run_query srq
                 LEFT JOIN source_post_attempt attempt
                   ON attempt.run_query_id = srq.run_query_id
                 GROUP BY srq.run_id
             ),
             item_progress AS (
                 SELECT srq.run_id,
                        COUNT(DISTINCT CASE WHEN attempt.state = 'added'
                                            THEN attempt.attempt_id END) AS posts_added,
                        COUNT(DISTINCT file.file_attempt_id) AS discovered,
                        COUNT(DISTINCT CASE WHEN file.state IN ('staged', 'retained')
                                            THEN file.file_attempt_id END) AS downloaded,
                        COUNT(DISTINCT CASE WHEN file.state = 'retained'
                                            THEN file.file_attempt_id END) AS ingested,
                        COUNT(DISTINCT CASE WHEN file.state = 'failed'
                                            THEN file.file_attempt_id END) AS failed,
                        0 AS deleted
                 FROM subscription_run_query srq
                 LEFT JOIN source_post_attempt attempt
                   ON attempt.run_query_id = srq.run_query_id
                 LEFT JOIN source_file_attempt file USING(attempt_id)
                 GROUP BY srq.run_id
             )
             SELECT s.subscription_id, s.name, s.schedule, s.paused,
                    s.initial_post_limit, s.periodic_post_limit, s.next_run_at,
                    active.run_id,
                    CASE
                        WHEN active.status = 'pending'
                         AND active.failure_kind IS NOT NULL
                        THEN active.failure_kind
                        ELSE COALESCE(active.status, latest.status)
                    END,
                    COALESCE(root_totals.root_count, 0),
                    COALESCE(issue_totals.issue_count, 0),
                    COALESCE(traversed_posts.posts_traversed, 0),
                    COALESCE(item_progress.posts_added, 0),
                    COALESCE(traversed_posts.posts_skipped, 0),
                    COALESCE(item_progress.discovered, 0),
                    COALESCE(item_progress.downloaded, 0),
                    COALESCE(item_progress.ingested, 0),
                    COALESCE(item_progress.failed, 0),
                    COALESCE(item_progress.deleted, 0)
             FROM subscription s
             LEFT JOIN active_runs active
               ON active.subscription_id = s.subscription_id
             LEFT JOIN latest_run_ids latest_id
               ON latest_id.subscription_id = s.subscription_id
             LEFT JOIN subscription_run latest ON latest.run_id = latest_id.run_id
             LEFT JOIN root_totals USING (subscription_id)
             LEFT JOIN issue_totals USING (subscription_id)
             LEFT JOIN traversed_posts
               ON traversed_posts.run_id = COALESCE(active.run_id, latest.run_id)
             LEFT JOIN item_progress
               ON item_progress.run_id = COALESCE(active.run_id, latest.run_id)
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
                root_count: row.get(9)?,
                open_issue_count: row.get(10)?,
                cover_file_hash: None,
                cover_focus_x: 500,
                cover_focus_y: 500,
                cover_zoom_percent: 100,
                progress: SubscriptionProgress {
                    posts_traversed: row.get(11)?,
                    posts_added: row.get(12)?,
                    posts_skipped: row.get(13)?,
                    discovered: row.get(14)?,
                    downloaded: row.get(15)?,
                    ingested: row.get(16)?,
                    failed: row.get(17)?,
                    deleted: row.get(18)?,
                },
                destination: SubscriptionDestinationPolicy::default(),
                queries: Vec::new(),
            })
        })?
        .collect()
}

impl LibraryApplication {
    pub async fn reset_subscription_library(
        &self,
        subscription_id: i64,
    ) -> Result<picto_library::MutationReceipt, String> {
        self.library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    require_subscription(connection, subscription_id)?;
                    reject_running_subscription_reset(connection, subscription_id)?;
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;
        let receipt = finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.reset",
                    serde_json::json!({"subscription_id": subscription_id}),
                    |transaction, _| {
                        require_subscription(transaction, subscription_id)?;
                        reject_running_subscription_reset(transaction, subscription_id)?;
                        transaction.execute(
                            "DELETE FROM ingest_job
                             WHERE source_item_id IN (
                                 SELECT si.source_item_id
                                 FROM subscription_source_post ssp
                                 JOIN source_item si
                                   ON si.source_post_id = ssp.source_post_id
                                 WHERE ssp.subscription_id = ?1
                                   AND si.media_item_id IS NULL
                                 UNION
                                 SELECT rsi.source_item_id
                                 FROM subscription_run_source_item rsi
                                 JOIN subscription_run_query srq
                                   ON srq.run_query_id = rsi.run_query_id
                                 JOIN subscription_run sr ON sr.run_id = srq.run_id
                                 JOIN source_item si
                                   ON si.source_item_id = rsi.source_item_id
                                 WHERE sr.subscription_id = ?1
                                   AND si.media_item_id IS NULL
                             )",
                            [subscription_id],
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
                        transaction.execute(
                            "DELETE FROM deletion_tombstone
                             WHERE stable_key IN (
                                 SELECT 'source:' || post.site_id || ':' || post.post_key || ':' || item.item_key
                                 FROM subscription_source_post linked
                                 JOIN source_post post USING(source_post_id)
                                 JOIN source_item item USING(source_post_id)
                                 WHERE linked.subscription_id = ?1
                             )",
                            [subscription_id],
                        )?;
                        transaction.execute(
                            "DELETE FROM subscription_source_post
                             WHERE subscription_id = ?1",
                            [subscription_id],
                        )?;
                        transaction.execute(
                            "DELETE FROM subscription_run WHERE subscription_id = ?1",
                            [subscription_id],
                        )?;
                        transaction.execute(
                            "DELETE FROM source_post
                             WHERE NOT EXISTS (
                                 SELECT 1 FROM subscription_source_post linked
                                 WHERE linked.source_post_id = source_post.source_post_id
                             )",
                            [],
                        )?;
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )?;
        crate::native_source::clear_subscription_state(self.root(), subscription_id)?;
        Ok(receipt)
    }

    pub fn create_subscription_definition_library(
        &self,
        input: &NewSubscription,
        now: &str,
    ) -> Result<(i64, picto_library::MutationReceipt), String> {
        validate_subscription(input)?;
        let queries = input
            .queries
            .iter()
            .map(prepare_query)
            .collect::<Result<Vec<_>, _>>()?;
        let next_run_at = subscriptions::next_schedule_at(&input.schedule, now)?;
        let payload = serde_json::json!({"name": input.name.trim()});
        let published = self
            .library()
            .auxiliary_semantic_write_if_changed(
                picto_library::database::WorkPriority::ForegroundMutation,
                subscription_resources(),
                [],
                "subscriptions.create",
                payload,
                |transaction, _| {
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
                    for query in &queries {
                        insert_query(transaction, subscription_id, query)?;
                    }
                    Ok(Some(subscription_id))
                },
            )
            .map_err(|error| error.to_string())?
            .expect("subscription creation always changes canonical state");
        Ok(published)
    }

    pub fn add_subscription_query_library(
        &self,
        subscription_id: i64,
        query: &NewSubscriptionQuery,
    ) -> Result<(i64, picto_library::MutationReceipt), String> {
        let query = prepare_query(query)?;
        let published = self
            .library()
            .auxiliary_semantic_write_if_changed(
                picto_library::database::WorkPriority::ForegroundMutation,
                subscription_resources(),
                [],
                "subscriptions.queries.add",
                serde_json::json!({"subscription_id": subscription_id}),
                |transaction, _| {
                    require_subscription(transaction, subscription_id)?;
                    Ok(Some(insert_query(transaction, subscription_id, &query)?))
                },
            )
            .map_err(|error| error.to_string())?
            .expect("query creation always changes canonical state");
        Ok(published)
    }

    pub fn update_subscription_query_library(
        &self,
        query_id: i64,
        query: &NewSubscriptionQuery,
    ) -> Result<picto_library::MutationReceipt, String> {
        let query = prepare_query(query)?;
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.queries.update",
                    serde_json::json!({"query_id": query_id}),
                    |transaction, _| {
                        reject_active_query_edit(transaction, query_id)?;
                        let changed = transaction.execute(
                            "UPDATE subscription_query
                             SET site_id = ?1, domain_key = ?2, query_kind = ?3,
                                 query_text = ?4, display_name = ?5, notes = ?6,
                                 resume_cursor = NULL, initial_run_complete = 0,
                                 last_failure_at = NULL, last_failure_kind = NULL,
                                 last_failure_message = NULL
                             WHERE query_id = ?7
                               AND (site_id != ?1 OR domain_key != ?2 OR query_kind != ?3
                                    OR query_text != ?4 OR display_name IS NOT ?5
                                    OR notes IS NOT ?6)",
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
                        if changed == 0 {
                            require_query(transaction, query_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn pause_subscription_query_library(
        &self,
        query_id: i64,
        paused: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.queries.pause",
                    serde_json::json!({"query_id": query_id, "paused": paused}),
                    |transaction, _| {
                        reject_active_query_edit(transaction, query_id)?;
                        let changed = transaction.execute(
                            "UPDATE subscription_query SET paused = ?1
                             WHERE query_id = ?2 AND paused != ?1",
                            params![paused, query_id],
                        )?;
                        if changed == 0 {
                            require_query(transaction, query_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn set_subscription_query_grouping_library(
        &self,
        query_id: i64,
        group_posts: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.queries.grouping",
                    serde_json::json!({"query_id": query_id, "group_posts": group_posts}),
                    |transaction, _| {
                        reject_active_query_edit(transaction, query_id)?;
                        let changed = transaction.execute(
                            "UPDATE subscription_query SET group_posts = ?1
                             WHERE query_id = ?2 AND group_posts != ?1",
                            params![group_posts, query_id],
                        )?;
                        if changed == 0 {
                            require_query(transaction, query_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn delete_subscription_query_library(
        &self,
        query_id: i64,
    ) -> Result<picto_library::MutationReceipt, String> {
        let receipt = finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.queries.delete",
                    serde_json::json!({"query_id": query_id}),
                    |transaction, _| {
                        reject_active_query_edit(transaction, query_id)?;
                        let changed = transaction.execute(
                            "DELETE FROM subscription_query WHERE query_id = ?1",
                            [query_id],
                        )?;
                        if changed != 1 {
                            return Err(invalid("subscription query does not exist").into());
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )?;
        Ok(receipt)
    }

    pub fn rename_subscription_library(
        &self,
        subscription_id: i64,
        name: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Subscription name is required".to_string());
        }
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.rename",
                    serde_json::json!({"subscription_id": subscription_id, "name": name}),
                    |transaction, _| {
                        let changed = transaction.execute(
                            "UPDATE subscription SET name = ?1
                             WHERE subscription_id = ?2 AND name != ?1",
                            params![name, subscription_id],
                        )?;
                        if changed == 0 {
                            require_subscription(transaction, subscription_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn set_subscription_hold_library(
        &self,
        subscription_id: i64,
        held: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.hold",
                    serde_json::json!({"subscription_id": subscription_id, "held": held}),
                    |transaction, _| {
                        reject_active_subscription_hold(transaction, subscription_id)?;
                        let changed = transaction.execute(
                            "UPDATE subscription SET paused = ?1
                             WHERE subscription_id = ?2 AND paused != ?1",
                            params![held, subscription_id],
                        )?;
                        if changed == 0 {
                            require_subscription(transaction, subscription_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn pause_subscription_run_library(
        &self,
        subscription_id: i64,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.run.pause",
                    serde_json::json!({"subscription_id": subscription_id}),
                    |transaction, _| {
                        require_active_subscription_run(transaction, subscription_id)?;
                        let changed = set_active_subscription_pause_state(
                            transaction,
                            subscription_id,
                            true,
                            None,
                        )?;
                        Ok((changed != 0).then_some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn resume_subscription_run_library(
        &self,
        subscription_id: i64,
        now: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.run.resume",
                    serde_json::json!({"subscription_id": subscription_id}),
                    |transaction, _| {
                        require_active_subscription_run(transaction, subscription_id)?;
                        let changed = set_active_subscription_pause_state(
                            transaction,
                            subscription_id,
                            false,
                            Some(now),
                        )?;
                        Ok((changed != 0).then_some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn pause_all_subscriptions_library(
        &self,
        paused: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.pause_all",
                    serde_json::json!({"paused": paused}),
                    |transaction, _| {
                        let value = if paused { "true" } else { "false" };
                        let changed = transaction.execute(
                            "INSERT INTO setting(key, value_json)
                             VALUES ('subscriptions.global_paused', ?1)
                             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json
                             WHERE setting.value_json != excluded.value_json",
                            [value],
                        )?;
                        Ok((changed != 0).then_some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn set_subscription_schedule_library(
        &self,
        subscription_id: i64,
        schedule: &str,
        now: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        let next_run_at = subscriptions::next_schedule_at(schedule, now)?;
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.schedule",
                    serde_json::json!({"subscription_id": subscription_id, "schedule": schedule}),
                    |transaction, _| {
                        let changed = transaction.execute(
                            "UPDATE subscription SET schedule = ?1, next_run_at = ?2
                             WHERE subscription_id = ?3
                               AND (schedule != ?1 OR next_run_at IS NOT ?2)",
                            params![schedule, next_run_at, subscription_id],
                        )?;
                        if changed == 0 {
                            require_subscription(transaction, subscription_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn set_subscription_posts_per_run_library(
        &self,
        subscription_id: i64,
        posts_per_run: i64,
    ) -> Result<picto_library::MutationReceipt, String> {
        if !(1..=10_000).contains(&posts_per_run) {
            return Err("Posts per run must be between 1 and 10,000".to_string());
        }
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.posts_per_run",
                    serde_json::json!({
                        "subscription_id": subscription_id,
                        "posts_per_run": posts_per_run
                    }),
                    |transaction, _| {
                        reject_active_subscription_edit(transaction, subscription_id)?;
                        let changed = transaction.execute(
                            "UPDATE subscription
                             SET initial_post_limit = ?1, periodic_post_limit = ?1
                             WHERE subscription_id = ?2
                               AND (initial_post_limit IS NOT ?1
                                    OR periodic_post_limit IS NOT ?1)",
                            params![posts_per_run, subscription_id],
                        )?;
                        if changed == 0 {
                            require_subscription(transaction, subscription_id)?;
                            return Ok(None);
                        }
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn set_subscription_destination_library(
        &self,
        subscription_id: i64,
        policy: &SubscriptionDestinationPolicy,
    ) -> Result<picto_library::MutationReceipt, String> {
        let policy = normalize_destination_policy(policy)?;
        let value = serde_json::to_string(&policy).map_err(|error| error.to_string())?;
        let key = destination_setting_key(subscription_id);
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.destination",
                    serde_json::json!({"subscription_id": subscription_id}),
                    |transaction, _| {
                        require_subscription(transaction, subscription_id)?;
                        for folder_id in &policy.target_folder_ids {
                            let exists: bool = transaction.query_row(
                                "SELECT EXISTS(
                                     SELECT 1 FROM folder_definition WHERE folder_id = ?1
                                 )",
                                [folder_id],
                                |row| row.get(0),
                            )?;
                            if !exists {
                                return Err(invalid("destination folder does not exist").into());
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
                            return Ok(None);
                        }
                        transaction.execute(
                            "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                             ON CONFLICT(key) DO UPDATE
                             SET value_json = excluded.value_json",
                            params![key, value],
                        )?;
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn delete_subscription_library(
        &self,
        subscription_id: i64,
    ) -> Result<picto_library::MutationReceipt, String> {
        crate::native_source::clear_subscription_state(self.root(), subscription_id)?;
        let receipt = finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.delete",
                    serde_json::json!({"subscription_id": subscription_id}),
                    |transaction, _| {
                        let changed = transaction.execute(
                            "DELETE FROM subscription WHERE subscription_id = ?1",
                            [subscription_id],
                        )?;
                        if changed != 1 {
                            return Err(invalid("subscription does not exist").into());
                        }
                        transaction.execute(
                            "DELETE FROM setting WHERE key IN (?1, ?2)",
                            params![
                                destination_setting_key(subscription_id),
                                cover_setting_key(subscription_id)
                            ],
                        )?;
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )?;
        Ok(receipt)
    }

    pub fn request_subscription_run_library(
        &self,
        subscription_id: i64,
        now: &str,
    ) -> Result<(CreatedRun, picto_library::MutationReceipt), String> {
        let published = self
            .library()
            .auxiliary_semantic_write_if_changed(
                picto_library::database::WorkPriority::ForegroundMutation,
                subscription_resources(),
                [],
                "subscriptions.run",
                serde_json::json!({"subscription_id": subscription_id}),
                |transaction, _| {
                    let run =
                        subscriptions::create_run_in(transaction, subscription_id, "manual", now)?;
                    Ok(run.created.then_some(run))
                },
            )
            .map_err(|error| error.to_string())?;
        if let Some(result) = published {
            return Ok(result);
        }
        let run = self
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT run_id, status FROM subscription_run
                             WHERE subscription_id = ?1
                               AND status IN ('pending', 'running')
                             ORDER BY run_id LIMIT 1",
                            [subscription_id],
                            |row| {
                                Ok(CreatedRun {
                                    run_id: row.get(0)?,
                                    created: false,
                                    state: subscriptions::parse_run_state(row.get(1)?)?,
                                })
                            },
                        )
                        .map_err(Into::into)
                },
            )
            .map_err(|error| error.to_string())?;
        Ok((run, current_subscription_receipt(self)?))
    }

    pub fn request_subscription_query_run_library(
        &self,
        query_id: i64,
        now: &str,
    ) -> Result<(CreatedRun, picto_library::MutationReceipt), String> {
        let published = self
            .library()
            .auxiliary_semantic_write_if_changed(
                picto_library::database::WorkPriority::ForegroundMutation,
                subscription_resources(),
                [],
                "subscriptions.queries.run",
                serde_json::json!({"query_id": query_id}),
                |transaction, _| {
                    let run = subscriptions::create_query_run_in(
                        transaction,
                        query_id,
                        "manual-query",
                        now,
                    )?;
                    Ok(run.created.then_some(run))
                },
            )
            .map_err(|error| error.to_string())?;
        if let Some(result) = published {
            return Ok(result);
        }
        let run = self
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row(
                            "SELECT run.run_id, run.status
                             FROM subscription_query query
                             JOIN subscription_run run
                               ON run.subscription_id = query.subscription_id
                             JOIN subscription_run_query run_query
                               ON run_query.run_id = run.run_id
                              AND run_query.query_id = query.query_id
                             WHERE query.query_id = ?1
                               AND run.status IN ('pending', 'running')
                             ORDER BY run.run_id LIMIT 1",
                            [query_id],
                            |row| {
                                Ok(CreatedRun {
                                    run_id: row.get(0)?,
                                    created: false,
                                    state: subscriptions::parse_run_state(row.get(1)?)?,
                                })
                            },
                        )
                        .map_err(Into::into)
                },
            )
            .map_err(|error| error.to_string())?;
        Ok((run, current_subscription_receipt(self)?))
    }

    pub fn cancel_subscription_run_library(
        &self,
        subscription_id: i64,
        now: &str,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.cancel",
                    serde_json::json!({"subscription_id": subscription_id}),
                    |transaction, _| {
                        let run_id: i64 = transaction
                            .query_row(
                                "SELECT run_id FROM subscription_run
                                 WHERE subscription_id = ?1
                                   AND status IN ('pending', 'running')",
                                [subscription_id],
                                |row| row.get(0),
                            )
                            .optional()?
                            .ok_or_else(|| invalid("subscription is not running"))?;
                        transaction.execute(
                            "UPDATE subscription_run_query
                             SET status = 'cancelled', finished_at = ?1
                             WHERE run_id = ?2 AND status IN ('pending', 'running')",
                            params![now, run_id],
                        )?;
                        transaction.execute(
                            "UPDATE source_post_attempt
                             SET state = 'cancelled', terminal_reason = 'user_cancelled', settled_at = ?1
                             WHERE run_query_id IN (
                                 SELECT run_query_id FROM subscription_run_query WHERE run_id = ?2
                             ) AND state NOT IN ('added', 'skipped', 'failed', 'cancelled')",
                            params![now, run_id],
                        )?;
                        transaction.execute(
                            "UPDATE source_file_attempt SET staged_path = NULL
                             WHERE attempt_id IN (
                                 SELECT attempt_id FROM source_post_attempt
                                 WHERE run_query_id IN (
                                     SELECT run_query_id FROM subscription_run_query WHERE run_id = ?1
                                 ) AND state = 'cancelled'
                             )",
                            [run_id],
                        )?;
                        transaction.execute(
                            "UPDATE subscription_run
                             SET status = 'cancelled', finished_at = ?1
                             WHERE run_id = ?2",
                            params![now, run_id],
                        )?;
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
    }

    pub fn set_subscription_cover_library(
        &self,
        subscription_id: i64,
        selection: &SubscriptionCoverSelection,
    ) -> Result<picto_library::MutationReceipt, String> {
        validate_cover_selection(selection)?;
        let source = self
            .library()
            .auxiliary_read_consistent(
                picto_library::database::WorkPriority::VisibleRead,
                |connection, projection| {
                    require_subscription(connection, subscription_id)?;
                    find_subscription_cover_candidate_library(
                        connection,
                        projection,
                        subscription_id,
                        selection.media_item_id,
                    )?
                    .ok_or_else(|| invalid("cover media is not active in this subscription").into())
                },
            )
            .map_err(|error| error.to_string())?;
        let rendered_hash =
            render_subscription_cover(self.blobs(), subscription_id, &source, selection)?;
        let stored = StoredSubscriptionCover {
            media_item_id: selection.media_item_id,
            focus_x: selection.focus_x,
            focus_y: selection.focus_y,
            zoom_percent: selection.zoom_percent,
            rendered_hash: Some(rendered_hash.clone()),
        };
        let value = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
        let key = cover_setting_key(subscription_id);
        let published = self
            .library()
            .auxiliary_semantic_write_if_changed(
                picto_library::database::WorkPriority::ForegroundMutation,
                subscription_resources(),
                [],
                "subscriptions.cover.set",
                serde_json::json!({
                    "subscription_id": subscription_id,
                    "media_item_id": selection.media_item_id
                }),
                |transaction, _| {
                    require_subscription(transaction, subscription_id)?;
                    let current_hash: Option<String> = transaction
                        .query_row(
                            "SELECT file.content_hash
                             FROM subscription_source_post ssp
                             JOIN source_item si ON si.source_post_id = ssp.source_post_id
                             JOIN media_item media ON media.media_id = si.media_item_id
                             JOIN media_file file ON file.file_id = media.file_id
                             WHERE ssp.subscription_id = ?1
                               AND si.media_item_id = ?2 AND si.state = 'ingested'
                             LIMIT 1",
                            params![subscription_id, selection.media_item_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if current_hash.as_deref() != Some(source.file_hash.as_str()) {
                        return Err(invalid("cover media changed while rendering").into());
                    }
                    let previous: Option<String> = transaction
                        .query_row(
                            "SELECT value_json FROM setting WHERE key = ?1",
                            [&key],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if previous.as_deref() == Some(value.as_str()) {
                        return Ok(None);
                    }
                    transaction.execute(
                        "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                        params![key, value],
                    )?;
                    Ok(Some(stored_rendered_hash(previous.as_deref())))
                },
            )
            .map_err(|error| error.to_string())?;
        let Some((previous_rendered_hash, receipt)) = published else {
            return current_subscription_receipt(self);
        };
        if previous_rendered_hash.as_deref() != Some(rendered_hash.as_str()) {
            if let Some(previous_rendered_hash) = previous_rendered_hash {
                let _ = self.blobs().delete_thumbnail(&previous_rendered_hash);
            }
        }
        Ok(receipt)
    }
}

fn subscription_resources() -> Vec<String> {
    vec!["subscriptions".to_string(), "tasks".to_string()]
}

fn finish_subscription_mutation(
    application: &LibraryApplication,
    published: Option<((), picto_library::MutationReceipt)>,
) -> Result<picto_library::MutationReceipt, String> {
    if let Some(((), receipt)) = published {
        return Ok(receipt);
    }
    current_subscription_receipt(application)
}

fn current_subscription_receipt(
    application: &LibraryApplication,
) -> Result<picto_library::MutationReceipt, String> {
    let revision = application
        .library()
        .database()
        .revision()
        .map_err(|error| error.to_string())?;
    Ok(picto_library::MutationReceipt {
        revision,
        resources: subscription_resources(),
        item_ids: Vec::new(),
    })
}

fn destination_setting_key(subscription_id: i64) -> String {
    format!("subscription.{subscription_id}.destination")
}

fn cover_setting_key(subscription_id: i64) -> String {
    format!("subscription.{subscription_id}.cover")
}

pub fn subscription_cover_candidates_library(
    application: &LibraryApplication,
    subscription_id: i64,
    cursor: Option<&SubscriptionCoverCandidateCursor>,
    limit: i64,
) -> Result<SubscriptionCoverCandidatePage, String> {
    let limit = limit.clamp(1, 200);
    application
        .library()
        .auxiliary_read_consistent(
            picto_library::database::WorkPriority::VisibleRead,
            |connection, projection| {
                require_subscription(connection, subscription_id)?;
                let mut raw_cursor = cursor.cloned();
                let mut rows = Vec::new();
                let batch_limit = ((limit + 1) * 4).clamp(64, 800);
                loop {
                    let cursor_updated_at =
                        raw_cursor.as_ref().map(|value| value.imported_at.as_str());
                    let cursor_media_id = raw_cursor.as_ref().map(|value| value.media_item_id);
                    let batch = connection
                        .prepare(
                            "SELECT media.media_id, file.content_hash, media.media_name,
                                    file.width, file.height, si.updated_at
                             FROM subscription_source_post ssp
                             JOIN source_item si ON si.source_post_id = ssp.source_post_id
                             JOIN media_item media ON media.media_id = si.media_item_id
                             JOIN media_file file ON file.file_id = media.file_id
                             WHERE ssp.subscription_id = ?1
                               AND si.state = 'ingested'
                               AND file.mime LIKE 'image/%'
                               AND (?2 IS NULL OR si.updated_at < ?2
                                    OR (si.updated_at = ?2 AND media.media_id < ?3))
                             GROUP BY media.media_id, file.content_hash, media.media_name,
                                      file.width, file.height, si.updated_at
                             ORDER BY si.updated_at DESC, media.media_id DESC
                             LIMIT ?4",
                        )?
                        .query_map(
                            params![
                                subscription_id,
                                cursor_updated_at,
                                cursor_media_id,
                                batch_limit,
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
                    let raw_count = batch.len() as i64;
                    let last = batch.last().map(|(candidate, imported_at)| {
                        SubscriptionCoverCandidateCursor {
                            imported_at: imported_at.clone(),
                            media_item_id: candidate.media_item_id,
                        }
                    });
                    rows.extend(batch.into_iter().filter(|(candidate, _)| {
                        u32::try_from(candidate.media_item_id)
                            .ok()
                            .and_then(|media_id| projection.media_owner.get(media_id))
                            .is_some_and(|root_id| projection.active().contains(root_id.0))
                    }));
                    if rows.len() as i64 > limit || raw_count < batch_limit || last.is_none() {
                        break;
                    }
                    raw_cursor = last;
                }
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
            },
        )
        .map_err(|error| error.to_string())
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

fn find_subscription_cover_candidate_library(
    connection: &rusqlite::Connection,
    projection: &picto_library::ProjectionSnapshot,
    subscription_id: i64,
    media_item_id: i64,
) -> rusqlite::Result<Option<SubscriptionCoverSource>> {
    let Some(media_id) = u32::try_from(media_item_id).ok() else {
        return Ok(None);
    };
    let Some(root_id) = projection.media_owner.get(media_id) else {
        return Ok(None);
    };
    if !projection.active().contains(root_id.0) {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT file.content_hash, file.file_path
             FROM subscription_source_post ssp
             JOIN source_item si ON si.source_post_id = ssp.source_post_id
             JOIN media_item media ON media.media_id = si.media_item_id
             JOIN media_file file ON file.file_id = media.file_id
             WHERE ssp.subscription_id = ?1
               AND media.media_id = ?2
               AND si.state = 'ingested'
             LIMIT 1",
            params![subscription_id, media_item_id],
            |row| {
                Ok(SubscriptionCoverSource {
                    file_hash: row.get(0)?,
                    file_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            },
        )
        .optional()
}

fn resolve_stored_cover(
    blobs: &BlobStore,
    stored: StoredSubscriptionCover,
    source_file_hash: Option<String>,
) -> rusqlite::Result<Option<(SubscriptionCoverSelection, String)>> {
    let selection = stored.selection();
    validate_cover_selection(&selection).map_err(invalid)?;
    let Some(source_file_hash) = source_file_hash else {
        return Ok(None);
    };
    let rendered_hash = stored.rendered_hash.filter(|hash| {
        blobs
            .find_thumbnail_path(hash)
            .map(|path| path.is_some())
            .unwrap_or(false)
    });
    if let Some(rendered_hash) = rendered_hash {
        return Ok(Some((
            SubscriptionCoverSelection {
                media_item_id: selection.media_item_id,
                focus_x: 500,
                focus_y: 500,
                zoom_percent: 100,
            },
            rendered_hash,
        )));
    }
    Ok(Some((selection, source_file_hash)))
}

fn stored_rendered_hash(value: Option<&str>) -> Option<String> {
    value
        .and_then(|value| serde_json::from_str::<StoredSubscriptionCover>(value).ok())
        .and_then(|cover| cover.rendered_hash)
}

fn render_subscription_cover(
    blobs: &BlobStore,
    subscription_id: i64,
    source: &SubscriptionCoverSource,
    selection: &SubscriptionCoverSelection,
) -> Result<String, String> {
    let identity = format!(
        "subscription-cover:v1:{subscription_id}:{}:{}:{}:{}",
        source.file_hash, selection.focus_x, selection.focus_y, selection.zoom_percent
    );
    let rendered_hash = hex::encode(Sha256::digest(identity.as_bytes()));
    if blobs
        .find_thumbnail_path(&rendered_hash)
        .map_err(|error| format!("Subscription cover lookup failed: {error}"))?
        .is_some()
    {
        return Ok(rendered_hash);
    }

    let original = std::fs::read(&source.file_path).ok();
    let thumbnail = || {
        blobs
            .read_thumbnail(&source.file_hash)
            .map_err(|error| format!("Subscription cover thumbnail read failed: {error}"))?
            .ok_or_else(|| "Subscription cover source is unavailable".to_string())
    };
    let image = original
        .as_deref()
        .and_then(|bytes| image::load_from_memory(bytes).ok())
        .map(Ok)
        .unwrap_or_else(|| {
            thumbnail().and_then(|bytes| {
                image::load_from_memory(&bytes)
                    .map_err(|error| format!("Subscription cover decode failed: {error}"))
            })
        })?;
    let zoom = selection.zoom_percent as f64 / 100.0;
    let crop_size = ((image.width().min(image.height()) as f64 / zoom).round() as u32)
        .clamp(1, image.width().min(image.height()));
    let max_x = image.width().saturating_sub(crop_size);
    let max_y = image.height().saturating_sub(crop_size);
    let crop_x = ((max_x as f64 * selection.focus_x as f64 / 1000.0).round() as u32).min(max_x);
    let crop_y = ((max_y as f64 * selection.focus_y as f64 / 1000.0).round() as u32).min(max_y);
    let rendered = image
        .crop_imm(crop_x, crop_y, crop_size, crop_size)
        .resize_exact(384, 384, image::imageops::FilterType::Lanczos3);
    let mut bytes = Cursor::new(Vec::new());
    rendered
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| format!("Subscription cover encode failed: {error}"))?;
    blobs
        .write_thumbnail(&rendered_hash, bytes.get_ref(), "png")
        .map_err(|error| format!("Subscription cover write failed: {error}"))?;
    Ok(rendered_hash)
}
pub fn subscription_destination_library(
    application: &LibraryApplication,
    subscription_id: i64,
) -> Result<SubscriptionDestinationPolicy, String> {
    application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(subscription_destination_from_connection(
                    connection,
                    subscription_id,
                )?)
            },
        )
        .map_err(|error| error.to_string())
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

type SubscriptionCoverOverride = Option<(SubscriptionCoverSelection, String)>;

fn subscription_settings_by_id_library(
    connection: &rusqlite::Connection,
    blobs: &BlobStore,
    projection: &picto_library::ProjectionSnapshot,
) -> rusqlite::Result<(
    HashMap<i64, SubscriptionDestinationPolicy>,
    HashMap<i64, SubscriptionCoverOverride>,
)> {
    let mut statement = connection.prepare(
        "SELECT s.subscription_id, destination.value_json, cover.value_json,
                CASE WHEN json_valid(cover.value_json)
                     THEN CAST(json_extract(cover.value_json, '$.media_item_id') AS INTEGER)
                END,
                (
                    SELECT file.content_hash
                    FROM subscription_source_post ssp
                    JOIN source_item si ON si.source_post_id = ssp.source_post_id
                    JOIN media_item media ON media.media_id = si.media_item_id
                    JOIN media_file file ON file.file_id = media.file_id
                    WHERE ssp.subscription_id = s.subscription_id
                      AND si.media_item_id = CASE
                          WHEN json_valid(cover.value_json)
                          THEN CAST(json_extract(cover.value_json, '$.media_item_id') AS INTEGER)
                      END
                      AND si.state = 'ingested'
                    LIMIT 1
                )
         FROM subscription s
         LEFT JOIN setting destination
           ON destination.key = 'subscription.' || s.subscription_id || '.destination'
         LEFT JOIN setting cover
           ON cover.key = 'subscription.' || s.subscription_id || '.cover'
         ORDER BY s.subscription_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut destinations = HashMap::new();
    let mut covers = HashMap::new();
    for row in rows {
        let (subscription_id, destination_json, cover_json, media_item_id, mut source_file_hash) =
            row?;
        let destination = destination_json
            .map(|json| {
                let policy = serde_json::from_str(&json).map_err(|error| {
                    invalid(format!("invalid subscription destination: {error}"))
                })?;
                normalize_destination_policy(&policy).map_err(invalid)
            })
            .transpose()?
            .unwrap_or_default();
        destinations.insert(subscription_id, destination);
        if let Some(json) = cover_json {
            let is_active = media_item_id
                .and_then(|media_item_id| u32::try_from(media_item_id).ok())
                .and_then(|media_item_id| projection.media_owner.get(media_item_id))
                .is_some_and(|root_id| projection.active().contains(root_id.0));
            if !is_active {
                source_file_hash = None;
            }
            let stored: StoredSubscriptionCover = serde_json::from_str(&json)
                .map_err(|error| invalid(format!("invalid subscription cover: {error}")))?;
            covers.insert(
                subscription_id,
                resolve_stored_cover(blobs, stored, source_file_hash)?,
            );
        }
    }
    Ok((destinations, covers))
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
        crate::tag_name::parse_local(tag)
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

fn query_views_by_subscription(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<HashMap<i64, Vec<SubscriptionQueryView>>> {
    let mut statement = connection.prepare(
        "WITH added_source_posts AS (
             SELECT run_query.query_id, attempt.source_post_id
             FROM subscription_run_query run_query
             JOIN source_post_attempt attempt USING(run_query_id)
             WHERE attempt.state = 'added'
             GROUP BY run_query.query_id, attempt.source_post_id
         ),
         source_counts AS (
             SELECT ssp.query_id,
                    COUNT(DISTINCT post.source_post_id) AS post_count,
                    COUNT(DISTINCT si.media_item_id) AS media_count
             FROM subscription_source_post ssp
             JOIN added_source_posts added
               ON added.query_id = ssp.query_id
              AND added.source_post_id = ssp.source_post_id
             JOIN source_post post ON post.source_post_id = ssp.source_post_id
             LEFT JOIN source_item si
               ON si.source_post_id = ssp.source_post_id
              AND si.state = 'ingested'
             WHERE post.root_item_id IS NOT NULL
             GROUP BY ssp.query_id
         ),
         successful_runs AS (
             SELECT query_id, COUNT(*) AS run_count
             FROM subscription_run_query
             WHERE status = 'succeeded'
             GROUP BY query_id
         )
         SELECT q.subscription_id, q.query_id, q.site_id, q.query_kind, q.query_text,
                    q.display_name, q.notes, q.group_posts, q.paused, q.initial_run_complete,
                    COALESCE(q.resume_cursor = '', 0),
                    q.last_success_at, q.last_failure_at, q.last_failure_kind,
                    q.last_failure_message,
                    COALESCE(source_counts.post_count, 0),
                    COALESCE(source_counts.media_count, 0),
                    COALESCE(successful_runs.run_count, 0)
             FROM subscription_query q
             LEFT JOIN source_counts ON source_counts.query_id = q.query_id
             LEFT JOIN successful_runs ON successful_runs.query_id = q.query_id
             ORDER BY q.subscription_id, q.query_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            SubscriptionQueryView {
                query_id: row.get(1)?,
                site_id: row.get(2)?,
                query_kind: row.get(3)?,
                query_text: row.get(4)?,
                display_name: row.get(5)?,
                notes: row.get(6)?,
                group_posts: row.get(7)?,
                paused: row.get(8)?,
                initial_run_complete: row.get(9)?,
                source_history_complete: row.get(10)?,
                last_success_at: row.get(11)?,
                last_failure_at: row.get(12)?,
                last_failure_kind: row.get(13)?,
                last_failure_message: row.get(14)?,
                post_count: row.get(15)?,
                media_count: row.get(16)?,
                successful_run_count: row.get(17)?,
            },
        ))
    })?;
    let mut by_subscription: HashMap<i64, Vec<SubscriptionQueryView>> = HashMap::new();
    for row in rows {
        let (subscription_id, query) = row?;
        by_subscription
            .entry(subscription_id)
            .or_default()
            .push(query);
    }
    Ok(by_subscription)
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
    subscriptions::next_schedule_at(&input.schedule, "2026-01-01T00:00:00Z")?;
    Ok(())
}

fn prepare_query(query: &NewSubscriptionQuery) -> Result<PreparedQuery, String> {
    let site_id = query.site_id.trim();
    let site = site_by_id(site_id).ok_or_else(|| format!("Unknown site: {site_id}"))?;
    let query_kind = infer_query_kind(site_id);
    let query_text = normalize_query_text(site_id, query_kind, &query.query_text);
    let adapter = picto_sources::ProviderRegistry::native()
        .get(site_id)
        .ok_or_else(|| format!("No native source adapter exists for {site_id}"))?;
    adapter
        .validate_query(&query_text)
        .map_err(|error| error.message)?;
    build_url(site_id, &query_text)
        .ok_or_else(|| format!("Invalid {site_id} subscription query"))?;
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

fn set_active_subscription_pause_state(
    transaction: &rusqlite::Transaction<'_>,
    subscription_id: i64,
    paused: bool,
    now: Option<&str>,
) -> rusqlite::Result<usize> {
    if paused {
        let query_count = transaction.execute(
            "UPDATE subscription_run_query
             SET status = 'pending', started_at = NULL, finished_at = NULL,
                 attempt_count = CASE WHEN status = 'running'
                                      THEN MAX(attempt_count - 1, 0)
                                      ELSE attempt_count END,
                 failure_kind = 'paused', error_message = 'Paused by user.'
             WHERE status IN ('pending', 'running')
               AND run_id IN (
                   SELECT run_id FROM subscription_run
                   WHERE subscription_id = ?1 AND status IN ('pending', 'running')
               )",
            [subscription_id],
        )?;
        let run_count = transaction.execute(
            "UPDATE subscription_run
             SET status = 'pending', started_at = NULL, finished_at = NULL,
                 failure_kind = 'paused', error_message = 'Paused by user.'
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')",
            [subscription_id],
        )?;
        return Ok(query_count + run_count);
    }

    let query_count = transaction.execute(
        "UPDATE subscription_run_query
         SET available_at = COALESCE(?1, available_at),
             failure_kind = NULL, error_message = NULL
         WHERE status = 'pending' AND failure_kind IS NOT NULL
           AND run_id IN (
               SELECT run_id FROM subscription_run
               WHERE subscription_id = ?2 AND status = 'pending'
           )",
        params![now, subscription_id],
    )?;
    let run_count = transaction.execute(
        "UPDATE subscription_run
         SET failure_kind = NULL, error_message = NULL
         WHERE subscription_id = ?1 AND status = 'pending'
           AND failure_kind IS NOT NULL",
        [subscription_id],
    )?;
    Ok(query_count + run_count)
}

pub(crate) fn subscriptions_globally_paused(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<bool> {
    connection
        .query_row(
            "SELECT value_json = 'true' FROM setting
             WHERE key = 'subscriptions.global_paused'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
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
) -> picto_library::Result<()> {
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
        return Err(picto_library::LibraryError::InvalidState(
            "stop the subscription before editing its query".into(),
        ));
    }
    Ok(())
}

fn reject_active_subscription_edit(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> picto_library::Result<()> {
    let active: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
         )",
        [subscription_id],
        |row| row.get(0),
    )?;
    if active {
        return Err(picto_library::LibraryError::InvalidState(
            "stop the subscription before changing its posts-per-run limit".into(),
        ));
    }
    Ok(())
}

fn reject_active_subscription_hold(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> picto_library::Result<()> {
    let active: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
         )",
        [subscription_id],
        |row| row.get(0),
    )?;
    if active {
        return Err(picto_library::LibraryError::InvalidState(
            "stop the active run before putting this subscription on hold".into(),
        ));
    }
    Ok(())
}

fn require_active_subscription_run(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> picto_library::Result<()> {
    require_subscription(connection, subscription_id)?;
    let active: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM subscription_run
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')
         )",
        [subscription_id],
        |row| row.get(0),
    )?;
    if !active {
        return Err(picto_library::LibraryError::InvalidState(
            "subscription has no active run".into(),
        ));
    }
    Ok(())
}

fn reject_running_subscription_reset(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> picto_library::Result<()> {
    let running: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM subscription_run
             WHERE subscription_id = ?1 AND status = 'running'
         )",
        [subscription_id],
        |row| row.get(0),
    )?;
    if running {
        return Err(picto_library::LibraryError::InvalidState(
            "stop the subscription before resetting it".into(),
        ));
    }
    Ok(())
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
    use super::{LibraryApplication, NewSubscription, NewSubscriptionQuery};
    use rusqlite::params;

    #[test]
    fn query_counts_exclude_skipped_posts_linked_by_duplicate_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Canonical source counts".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![NewSubscriptionQuery {
                site_id: "konachan".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-30T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-30T00:00:01Z")
            .unwrap();

        application
            .library()
            .auxiliary_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                ["tests".to_owned()],
                [],
                |transaction, _revision| {
                    let (query_id, run_id, run_query_id): (i64, i64, i64) = transaction.query_row(
                        "SELECT query.query_id, run.run_id, run_query.run_query_id
                         FROM subscription_query query
                         JOIN subscription_run run USING(subscription_id)
                         JOIN subscription_run_query run_query
                           ON run_query.run_id = run.run_id
                          AND run_query.query_id = query.query_id
                         WHERE query.subscription_id = ?1",
                        [subscription_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;

                    for offset in 0_i64..2 {
                        let item_id = 5000 + offset;
                        let file_id = 5100 + offset;
                        transaction.execute(
                            "INSERT INTO library_item(local_id, stable_key, item_kind)
                             VALUES (?1, ?2, 1)",
                            params![item_id, format!("source-count-root-{offset}")],
                        )?;
                        transaction.execute(
                            "INSERT INTO media_file
                                 (file_id, content_hash, file_path, mime, size_bytes)
                             VALUES (?1, ?2, ?3, 'image/png', 1)",
                            params![
                                file_id,
                                format!("source-count-hash-{offset}"),
                                format!("/source-count-{offset}.png"),
                            ],
                        )?;
                        transaction.execute(
                            "INSERT INTO media_item(media_id, media_name, file_id)
                             VALUES (?1, ?2, ?3)",
                            params![item_id, format!("media-{offset}"), file_id],
                        )?;
                        transaction.execute(
                            "INSERT INTO library_root
                                 (root_id, name, cover_media_id, imported_at_ms, modified_at_ms,
                                  media_count, total_size_bytes)
                             VALUES (?1, ?2, ?1, 1, 1, 1, 1)",
                            params![item_id, format!("root-{offset}")],
                        )?;
                        transaction.execute(
                            "INSERT INTO source_post
                                 (site_id, post_key, root_item_id, created_at, updated_at)
                             VALUES ('konachan', ?1, ?2, '2026-08-30T00:00:02Z',
                                     '2026-08-30T00:00:02Z')",
                            params![format!("post-{offset}"), item_id],
                        )?;
                        let source_post_id = transaction.last_insert_rowid();
                        transaction.execute(
                            "INSERT INTO source_item
                                 (source_post_id, item_key, position, media_item_id, state,
                                  created_at, updated_at)
                             VALUES (?1, ?2, 0, ?3, 'ingested', '2026-08-30T00:00:02Z',
                                     '2026-08-30T00:00:02Z')",
                            params![source_post_id, format!("media-{offset}"), item_id],
                        )?;
                        transaction.execute(
                            "INSERT INTO subscription_source_post
                                 (subscription_id, query_id, source_post_id, last_seen_run_id)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![subscription_id, query_id, source_post_id, run_id],
                        )?;
                        transaction.execute(
                            "INSERT INTO source_post_attempt
                                 (run_query_id, source_post_id, state, terminal_reason,
                                  started_at, settled_at)
                             VALUES (?1, ?2, ?3, ?4, '2026-08-30T00:00:02Z',
                                     '2026-08-30T00:00:03Z')",
                            params![
                                run_query_id,
                                source_post_id,
                                if offset == 0 { "added" } else { "skipped" },
                                if offset == 0 {
                                    None
                                } else {
                                    Some("exact_duplicate")
                                },
                            ],
                        )?;
                    }
                    Ok(())
                },
            )
            .unwrap();

        let query = super::list_library(&application)
            .unwrap()
            .subscriptions
            .into_iter()
            .find(|subscription| subscription.subscription_id == subscription_id)
            .unwrap()
            .queries
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(query.post_count, 1);
        assert_eq!(query.media_count, 1);
    }

    #[test]
    fn completed_subscription_keeps_the_latest_run_progress_visible() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Completed progress".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![NewSubscriptionQuery {
                site_id: "e621".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:00Z")
            .unwrap();
        let query = crate::library_subscription_state::claim_next_query(
            &application,
            &mut crate::subscriptions::DomainSchedule::new(),
            "2026-08-29T00:00:01Z",
        )
        .unwrap()
        .unwrap();
        crate::library_subscription_state::record_post(
            &application,
            query.run_query_id,
            &crate::subscriptions::NormalizedPost {
                site_id: "e621".into(),
                post_key: "no-media".into(),
                canonical_url: None,
                creator_name: None,
                title: None,
                description: None,
                captured_at: None,
                metadata_json: None,
                items: Vec::new(),
            },
            "2026-08-29T00:00:02Z",
        )
        .unwrap();
        assert!(matches!(
            crate::library_subscription_state::settled_post_outcome(
                &application,
                &query,
                "no-media",
            )
            .unwrap(),
            picto_sources::SourcePostOutcome::Skipped { .. }
        ));
        crate::library_subscription_state::complete_query(
            &application,
            &query,
            Some(""),
            "2026-08-29T00:00:03Z",
        )
        .unwrap();

        let subscription = super::list_library(&application)
            .unwrap()
            .subscriptions
            .into_iter()
            .find(|entry| entry.subscription_id == subscription_id)
            .unwrap();
        assert_eq!(subscription.status.as_deref(), Some("succeeded"));
        assert_eq!(subscription.progress.posts_traversed, 1);
        assert_eq!(subscription.progress.posts_skipped, 1);
        assert_eq!(subscription.progress.posts_added, 0);
    }

    #[tokio::test]
    async fn reset_discards_a_paused_pending_run() {
        let directory = tempfile::tempdir().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let definition = NewSubscription {
            name: "Example".into(),
            schedule: "manual".into(),
            initial_post_limit: Some(2),
            periodic_post_limit: Some(2),
            queries: vec![NewSubscriptionQuery {
                site_id: "twitter".into(),
                query_text: "example".into(),
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        let (other_subscription_id, _) = application
            .create_subscription_definition_library(&definition, "2026-08-29T00:00:00Z")
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, "2026-08-29T00:00:01Z")
            .unwrap();
        application
            .pause_subscription_run_library(subscription_id)
            .unwrap();

        let native_state = application
            .root()
            .join("source-runners/native")
            .join(format!(
                "subscription-{subscription_id}/query-1/run-query-1"
            ));
        std::fs::create_dir_all(&native_state).unwrap();
        std::fs::write(native_state.join("download.part"), b"stale").unwrap();
        let other_native_state = application
            .root()
            .join("source-runners/native/subscription-999/query-1/run-query-1");
        std::fs::create_dir_all(&other_native_state).unwrap();
        std::fs::write(other_native_state.join("download.part"), b"other").unwrap();
        application
            .library()
            .auxiliary_write(
                picto_library::database::WorkPriority::ForegroundMutation,
                ["tests".to_owned()],
                [],
                |transaction, revision| {
                    let (query_id, run_id, run_query_id): (i64, i64, i64) = transaction.query_row(
                        "SELECT query.query_id, run.run_id, run_query.run_query_id
                             FROM subscription_query query
                             JOIN subscription_run run USING(subscription_id)
                             JOIN subscription_run_query run_query
                               ON run_query.run_id = run.run_id
                              AND run_query.query_id = query.query_id
                             WHERE query.subscription_id = ?1",
                        [subscription_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                    let other_query_id: i64 = transaction.query_row(
                        "SELECT query_id FROM subscription_query WHERE subscription_id = ?1",
                        [other_subscription_id],
                        |row| row.get(0),
                    )?;
                    transaction.execute(
                        "INSERT INTO library_item(local_id, stable_key, item_kind)
                         VALUES (5000, 'existing-media', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_file
                             (file_id, content_hash, file_path, mime, size_bytes)
                         VALUES (5001, 'existing-hash', '/existing', 'image/png', 1)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_item(media_id, media_name, file_id)
                         VALUES (5000, 'existing', 5001)",
                        [],
                    )?;
                    transaction.execute(
                        "INSERT INTO source_post
                             (site_id, post_key, created_at, updated_at)
                         VALUES ('twitter', 'reset-post', '2026-08-29T00:00:00Z',
                                 '2026-08-29T00:00:00Z')",
                        [],
                    )?;
                    let source_post_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO source_item
                             (source_post_id, item_key, position, media_item_id, state,
                              created_at, updated_at)
                         VALUES (?1, 'media-1', 0, 5000, 'ingested',
                                 '2026-08-29T00:00:00Z', '2026-08-29T00:00:00Z')",
                        [source_post_id],
                    )?;
                    let source_item_id = transaction.last_insert_rowid();
                    transaction.execute(
                        "INSERT INTO subscription_source_post
                             (subscription_id, query_id, source_post_id, last_seen_run_id)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![subscription_id, query_id, source_post_id, run_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO subscription_source_post
                             (subscription_id, query_id, source_post_id, last_seen_run_id)
                         VALUES (?1, ?2, ?3, NULL)",
                        params![other_subscription_id, other_query_id, source_post_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO subscription_run_source_item(run_query_id, source_item_id)
                         VALUES (?1, ?2)",
                        params![run_query_id, source_item_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO deletion_tombstone(stable_key, revision, deleted_at_ms)
                         VALUES ('source:twitter:reset-post:media-1', ?1, 1),
                                ('unrelated-root', ?1, 1)",
                        [revision as i64],
                    )?;
                    transaction.execute(
                        "INSERT INTO subscription_issue
                             (issue_key, subscription_id, query_id, issue_kind, message,
                              first_seen_at, last_seen_at)
                         VALUES ('reset-issue', ?1, ?2, 'download_item', 'stale',
                                 '2026-08-29T00:00:00Z', '2026-08-29T00:00:00Z')",
                        params![subscription_id, query_id],
                    )?;
                    Ok(())
                },
            )
            .unwrap();

        application
            .reset_subscription_library(subscription_id)
            .await
            .unwrap();

        let subscription = super::list_library(&application)
            .unwrap()
            .subscriptions
            .into_iter()
            .find(|subscription| subscription.subscription_id == subscription_id)
            .unwrap();
        assert!(subscription.active_run_id.is_none());
        assert_eq!(subscription.status.as_deref(), None);
        assert_eq!(subscription.progress.posts_traversed, 0);
        assert_eq!(subscription.progress.posts_added, 0);
        assert_eq!(subscription.progress.downloaded, 0);
        application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    let state: String = connection.query_row(
                        "SELECT state FROM source_item WHERE item_key = 'media-1'",
                        [],
                        |row| row.get(0),
                    )?;
                    assert_eq!(state, "ingested");
                    assert_eq!(
                        connection.query_row(
                            "SELECT COUNT(*) FROM subscription_source_post
                             WHERE subscription_id = ?1",
                            [other_subscription_id],
                            |row| row.get::<_, i64>(0),
                        )?,
                        1
                    );
                    let source_tombstone: bool = connection.query_row(
                        "SELECT EXISTS(
                             SELECT 1 FROM deletion_tombstone
                             WHERE stable_key = 'source:twitter:reset-post:media-1'
                         )",
                        [],
                        |row| row.get(0),
                    )?;
                    let unrelated_tombstone: bool = connection.query_row(
                        "SELECT EXISTS(
                             SELECT 1 FROM deletion_tombstone
                             WHERE stable_key = 'unrelated-root'
                         )",
                        [],
                        |row| row.get(0),
                    )?;
                    assert!(!source_tombstone);
                    assert!(unrelated_tombstone);
                    assert_eq!(
                        connection.query_row(
                            "SELECT COUNT(*) FROM subscription_run WHERE subscription_id = ?1",
                            [subscription_id],
                            |row| row.get::<_, i64>(0),
                        )?,
                        0
                    );
                    assert_eq!(
                        connection.query_row(
                            "SELECT COUNT(*) FROM subscription_issue WHERE subscription_id = ?1",
                            [subscription_id],
                            |row| row.get::<_, i64>(0),
                        )?,
                        0
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert!(!native_state.exists());
        assert!(other_native_state.exists());
    }
}
