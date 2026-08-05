use picto_core::db::LibraryDatabase;
use picto_core::ingest_queue::IngestQueueItemResultKind;
use picto_core::subscriptions::runtime_service::SubscriptionRuntimeService;
use picto_core::subscriptions::types::SubscriptionQueryRunCompletion;

fn open_db() -> (tempfile::TempDir, LibraryDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = LibraryDatabase::open(dir.path()).unwrap();
    (dir, db)
}

/// Direct SQL against the library file — for seeding states the public API
/// (correctly) refuses to produce, e.g. corrupted rows from older builds.
fn raw_conn(dir: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.path().join("library.db")).unwrap()
}

#[tokio::test]
async fn add_subscription_query_infers_query_kind_from_legacy_site() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));

    let subscription = runtime
        .create_subscription("Test".to_string(), None, None, None)
        .await
        .unwrap();
    assert_eq!(subscription.schedule, "daily");
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            "pixivuser".to_string(),
            None,
            "12345".to_string(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(query.query_kind, "user");
}

#[tokio::test]
async fn subscription_query_jobs_queue_lease_and_finish() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));

    let subscription = runtime
        .create_subscription("Queue Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id: i64 = subscription.id.parse().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id: i64 = query.id.parse().unwrap();

    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let (job_id, created) = runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    assert!(created);

    // Re-enqueue while the job is in flight: deduplicates, reports not-created.
    let (dup_id, dup_created) = runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    assert_eq!(dup_id, job_id);
    assert!(!dup_created);

    let queued = runtime
        .list_queued_subscription_query_jobs(20)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].job_id, job_id);

    let leased = runtime
        .lease_subscription_query_job(job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(leased.status, "running");
    assert_eq!(
        runtime
            .count_active_subscription_query_jobs(subscription_id)
            .await
            .unwrap(),
        1
    );

    runtime
        .finish_subscription_query_job(job_id, "succeeded", None, None)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .count_active_subscription_query_jobs(subscription_id)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn automatic_retry_reuses_the_same_durable_job_after_backoff() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Retry Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id,
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id = query.id.parse::<i64>().unwrap();
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let (job_id, _) = runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    runtime.lease_subscription_query_job(job_id).await.unwrap();

    let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    assert!(runtime
        .reschedule_subscription_query_job(
            job_id,
            future,
            "network".to_string(),
            Some("temporary network failure".to_string()),
        )
        .await
        .unwrap());
    assert!(runtime
        .list_queued_subscription_query_jobs(10)
        .await
        .unwrap()
        .is_empty());
    let jobs = runtime
        .list_subscription_query_jobs_for_run(run_id)
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].job_id, job_id);
    assert_eq!(jobs[0].status, "queued");
    assert_eq!(jobs[0].attempt_count, 1);

    runtime
        .upsert_subscription_issue(
            subscription_id,
            Some(query_id),
            picto_core::subscriptions::gallery_dl_runner::FailureKind::Network,
            "temporary network failure",
            None,
        )
        .await
        .unwrap();
    let next_retry_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    runtime
        .set_subscription_issue_next_retry(
            subscription_id,
            query_id,
            picto_core::subscriptions::gallery_dl_runner::FailureKind::Network,
            next_retry_at.clone(),
        )
        .await
        .unwrap();
    let issues = runtime
        .list_subscription_issues(subscription_id, Some(query_id), 10)
        .await
        .unwrap();
    assert_eq!(
        issues[0].next_retry_at.as_deref(),
        Some(next_retry_at.as_str())
    );
}

