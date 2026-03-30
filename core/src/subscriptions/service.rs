//! Subscription CRUD and reset behavior.

use rusqlite::params;

use crate::blob_store::BlobStore;
use crate::sqlite::SqliteDatabase;
use crate::subscriptions::archive::{
    clear_subscription_archive_entries, subscription_query_archive_prefix,
};
use crate::types::{RunningSubscriptions, SubscriptionInfo, SubscriptionQueryInfo};

// Bulk read — 2 queries instead of O(N).
pub async fn get_subscriptions(db: &SqliteDatabase) -> Result<Vec<SubscriptionInfo>, String> {
    let start = std::time::Instant::now();

    let subs_with_counts = db.list_subscriptions_with_file_counts().await?;
    let all_queries = db.list_all_subscription_queries().await?;

    let mut queries_map: std::collections::HashMap<i64, Vec<SubscriptionQueryInfo>> =
        std::collections::HashMap::new();
    for q in all_queries {
        queries_map
            .entry(q.subscription_id)
            .or_default()
            .push(SubscriptionQueryInfo {
                id: q.query_id.to_string(),
                site_id: crate::subscriptions::gallery_dl_runner::canonical_site_id(&q.site_id)
                    .to_string(),
                query_text: q.query_text.clone(),
                display_name: q.display_name.or(Some(q.query_text)),
                notes: q.notes,
                paused: q.paused,
                last_check_time: q.last_check_time,
                files_found: q.files_found as u64,
                posts_found: q.posts_found as u64,
                completed_initial_run: q.completed_initial_run,
                resume_cursor: q.resume_cursor,
                resume_strategy: q.resume_strategy,
                last_success_at: q.last_success_at,
                last_failure_at: q.last_failure_at,
                last_failure_kind: q.last_failure_kind,
                last_failure_message: q.last_failure_message,
            });
    }

    let result: Vec<SubscriptionInfo> = subs_with_counts
        .into_iter()
        .map(|(sub, total_files)| {
            let sub_id = sub.subscription_id;
            SubscriptionInfo {
                id: sub_id.to_string(),
                name: sub.name,
                paused: sub.paused,
                group_id: sub.group_id.map(|id| id.to_string()),
                initial_post_limit: sub.initial_post_limit as u32,
                periodic_post_limit: sub.periodic_post_limit as u32,
                auto_collections: sub.auto_collections,
                created_at: sub.created_at,
                total_files: total_files as u64,
                queries: queries_map.remove(&sub_id).unwrap_or_default(),
            }
        })
        .collect();

    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        count = result.len(),
        "get_subscriptions bulk read"
    );

    Ok(result)
}

pub async fn create_subscription(
    db: &SqliteDatabase,
    name: String,
    group_id: Option<i64>,
    initial_post_limit: Option<u32>,
    periodic_post_limit: Option<u32>,
) -> Result<SubscriptionInfo, String> {
    let sub = db.create_subscription(&name, group_id).await?;
    let sub_id = sub.subscription_id;

    if initial_post_limit.is_some() || periodic_post_limit.is_some() {
        let il = initial_post_limit.unwrap_or(100) as i64;
        let pl = periodic_post_limit.unwrap_or(50) as i64;
        db.with_conn(move |conn| {
            conn.execute(
                "UPDATE subscription SET initial_post_limit = ?1, periodic_post_limit = ?2
                 WHERE subscription_id = ?3",
                params![il, pl, sub_id],
            )?;
            Ok(())
        })
        .await?;
    }

    Ok(SubscriptionInfo {
        id: sub_id.to_string(),
        name,
        paused: false,
        group_id: group_id.map(|id| id.to_string()),
        initial_post_limit: initial_post_limit.unwrap_or(100),
        periodic_post_limit: periodic_post_limit.unwrap_or(50),
        auto_collections: true,
        created_at: sub.created_at,
        total_files: 0,
        queries: vec![],
    })
}

pub async fn delete_subscription(
    db: &SqliteDatabase,
    _blob_store: &BlobStore,
    id: String,
) -> Result<usize, String> {
    let sub_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", id))?;
    db.delete_subscription(sub_id).await?;
    Ok(1)
}

pub async fn pause_subscription(
    db: &SqliteDatabase,
    id: String,
    paused: bool,
) -> Result<(), String> {
    let sub_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", id))?;
    db.set_subscription_paused(sub_id, paused).await
}

