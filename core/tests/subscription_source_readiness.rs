use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::Utc;
use picto_core::blob_store::{mime_to_extension, BlobStore};
use picto_core::library_application::LibraryApplication;
use picto_core::native_source::NativeSourceRunner;
use picto_core::subscription_catalog::{NewSubscription, NewSubscriptionQuery};
use picto_core::subscription_runtime::SubscriptionWorker;
use picto_core::subscriptions::sites::site_by_id;
use picto_core::subscriptions::source_adapter::describe_site;
use picto_library::database::WorkPriority;
use picto_library::{LibraryError, Lifecycle, RootId, RootKind};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct MediaEvidence {
    source_item_id: i64,
    item_key: String,
    position: i64,
    media_item_id: i64,
    file_hash: String,
    mime_type: String,
    size_bytes: i64,
    tag_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct PostEvidence {
    source_post_id: i64,
    post_key: String,
    canonical_url: String,
    creator_name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    captured_at: Option<String>,
    root_item_id: i64,
    root_kind: String,
    lifecycle: String,
    items: Vec<MediaEvidence>,
    collection_member_order: Vec<i64>,
    rooted_media_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Evidence {
    traversed_post_count: usize,
    posts: Vec<PostEvidence>,
    /// Every distinct tag namespace the run persisted. Source tags must land
    /// in a canonical namespace or fall back to `general` — never invent one.
    tag_namespaces: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct RunEvidence {
    run_id: i64,
    status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct AttemptEvidence {
    post_key: String,
    outcome: String,
    terminal_reason: Option<String>,
    started_at: String,
    settled_at: String,
    retained_files: i64,
    duplicate_files: i64,
    failed_files: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct AttemptSequenceEvidence {
    traversed: usize,
    added: usize,
    skipped: usize,
    failed: usize,
    retained_files: i64,
    duplicate_files: i64,
    failed_files: i64,
    attempts: Vec<AttemptEvidence>,
}

/// Certifies one real source against Picto's replacement subscription, ingest,
/// collection, blob, and restart boundaries. The source is selected through
/// environment variables by `scripts/verify-sites.mjs`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an explicit live source and network access"]
async fn live_subscription_source_persistence_certification() {
    certify_selected_source()
        .await
        .unwrap_or_else(|error| panic!("subscription certification failed: {error}"));
}

async fn certify_selected_source() -> Result<(), String> {
    picto_core::state::init_tracing();
    let site_id = required_env("PICTO_LIVE_SUBSCRIPTION_SITE")?;
    let query_text = required_env("PICTO_LIVE_SUBSCRIPTION_QUERY")?;
    let batch_size = requested_batch_size()?;
    let site = site_by_id(&site_id).ok_or_else(|| format!("unknown source '{site_id}'"))?;
    let descriptor = describe_site(&site_id)
        .ok_or_else(|| format!("source '{site_id}' has no query descriptor"))?;
    if descriptor.query_kinds.len() != 1 {
        return Err(format!(
            "source '{site_id}' exposes {} query kinds; certification requires one explicit behavior",
            descriptor.query_kinds.len()
        ));
    }
    let auth_mode = configure_credential(site.credential_owner_site_id)?;

    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = temp.path();
    // Every certification proves provider-specific per-domain pacing from a
    // trace recorded at the bridge's HTTP boundary. An externally provided
    // absolute path survives the run for inspection.
    let request_trace = match std::env::var_os("PICTO_TRACE_REQUESTS") {
        Some(existing) if std::path::Path::new(&existing).is_absolute() => {
            std::path::PathBuf::from(existing)
        }
        _ => {
            let path = root.join("request-trace.jsonl");
            std::env::set_var("PICTO_TRACE_REQUESTS", &path);
            path
        }
    };
    let application = LibraryApplication::create(root)?;
    let (subscription_id, _) = application.create_subscription_definition_library(
        &NewSubscription {
            name: format!("certify-{site_id}"),
            schedule: "manual".into(),
            initial_post_limit: Some(i64::from(batch_size)),
            periodic_post_limit: Some(i64::from(batch_size)),
            queries: vec![NewSubscriptionQuery {
                site_id: site_id.clone(),
                query_text: query_text.clone(),
                display_name: Some("release certification".into()),
                notes: None,
                group_posts: true,
            }],
        },
        &Utc::now().to_rfc3339(),
    )?;

    let first_run = execute_run(&application, subscription_id, batch_size).await?;
    require_success(first_run, &application)?;
    let first_attempts = read_attempt_sequence(&application, first_run.run_id, batch_size)?;
    let first = read_evidence(&application, subscription_id)?;
    validate_evidence(root, &site_id, &first)?;
    if first.posts.is_empty() {
        return Err("source run succeeded without materializing any media posts".into());
    }
    // Traversal may exceed the batch: no-media and duplicate posts are
    // skipped without consuming the added-post budget, and page-window
    // boundary detection announces the first post beyond the limit.
    if first.posts.len() > batch_size as usize {
        return Err(format!(
            "source materialized {} posts for a requested batch of {batch_size}",
            first.posts.len()
        ));
    }
    let checkpoint = read_checkpoint(&application, subscription_id)?;
    drop(application);

    let reopened = LibraryApplication::open(root)?;
    let after_restart = read_evidence(&reopened, subscription_id)?;
    if first != after_restart {
        return Err("closing and reopening changed persisted source or media identity".into());
    }
    if checkpoint != read_checkpoint(&reopened, subscription_id)? {
        return Err("closing and reopening changed the durable continuation cursor".into());
    }

    // One more source post proves that the next run continues from persisted
    // state instead of replaying the first source window.
    reopened.set_subscription_posts_per_run_library(subscription_id, 1)?;
    let second_run = execute_run(&reopened, subscription_id, 1).await?;
    require_success(second_run, &reopened)?;
    let second_attempts = read_attempt_sequence(&reopened, second_run.run_id, 1)?;
    let continued = read_evidence(&reopened, subscription_id)?;
    validate_evidence(root, &site_id, &continued)?;
    require_prefix_preserved(&first, &continued)?;
    let continued_checkpoint = read_checkpoint(&reopened, subscription_id)?;
    if continued.posts.len() == first.posts.len() && continued_checkpoint == checkpoint {
        // A concrete gallery is one finite import unit: the correct
        // continuation outcome is an idempotent replay that changes nothing.
        if site_id == "ehentai" {
            if continued != first {
                return Err("idempotent gallery replay changed persisted evidence".into());
            }
        } else {
            return Err(
                "continuation neither materialized media nor advanced source history".into(),
            );
        }
    }

    let pacing = validate_request_pacing(&request_trace)?;
    write_report(
        &site_id,
        &query_text,
        batch_size,
        auth_mode,
        &first,
        &first_attempts,
        &continued,
        &second_attempts,
        &pacing,
    )?;
    println!(
        "subscription_certification: site={} first_posts={} first_media={} collections={} final_posts={} restart=passed continuation=passed",
        site_id,
        first.posts.len(),
        first.media_count(),
        first.collection_count(),
        continued.posts.len(),
    );
    Ok(())
}

fn read_attempt_sequence(
    application: &LibraryApplication,
    run_id: i64,
    added_post_limit: u32,
) -> Result<AttemptSequenceEvidence, String> {
    let rows = application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            let mut statement = connection.prepare(
                "SELECT attempt.run_query_id, post.post_key, attempt.state,
                        attempt.terminal_reason, attempt.started_at, attempt.settled_at,
                        COALESCE(SUM(file.state = 'retained'), 0),
                        COALESCE(SUM(file.state = 'duplicate'), 0),
                        COALESCE(SUM(file.state = 'failed'), 0)
                 FROM subscription_run_query run_query
                 JOIN source_post_attempt attempt USING(run_query_id)
                 JOIN source_post post USING(source_post_id)
                 LEFT JOIN source_file_attempt file USING(attempt_id)
                 WHERE run_query.run_id = ?1
                 GROUP BY attempt.attempt_id
                 ORDER BY attempt.run_query_id, attempt.attempt_id",
            )?;
            let rows = statement
                .query_map([run_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        AttemptEvidence {
                            post_key: row.get(1)?,
                            outcome: row.get(2)?,
                            terminal_reason: row.get(3)?,
                            started_at: row.get(4)?,
                            settled_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                            retained_files: row.get(6)?,
                            duplicate_files: row.get(7)?,
                            failed_files: row.get(8)?,
                        },
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .map_err(|error| error.to_string())?;

    let mut last_settlement_by_query = BTreeMap::new();
    let mut attempts = Vec::with_capacity(rows.len());
    for (run_query_id, attempt) in rows {
        if !matches!(attempt.outcome.as_str(), "added" | "skipped" | "failed") {
            return Err(format!(
                "successful run {run_id} retained nonterminal post {} in state {}",
                attempt.post_key, attempt.outcome
            ));
        }
        let started = chrono::DateTime::parse_from_rfc3339(&attempt.started_at)
            .map_err(|error| format!("invalid attempt start time: {error}"))?;
        let settled = chrono::DateTime::parse_from_rfc3339(&attempt.settled_at)
            .map_err(|error| format!("invalid attempt settlement time: {error}"))?;
        if settled < started {
            return Err(format!(
                "post {} settled before it was traversed",
                attempt.post_key
            ));
        }
        if let Some(previous) = last_settlement_by_query.insert(run_query_id, settled) {
            if started < previous {
                return Err(format!(
                    "post {} was traversed before the preceding post settled",
                    attempt.post_key
                ));
            }
        }
        if attempt.outcome == "skipped" && attempt.retained_files != 0 {
            return Err(format!(
                "skipped post {} retained {} downloaded files",
                attempt.post_key, attempt.retained_files
            ));
        }
        attempts.push(attempt);
    }

    let added = attempts
        .iter()
        .filter(|attempt| attempt.outcome == "added")
        .count();
    if added > added_post_limit as usize {
        return Err(format!(
            "run {run_id} added {added} posts beyond its limit of {added_post_limit}"
        ));
    }
    Ok(AttemptSequenceEvidence {
        traversed: attempts.len(),
        added,
        skipped: attempts
            .iter()
            .filter(|attempt| attempt.outcome == "skipped")
            .count(),
        failed: attempts
            .iter()
            .filter(|attempt| attempt.outcome == "failed")
            .count(),
        retained_files: attempts.iter().map(|attempt| attempt.retained_files).sum(),
        duplicate_files: attempts.iter().map(|attempt| attempt.duplicate_files).sum(),
        failed_files: attempts.iter().map(|attempt| attempt.failed_files).sum(),
        attempts,
    })
}

async fn execute_run(
    application: &LibraryApplication,
    subscription_id: i64,
    _batch_size: u32,
) -> Result<RunEvidence, String> {
    let now = Utc::now().to_rfc3339();
    let (created, _) = application.request_subscription_run_library(subscription_id, &now)?;
    if !created.created {
        return Err("subscription already had an active run".into());
    }
    let runner = NativeSourceRunner::open(application.root());
    let cancel = CancellationToken::new();
    let worker = SubscriptionWorker::with_cancellation(application, runner, cancel.clone());
    // Batched-window providers settle one source window per tick and return
    // the query to pending until the run's post budget or cursor is
    // exhausted, exactly like the production scheduler loop. Drive ticks
    // until the run itself is terminal.
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(
            std::env::var("PICTO_LIVE_SUBSCRIPTION_TIMEOUT_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(7_200),
        );
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            cancel.cancel();
            return Err(format!(
                "run {} exceeded the certification timeout",
                created.run_id
            ));
        }
        let tick_at = Utc::now().to_rfc3339();
        let tick = worker.tick(&tick_at);
        tokio::pin!(tick);
        match tokio::time::timeout(remaining, &mut tick).await {
            Ok(result) => {
                result?;
            }
            Err(_) => {
                cancel.cancel();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut tick).await;
                return Err(format!(
                    "run {} exceeded the certification timeout",
                    created.run_id
                ));
            }
        }
        loop {
            let report = picto_core::library_ingest_runtime::run_batch(application, 64)?;
            if report.ingested == 0 && report.failed == 0 {
                break;
            }
        }
        let status: String = application
            .library()
            .auxiliary_read(WorkPriority::VisibleRead, |connection| {
                Ok(connection.query_row(
                    "SELECT status FROM subscription_run WHERE run_id = ?1",
                    [created.run_id],
                    |row| row.get(0),
                )?)
            })
            .map_err(|error| error.to_string())?;
        if !matches!(status.as_str(), "pending" | "running") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Ok(RunEvidence {
        run_id: created.run_id,
        status: "finished",
    })
}

fn require_success(run: RunEvidence, application: &LibraryApplication) -> Result<(), String> {
    let (status, query_status, failure_kind, error): (
        String,
        String,
        Option<String>,
        Option<String>,
    ) = application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(
                "SELECT sr.status, srq.status, srq.failure_kind, srq.error_message
             FROM subscription_run sr
             JOIN subscription_run_query srq ON srq.run_id = sr.run_id
             WHERE sr.run_id = ?1",
                [run.run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?)
        })
        .map_err(|error| error.to_string())?;
    if status != "succeeded" || query_status != "succeeded" {
        return Err(format!(
            "run {} did not succeed: run={status}, query={query_status}, kind={failure_kind:?}, error={error:?}",
            run.run_id
        ));
    }
    let _ = run.status;
    Ok(())
}

fn read_evidence(
    application: &LibraryApplication,
    subscription_id: i64,
) -> Result<Evidence, String> {
    let mut evidence = application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            let traversed_post_count = connection.query_row(
                "SELECT COUNT(DISTINCT ssp.source_post_id)
                 FROM subscription_source_post ssp
                 WHERE ssp.subscription_id = ?1",
                [subscription_id],
                |row| row.get::<_, i64>(0),
            )? as usize;
            let unsettled_items: i64 = connection.query_row(
                "SELECT COUNT(*)
                 FROM subscription_source_post ssp
                 JOIN source_item si ON si.source_post_id = ssp.source_post_id
                 WHERE ssp.subscription_id = ?1 AND si.state <> 'ingested'",
                [subscription_id],
                |row| row.get(0),
            )?;
            if unsettled_items != 0 {
                return Err(LibraryError::InvalidState(format!(
                    "successful run retained {unsettled_items} non-ingested source items"
                )));
            }
            let mut statement = connection.prepare(
                "SELECT sp.source_post_id, sp.post_key, sp.canonical_url,
                        sp.creator_name, sp.title, sp.description, sp.captured_at,
                        sp.root_item_id
                 FROM subscription_source_post ssp
                 JOIN source_post sp ON sp.source_post_id = ssp.source_post_id
                 WHERE ssp.subscription_id = ?1
                   AND sp.root_item_id IS NOT NULL
                 ORDER BY sp.source_post_id",
            )?;
            let rows = statement.query_map([subscription_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })?;
            let mut posts = Vec::new();
            for row in rows {
                let (
                    source_post_id,
                    post_key,
                    canonical_url,
                    creator_name,
                    title,
                    description,
                    captured_at,
                    root_item_id,
                ) = row?;
                let mut item_statement = connection.prepare(
                    "SELECT si.source_item_id, si.item_key, si.position, si.media_item_id,
                            mf.content_hash, mf.mime, mf.size_bytes
                     FROM source_item si
                     JOIN media_item mi ON mi.media_id = si.media_item_id
                     JOIN media_file mf ON mf.file_id = mi.file_id
                     WHERE si.source_post_id = ?1 AND si.state = 'ingested'
                     ORDER BY si.position, si.source_item_id",
                )?;
                let items = item_statement
                    .query_map([source_post_id], |row| {
                        Ok(MediaEvidence {
                            source_item_id: row.get(0)?,
                            item_key: row.get(1)?,
                            position: row.get(2)?,
                            media_item_id: row.get(3)?,
                            file_hash: row.get(4)?,
                            mime_type: row.get(5)?,
                            size_bytes: row.get(6)?,
                            tag_count: 0,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let root_item_id = root_item_id.ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "source post {source_post_id} has no visible root"
                    ))
                })?;
                let collection_member_order = picto_library::ordering::load(
                    connection,
                    picto_library::ordering::OrderOwnerKind::Collection,
                    u32::try_from(root_item_id)
                        .map_err(|_| LibraryError::InvalidState("root ID exceeds u32".into()))?,
                )?
                .unwrap_or_default()
                .into_iter()
                .map(i64::from)
                .collect::<Vec<i64>>();
                let rooted_media_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root
                     WHERE root_id IN (
                         SELECT media_item_id FROM source_item
                         WHERE source_post_id = ?1 AND state = 'ingested'
                     )",
                    [source_post_id],
                    |row| row.get(0),
                )?;
                posts.push(PostEvidence {
                    source_post_id,
                    post_key,
                    canonical_url: canonical_url.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "source post {source_post_id} has no canonical URL"
                        ))
                    })?,
                    creator_name,
                    title,
                    description,
                    captured_at,
                    root_item_id,
                    root_kind: String::new(),
                    lifecycle: String::new(),
                    items,
                    collection_member_order,
                    rooted_media_count,
                });
            }
            let tag_namespaces = connection
                .prepare(
                    "SELECT DISTINCT ns.display_name
                     FROM tag_definition def
                     JOIN tag_namespace ns USING (namespace_id)
                     ORDER BY ns.display_name",
                )?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Evidence {
                traversed_post_count,
                posts,
                tag_namespaces,
            })
        })
        .map_err(|error| error.to_string())?;
    for post in &mut evidence.posts {
        let details = application.details(RootId(
            u32::try_from(post.root_item_id).map_err(|_| "root ID exceeds u32".to_string())?,
        ))?;
        post.root_kind = match details.root.kind {
            RootKind::Media => "media",
            RootKind::Collection => "collection",
        }
        .into();
        post.lifecycle = match details.lifecycle {
            Lifecycle::Active => "active",
            Lifecycle::Inbox => "inbox",
            Lifecycle::Trash => "trash",
        }
        .into();
        for item in &mut post.items {
            item.tag_count = details.tag_ids.len() as i64;
        }
    }
    Ok(evidence)
}

