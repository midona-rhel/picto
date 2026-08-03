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
async fn startup_reconcile_finalizes_orphaned_runs_and_jobs() {
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
    assert_eq!(report.jobs_cancelled, 1);
    assert_eq!(report.runs_finalized, 1);

    assert_eq!(
        runtime
            .count_active_subscription_query_jobs(subscription_id)
            .await
            .unwrap(),
        0
    );
    let runs = runtime
        .list_subscription_runs(subscription_id, 10)
        .await
        .unwrap();
    assert_eq!(runs[0].status, "cancelled");
    assert_eq!(runs[0].failure_kind.as_deref(), Some("stale"));
    assert!(runs[0].finished_at.is_some());
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
