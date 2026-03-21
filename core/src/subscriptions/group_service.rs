//! Subscription group orchestration — groups subscriptions into scheduled execution units.
//!
//! Owns group CRUD and schedule state only.

use crate::blob_store::BlobStore;
use crate::sqlite::SqliteDatabase;
use crate::types::{SubscriptionGroupInfo, SubscriptionInfo, SubscriptionQueryInfo};

// Bulk read — constant query count per group instead of O(N) per subscription.
pub async fn get_groups(db: &SqliteDatabase) -> Result<Vec<SubscriptionGroupInfo>, String> {
    let start = std::time::Instant::now();
    let groups = db.list_groups().await?;
    let mut result = Vec::with_capacity(groups.len());

    for group in groups {
        let fid = group.group_id;

        let subs_with_counts = db
            .list_subscriptions_for_group_with_file_counts(fid)
            .await?;
        let all_queries = db.list_subscription_queries_for_group(fid).await?;

        let mut queries_map: std::collections::HashMap<i64, Vec<SubscriptionQueryInfo>> =
            std::collections::HashMap::new();
        for q in all_queries {
            queries_map
                .entry(q.subscription_id)
                .or_default()
                .push(SubscriptionQueryInfo {
                    id: q.query_id.to_string(),
                    query_text: q.query_text.clone(),
                    display_name: q.display_name.or(Some(q.query_text)),
                    paused: q.paused,
                    last_check_time: q.last_check_time,
                    files_found: q.files_found as u64,
                    posts_found: q.posts_found as u64,
                    completed_initial_run: q.completed_initial_run,
                    resume_cursor: q.resume_cursor,
                    resume_strategy: q.resume_strategy,
                });
        }

        let mut group_total: u64 = 0;
        let sub_infos: Vec<SubscriptionInfo> = subs_with_counts
            .into_iter()
            .map(|(sub, file_count)| {
                let sub_id = sub.subscription_id;
                group_total += file_count as u64;
                let canonical_site_id =
                    crate::subscriptions::gallery_dl_runner::canonical_site_id(&sub.site_id);
                SubscriptionInfo {
                    id: sub_id.to_string(),
                    name: sub.name,
                    site_id: canonical_site_id.to_string(),
                    paused: sub.paused,
                    group_id: sub.group_id.map(|id| id.to_string()),
                    initial_post_limit: sub.initial_post_limit as u32,
                    periodic_post_limit: sub.periodic_post_limit as u32,
                    auto_collections: sub.auto_collections,
                    created_at: sub.created_at,
                    total_files: file_count as u64,
                    queries: queries_map.remove(&sub_id).unwrap_or_default(),
                }
            })
            .collect();

        result.push(SubscriptionGroupInfo {
            id: group.group_id.to_string(),
            name: group.name,
            schedule: group.schedule,
            created_at: group.created_at,
            total_files: group_total,
            subscriptions: sub_infos,
        });
    }

    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        count = result.len(),
        "get_groups bulk read"
    );

    Ok(result)
}

/// Create a new group with optional schedule.
pub async fn create_group(
    db: &SqliteDatabase,
    name: String,
    schedule: Option<String>,
) -> Result<SubscriptionGroupInfo, String> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Err("Group name cannot be empty".to_string());
    }
    let grp = db.create_group(&trimmed).await?;
    let group_id = grp.group_id;

    if let Some(ref sched) = schedule {
        validate_schedule(sched)?;
        db.set_group_schedule(group_id, sched).await?;
    }

    let final_grp = db
        .get_group(group_id)
        .await?
        .ok_or_else(|| "Group not found after creation".to_string())?;

    Ok(SubscriptionGroupInfo {
        id: final_grp.group_id.to_string(),
        name: final_grp.name,
        schedule: final_grp.schedule,
        created_at: final_grp.created_at,
        total_files: 0,
        subscriptions: vec![],
    })
}

/// Delete a group (CASCADE deletes subscriptions).
pub async fn delete_group(
    db: &SqliteDatabase,
    _blob_store: &BlobStore,
    id: String,
) -> Result<(), String> {
    let group_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid group id: {}", id))?;

    db.delete_group(group_id).await?;
    Ok(())
}

/// Rename a group.
pub async fn rename_group(db: &SqliteDatabase, id: String, name: String) -> Result<(), String> {
    let group_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid group id: {}", id))?;
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    db.rename_group(group_id, &trimmed).await
}

/// Set a group's schedule.
pub async fn set_group_schedule(
    db: &SqliteDatabase,
    id: String,
    schedule: String,
) -> Result<(), String> {
    let group_id: i64 = id
        .parse()
        .map_err(|_| format!("Invalid group id: {}", id))?;
    validate_schedule(&schedule)?;
    db.set_group_schedule(group_id, &schedule).await
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
