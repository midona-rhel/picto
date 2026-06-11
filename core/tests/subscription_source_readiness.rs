use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use picto_core::db::LibraryDatabase;
use picto_core::ingest_queue::IngestQueueItemPayload;
use picto_core::subscriptions::credential_service::SubscriptionCredentialService;
use picto_core::subscriptions::gallery_dl_runner;
use picto_core::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site,
};
use picto_core::subscriptions::runtime_service::SubscriptionRuntimeService;
use picto_core::subscriptions::source_adapter::{
    describe_site, validate_query_kind, ParsedMetadata,
};
use picto_core::subscriptions::types::SubscriptionQueryRunRecord;

#[derive(Debug, Clone, Copy)]
struct ExpectedMetadata {
    tags: bool,
    created_at: bool,
    title_or_description: bool,
    rating: bool,
    page_fields: bool,
}

#[derive(Debug, Clone, Copy)]
struct SourceReadinessFixture {
    site_id: &'static str,
    query_kind: &'static str,
    query_text: &'static str,
    requires_credentials: bool,
    expected_url_contains: &'static [&'static str],
    expected_metadata: ExpectedMetadata,
    expected_cursor_strategy: Option<&'static str>,
}

#[derive(Debug)]
struct ReadinessReport {
    site_id: &'static str,
    status: &'static str,
    detail: String,
}

const FIXTURES: &[SourceReadinessFixture] = &[
    SourceReadinessFixture {
        site_id: "gelbooru",
        query_kind: "search",
        query_text: "id:13753749 rating:safe",
        requires_credentials: false,
        expected_url_contains: &["gelbooru.com", "tags=id:13753749"],
        expected_metadata: ExpectedMetadata {
            tags: true,
            created_at: false,
            title_or_description: false,
            rating: true,
            page_fields: false,
        },
        expected_cursor_strategy: Some("tag_id_lt"),
    },
    SourceReadinessFixture {
        site_id: "pixiv",
        query_kind: "search",
        query_text: "風景",
        requires_credentials: true,
        expected_url_contains: &["pixiv.net", "/tags/"],
        expected_metadata: ExpectedMetadata {
            tags: true,
            created_at: true,
            title_or_description: true,
            rating: false,
            page_fields: true,
        },
        expected_cursor_strategy: Some("range_offset"),
    },
    SourceReadinessFixture {
        site_id: "furaffinity",
        query_kind: "user",
        query_text: "example",
        requires_credentials: true,
        expected_url_contains: &["furaffinity.net", "/user/"],
        expected_metadata: ExpectedMetadata {
            tags: false,
            created_at: true,
            title_or_description: true,
            rating: false,
            page_fields: false,
        },
        expected_cursor_strategy: Some("range_offset"),
    },
    SourceReadinessFixture {
        site_id: "coomer",
        query_kind: "user",
        query_text: "onlyfans/user/onlyfans",
        requires_credentials: false,
        expected_url_contains: &["coomer.st", "onlyfans/user"],
        expected_metadata: ExpectedMetadata {
            tags: false,
            created_at: true,
            title_or_description: true,
            rating: false,
            page_fields: true,
        },
        expected_cursor_strategy: Some("range_offset"),
    },
    SourceReadinessFixture {
        site_id: "fanbox",
        query_kind: "creator",
        query_text: "example",
        requires_credentials: false,
        expected_url_contains: &["fanbox.cc"],
        expected_metadata: ExpectedMetadata {
            tags: true,
            created_at: true,
            title_or_description: true,
            rating: false,
            page_fields: true,
        },
        expected_cursor_strategy: Some("range_offset"),
    },
];

#[test]
fn readiness_fixtures_have_valid_descriptors_and_urls() {
    for fixture in FIXTURES {
        let descriptor = describe_site(fixture.site_id).expect("site descriptor");
        assert_eq!(descriptor.site_id, fixture.site_id);
        assert!(descriptor
            .query_kinds
            .iter()
            .any(|kind| kind.id == fixture.query_kind));
        validate_query_kind(fixture.site_id, fixture.query_kind).unwrap();

        let url = gallery_dl_runner::build_url(fixture.site_id, fixture.query_text)
            .expect("fixture URL");
        for expected in fixture.expected_url_contains {
            assert!(
                url.contains(expected),
                "{} URL `{}` should contain `{}`",
                fixture.site_id,
                url,
                expected
            );
        }
    }
}

