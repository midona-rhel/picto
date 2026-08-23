//! Product-facing subscription definitions and persisted status reads.

use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, MutationReceipt};
use crate::subscriptions::gallery_dl_runner::{build_url, site_by_id};
use crate::subscriptions::source_adapter::{
    normalize_query_text, validate_query_kind, validate_query_text,
};
use crate::subscriptions_v2::{self, CreatedRun};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct NewSubscriptionQuery {
    pub site_id: String,
    pub query_kind: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
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
    pub paused: bool,
    pub initial_run_complete: bool,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_message: Option<String>,
    #[ts(type = "number")]
    pub post_count: i64,
    #[ts(type = "number")]
    pub media_count: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionProgress {
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
    pub cover_file_hash: Option<String>,
    pub progress: SubscriptionProgress,
    pub queries: Vec<SubscriptionQueryView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionList {
    pub subscriptions: Vec<SubscriptionView>,
    #[ts(type = "number")]
    pub revision: u64,
}

pub fn list(application: &Application) -> Result<SubscriptionList, String> {
    application.store().read(|connection| {
        let rows = connection
            .prepare(
                "SELECT s.subscription_id, s.name, s.schedule, s.paused,
                        s.initial_post_limit, s.periodic_post_limit, s.next_run_at,
                        active.run_id, COALESCE(active.status, latest.status),
                        COUNT(DISTINCT CASE WHEN si.state = 'ingested' THEN si.media_item_id END),
                        (
                            SELECT mf.file_hash
                            FROM subscription_source_post cover_ssp
                            JOIN source_item cover_si
                              ON cover_si.source_post_id = cover_ssp.source_post_id
                            JOIN media_asset cover_ma
                              ON cover_ma.item_id = cover_si.media_item_id
                            JOIN media_file mf ON mf.file_id = cover_ma.file_id
                            WHERE cover_ssp.subscription_id = s.subscription_id
                              AND cover_si.state = 'ingested'
                            ORDER BY cover_si.updated_at DESC, cover_si.source_item_id DESC
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
                    cover_file_hash: row.get(10)?,
                    progress: SubscriptionProgress::default(),
                    queries: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut subscriptions = Vec::with_capacity(rows.len());
        for mut subscription in rows {
            subscription.queries = query_views(connection, subscription.subscription_id)?;
            if let Some(run_id) = subscription.active_run_id {
                subscription.progress = connection.query_row(
                    "SELECT
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
                            discovered: row.get(0)?,
                            downloaded: row.get(1)?,
                            ingested: row.get(2)?,
                            failed: row.get(3)?,
                            deleted: row.get(4)?,
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
            for query in &queries {
                insert_query(transaction, subscription_id, query)?;
            }
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
            insert_query(transaction, subscription_id, &query)
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
            }
            Ok(((), changed != 0))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn delete_subscription_query(&self, query_id: i64) -> Result<MutationReceipt, String> {
        let (_, revision) = self.store().transaction(|transaction| {
            reject_active_query_edit(transaction, query_id)?;
            let subscription_id: i64 = transaction
                .query_row(
                    "SELECT subscription_id FROM subscription_query WHERE query_id = ?1",
                    [query_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| invalid("subscription query does not exist"))?;
            let query_count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM subscription_query WHERE subscription_id = ?1",
                [subscription_id],
                |row| row.get(0),
            )?;
            if query_count == 1 {
                return Err(invalid("a subscription needs at least one query"));
            }
            transaction.execute(
                "DELETE FROM subscription_query WHERE query_id = ?1",
                [query_id],
            )?;
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
            Ok(((), true))
        })?;
        Ok(subscription_receipt(revision))
    }

    pub fn delete_subscription(&self, subscription_id: i64) -> Result<MutationReceipt, String> {
        let (_, revision) = self.store().transaction(|transaction| {
            let changed = transaction.execute(
                "DELETE FROM subscription WHERE subscription_id = ?1",
                [subscription_id],
            )?;
            if changed != 1 {
                return Err(invalid("subscription does not exist"));
            }
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

fn query_views(
    connection: &rusqlite::Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQueryView>> {
    connection
        .prepare(
            "SELECT q.query_id, q.site_id, q.query_kind, q.query_text,
                    q.display_name, q.notes, q.paused, q.initial_run_complete,
                    q.last_success_at, q.last_failure_at, q.last_failure_kind,
                    q.last_failure_message,
                    COUNT(DISTINCT ssp.source_post_id),
                    COUNT(DISTINCT CASE WHEN si.state = 'ingested' THEN si.media_item_id END)
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
                paused: row.get(6)?,
                initial_run_complete: row.get(7)?,
                last_success_at: row.get(8)?,
                last_failure_at: row.get(9)?,
                last_failure_kind: row.get(10)?,
                last_failure_message: row.get(11)?,
                post_count: row.get(12)?,
                media_count: row.get(13)?,
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
}

fn insert_query(
    transaction: &rusqlite::Transaction<'_>,
    subscription_id: i64,
    query: &PreparedQuery,
) -> rusqlite::Result<i64> {
    transaction.execute(
        "INSERT INTO subscription_query (
             query_key, subscription_id, site_id, domain_key, query_kind,
             query_text, display_name, notes
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            new_key("query"),
            subscription_id,
            query.site_id,
            query.domain_key,
            query.query_kind,
            query.query_text.trim(),
            query.display_name,
            query.notes,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn validate_subscription(input: &NewSubscription) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("Subscription name is required".to_string());
    }
    subscriptions_v2::next_schedule_at(&input.schedule, "2026-01-01T00:00:00Z")?;
    if input.queries.is_empty() {
        return Err("A subscription needs at least one query".to_string());
    }
    Ok(())
}

fn prepare_query(query: &NewSubscriptionQuery) -> Result<PreparedQuery, String> {
    let site_id = query.site_id.trim();
    let query_kind = query.query_kind.trim();
    validate_query_kind(site_id, query_kind)?;
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
                query_kind: "user".into(),
                query_text: "42".into(),
                display_name: Some("Artist".into()),
                notes: None,
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

        let list = list(&application).unwrap();
        assert_eq!(list.subscriptions.len(), 1);
        let subscription = &list.subscriptions[0];
        assert_eq!(subscription.subscription_id, subscription_id);
        assert_eq!(subscription.queries.len(), 1);
        assert_eq!(subscription.queries[0].query_text, "42");
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
    fn query_edits_are_normalized_and_cannot_remove_the_last_query() {
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
                    query_kind: "user".into(),
                    query_text: "@NASA".into(),
                    display_name: Some(" NASA ".into()),
                    notes: Some(" ".into()),
                },
            )
            .unwrap();
        let query = &list(&application).unwrap().subscriptions[0].queries[0];
        assert_eq!(query.query_text, "NASA");
        assert_eq!(query.display_name.as_deref(), Some("NASA"));
        assert_eq!(query.notes, None);
        assert!(application.delete_subscription_query(query_id).is_err());

        application
            .add_subscription_query(
                subscription_id,
                &NewSubscriptionQuery {
                    site_id: "e621".into(),
                    query_kind: "search".into(),
                    query_text: "canine".into(),
                    display_name: None,
                    notes: None,
                },
            )
            .unwrap();
        application.delete_subscription_query(query_id).unwrap();
        assert_eq!(
            list(&application).unwrap().subscriptions[0].queries.len(),
            1
        );
    }
}