fn validate_evidence(root: &Path, site_id: &str, evidence: &Evidence) -> Result<(), String> {
    let blob_store = BlobStore::open(root).map_err(|error| error.to_string())?;
    // Source tags must land in a canonical namespace; every unmapped source
    // category has to fall back to `general` instead of inventing one. The
    // schema stores general as the empty display name (bare tag names split
    // to ("", name) at ingest).
    const CANONICAL_NAMESPACES: &[&str] = &[
        "",
        "general",
        "creator",
        "character",
        "series",
        "species",
        "rating",
    ];
    for namespace in &evidence.tag_namespaces {
        if !CANONICAL_NAMESPACES.contains(&namespace.as_str()) {
            return Err(format!(
                "source run persisted non-canonical tag namespace `{namespace}`"
            ));
        }
    }
    let creator_source = matches!(
        site_id,
        "pixiv"
            | "pixivuser"
            | "hentaifoundry"
            | "baraag"
            | "deviantart"
            | "tumblr"
            | "furaffinity"
            | "patreon"
            | "fanbox"
            | "subscribestar"
            | "onlyfans"
    );
    let tagged_source = !matches!(site_id, "patreon" | "fanbox" | "subscribestar" | "onlyfans");
    let mut identities = BTreeSet::new();
    for post in &evidence.posts {
        if post.post_key.trim().is_empty() || !identities.insert(post.post_key.as_str()) {
            return Err(format!(
                "source post identity is empty or duplicated: {}",
                post.post_key
            ));
        }
        if post.canonical_url.trim().is_empty() {
            return Err(format!(
                "source post {} has an empty canonical URL",
                post.post_key
            ));
        }
        if creator_source && post.creator_name.as_deref().is_none_or(str::is_empty) {
            return Err(format!(
                "source post {} has no creator metadata",
                post.post_key
            ));
        }
        for text in [post.title.as_deref(), post.description.as_deref()]
            .into_iter()
            .flatten()
        {
            require_sanitized_text(&post.post_key, text)?;
        }
        if post.items.is_empty() {
            return Err(format!(
                "source post {} has no ingested media",
                post.post_key
            ));
        }
        if post.lifecycle != "inbox" {
            return Err(format!(
                "source post {} entered {} instead of Inbox",
                post.post_key, post.lifecycle
            ));
        }
        validate_root_shape(post)?;
        for media in &post.items {
            if !picto_core::media_capabilities::capabilities_for_stored_media(
                &media.mime_type,
                None,
            )
            .ingest_supported
            {
                return Err(format!(
                    "source item {} produced unsupported MIME {}",
                    media.item_key, media.mime_type
                ));
            }
            if tagged_source && media.tag_count == 0 {
                return Err(format!("source item {} persisted no tags", media.item_key));
            }
            let path = blob_store
                .original_path_with_ext(&media.file_hash, Some(mime_to_extension(&media.mime_type)))
                .map_err(|error| error.to_string())?;
            let bytes = std::fs::metadata(&path)
                .map_err(|error| format!("missing blob {}: {error}", path.display()))?
                .len() as i64;
            if bytes == 0 || bytes != media.size_bytes {
                return Err(format!(
                    "blob {} has {bytes} bytes but SQLite records {}",
                    media.file_hash, media.size_bytes
                ));
            }
        }
    }
    Ok(())
}