pub async fn add_subscription_query(
    db: &SqliteDatabase,
    subscription_id: String,
    site_id: String,
    query_text: String,
    notes: Option<String>,
) -> Result<SubscriptionQueryInfo, String> {
    let sub_id: i64 = subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", subscription_id))?;
    if crate::subscriptions::gallery_dl_runner::site_by_id(&site_id).is_none() {
        return Err(format!("Unknown site: {site_id}"));
    }
    let canonical_site_id =
        crate::subscriptions::gallery_dl_runner::canonical_site_id(&site_id).to_string();
    let q = db
        .add_subscription_query(
            sub_id,
            &canonical_site_id,
            query_text.trim(),
            Some(query_text.trim()),
            notes.as_deref(),
        )
        .await?;
    Ok(SubscriptionQueryInfo {
        id: q.query_id.to_string(),
        site_id: canonical_site_id,
        query_text: q.query_text,
        display_name: q.display_name,
        notes: q.notes,
        paused: q.paused,
        last_check_time: q.last_check_time,
        files_found: q.files_found as u64,
        posts_found: q.posts_found as u64,
        completed_initial_run: q.completed_initial_run,
        resume_cursor: q.resume_cursor,
        resume_strategy: q.resume_strategy,
        last_success_at: q.last_success_at,
        last_failure_at: q.last_failure_at,
        last_failure_kind: q.last_failure_kind,
        last_failure_message: q.last_failure_message,
    })
}

pub async fn delete_subscription_query(db: &SqliteDatabase, id: String) -> Result<(), String> {
    let query_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid query id: {}", id))?;
    db.delete_subscription_query(query_id).await
}

pub async fn pause_subscription_query(
    db: &SqliteDatabase,
    id: String,
    paused: bool,
) -> Result<(), String> {
    let query_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid query id: {}", id))?;
    db.set_query_paused(query_id, paused).await
}

pub async fn rename_subscription(
    db: &SqliteDatabase,
    id: String,
    name: String,
) -> Result<(), String> {
    let sub_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", id))?;
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    db.rename_subscription(sub_id, &trimmed).await
}

pub async fn reset_subscription(db: &SqliteDatabase, id: String) -> Result<(), String> {
    let sub_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", id))?;
    let queries = db.get_subscription_queries(sub_id).await?;
    let mut archive_prefixes: Vec<String> = queries
        .iter()
        .map(|q| subscription_query_archive_prefix(sub_id, q.query_id))
        .collect();
    archive_prefixes.push(format!("picto_s{sub_id}_q"));

    let (queries_reset, entities_deleted, post_maps_deleted) =
        db.reset_subscription_state(sub_id).await?;
    clear_subscription_archive_entries(db, &archive_prefixes).await?;

    tracing::info!(
        subscription_id = sub_id,
        queries_reset,
        entities_deleted,
        post_maps_deleted,
        "Subscription reset: state cleared"
    );
    Ok(())
}

pub async fn reset_subscription_query_checked(
    db: &SqliteDatabase,
    running_subs: &RunningSubscriptions,
    id: String,
) -> Result<(), String> {
    let query_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid query id: {}", id))?;
    let query = db
        .get_subscription_query(query_id)
        .await?
        .ok_or_else(|| format!("Query {} not found", id))?;
    let sub_id_str = query.subscription_id.to_string();

    {
        let map = running_subs.lock().await;
        if map.contains_key(&sub_id_str) {
            return Err(format!(
                "Subscription {} is running; stop it before resetting query {}",
                query.subscription_id, query_id
            ));
        }
    }

    let archive_prefix = subscription_query_archive_prefix(query.subscription_id, query_id);
    let (query_reset, query_runs_deleted, issues_deleted, attempts_deleted, queues_deleted) =
        db.reset_subscription_query_state(query_id).await?;
    clear_subscription_archive_entries(db, &[archive_prefix]).await?;

    tracing::info!(
        subscription_id = query.subscription_id,
        query_id,
        query_reset,
        query_runs_deleted,
        issues_deleted,
        attempts_deleted,
        queues_deleted,
        "Subscription query reset: state cleared"
    );
    Ok(())
}

pub async fn reset_subscription_checked(
    db: &SqliteDatabase,
    running_subs: &RunningSubscriptions,
    id: String,
) -> Result<(), String> {
    {
        let map = running_subs.lock().await;
        if map.contains_key(&id) {
            return Err(format!(
                "Subscription {} is running; stop it before reset",
                id
            ));
        }
    }
    reset_subscription(db, id).await
}
