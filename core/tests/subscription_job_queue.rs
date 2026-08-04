use picto_core::db::LibraryDatabase;
use picto_core::subscriptions::runtime_service::SubscriptionRuntimeService;

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