fn validate_root_shape(post: &PostEvidence) -> Result<(), String> {
    let expected_kind = if post.items.len() == 1 {
        "media"
    } else {
        "collection"
    };
    if post.root_kind != expected_kind {
        return Err(format!(
            "source post {} has {} media but root kind {}",
            post.post_key,
            post.items.len(),
            post.root_kind
        ));
    }
    let source_order = post
        .items
        .iter()
        .map(|item| item.media_item_id)
        .collect::<Vec<_>>();
    if post.items.len() == 1 {
        if post.root_item_id != post.items[0].media_item_id
            || post.rooted_media_count != 1
            || !post.collection_member_order.is_empty()
        {
            return Err(format!(
                "single-media source post {} is not one independent media root",
                post.post_key
            ));
        }
    } else if post.collection_member_order != source_order || post.rooted_media_count != 0 {
        return Err(format!(
            "collection source post {} has member order {:?}, expected {:?}, or exposed an independent member root",
            post.post_key, post.collection_member_order, source_order
        ));
    }
    Ok(())
}

fn require_sanitized_text(post_key: &str, text: &str) -> Result<(), String> {
    let html_tag = regex::Regex::new(r"(?i)</?[a-z][^>]*>").expect("valid HTML tag regex");
    let html_entity =
        regex::Regex::new(r"(?i)&(?:#x?[0-9a-f]+|[a-z][a-z0-9]+);").expect("valid entity regex");
    if html_tag.is_match(text) || html_entity.is_match(text) {
        return Err(format!(
            "source post {post_key} persisted unsanitized HTML text: {text:?}"
        ));
    }
    // Formatting BBCode/DText that the normalizer must have consumed. Bracket
    // tags are restricted to the markup vocabulary so bracketed prose passes.
    let bbcode_tag = regex::Regex::new(
        r"(?i)\[/?(?:b|i|u|s|code|quote|section|spoiler|url|color|sup|sub|size|center|table)(?:=[^\]]*)?\]",
    )
    .expect("valid BBCode regex");
    if bbcode_tag.is_match(text) || text.contains("[[") || text.contains("{{") {
        return Err(format!(
            "source post {post_key} persisted unsanitized BBCode/DText markup: {text:?}"
        ));
    }
    let labeled_link =
        regex::Regex::new(r#""[^"\n]*":\[?(?:https?://|/)"#).expect("valid labeled link regex");
    if labeled_link.is_match(text) {
        return Err(format!(
            "source post {post_key} persisted an unstripped labeled link: {text:?}"
        ));
    }
    Ok(())
}

fn read_checkpoint(
    application: &LibraryApplication,
    subscription_id: i64,
) -> Result<String, String> {
    application
        .library()
        .auxiliary_read(WorkPriority::VisibleRead, |connection| {
            Ok(connection.query_row(
                "SELECT COALESCE(resume_cursor, '<null>')
             FROM subscription_query WHERE subscription_id = ?1",
                [subscription_id],
                |row| row.get(0),
            )?)
        })
        .map_err(|error| error.to_string())
}

fn require_prefix_preserved(before: &Evidence, after: &Evidence) -> Result<(), String> {
    let after_by_key = after
        .posts
        .iter()
        .map(|post| (post.post_key.as_str(), post))
        .collect::<BTreeMap<_, _>>();
    for post in &before.posts {
        if after_by_key.get(post.post_key.as_str()).copied() != Some(post) {
            return Err(format!(
                "continuation changed or removed source post {}",
                post.post_key
            ));
        }
    }
    Ok(())
}

impl Evidence {
    fn media_count(&self) -> usize {
        self.posts.iter().map(|post| post.items.len()).sum()
    }

    fn collection_count(&self) -> usize {
        self.posts
            .iter()
            .filter(|post| post.root_kind == "collection")
            .count()
    }
}

#[derive(Debug, serde::Serialize)]
struct PacingEvidence {
    requests: usize,
    hosts: usize,
    minimum_same_host_gap_ms: i64,
}

/// Prove every consecutive pair of requests to one host is at least the
/// policy interval apart, from the bridge's HTTP-boundary trace.
fn validate_request_pacing(trace_path: &std::path::Path) -> Result<PacingEvidence, String> {
    let raw = std::fs::read_to_string(trace_path).map_err(|error| {
        format!(
            "certification ran without a request trace at {}: {error}",
            trace_path.display()
        )
    })?;
    let mut last_by_host: std::collections::BTreeMap<String, i64> = Default::default();
    let mut minimum_gap = i64::MAX;
    let mut requests = 0usize;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let entry: serde_json::Value =
            serde_json::from_str(line).map_err(|error| format!("invalid trace line: {error}"))?;
        let host = entry["host"].as_str().unwrap_or_default().to_string();
        let at = entry["monotonic_ms"]
            .as_i64()
            .ok_or("trace line without a monotonic timestamp")?;
        let expected_gap = entry["minimum_interval_ms"]
            .as_i64()
            .ok_or("trace line without its effective domain policy")?
            .saturating_sub(5);
        requests += 1;
        if let Some(previous) = last_by_host.insert(host.clone(), at) {
            let gap = at - previous;
            minimum_gap = minimum_gap.min(gap);
            if gap < expected_gap {
                return Err(format!(
                    "two requests to {host} were only {gap}ms apart; its policy requires at least {expected_gap}ms"
                ));
            }
        }
    }
    if requests == 0 {
        return Err("the request trace recorded no requests".into());
    }
    Ok(PacingEvidence {
        requests,
        hosts: last_by_host.len(),
        minimum_same_host_gap_ms: if minimum_gap == i64::MAX {
            -1
        } else {
            minimum_gap
        },
    })
}

