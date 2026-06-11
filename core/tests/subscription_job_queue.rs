use picto_core::db::LibraryDatabase;
use picto_core::subscriptions::runtime_service::SubscriptionRuntimeService;

fn open_db() -> (tempfile::TempDir, LibraryDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = LibraryDatabase::open(dir.path()).unwrap();
    (dir, db)
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

    let run_id = runtime.create_subscription_run(subscription_id).await.unwrap();
    let job_id = runtime
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

    let queued = runtime.list_queued_subscription_query_jobs(20).await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].job_id, job_id);

    let leased = runtime.lease_subscription_query_job(job_id).await.unwrap().unwrap();
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