#[tokio::test]
async fn full_run_finalizes_only_after_its_own_jobs_are_terminal() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Finalization Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let mut query_ids = Vec::new();
    for query_text in ["1girl", "2girls"] {
        let query = runtime
            .add_subscription_query(
                subscription.id.clone(),
                "gelbooru".to_string(),
                Some("search".to_string()),
                query_text.to_string(),
                None,
            )
            .await
            .unwrap();
        query_ids.push(query.id.parse::<i64>().unwrap());
    }
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let mut job_ids = Vec::new();
    for query_id in query_ids {
        let (job_id, _) = runtime
            .enqueue_subscription_query_job(
                Some(run_id),
                subscription_id,
                query_id,
                "gelbooru",
                "query_sync",
                "subscription",
                None,
            )
            .await
            .unwrap();
        runtime.lease_subscription_query_job(job_id).await.unwrap();
        job_ids.push(job_id);
    }

    runtime
        .finish_subscription_query_job(
            job_ids[0],
            "failed",
            Some("network".to_string()),
            Some("failed".to_string()),
        )
        .await
        .unwrap();
    assert!(runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await
        .unwrap()
        .is_none());
    runtime
        .finish_subscription_query_job(job_ids[1], "succeeded", None, None)
        .await
        .unwrap();
    let run = runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(run.failure_kind.as_deref(), Some("network"));
}