fn write_report(
    site_id: &str,
    query: &str,
    batch_size: u32,
    auth_mode: &'static str,
    first: &Evidence,
    first_attempts: &AttemptSequenceEvidence,
    final_evidence: &Evidence,
    continuation_attempts: &AttemptSequenceEvidence,
    pacing: &PacingEvidence,
) -> Result<(), String> {
    let Some(path) = std::env::var_os("PICTO_LIVE_SUBSCRIPTION_REPORT") else {
        return Ok(());
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let report = serde_json::json!({
        "schema_version": 4,
        "generated_at": Utc::now().to_rfc3339(),
        "site_id": site_id,
        "query": query,
        "authentication": auth_mode,
        "requested_source_posts": batch_size,
        "first_fetch": {
            "traversed_posts": first.traversed_post_count,
            "materialized_posts": first.posts.len(),
            "media_items": first.media_count(),
            "collections": first.collection_count(),
            "posts": first.posts,
            "attempt_sequence": first_attempts,
        },
        "final_state": {
            "traversed_posts": final_evidence.traversed_post_count,
            "materialized_posts": final_evidence.posts.len(),
            "media_items": final_evidence.media_count(),
            "collections": final_evidence.collection_count(),
            "continuation_attempt_sequence": continuation_attempts,
        },
        "request_pacing": pacing,
        "checks": {
            "provider_request_pacing": true,
            "source_identity_persisted": true,
            "canonical_urls_persisted": true,
            "metadata_text_sanitized": true,
            "tags_and_creator_metadata_checked": true,
            "tag_namespaces_canonical_or_general": true,
            "supported_media_mime": true,
            "blob_sizes_match_sqlite": true,
            "single_posts_are_media_roots": true,
            "multi_media_posts_are_collection_roots": true,
            "collection_members_follow_source_order": true,
            "restart_is_stable": true,
            "next_run_continues": true,
            "post_attempts_do_not_overlap": true,
            "successful_runs_have_only_terminal_attempts": true,
            "added_post_budget_not_exceeded": true,
            "downloaded_files_are_newly_retained_files": true,
        }
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    println!("subscription_certification_report={}", path.display());
    Ok(())
}

fn configure_credential(site_id: &str) -> Result<&'static str, String> {
    if std::env::var("PICTO_LIVE_SUBSCRIPTION_ALLOW_KEYCHAIN")
        .ok()
        .as_deref()
        != Some("1")
    {
        // Certification must never read, write, or prompt the OS keychain
        // unless the operator explicitly opted in: the process-local
        // ephemeral store takes over before the first credential access.
        std::env::set_var("PICTO_EPHEMERAL_CREDENTIALS", "1");
    }
    if let Some(path) = std::env::var_os("PICTO_LIVE_SUBSCRIPTION_CREDENTIAL_FILE") {
        install_fixture_credential(Path::new(&path), site_id)?;
        return Ok("fixture");
    }
    if std::env::var("PICTO_LIVE_SUBSCRIPTION_ALLOW_KEYCHAIN")
        .ok()
        .as_deref()
        == Some("1")
    {
        return picto_core::credential_store::get_credential(site_id)?
            .map(|_| "keychain")
            .ok_or_else(|| format!("no stored direct-site credential exists for '{site_id}'"));
    }
    Ok("anonymous")
}

fn install_fixture_credential(path: &Path, site_id: &str) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct CredentialFile {
        credentials: BTreeMap<String, picto_core::credential_store::SiteCredential>,
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read verifier credential file: {error}"))?;
    let file: CredentialFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid verifier credential file: {error}"))?;
    let credential = file
        .credentials
        .get(site_id)
        .ok_or_else(|| format!("verifier credential file has no entry for '{site_id}'"))?;
    picto_core::credential_store::set_credential(credential)
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn requested_batch_size() -> Result<u32, String> {
    let value = std::env::var("PICTO_LIVE_SUBSCRIPTION_POST_LIMIT").unwrap_or_else(|_| "5".into());
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| "PICTO_LIVE_SUBSCRIPTION_POST_LIMIT must be positive".into())
}
