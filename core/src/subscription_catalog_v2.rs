//! Product-facing subscription definitions and persisted status reads.

use std::collections::HashMap;
use std::io::Cursor;

use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};
use crate::blob_store::{mime_to_extension, BlobStore};
use crate::library_application::LibraryApplication;
use crate::projection_v2::ProjectionSelectionSnapshot;
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
    root_item_id: i64,
    file_hash: String,
    mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionList {
    pub subscriptions: Vec<SubscriptionView>,
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
                    if let Some(cover) = covers.remove(&subscription.subscription_id) {
                        if let Some((selection, file_hash)) = cover {
                            subscription.cover_file_hash = Some(file_hash);
                            subscription.cover_focus_x = selection.focus_x;
                            subscription.cover_focus_y = selection.focus_y;
                            subscription.cover_zoom_percent = selection.zoom_percent;
                        }
                    }
                }
                Ok(SubscriptionList {
                    subscriptions,
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
             media_totals AS (
                 SELECT ssp.subscription_id,
                        COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                            THEN si.media_item_id END) AS media_count
                 FROM subscription_source_post ssp
                 JOIN source_item si ON si.source_post_id = ssp.source_post_id
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
                        COUNT(DISTINCT ssp.source_post_id) AS posts_traversed
                 FROM active_runs active
                 JOIN subscription_run_query srq ON srq.run_id = active.run_id
                 JOIN subscription_source_post ssp
                   ON ssp.query_id = srq.query_id
                  AND ssp.last_seen_run_id = srq.run_id
                 GROUP BY srq.run_id
             ),
             item_progress AS (
                 SELECT srq.run_id,
                        COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                            THEN si.source_post_id END) AS posts_added,
                        COUNT(DISTINCT rsi.source_item_id) AS discovered,
                        COUNT(DISTINCT CASE WHEN si.state IN ('downloaded', 'ingested')
                                            THEN rsi.source_item_id END) AS downloaded,
                        COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                            THEN rsi.source_item_id END) AS ingested,
                        COUNT(DISTINCT CASE WHEN si.state = 'failed'
                                            THEN rsi.source_item_id END) AS failed,
                        COUNT(DISTINCT CASE WHEN si.state = 'deleted'
                                            THEN rsi.source_item_id END) AS deleted
                 FROM active_runs active
                 JOIN subscription_run_query srq ON srq.run_id = active.run_id
                 LEFT JOIN subscription_run_source_item rsi
                   ON rsi.run_query_id = srq.run_query_id
                 LEFT JOIN source_item si ON si.source_item_id = rsi.source_item_id
                 GROUP BY srq.run_id
             )
             SELECT s.subscription_id, s.name, s.schedule, s.paused,
                    s.initial_post_limit, s.periodic_post_limit, s.next_run_at,
                    active.run_id,
                    CASE
                        WHEN active.status = 'pending'
                         AND active.failure_kind IN ('paused', 'inbox_full')
                        THEN active.failure_kind
                        ELSE COALESCE(active.status, latest.status)
                    END,
                    COALESCE(media_totals.media_count, 0),
                    COALESCE(issue_totals.issue_count, 0),
                    COALESCE(traversed_posts.posts_traversed, 0),
                    COALESCE(item_progress.posts_added, 0),
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
             LEFT JOIN media_totals USING (subscription_id)
             LEFT JOIN issue_totals USING (subscription_id)
             LEFT JOIN traversed_posts ON traversed_posts.run_id = active.run_id
             LEFT JOIN item_progress ON item_progress.run_id = active.run_id
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
                cover_file_hash: None,
                cover_focus_x: 500,
                cover_focus_y: 500,
                cover_zoom_percent: 100,
                progress: SubscriptionProgress {
                    posts_traversed: row.get(11)?,
                    posts_added: row.get(12)?,
                    discovered: row.get(13)?,
                    downloaded: row.get(14)?,
                    ingested: row.get(15)?,
                    failed: row.get(16)?,
                    deleted: row.get(17)?,
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
                    reject_active_subscription_edit(connection, subscription_id)?;
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;
        crate::subscriptions::archive::clear_subscription_archive_entries_at_root(
            self.root(),
            subscription_id,
        )
        .await?;
        crate::onlyfans_source_v2::clear_subscription_state(self.root(), subscription_id)?;
        finish_subscription_mutation(
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
                        reject_active_subscription_edit(transaction, subscription_id)?;
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
                            "UPDATE source_item
                             SET state = 'pending', last_error = NULL,
                                 updated_at = datetime('now')
                             WHERE media_item_id IS NULL AND source_item_id IN (
                                 SELECT si.source_item_id
                                 FROM subscription_source_post ssp
                                 JOIN source_item si
                                   ON si.source_post_id = ssp.source_post_id
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
                            "DELETE FROM subscription_source_post
                             WHERE subscription_id = ?1",
                            [subscription_id],
                        )?;
                        transaction.execute(
                            "DELETE FROM subscription_run WHERE subscription_id = ?1",
                            [subscription_id],
                        )?;
                        Ok(Some(()))
                    },
                )
                .map_err(|error| error.to_string())?,
        )
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
        let next_run_at = subscriptions_v2::next_schedule_at(&input.schedule, now)?;
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
                        let changed = transaction.execute(
                            "UPDATE subscription_query SET paused = ?1
                             WHERE query_id = ?2 AND paused != ?1",
                            params![paused, query_id],
                        )?;
                        if changed == 0 {
                            require_query(transaction, query_id)?;
                            return Ok(None);
                        }
                        set_active_query_pause_state(transaction, query_id, paused)?;
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
        finish_subscription_mutation(
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
        )
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

    pub fn pause_subscription_library(
        &self,
        subscription_id: i64,
        paused: bool,
    ) -> Result<picto_library::MutationReceipt, String> {
        finish_subscription_mutation(
            self,
            self.library()
                .auxiliary_semantic_write_if_changed(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    subscription_resources(),
                    [],
                    "subscriptions.pause",
                    serde_json::json!({"subscription_id": subscription_id, "paused": paused}),
                    |transaction, _| {
                        let changed = transaction.execute(
                            "UPDATE subscription SET paused = ?1
                             WHERE subscription_id = ?2 AND paused != ?1",
                            params![paused, subscription_id],
                        )?;
                        if changed == 0 {
                            require_subscription(transaction, subscription_id)?;
                            return Ok(None);
                        }
                        set_active_subscription_pause_state(transaction, subscription_id, paused)?;
                        Ok(Some(()))
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
        let next_run_at = subscriptions_v2::next_schedule_at(schedule, now)?;
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
        crate::onlyfans_source_v2::clear_subscription_state(self.root(), subscription_id)?;
        finish_subscription_mutation(
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
        )
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
                    let run = subscriptions_v2::create_run_in(
                        transaction,
                        subscription_id,
                        "manual",
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
                            "SELECT run_id, status FROM subscription_run
                             WHERE subscription_id = ?1
                               AND status IN ('pending', 'running')
                             ORDER BY run_id LIMIT 1",
                            [subscription_id],
                            |row| {
                                Ok(CreatedRun {
                                    run_id: row.get(0)?,
                                    created: false,
                                    state: subscriptions_v2::parse_run_state(row.get(1)?)?,
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
    vec![
        resources::SUBSCRIPTIONS.to_string(),
        resources::TASKS.to_string(),
    ]
}

fn finish_subscription_mutation(
    application: &LibraryApplication,
    published: Option<((), picto_library::MutationReceipt)>,
) -> Result<picto_library::MutationReceipt, String> {
    if let Some(((), receipt)) = published {
        return Ok(receipt);
    }
    Ok(current_subscription_receipt(application)?)
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

pub fn list(application: &Application) -> Result<SubscriptionList, String> {
    application.store().read_snapshot_captured(
        || application.projections().selection_snapshot(),
        |connection, _, projection| {
            (|| -> rusqlite::Result<SubscriptionList> {
                let mut subscriptions = connection
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
                 media_totals AS (
                     SELECT ssp.subscription_id,
                            COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                                THEN si.media_item_id END) AS media_count
                     FROM subscription_source_post ssp
                     JOIN source_item si ON si.source_post_id = ssp.source_post_id
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
                            COUNT(DISTINCT ssp.source_post_id) AS posts_traversed
                     FROM active_runs active
                     JOIN subscription_run_query srq ON srq.run_id = active.run_id
                     JOIN subscription_source_post ssp
                       ON ssp.query_id = srq.query_id
                      AND ssp.last_seen_run_id = srq.run_id
                     GROUP BY srq.run_id
                 ),
                 item_progress AS (
                     SELECT srq.run_id,
                            COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                                THEN si.source_post_id END) AS posts_added,
                            COUNT(DISTINCT rsi.source_item_id) AS discovered,
                            COUNT(DISTINCT CASE WHEN si.state IN ('downloaded', 'ingested')
                                                THEN rsi.source_item_id END) AS downloaded,
                            COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                                THEN rsi.source_item_id END) AS ingested,
                            COUNT(DISTINCT CASE WHEN si.state = 'failed'
                                                THEN rsi.source_item_id END) AS failed,
                            COUNT(DISTINCT CASE WHEN si.state = 'deleted'
                                                THEN rsi.source_item_id END) AS deleted
                     FROM active_runs active
                     JOIN subscription_run_query srq ON srq.run_id = active.run_id
                     LEFT JOIN subscription_run_source_item rsi
                       ON rsi.run_query_id = srq.run_query_id
                     LEFT JOIN source_item si ON si.source_item_id = rsi.source_item_id
                     GROUP BY srq.run_id
                 )
                 SELECT s.subscription_id, s.name, s.schedule, s.paused,
                        s.initial_post_limit, s.periodic_post_limit, s.next_run_at,
                        active.run_id,
                        CASE
                            WHEN active.status = 'pending'
                             AND active.failure_kind IN ('paused', 'inbox_full')
                            THEN active.failure_kind
                            ELSE COALESCE(active.status, latest.status)
                        END,
                        COALESCE(media_totals.media_count, 0),
                        COALESCE(issue_totals.issue_count, 0),
                        NULL,
                        COALESCE(traversed_posts.posts_traversed, 0),
                        COALESCE(item_progress.posts_added, 0),
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
                 LEFT JOIN media_totals USING (subscription_id)
                 LEFT JOIN issue_totals USING (subscription_id)
                 LEFT JOIN traversed_posts ON traversed_posts.run_id = active.run_id
                 LEFT JOIN item_progress ON item_progress.run_id = active.run_id
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
                            progress: SubscriptionProgress {
                                posts_traversed: row.get(12)?,
                                posts_added: row.get(13)?,
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
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut queries = query_views_by_subscription(connection)?;
                let (mut destinations, mut covers) =
                    subscription_settings_by_id(connection, application.blobs(), &projection)?;
                for subscription in &mut subscriptions {
                    subscription.queries = queries
                        .remove(&subscription.subscription_id)
                        .unwrap_or_default();
                    subscription.destination = destinations
                        .remove(&subscription.subscription_id)
                        .unwrap_or_default();
                    if let Some(cover) = covers.remove(&subscription.subscription_id) {
                        if let Some((selection, file_hash)) = cover {
                            subscription.cover_file_hash = Some(file_hash);
                            subscription.cover_focus_x = selection.focus_x;
                            subscription.cover_focus_y = selection.focus_y;
                            subscription.cover_zoom_percent = selection.zoom_percent;
                        } else {
                            subscription.cover_file_hash = None;
                        }
                    }
                }
                Ok(SubscriptionList {
                    subscriptions,
                    revision: crate::store::schema::revision(connection)?,
                })
            })()
            .map_err(|error| error.to_string())
        },
    )
}

impl Application {
    pub fn set_subscription_cover(
        &self,
        subscription_id: i64,
        selection: &SubscriptionCoverSelection,
    ) -> Result<MutationReceipt, String> {
        validate_cover_selection(selection)?;
        let source = self.store().read_snapshot_captured(
            || self.projections().selection_snapshot(),
            |connection, _, projection| {
                require_subscription(connection, subscription_id)
                    .map_err(|error| error.to_string())?;
                require_subscription_cover_candidate(
                    connection,
                    &projection,
                    subscription_id,
                    selection.media_item_id,
                )
                .map_err(|error| error.to_string())
            },
        )?;
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
        let (previous_rendered_hash, revision, changed) =
            self.store().transaction_if_changed(|transaction| {
                require_subscription(transaction, subscription_id)?;
                let current_source = require_subscription_cover_candidate(
                    transaction,
                    &self.projections().selection_snapshot(),
                    subscription_id,
                    selection.media_item_id,
                )?;
                if current_source.file_hash != source.file_hash
                    || current_source.root_item_id != source.root_item_id
                {
                    return Err(invalid("cover media changed while rendering"));
                }
                let previous: Option<String> = transaction
                    .query_row(
                        "SELECT value_json FROM setting WHERE key = ?1",
                        [&key],
                        |row| row.get(0),
                    )
                    .optional()?;
                if previous.as_deref() == Some(value.as_str()) {
                    return Ok((stored_rendered_hash(previous.as_deref()), false));
                }
                let previous_rendered_hash = stored_rendered_hash(previous.as_deref());
                transaction.execute(
                    "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                    params![key, value],
                )?;
                Ok((previous_rendered_hash, true))
            })?;
        if changed && previous_rendered_hash.as_deref() != Some(rendered_hash.as_str()) {
            if let Some(previous_rendered_hash) = previous_rendered_hash {
                let _ = self.blobs().delete_thumbnail(&previous_rendered_hash);
            }
        }
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
                set_active_query_pause_state(transaction, query_id, paused)?;
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
                set_active_subscription_pause_state(transaction, subscription_id, paused)?;
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

fn set_active_subscription_pause_state(
    transaction: &rusqlite::Transaction<'_>,
    subscription_id: i64,
    paused: bool,
) -> rusqlite::Result<()> {
    if paused {
        transaction.execute(
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
        transaction.execute(
            "UPDATE subscription_run
             SET status = 'pending', finished_at = NULL,
                 failure_kind = 'paused', error_message = 'Paused by user.'
             WHERE subscription_id = ?1 AND status IN ('pending', 'running')",
            [subscription_id],
        )?;
    } else {
        transaction.execute(
            "UPDATE subscription_run_query
             SET failure_kind = NULL, error_message = NULL
             WHERE status = 'pending' AND failure_kind = 'paused'
               AND run_id IN (
                   SELECT run_id FROM subscription_run
                   WHERE subscription_id = ?1 AND status = 'pending'
               )",
            [subscription_id],
        )?;
        transaction.execute(
            "UPDATE subscription_run
             SET failure_kind = NULL
             WHERE subscription_id = ?1 AND status = 'pending'
               AND failure_kind = 'paused'",
            [subscription_id],
        )?;
    }
    Ok(())
}

fn set_active_query_pause_state(
    transaction: &rusqlite::Transaction<'_>,
    query_id: i64,
    paused: bool,
) -> rusqlite::Result<()> {
    if paused {
        transaction.execute(
            "UPDATE subscription_run_query
             SET status = 'pending', started_at = NULL, finished_at = NULL,
                 attempt_count = CASE WHEN status = 'running'
                                      THEN MAX(attempt_count - 1, 0)
                                      ELSE attempt_count END,
                 failure_kind = 'paused', error_message = 'Paused by user.'
             WHERE query_id = ?1 AND status IN ('pending', 'running')",
            [query_id],
        )?;
        transaction.execute(
            "UPDATE subscription_run
             SET failure_kind = 'paused', error_message = 'Paused by user.'
             WHERE run_id IN (
                 SELECT run_id FROM subscription_run_query
                 WHERE query_id = ?1 AND status = 'pending'
             ) AND status IN ('pending', 'running')",
            [query_id],
        )?;
    } else {
        transaction.execute(
            "UPDATE subscription_run_query
             SET failure_kind = NULL, error_message = NULL
             WHERE query_id = ?1 AND status = 'pending' AND failure_kind = 'paused'",
            [query_id],
        )?;
        transaction.execute(
            "UPDATE subscription_run
             SET failure_kind = NULL
             WHERE status = 'pending' AND failure_kind = 'paused'
               AND run_id IN (
                   SELECT run_id FROM subscription_run_query WHERE query_id = ?1
               )
               AND NOT EXISTS (
                   SELECT 1 FROM subscription_run_query pending
                   WHERE pending.run_id = subscription_run.run_id
                     AND pending.status = 'pending'
                     AND pending.failure_kind = 'paused'
               )",
            [query_id],
        )?;
    }
    Ok(())
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

pub fn subscription_cover_candidates(
    application: &Application,
    subscription_id: i64,
    cursor: Option<&SubscriptionCoverCandidateCursor>,
    limit: i64,
) -> Result<SubscriptionCoverCandidatePage, String> {
    const MAX_LIMIT: i64 = 200;
    let limit = limit.clamp(1, MAX_LIMIT);
    application.store().read_snapshot_captured(
        || application.projections().selection_snapshot(),
        |connection, _, projection| {
            (|| -> rusqlite::Result<SubscriptionCoverCandidatePage> {
                require_subscription(connection, subscription_id)?;
                let active = projection.lifecycle_bitmap(crate::app::Lifecycle::Active);
                let mut raw_cursor = cursor.cloned();
                let mut rows = Vec::new();
                let batch_limit = ((limit + 1) * 4).clamp(64, 800);
                loop {
                    let cursor_imported_at =
                        raw_cursor.as_ref().map(|value| value.imported_at.as_str());
                    let cursor_media_item_id = raw_cursor.as_ref().map(|value| value.media_item_id);
                    let batch = connection
                        .prepare(
                            "SELECT ma.item_id, mf.file_hash, ma.name,
                                mf.pixel_width, mf.pixel_height, ma.imported_at
                         FROM subscription_source_post ssp
                         JOIN source_item si ON si.source_post_id = ssp.source_post_id
                         JOIN media_asset ma ON ma.item_id = si.media_item_id
                         JOIN media_file mf ON mf.file_id = ma.file_id
                         WHERE ssp.subscription_id = ?1
                           AND +si.state = 'ingested'
                           AND mf.mime_type LIKE 'image/%'
                           AND (?2 IS NULL
                                OR ma.imported_at < ?2
                                OR (ma.imported_at = ?2 AND ma.item_id < ?3))
                         GROUP BY ma.item_id, mf.file_hash, ma.name,
                                  mf.pixel_width, mf.pixel_height, ma.imported_at
                         ORDER BY ma.imported_at DESC, ma.item_id DESC
                         LIMIT ?4",
                        )?
                        .query_map(
                            params![
                                subscription_id,
                                cursor_imported_at,
                                cursor_media_item_id,
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
                        projection
                            .root_for_media(candidate.media_item_id)
                            .and_then(|root_id| u32::try_from(root_id).ok())
                            .is_some_and(|root_id| active.contains(root_id))
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
            })()
            .map_err(|error| error.to_string())
        },
    )
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
    projection: &ProjectionSelectionSnapshot,
    subscription_id: i64,
    media_item_id: i64,
) -> rusqlite::Result<SubscriptionCoverSource> {
    find_subscription_cover_candidate(connection, projection, subscription_id, media_item_id)?
        .ok_or_else(|| invalid("cover media is not active in this subscription"))
}

fn find_subscription_cover_candidate(
    connection: &rusqlite::Connection,
    projection: &ProjectionSelectionSnapshot,
    subscription_id: i64,
    media_item_id: i64,
) -> rusqlite::Result<Option<SubscriptionCoverSource>> {
    let Some(root_item_id) = projection.root_for_media(media_item_id) else {
        return Ok(None);
    };
    let Some(root_id) = u32::try_from(root_item_id).ok() else {
        return Ok(None);
    };
    if !projection
        .lifecycle_bitmap(crate::app::Lifecycle::Active)
        .contains(root_id)
    {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT mf.file_hash, mf.mime_type
             FROM subscription_source_post ssp
             JOIN source_item si ON si.source_post_id = ssp.source_post_id
             JOIN media_asset ma ON ma.item_id = si.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             WHERE ssp.subscription_id = ?1
               AND ma.item_id = ?2
               AND si.state = 'ingested'
             LIMIT 1",
            params![subscription_id, media_item_id],
            |row| {
                Ok(SubscriptionCoverSource {
                    root_item_id,
                    file_hash: row.get(0)?,
                    mime_type: row.get(1)?,
                })
            },
        )
        .optional()
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
            "SELECT file.content_hash, file.mime
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
                    root_item_id: i64::from(root_id.0),
                    file_hash: row.get(0)?,
                    mime_type: row.get(1)?,
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

    let original = blobs
        .read_original(
            &source.file_hash,
            Some(mime_to_extension(&source.mime_type)),
        )
        .ok();
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

fn subscription_settings_by_id(
    connection: &rusqlite::Connection,
    blobs: &BlobStore,
    projection: &ProjectionSelectionSnapshot,
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
                    SELECT mf.file_hash
                    FROM subscription_source_post ssp
                    JOIN source_item si ON si.source_post_id = ssp.source_post_id
                    JOIN media_asset ma ON ma.item_id = si.media_item_id
                    JOIN media_file mf ON mf.file_id = ma.file_id
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
                .and_then(|media_item_id| projection.root_for_media(media_item_id))
                .and_then(|root_id| u32::try_from(root_id).ok())
                .is_some_and(|root_id| {
                    projection
                        .lifecycle_bitmap(crate::app::Lifecycle::Active)
                        .contains(root_id)
                });
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

fn query_views_by_subscription(
    connection: &rusqlite::Connection,
) -> rusqlite::Result<HashMap<i64, Vec<SubscriptionQueryView>>> {
    let mut statement = connection.prepare(
        "WITH source_counts AS (
             SELECT ssp.query_id,
                    COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                        THEN ssp.source_post_id END) AS post_count,
                    COUNT(DISTINCT CASE WHEN si.state = 'ingested'
                                        THEN si.media_item_id END) AS media_count
             FROM subscription_source_post ssp
             LEFT JOIN source_item si ON si.source_post_id = ssp.source_post_id
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
    use crate::app::{ItemTarget, Lifecycle};
    use crate::ingest_v2::{PreparedMediaInput, SourcePostInput};
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

    fn ingest_subscription_media(
        application: &Application,
        subscription_id: i64,
        query_id: i64,
        file_hash: &str,
        mime_type: &str,
        name: &str,
        post_key: &str,
        imported_at: &str,
    ) -> i64 {
        let result = application
            .ingest_prepared(&PreparedMediaInput {
                file_hash: file_hash.to_string(),
                mime_type: mime_type.to_string(),
                size_bytes: 10,
                pixel_width: Some(800),
                pixel_height: Some(600),
                duration_ms: None,
                frame_count: Some(1),
                has_audio: false,
                name: Some(name.to_string()),
                notes: None,
                rating: None,
                source_urls: Vec::new(),
                tags: Vec::new(),
                lifecycle: Lifecycle::Active,
                captured_at: None,
                source: Some(SourcePostInput {
                    site_id: "pixiv".to_string(),
                    post_key: post_key.to_string(),
                    item_key: format!("media:{post_key}"),
                    position: 0,
                    post_complete: true,
                    force_collection: false,
                    group_post: false,
                    canonical_post_url: None,
                    canonical_media_url: None,
                    creator_name: None,
                    title: None,
                    description: None,
                    captured_at: None,
                    metadata_json: None,
                }),
                target_folder_id: None,
                target_folder_ids: Vec::new(),
            })
            .unwrap();
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET imported_at = ?2 WHERE item_id = ?1",
                    params![result.media_item_id.0, imported_at],
                )?;
                transaction.execute(
                    "INSERT INTO subscription_source_post (
                         subscription_id, query_id, source_post_id
                     )
                     SELECT ?1, ?2, source_post_id
                     FROM source_post
                     WHERE site_id = 'pixiv' AND post_key = ?3",
                    params![subscription_id, query_id, post_key],
                )?;
                Ok(())
            })
            .unwrap();
        result.media_item_id.0
    }

    #[test]
    fn create_is_atomic_and_list_reads_one_persisted_view() {
        let (_directory, application) = fixture();
        let (subscription_id, receipt) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(receipt.revision, 2);

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
        assert!(application.history_state().unwrap().undo.is_none());
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
        let source_image =
            image::DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(800, 600, |x, y| {
                image::Rgb([(x % 255) as u8, (y % 255) as u8, 80])
            }));
        let mut source_bytes = Cursor::new(Vec::new());
        source_image
            .write_to(&mut source_bytes, image::ImageFormat::Png)
            .unwrap();
        let source_hash = hex::encode(Sha256::digest(source_bytes.get_ref()));
        application
            .blobs()
            .write_original(&source_hash, source_bytes.get_ref(), Some("png"))
            .unwrap();
        let media_item_id = ingest_subscription_media(
            &application,
            subscription_id,
            query_id,
            &source_hash,
            "image/png",
            "Cover",
            "post",
            "2026-01-01T00:00:00Z",
        );

        let page = subscription_cover_candidates(&application, subscription_id, None, 200).unwrap();
        assert_eq!(page.candidates.len(), 1);
        assert_eq!(page.candidates[0].file_hash, source_hash);
        assert!(page.next_cursor.is_none());

        application
            .set_subscription_cover(
                subscription_id,
                &SubscriptionCoverSelection {
                    media_item_id,
                    focus_x: 250,
                    focus_y: 750,
                    zoom_percent: 160,
                },
            )
            .unwrap();
        let subscription = &list(&application).unwrap().subscriptions[0];
        let rendered_hash = subscription
            .cover_file_hash
            .as_deref()
            .expect("rendered cover hash");
        assert_ne!(rendered_hash, source_hash);
        assert_eq!(rendered_hash.len(), 64);
        let rendered = application
            .blobs()
            .read_thumbnail(rendered_hash)
            .unwrap()
            .expect("rendered cover thumbnail");
        let rendered = image::load_from_memory(&rendered).unwrap();
        assert_eq!((rendered.width(), rendered.height()), (384, 384));
        assert_eq!(subscription.cover_focus_x, 500);
        assert_eq!(subscription.cover_focus_y, 500);
        assert_eq!(subscription.cover_zoom_percent, 100);

        application
            .set_lifecycle(
                &ItemTarget::Explicit {
                    item_ids: vec![crate::app::ItemId(media_item_id)],
                },
                Lifecycle::Inbox,
            )
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
    fn cover_candidates_are_loaded_only_when_requested() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let query_id = list(&application).unwrap().subscriptions[0].queries[0].query_id;
        let mut media_ids = Vec::new();
        for (position, imported_at, mime_type) in [
            (1_i64, "2026-01-01T00:00:01Z", "image/jpeg"),
            (2_i64, "2026-01-01T00:00:03Z", "image/jpeg"),
            (3_i64, "2026-01-01T00:00:03Z", "image/jpeg"),
            (4_i64, "2026-01-01T00:00:04Z", "video/mp4"),
        ] {
            media_ids.push(ingest_subscription_media(
                &application,
                subscription_id,
                query_id,
                &format!("hash-{position}"),
                mime_type,
                &format!("Cover {position}"),
                &format!("post-{position}"),
                imported_at,
            ));
        }
        // Retraversal and state bookkeeping must not make an old image the cover.
        application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE source_item SET updated_at = '2099-01-01T00:00:00Z'
                     WHERE source_post_id = (
                         SELECT source_post_id FROM source_post
                         WHERE site_id = 'pixiv' AND post_key = 'post-1'
                     )",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(list(&application).unwrap().subscriptions[0]
            .cover_file_hash
            .is_none());
        let first = subscription_cover_candidates(&application, subscription_id, None, 2).unwrap();
        assert_eq!(
            first
                .candidates
                .iter()
                .map(|candidate| candidate.media_item_id)
                .collect::<Vec<_>>(),
            vec![media_ids[2], media_ids[1]]
        );
        let cursor = first.next_cursor.expect("first page cursor");
        assert_eq!(cursor.imported_at, "2026-01-01T00:00:03Z");
        assert_eq!(cursor.media_item_id, media_ids[1]);

        let second =
            subscription_cover_candidates(&application, subscription_id, Some(&cursor), 2).unwrap();
        assert_eq!(
            second
                .candidates
                .iter()
                .map(|candidate| candidate.media_item_id)
                .collect::<Vec<_>>(),
            vec![media_ids[0]]
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
    fn pausing_an_active_subscription_persists_a_resumable_run() {
        let (_directory, application) = fixture();
        let (subscription_id, _) = application
            .create_subscription_definition(&input(), "2026-01-01T00:00:00Z")
            .unwrap();
        let (run, _) = application
            .request_subscription_run(subscription_id, "2026-01-01T00:00:01Z")
            .unwrap();
        let query_id = list(&application).unwrap().subscriptions[0].queries[0].query_id;

        application
            .pause_subscription(subscription_id, true)
            .unwrap();

        let paused = list(&application).unwrap().subscriptions.remove(0);
        assert!(paused.paused);
        assert_eq!(paused.status.as_deref(), Some("paused"));
        let state: (String, Option<String>, String, Option<String>) = application
            .store()
            .read(|connection| {
                let run_state = connection.query_row(
                    "SELECT status, failure_kind FROM subscription_run WHERE run_id = ?1",
                    [run.run_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                let query = connection.query_row(
                    "SELECT status, failure_kind FROM subscription_run_query
                     WHERE run_id = ?1 AND query_id = ?2",
                    params![run.run_id, query_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                Ok((run_state.0, run_state.1, query.0, query.1))
            })
            .unwrap();
        assert_eq!(
            state,
            (
                "pending".into(),
                Some("paused".into()),
                "pending".into(),
                Some("paused".into()),
            )
        );

        application
            .pause_subscription(subscription_id, false)
            .unwrap();

        let resumed = list(&application).unwrap().subscriptions.remove(0);
        assert!(!resumed.paused);
        assert_eq!(resumed.status.as_deref(), Some("pending"));
        let reasons: (Option<String>, Option<String>) = application
            .store()
            .read(|connection| {
                Ok((
                    connection.query_row(
                        "SELECT failure_kind FROM subscription_run WHERE run_id = ?1",
                        [run.run_id],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT failure_kind FROM subscription_run_query
                         WHERE run_id = ?1 AND query_id = ?2",
                        params![run.run_id, query_id],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(reasons, (None, None));
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