#[tokio::test]
async fn full_run_derives_its_snapshot_from_durable_work() {
    let (dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Ingest Finalization Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id,
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id = query.id.parse::<i64>().unwrap();
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let (job_id, _) = runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    let query_run_id = runtime
        .create_subscription_query_run(Some(run_id), subscription_id, query_id)
        .await
        .unwrap();
    {
        let conn = raw_conn(&dir);
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_kind, source_kind, subscription_id, query_id, query_run_id,
                 status, created_at, updated_at
             ) VALUES ('single', 'subscription', ?1, ?2, ?3, 'pending', 'now', 'now')",
            rusqlite::params![subscription_id, query_id, query_run_id],
        )
        .unwrap();
        let queue_id = conn.last_insert_rowid();
        conn.execute_batch(&format!(
            "INSERT INTO ingest_queue_item (
                 queue_id, source_path, page_num, payload_json, delete_after_ingest,
                 status, created_at, updated_at
             ) VALUES ({queue_id}, '/tmp/first', 0, '{{}}', 1, 'pending', 'now', 'now');
             INSERT INTO ingest_queue_item (
                 queue_id, source_path, page_num, payload_json, delete_after_ingest,
                 status, created_at, updated_at
             ) VALUES ({queue_id}, '/tmp/second', 1, '{{}}', 1, 'pending', 'now', 'now');"
        ))
        .unwrap();
    }

    runtime
        .finish_subscription_query_job(job_id, "succeeded", None, None)
        .await
        .unwrap();
    assert!(runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await
        .unwrap()
        .is_none());

    db.mark_ingest_queue_item_complete(
        1,
        IngestQueueItemResultKind::Imported,
        Some("first-entity".to_string()),
        Some("first-file".to_string()),
    )
    .await
    .unwrap();
    db.mark_ingest_queue_item_complete(
        2,
        IngestQueueItemResultKind::Reused,
        Some("second-entity".to_string()),
        Some("second-file".to_string()),
    )
    .await
    .unwrap();
    db.mark_ingest_queue_item_complete(
        2,
        IngestQueueItemResultKind::Reused,
        Some("second-entity".to_string()),
        Some("second-file".to_string()),
    )
    .await
    .unwrap();
    runtime
        .finish_subscription_query_run(
            query_run_id,
            SubscriptionQueryRunCompletion {
                status: "succeeded".to_string(),
                failure_kind: None,
                error_message: None,
                posts_processed: 2,
                files_downloaded: 2,
                files_skipped: 0,
                metadata_validated: 2,
                metadata_invalid: 1,
            },
        )
        .await
        .unwrap();
    raw_conn(&dir)
        .execute("UPDATE ingest_queue SET status = 'complete'", [])
        .unwrap();
    db.cleanup_ingest_queue().await.unwrap();
    assert_eq!(
        raw_conn(&dir)
            .query_row("SELECT COUNT(*) FROM ingest_queue", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let run = runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, "succeeded");
    assert_eq!(run.files_downloaded, 2);
    assert_eq!(run.files_skipped, 1);
    assert_eq!(run.metadata_validated, 2);
    assert_eq!(run.metadata_invalid, 1);
    assert!(runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await
        .unwrap()
        .is_none());
    db.cleanup_ingest_queue().await.unwrap();
    assert_eq!(
        raw_conn(&dir)
            .query_row("SELECT COUNT(*) FROM ingest_queue", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let persisted = runtime
        .list_subscription_runs(subscription_id, 1)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(persisted.files_downloaded, 2);
    assert_eq!(persisted.files_skipped, 1);
}

#[tokio::test]
async fn current_ingest_counts_exclude_previous_runs() {
    let (dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Current Counts".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id,
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id = query.id.parse::<i64>().unwrap();
    let old_run = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let old_query_run = runtime
        .create_subscription_query_run(Some(old_run), subscription_id, query_id)
        .await
        .unwrap();
    let current_run = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let current_query_run = runtime
        .create_subscription_query_run(Some(current_run), subscription_id, query_id)
        .await
        .unwrap();

    let conn = raw_conn(&dir);
    for (query_run_id, queue_status, item_status, result_kind) in [
        (old_query_run, "complete", "complete", Some("imported")),
        (current_query_run, "running", "pending", None),
        (current_query_run, "running", "complete", Some("reused")),
    ] {
        conn.execute(
            "INSERT INTO ingest_queue (
                 queue_kind, source_kind, subscription_id, query_id, query_run_id,
                 status, created_at, updated_at
             ) VALUES ('single', 'subscription', ?1, ?2, ?3, ?4, 'now', 'now')",
            rusqlite::params![subscription_id, query_id, query_run_id, queue_status],
        )
        .unwrap();
        let queue_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO ingest_queue_item (
                 queue_id, source_path, payload_json, status, result_kind, created_at, updated_at
             ) VALUES (?1, '/tmp/source', '{}', ?2, ?3, 'now', 'now')",
            rusqlite::params![queue_id, item_status, result_kind],
        )
        .unwrap();
    }
    drop(conn);

    let counts = runtime.count_current_ingest_queue(query_id).await.unwrap();
    assert_eq!(counts.queued, 1);
    assert_eq!(counts.ingesting, 0);
    assert_eq!(counts.ingested, 0);
    assert_eq!(counts.reused, 1);
    assert_eq!(counts.failed, 0);
}

#[tokio::test]
async fn failed_ingest_fails_an_otherwise_successful_run() {
    let (dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Failed Ingest Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id,
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id = query.id.parse::<i64>().unwrap();
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let (job_id, _) = runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    let query_run_id = runtime
        .create_subscription_query_run(Some(run_id), subscription_id, query_id)
        .await
        .unwrap();
    let conn = raw_conn(&dir);
    conn.execute(
        "INSERT INTO ingest_queue (
             queue_kind, source_kind, subscription_id, query_id, query_run_id,
             status, last_error, created_at, updated_at
         ) VALUES ('single', 'subscription', ?1, ?2, ?3,
                   'failed', 'queued source disappeared', 'now', 'now')",
        rusqlite::params![subscription_id, query_id, query_run_id],
    )
    .unwrap();
    let queue_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO ingest_queue_item (
             queue_id, source_path, page_num, payload_json, delete_after_ingest,
             status, result_kind, last_error, created_at, updated_at
         ) VALUES (?1, '/tmp/missing', 0, '{}', 1,
                   'failed', 'failed', 'queued source disappeared', 'now', 'now')",
        [queue_id],
    )
    .unwrap();
    drop(conn);

    runtime
        .finish_subscription_query_run(
            query_run_id,
            SubscriptionQueryRunCompletion {
                status: "succeeded".to_string(),
                failure_kind: None,
                error_message: None,
                posts_processed: 1,
                files_downloaded: 1,
                files_skipped: 0,
                metadata_validated: 1,
                metadata_invalid: 0,
            },
        )
        .await
        .unwrap();
    runtime
        .finish_subscription_query_job(job_id, "succeeded", None, None)
        .await
        .unwrap();
    let run = runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(run.status, "failed");
    assert_eq!(run.failure_kind.as_deref(), Some("ingest_queue_failure"));
    assert_eq!(
        run.error_message.as_deref(),
        Some("queued source disappeared")
    );
    assert_eq!(run.files_downloaded, 1);
}