#[test]
fn readiness_resume_url_uses_same_query_shaping_as_sync_engine() {
    let query = "id:13753749 rating:safe";
    let resumed = apply_resume_to_query(query, "123456", "tag_id_lt");
    let url = gallery_dl_runner::build_url("gelbooru", &resumed).expect("resumed URL");

    assert_eq!(default_resume_strategy_for_site("gelbooru"), Some("tag_id_lt"));
    assert!(resumed.contains("id:<123456"));
    assert!(url.contains("id:%3C123456") || url.contains("id:<123456"));
}

#[test]
fn readiness_metadata_contract_separates_post_and_asset_fields() {
    let metadata = ParsedMetadata {
        tags: vec![
            ("creator".to_string(), "artist".to_string()),
            (String::new(), "landscape".to_string()),
        ],
        description: Some("body".to_string()),
        source_url: Some("https://example.test/post/42".to_string()),
        source_urls: vec!["https://example.test/post/42".to_string()],
        media_url: Some("https://cdn.example.test/file.jpg".to_string()),
        rating: Some("safe".to_string()),
        title: Some("title".to_string()),
        post_id: Some("42".to_string()),
        created_at: Some("2026-04-24T00:00:00Z".to_string()),
        category: Some("example".to_string()),
        page_num: Some(0),
        page_count: Some(1),
        canonical_post_url: Some("https://example.test/post/42".to_string()),
        item_key: Some("example:42:0".to_string()),
        raw_metadata: Some(serde_json::json!({"id": 42})),
    };

    let post = metadata.post_metadata();
    let asset = metadata.asset_metadata();

    assert_eq!(post.post_id.as_deref(), Some("42"));
    assert_eq!(post.tags.len(), 2);
    assert_eq!(asset.media_url.as_deref(), Some("https://cdn.example.test/file.jpg"));
    assert_eq!(asset.item_key.as_deref(), Some("example:42:0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires network and may require real service credentials"]
async fn live_subscription_source_readiness_matrix() {
    if std::env::var("PICTO_LIVE_SUBSCRIPTION_TESTS").ok().as_deref() != Some("1") {
        println!("subscription_readiness: skipped_live_disabled");
        return;
    }

    let selected = selected_services();
    let known: HashSet<_> = FIXTURES.iter().map(|fixture| fixture.site_id).collect();
    let unknown: Vec<_> = selected
        .iter()
        .filter(|site_id| !known.contains(site_id.as_str()))
        .cloned()
        .collect();
    assert!(
        unknown.is_empty(),
        "unknown subscription readiness services selected: {unknown:?}"
    );
    let tmp = tempfile::tempdir().expect("temp library");
    let state = picto_core::state::open_library(tmp.path().to_path_buf())
        .await
        .expect("open test library");
    let mut settings = state.settings.get();
    settings.sub_batch_size = 1;
    settings.sub_rate_limit_secs = 0.25;
    settings.sub_abort_threshold = 1;
    state.settings.update(settings);

    let db = LibraryDatabase::open(tmp.path()).expect("open verification db");
    let runtime = SubscriptionRuntimeService::new(&db, tmp.path());

    let mut reports = Vec::new();
    for fixture in FIXTURES.iter().filter(|fixture| selected.contains(fixture.site_id)) {
        reports.push(run_live_fixture(&db, &runtime, tmp.path(), fixture).await);
    }

    let _ = picto_core::state::close_library().await;

    for report in &reports {
        println!(
            "subscription_readiness: site={} status={} detail={}",
            report.site_id, report.status, report.detail
        );
    }

    let failures: Vec<_> = reports
        .iter()
        .filter(|report| report.status.starts_with("failed_"))
        .collect();
    assert!(failures.is_empty(), "readiness failures: {failures:#?}");
}

fn selected_services() -> HashSet<String> {
    std::env::var("PICTO_LIVE_SUBSCRIPTION_SERVICES")
        .unwrap_or_else(|_| "gelbooru".to_string())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

async fn run_live_fixture(
    db: &LibraryDatabase,
    runtime: &SubscriptionRuntimeService<'_>,
    library_root: &Path,
    fixture: &SourceReadinessFixture,
) -> ReadinessReport {
    match run_live_fixture_inner(db, runtime, library_root, fixture).await {
        Ok(report) => report,
        Err((status, detail)) => ReadinessReport {
            site_id: fixture.site_id,
            status,
            detail,
        },
    }
}

async fn run_live_fixture_inner(
    db: &LibraryDatabase,
    runtime: &SubscriptionRuntimeService<'_>,
    library_root: &Path,
    fixture: &SourceReadinessFixture,
) -> Result<ReadinessReport, (&'static str, String)> {
    let descriptor = describe_site(fixture.site_id)
        .ok_or_else(|| ("failed_descriptor", format!("unknown site {}", fixture.site_id)))?;
    validate_query_kind(fixture.site_id, fixture.query_kind)
        .map_err(|error| ("failed_descriptor", error))?;
    if fixture.requires_credentials && !descriptor.auth_supported {
        return Err((
            "failed_descriptor",
            format!("{} requires credentials but descriptor says auth is unsupported", fixture.site_id),
        ));
    }

    let query_text = query_text_for_fixture(fixture);
    let url = gallery_dl_runner::build_url(fixture.site_id, &query_text)
        .ok_or_else(|| ("failed_url", "site URL could not be built".to_string()))?;
    for expected in fixture.expected_url_contains {
        if !url.contains(expected) {
            return Err((
                "failed_url",
                format!("URL `{url}` does not contain `{expected}`"),
            ));
        }
    }

    let group = runtime
        .create_group(format!("readiness-{}", fixture.site_id), Some("manual".to_string()))
        .await
        .map_err(|error| ("failed_descriptor", error))?;
    let group_id: i64 = group.id.parse().map_err(|error| {
        (
            "failed_descriptor",
            format!("invalid created group id {}: {error}", group.id),
        )
    })?;
    let subscription = runtime
        .create_subscription(
            format!("readiness-{}", fixture.site_id),
            Some(group_id),
            Some(1),
            Some(1),
        )
        .await
        .map_err(|error| ("failed_descriptor", error))?;
    let subscription_id: i64 = subscription.id.parse().map_err(|error| {
        (
            "failed_descriptor",
            format!("invalid created subscription id {}: {error}", subscription.id),
        )
    })?;
    let query = runtime
        .add_subscription_query(
            subscription.id.clone(),
            fixture.site_id.to_string(),
            Some(fixture.query_kind.to_string()),
            query_text.clone(),
            Some("live readiness fixture".to_string()),
        )
        .await
        .map_err(|error| ("failed_descriptor", error))?;
    let query_id: i64 = query.id.parse().map_err(|error| {
        (
            "failed_descriptor",
            format!("invalid created query id {}: {error}", query.id),
        )
    })?;

    if fixture.requires_credentials {
        let credential = SubscriptionCredentialService::new(db)
            .resolve_for_run(subscription_id, Some(query_id), fixture.site_id, &url)
            .await;
        if !credential.has_credential() {
            return Ok(ReadinessReport {
                site_id: fixture.site_id,
                status: "skipped_missing_credential",
                detail: format!("no credential available for {}", credential.canonical_site_category),
            });
        }
    }

    picto_core::dispatch::dispatch(
        "run_subscription_query",
        &serde_json::json!({
            "subscription_id": subscription.id,
            "query_id": query.id,
        })
        .to_string(),
    )
    .await
    .map_err(|error| ("failed_download", error))?;

    let first_run = wait_for_finished_query_run(runtime, query_id, Duration::from_secs(180))
        .await
        .map_err(|error| ("failed_download", error))?;
    if first_run.status != "succeeded" {
        return Err((
            "failed_download",
            format!(
                "first run status={} kind={:?} error={:?}",
                first_run.status, first_run.failure_kind, first_run.error_message
            ),
        ));
    }
    if first_run.files_downloaded != 1 {
        return Err((
            "failed_download",
            format!("first run downloaded {} assets, expected exactly 1", first_run.files_downloaded),
        ));
    }

    let payloads = read_subscription_payloads(library_root, subscription_id, query_id)
        .map_err(|error| ("failed_ingest", error))?;
    if payloads.len() != 1 {
        return Err((
            "failed_ingest",
            format!("first run persisted {} ingest payloads, expected exactly 1", payloads.len()),
        ));
    }
    let payload = payloads
        .first()
        .ok_or_else(|| ("failed_ingest", "no ingest queue payload was persisted".to_string()))?;
    let metadata = payload
        .subscription_metadata
        .as_ref()
        .ok_or_else(|| ("failed_metadata", "payload missing subscription metadata".to_string()))?;
    verify_metadata(fixture, metadata)?;

    let refreshed = runtime
        .get_subscription_query(query_id)
        .await
        .map_err(|error| ("failed_resume", error))?
        .ok_or_else(|| ("failed_resume", "query disappeared after run".to_string()))?;
    if refreshed.files_found < 1 {
        return Err((
            "failed_download",
            format!("query files_found stayed at {}", refreshed.files_found),
        ));
    }
    if let Some(strategy) = fixture.expected_cursor_strategy {
        if default_resume_strategy_for_site(fixture.site_id) != Some(strategy) {
            return Err((
                "failed_resume",
                format!("default resume strategy was not {strategy}"),
            ));
        }
        let resumed_query = apply_resume_to_query(&query_text, "123456", strategy);
        let resumed_url = gallery_dl_runner::build_url(fixture.site_id, &resumed_query)
            .ok_or_else(|| ("failed_resume", "resumed URL could not be built".to_string()))?;
        if strategy == "tag_id_lt" && !resumed_url.contains("id:%3C123456") && !resumed_url.contains("id:<123456") {
            return Err((
                "failed_resume",
                format!("resumed URL did not include cursor: {resumed_url}"),
            ));
        }

        let resume_cursor = metadata
            .post_id
            .as_deref()
            .ok_or_else(|| ("failed_resume", "metadata did not provide a post id cursor".to_string()))?
            .to_string();
        runtime
            .set_query_completed_initial_run(query_id, false)
            .await
            .map_err(|error| ("failed_resume", error))?;
        runtime
            .set_query_resume_state(query_id, Some(resume_cursor), Some(strategy.to_string()))
            .await
            .map_err(|error| ("failed_resume", error))?;

        picto_core::dispatch::dispatch(
            "run_subscription_query",
            &serde_json::json!({
                "subscription_id": subscription.id,
                "query_id": query.id,
            })
            .to_string(),
        )
        .await
        .map_err(|error| ("failed_resume", error))?;

        let resumed_run = wait_for_finished_query_run_after(
            runtime,
            query_id,
            first_run.query_run_id,
            Duration::from_secs(180),
        )
        .await
        .map_err(|error| ("failed_resume", error))?;
        if resumed_run.status != "succeeded" {
            return Err((
                "failed_resume",
                format!(
                    "resumed run status={} kind={:?} error={:?}",
                    resumed_run.status, resumed_run.failure_kind, resumed_run.error_message
                ),
            ));
        }

        let resumed_payloads = read_subscription_payloads(library_root, subscription_id, query_id)
            .map_err(|error| ("failed_ingest", error))?;
        if resumed_payloads.len() != payloads.len() {
            let resumed_metadata = resumed_payloads
                .last()
                .and_then(|payload| payload.subscription_metadata.as_ref())
                .ok_or_else(|| {
                    (
                        "failed_metadata",
                        "resumed payload missing subscription metadata".to_string(),
                    )
                })?;
            verify_metadata(fixture, resumed_metadata)?;
        }
    }

    Ok(ReadinessReport {
        site_id: fixture.site_id,
        status: "ready",
        detail: format!(
            "downloaded={} payloads={}",
            first_run.files_downloaded,
            payloads.len()
        ),
    })
}

fn query_text_for_fixture(fixture: &SourceReadinessFixture) -> String {
    let env_key = format!(
        "PICTO_LIVE_SUBSCRIPTION_QUERY_{}",
        fixture.site_id.to_ascii_uppercase()
    );
    std::env::var(env_key).unwrap_or_else(|_| fixture.query_text.to_string())
}

async fn wait_for_finished_query_run(
    runtime: &SubscriptionRuntimeService<'_>,
    query_id: i64,
    timeout: Duration,
) -> Result<SubscriptionQueryRunRecord, String> {
    wait_for_finished_query_run_after(runtime, query_id, 0, timeout).await
}

async fn wait_for_finished_query_run_after(
    runtime: &SubscriptionRuntimeService<'_>,
    query_id: i64,
    after_query_run_id: i64,
    timeout: Duration,
) -> Result<SubscriptionQueryRunRecord, String> {
    let start = Instant::now();
    loop {
        let runs = runtime.list_subscription_query_runs(query_id, 10).await?;
        if let Some(run) = runs
            .into_iter()
            .filter(|run| run.query_run_id > after_query_run_id)
            .find(|run| run.finished_at.is_some())
        {
            return Ok(run);
        }
        if start.elapsed() > timeout {
            return Err(format!("timed out waiting for query {query_id} to finish"));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn read_subscription_payloads(
    library_root: &Path,
    subscription_id: i64,
    query_id: i64,
) -> Result<Vec<IngestQueueItemPayload>, String> {
    let conn = rusqlite::Connection::open(library_root.join("library.db"))
        .map_err(|error| error.to_string())?;
    let mut stmt = conn
        .prepare_cached(
            "SELECT i.payload_json
             FROM ingest_queue_item i
             JOIN ingest_queue q ON q.queue_id = i.queue_id
             WHERE q.subscription_id = ?1 AND q.query_id = ?2
             ORDER BY i.item_id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([subscription_id, query_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut payloads = Vec::new();
    for row in rows {
        let json = row.map_err(|error| error.to_string())?;
        payloads.push(serde_json::from_str(&json).map_err(|error| error.to_string())?);
    }
    Ok(payloads)
}

fn verify_metadata(
    fixture: &SourceReadinessFixture,
    metadata: &ParsedMetadata,
) -> Result<(), (&'static str, String)> {
    if metadata.post_id.as_deref().unwrap_or_default().trim().is_empty() {
        return Err(("failed_metadata", "missing post_id".to_string()));
    }
    if metadata.item_key.as_deref().unwrap_or_default().trim().is_empty() {
        return Err(("failed_metadata", "missing item_key".to_string()));
    }
    if metadata.raw_metadata.is_none() {
        return Err(("failed_metadata", "missing raw metadata".to_string()));
    }
    if metadata.canonical_post_url.is_none()
        && metadata.source_url.is_none()
        && metadata.source_urls.is_empty()
    {
        return Err(("failed_metadata", "missing source/post URL".to_string()));
    }
    if metadata.media_url.as_deref().unwrap_or_default().trim().is_empty() {
        return Err(("failed_metadata", "missing media_url".to_string()));
    }
    if fixture.expected_metadata.tags && metadata.tags.is_empty() {
        return Err(("failed_metadata", "missing tags".to_string()));
    }
    if fixture.expected_metadata.created_at && metadata.created_at.is_none() {
        return Err(("failed_metadata", "missing created_at".to_string()));
    }
    if fixture.expected_metadata.title_or_description
        && metadata.title.is_none()
        && metadata.description.is_none()
    {
        return Err(("failed_metadata", "missing title/description".to_string()));
    }
    if fixture.expected_metadata.rating && metadata.rating.is_none() {
        return Err(("failed_metadata", "missing rating".to_string()));
    }
    if fixture.expected_metadata.page_fields && metadata.page_count.is_none() {
        return Err(("failed_metadata", "missing page_count".to_string()));
    }
    Ok(())
}