#[tokio::test]
async fn query_only_jobs_do_not_create_full_subscription_runs() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Query Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id,
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    runtime
        .enqueue_subscription_query_job(
            None,
            subscription_id,
            query.id.parse().unwrap(),
            "gelbooru",
            "query_sync",
            "query",
            None,
        )
        .await
        .unwrap();
    assert!(runtime
        .list_subscription_runs(subscription_id, 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn recurring_schedule_belongs_to_an_enabled_subscription() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let group = runtime.create_group("Artists".to_string()).await.unwrap();
    let subscription = runtime
        .create_subscription(
            "Scheduled".to_string(),
            Some(group.id.parse().unwrap()),
            None,
            None,
        )
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    runtime
        .add_subscription_query(
            subscription.id.clone(),
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    runtime
        .set_subscription_schedule(subscription.id.clone(), "daily".to_string())
        .await
        .unwrap();

    let scheduled = runtime.list_scheduled_subscriptions().await.unwrap();
    assert_eq!(scheduled.len(), 1);
    assert_eq!(scheduled[0].subscription_id, subscription_id);
    assert_eq!(scheduled[0].schedule, "daily");
    assert!(scheduled[0].last_full_run_at.is_none());

    runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    assert!(runtime.list_scheduled_subscriptions().await.unwrap()[0]
        .last_full_run_at
        .is_some());

    runtime
        .pause_subscription(subscription.id, true)
        .await
        .unwrap();
    assert!(runtime
        .list_scheduled_subscriptions()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn deleting_a_group_ungroups_its_subscriptions() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let group = runtime.create_group("Artists".to_string()).await.unwrap();
    let subscription = runtime
        .create_subscription(
            "Kept".to_string(),
            Some(group.id.parse().unwrap()),
            None,
            None,
        )
        .await
        .unwrap();
    runtime.delete_group(group.id).await.unwrap();
    let kept = runtime
        .get_subscription(subscription.id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.group_id, None);
}

#[tokio::test]
async fn reset_and_delete_remove_tracking_but_keep_imported_media() {
    let (dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Cleanup Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id = query.id.parse::<i64>().unwrap();
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    runtime
        .upsert_subscription_issue(
            subscription_id,
            Some(query_id),
            picto_core::subscriptions::gallery_dl_runner::FailureKind::Network,
            "temporary",
            None,
        )
        .await
        .unwrap();
    let conn = raw_conn(&dir);
    conn.execute(
        "INSERT INTO media_entity (
             entity_id, entity_hash, entity_kind, status, date_created, date_added, date_modified
         ) VALUES (1, 'kept-media', 'single', 1, '2026-01-01', '2026-01-01', '2026-01-01')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO subscription_entity (subscription_id, entity_id) VALUES (?1, 1)",
        [subscription_id],
    )
    .unwrap();
    drop(conn);

    runtime
        .reset_subscription(subscription.id.clone())
        .await
        .unwrap();
    assert!(runtime
        .list_subscription_runs(subscription_id, 10)
        .await
        .unwrap()
        .is_empty());
    assert!(runtime
        .list_subscription_issues(subscription_id, None, 10)
        .await
        .unwrap()
        .is_empty());
    let conn = raw_conn(&dir);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM subscription_entity", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);

    runtime.delete_subscription(subscription.id).await.unwrap();
    let conn = raw_conn(&dir);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM subscription", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn stop_is_idempotent_and_settles_queued_work() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));
    let subscription = runtime
        .create_subscription("Stop Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id = subscription.id.parse::<i64>().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query.id.parse().unwrap(),
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    let running = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    picto_core::subscriptions::job_queue::activate_subscription_guard(&running, &subscription.id)
        .await
        .unwrap();

    picto_core::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        &db,
        std::path::Path::new("/tmp"),
        &running,
        subscription.id.clone(),
    )
    .await
    .unwrap();
    picto_core::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        &db,
        std::path::Path::new("/tmp"),
        &running,
        subscription.id,
    )
    .await
    .unwrap();

    assert!(running.lock().await.is_empty());
    assert_eq!(
        runtime
            .count_active_subscription_query_jobs(subscription_id)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        runtime
            .list_subscription_runs(subscription_id, 10)
            .await
            .unwrap()[0]
            .status,
        "cancelled"
    );
}

#[tokio::test]
async fn startup_reconcile_requeues_interrupted_jobs_in_the_same_run() {
    let (_dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));

    let subscription = runtime
        .create_subscription("Orphan Test".to_string(), None, None, None)
        .await
        .unwrap();
    let subscription_id: i64 = subscription.id.parse().unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            "gelbooru".to_string(),
            Some("search".to_string()),
            "1girl".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id: i64 = query.id.parse().unwrap();

    // Simulate an app quit mid-run: a running run with a leased job.
    let run_id = runtime
        .create_subscription_run(subscription_id)
        .await
        .unwrap();
    let (job_id, _created) = runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            "gelbooru",
            "query_sync",
            "subscription",
            None,
        )
        .await
        .unwrap();
    runtime.lease_subscription_query_job(job_id).await.unwrap();

    let report = runtime
        .reconcile_subscription_runtime_state()
        .await
        .unwrap();
    assert_eq!(report.jobs_requeued, 1);
    assert_eq!(report.orphan_runs_finalized, 0);

    assert_eq!(
        runtime
            .count_active_subscription_query_jobs(subscription_id)
            .await
            .unwrap(),
        1
    );
    let runs = runtime
        .list_subscription_runs(subscription_id, 10)
        .await
        .unwrap();
    assert_eq!(runs[0].status, "running");
    assert!(runs[0].finished_at.is_none());
    let queued = runtime
        .list_queued_subscription_query_jobs(10)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].job_id, job_id);
    assert_eq!(queued[0].run_id, Some(run_id));
}

#[tokio::test]
async fn startup_reconcile_repairs_invalid_query_kind() {
    let (dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));

    let subscription = runtime
        .create_subscription("Kind Repair".to_string(), None, None, None)
        .await
        .unwrap();
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            "coomer".to_string(),
            None,
            "fansly/user/123".to_string(),
            None,
        )
        .await
        .unwrap();
    let query_id: i64 = query.id.parse().unwrap();

    // Corrupt the stored kind the way pre-fix builds did.
    raw_conn(&dir)
        .execute(
            "UPDATE subscription_query SET query_kind = 'search' WHERE query_id = ?1",
            [query_id],
        )
        .unwrap();

    let report = runtime
        .reconcile_subscription_runtime_state()
        .await
        .unwrap();
    assert_eq!(report.query_kinds_repaired, 1);

    let kind: String = raw_conn(&dir)
        .query_row(
            "SELECT query_kind FROM subscription_query WHERE query_id = ?1",
            [query_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kind, "user");
}

#[tokio::test]
async fn startup_reconcile_repairs_poisoned_credential_health() {
    let (dir, db) = open_db();
    let runtime = SubscriptionRuntimeService::new(&db, std::path::Path::new("/tmp"));

    {
        let conn = raw_conn(&dir);
        // Phantom auth failure for a site with no stored credential.
        conn.execute(
            "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
             VALUES ('danbooru', 'unauthorized', '2026-06-11T20:00:00+00:00', 'download failed')",
            [],
        )
        .unwrap();
        // Content failure written as credential 'error' for a real credential.
        conn.execute(
            "INSERT INTO credential_domain (site_category, credential_type, display_name, date_added)
             VALUES ('twitter', 'cookies', 'Twitter', '2026-07-17T19:32:43+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
             VALUES ('twitter', 'error', '2026-07-17T19:33:41+00:00', 'NotFoundError: Requested user could not be found')",
            [],
        )
        .unwrap();
        // Legitimate 'missing' row must survive.
        conn.execute(
            "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
             VALUES ('e621', 'missing', '2026-03-03T23:20:44+00:00', NULL)",
            [],
        )
        .unwrap();
    }

    let report = runtime
        .reconcile_subscription_runtime_state()
        .await
        .unwrap();
    assert_eq!(report.health_rows_repaired, 2);

    let conn = raw_conn(&dir);
    let danbooru: Option<String> = conn
        .query_row(
            "SELECT health_status FROM credential_health WHERE site_category = 'danbooru'",
            [],
            |row| row.get(0),
        )
        .ok();
    assert_eq!(danbooru, None);
    let twitter: String = conn
        .query_row(
            "SELECT health_status FROM credential_health WHERE site_category = 'twitter'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(twitter, "unknown");
    let e621: String = conn
        .query_row(
            "SELECT health_status FROM credential_health WHERE site_category = 'e621'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(e621, "missing");
}
