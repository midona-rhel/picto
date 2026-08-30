//! Production bridge from native source adapters into the durable subscription worker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::subscription_runtime::{
    DownloadedItem, FailedMediaItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess,
    SourceEvent, SourceRunner,
};
use crate::subscriptions::{ClaimedQueryRun, NormalizedItem, NormalizedPost};
use picto_sources::{
    DomainPolicy, HttpPolicy, HttpRuntime, NextPost, PartitionedSourceSession, PostDownloader,
    ProviderRegistry, RequestCredentials, SourceError, SourceErrorKind, SourcePartition,
    SourcePost, SourcePostOutcome,
};

const CURRENT_POST_DOWNLOAD_CONCURRENCY: usize = 4;
const MAX_TRAVERSED_PER_ADDED_TARGET: u32 = 10;
const SOURCE_POST_PULL_SIZE: u32 = 1;

pub struct NativeSourceRunner {
    library_root: PathBuf,
    registry: ProviderRegistry,
    http: Arc<HttpRuntime>,
    downloader: PostDownloader,
}

impl NativeSourceRunner {
    pub fn open(library_root: &Path) -> Self {
        Self {
            library_root: library_root.to_path_buf(),
            registry: ProviderRegistry::native(),
            http: shared_http_runtime(),
            downloader: PostDownloader::new(CURRENT_POST_DOWNLOAD_CONCURRENCY)
                .expect("native download concurrency is nonzero"),
        }
    }

    pub fn supports(&self, site_id: &str) -> bool {
        self.registry.get(site_id).is_some()
    }

    async fn execute(
        &self,
        query: &ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> Result<RunnerSuccess, RunnerFailure> {
        let adapter = self.registry.get(&query.site_id).ok_or_else(|| {
            RunnerFailure::terminal(
                RunnerFailureKind::InvalidQuery,
                format!("No native source adapter exists for {}", query.site_id),
            )
        })?;
        let descriptor = adapter.descriptor();
        let credentials = load_credentials(&query.site_id, descriptor.domain)?;
        if !descriptor.anonymous && credentials.is_empty() {
            return Err(RunnerFailure::terminal(
                RunnerFailureKind::Authentication,
                format!("{} requires a connected account", descriptor.display_name),
            ));
        }
        let partitions = adapter.partition_order();
        if partitions.is_empty() {
            return Err(RunnerFailure::terminal(
                RunnerFailureKind::InvalidOutput,
                "Native source has no stream partition",
            ));
        }
        let cursors = decode_runtime_cursor(&partitions, query.resume_cursor.as_deref())?;
        let mut session = PartitionedSourceSession::new(
            adapter,
            credentials.clone(),
            query.query_text.clone(),
            cursors,
            SOURCE_POST_PULL_SIZE,
            query.source_post_batch_size(),
        )
        .map_err(map_source_error)?;
        let run_staging = run_staging_path(&self.library_root, query);
        tokio::fs::create_dir_all(&run_staging)
            .await
            .map_err(|error| {
                RunnerFailure::retryable(RunnerFailureKind::Download, error.to_string())
            })?;

        let traversal_budget = query
            .configured_post_limit()
            .saturating_mul(MAX_TRAVERSED_PER_ADDED_TARGET)
            .max(MAX_TRAVERSED_PER_ADDED_TARGET);
        let mut traversed = 0_u32;
        let mut stop_after_current_execution = false;
        let resume_cursor = loop {
            match session
                .next_post(&self.http, &cancel)
                .await
                .map_err(map_source_error)?
            {
                NextPost::AddedBudgetReached => {
                    break Some(encode_runtime_cursor(&partitions, session.cursors())?);
                }
                NextPost::SourceExhausted => break Some(String::new()),
                NextPost::Post(post) => {
                    let post = *post;
                    let normalized = normalize_post(&post)?;
                    send_event(
                        &output,
                        SourceEvent::PostTraversed(normalized.clone()),
                        &cancel,
                    )
                    .await?;
                    let post_staging = run_staging.join(staging_name(&post));
                    let mut downloads = self
                        .downloader
                        .stream(&post, &credentials, &post_staging, &self.http, &cancel)
                        .await
                        .map_err(map_source_error)?;
                    while let Some((descriptor, result)) = downloads.next().await {
                        let download = match result {
                            Ok(media) => picto_sources::PostDownload {
                                downloaded: vec![media],
                                failures: Vec::new(),
                            },
                            Err(error)
                                if error.kind == picto_sources::SourceErrorKind::Cancelled =>
                            {
                                return Err(map_source_error(error));
                            }
                            Err(error) => picto_sources::PostDownload {
                                downloaded: Vec::new(),
                                failures: vec![picto_sources::MediaDownloadFailure {
                                    descriptor,
                                    message: error.message,
                                    retryable: error.retryable,
                                }],
                            },
                        };
                        let prepared = crate::native_source_import::prepare_source_post(
                            &post,
                            download,
                            chrono::Utc::now().timestamp_millis(),
                        )
                        .await
                        .map_err(|error| {
                            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error)
                        })?;
                        for rejected in prepared.rejected_media {
                            send_event(
                                &output,
                                SourceEvent::MediaFailed(FailedMediaItem {
                                    post: normalized.clone(),
                                    item_key: rejected.media_id,
                                    error_message: rejected.message,
                                }),
                                &cancel,
                            )
                            .await?;
                        }
                        for input in prepared.members {
                            send_event(
                                &output,
                                SourceEvent::MediaDownloaded(DownloadedItem {
                                    post: normalized.clone(),
                                    input,
                                    post_complete: false,
                                    force_collection: false,
                                    delete_after_ingest: true,
                                }),
                                &cancel,
                            )
                            .await?;
                        }
                    }
                    let outcome = complete_post(&output, &post.stable_id, &cancel).await?;
                    session
                        .settle(&post.stable_id, outcome)
                        .map_err(map_source_error)?;
                    cleanup_staging(&post_staging).await;
                    traversed = traversed.saturating_add(1);
                    if traversed >= traversal_budget
                        && session.added_count() < query.source_post_batch_size()
                    {
                        stop_after_current_execution = true;
                        break Some(encode_runtime_cursor(&partitions, session.cursors())?);
                    }
                }
            }
        };

        Ok(RunnerSuccess {
            resume_cursor,
            cleanup_paths: vec![run_staging],
            stop_after_current_execution,
        })
    }
}

fn decode_runtime_cursor(
    partitions: &[SourcePartition],
    raw: Option<&str>,
) -> Result<BTreeMap<SourcePartition, Option<String>>, RunnerFailure> {
    let raw = raw.filter(|value| !value.is_empty());
    if partitions.len() == 1 {
        return Ok([(partitions[0].clone(), raw.map(ToOwned::to_owned))]
            .into_iter()
            .collect());
    }
    let Some(raw) = raw else {
        return Ok(partitions
            .iter()
            .cloned()
            .map(|partition| (partition, None))
            .collect());
    };
    let decoded = serde_json::from_str::<BTreeMap<String, Option<String>>>(raw).map_err(|_| {
        RunnerFailure::terminal(
            RunnerFailureKind::InvalidQuery,
            "Invalid native source partition cursor",
        )
    })?;
    Ok(partitions
        .iter()
        .cloned()
        .map(|partition| {
            let cursor = decoded.get(&partition.0).cloned().flatten();
            (partition, cursor)
        })
        .collect())
}

fn encode_runtime_cursor(
    partitions: &[SourcePartition],
    cursors: &BTreeMap<SourcePartition, Option<String>>,
) -> Result<String, RunnerFailure> {
    if partitions.len() == 1 {
        return Ok(cursors
            .get(&partitions[0])
            .cloned()
            .flatten()
            .unwrap_or_default());
    }
    serde_json::to_string(
        &partitions
            .iter()
            .map(|partition| {
                (
                    partition.0.clone(),
                    cursors.get(partition).cloned().flatten(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Runtime, error.to_string()))
}

impl SourceRunner for NativeSourceRunner {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a> {
        Box::pin(async move {
            self.execute(query, output, cancel)
                .await
                .map_err(|mut failure| {
                    failure
                        .cleanup_paths
                        .push(run_staging_path(&self.library_root, query));
                    failure
                })
        })
    }
}

pub fn clear_subscription_state(library_root: &Path, subscription_id: i64) -> Result<(), String> {
    let path = library_root
        .join("source-runners/native")
        .join(format!("subscription-{subscription_id}"));
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn run_staging_path(library_root: &Path, query: &ClaimedQueryRun) -> PathBuf {
    library_root
        .join("source-runners/native")
        .join(format!("subscription-{}", query.subscription_id))
        .join(format!("query-{}", query.query_id))
        .join(format!("run-query-{}", query.run_query_id))
}

fn shared_http_runtime() -> Arc<HttpRuntime> {
    static HTTP: OnceLock<Arc<HttpRuntime>> = OnceLock::new();
    Arc::clone(HTTP.get_or_init(|| {
        let onlyfans = DomainPolicy {
            minimum_interval: std::time::Duration::from_millis(500),
            maximum_interval: std::time::Duration::from_millis(500),
            media_minimum_interval: std::time::Duration::ZERO,
            media_maximum_interval: std::time::Duration::ZERO,
            request_timeout: std::time::Duration::from_secs(45),
            retries: 10,
        };
        Arc::new(
            HttpRuntime::with_domain_policies(
                HttpPolicy::default(),
                BTreeMap::from([("onlyfans.com".to_string(), onlyfans)]),
            )
            .expect("default native HTTP policy"),
        )
    }))
}

fn load_credentials(site_id: &str, domain: &str) -> Result<RequestCredentials, RunnerFailure> {
    let owner = crate::subscriptions::sites::site_by_id(site_id)
        .map(|site| site.credential_owner_site_id)
        .unwrap_or(site_id);
    let credential = crate::credential_store::get_credential(owner)
        .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Authentication, error))?;
    let Some(credential) = credential else {
        return Ok(RequestCredentials::default());
    };
    Ok(stored_request_credentials(credential, domain))
}

fn stored_request_credentials(
    credential: crate::credential_store::SiteCredential,
    domain: &str,
) -> RequestCredentials {
    let mut credentials = RequestCredentials {
        headers: credential.headers.unwrap_or_default().into_iter().collect(),
        cookies: credential.cookies.unwrap_or_default().into_iter().collect(),
        username: credential.username,
        allowed_domains: [domain.to_ascii_lowercase()].into_iter().collect(),
        ..RequestCredentials::default()
    };
    match credential.credential_type {
        crate::credential_store::CredentialType::Cookies => {}
        crate::credential_store::CredentialType::ApiKey => {
            credentials.api_key = credential.password;
        }
        crate::credential_store::CredentialType::OAuthToken => {
            credentials.oauth_token = credential.oauth_token;
            credentials.oauth_token_secret = credential.password;
        }
    }
    credentials
}

fn normalize_post(post: &SourcePost) -> Result<NormalizedPost, RunnerFailure> {
    let metadata_json = serde_json::to_string(post).map_err(|error| {
        RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
    })?;
    Ok(NormalizedPost {
        site_id: post.site_id.clone(),
        post_key: post.stable_id.clone(),
        canonical_url: post.canonical_url.clone(),
        creator_name: post.creator.clone(),
        title: post.name.clone(),
        description: post.notes.clone(),
        captured_at: post.created_at.clone(),
        metadata_json: Some(metadata_json),
        items: post
            .media
            .iter()
            .map(|media| NormalizedItem {
                item_key: media.stable_id.clone(),
                position: i64::from(media.position),
                media_url: Some(media.url.clone()),
                canonical_url: media.canonical_url.clone(),
            })
            .collect(),
    })
}

async fn complete_post(
    output: &mpsc::Sender<SourceEvent>,
    post_key: &str,
    cancel: &CancellationToken,
) -> Result<SourcePostOutcome, RunnerFailure> {
    let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
    send_event(
        output,
        SourceEvent::PostComplete {
            post_key: post_key.to_string(),
            acknowledge,
        },
        cancel,
    )
    .await?;
    tokio::select! {
        _ = cancel.cancelled() => Err(RunnerFailure::retryable(
            RunnerFailureKind::Interrupted,
            "Native source stopped while settling its current post",
        )),
        outcome = acknowledged => outcome.map_err(|_| RunnerFailure::terminal(
            RunnerFailureKind::Runtime,
            "Native source post settlement was not acknowledged",
        )),
    }
}

async fn send_event(
    output: &mpsc::Sender<SourceEvent>,
    event: SourceEvent,
    cancel: &CancellationToken,
) -> Result<(), RunnerFailure> {
    tokio::select! {
        _ = cancel.cancelled() => Err(RunnerFailure::retryable(
            RunnerFailureKind::Interrupted,
            "Native source stopped",
        )),
        result = output.send(event) => result.map_err(|_| RunnerFailure::terminal(
            RunnerFailureKind::Runtime,
            "subscription receiver closed",
        )),
    }
}

fn staging_name(post: &SourcePost) -> String {
    let digest = Sha256::digest(format!("{}:{}", post.site_id, post.stable_id).as_bytes());
    format!("post-{}", hex::encode(&digest[..12]))
}

async fn cleanup_staging(path: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "Could not clean native source staging");
        }
    }
}

fn map_source_error(error: SourceError) -> RunnerFailure {
    match error.kind {
        SourceErrorKind::Authentication => {
            RunnerFailure::terminal(RunnerFailureKind::Authentication, error.message)
        }
        SourceErrorKind::InvalidQuery => {
            RunnerFailure::terminal(RunnerFailureKind::InvalidQuery, error.message)
        }
        SourceErrorKind::RateLimited => {
            RunnerFailure::retryable(RunnerFailureKind::RateLimited, error.message)
        }
        SourceErrorKind::Network => {
            RunnerFailure::retryable(RunnerFailureKind::Network, error.message)
        }
        SourceErrorKind::InvalidResponse => {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.message)
        }
        SourceErrorKind::Download => {
            RunnerFailure::retryable(RunnerFailureKind::Download, error.message)
        }
        SourceErrorKind::Cancelled => {
            RunnerFailure::retryable(RunnerFailureKind::Interrupted, error.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential_store::{CredentialType, SiteCredential};
    use picto_sources::SourcePartition;

    #[test]
    fn every_product_source_has_exactly_one_native_adapter() {
        let catalog = crate::subscriptions::sites::SITES
            .iter()
            .map(|site| (site.id, site.domain))
            .collect::<BTreeMap<_, _>>();
        let native = ProviderRegistry::native()
            .descriptors()
            .into_iter()
            .map(|provider| (provider.id, provider.domain))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(native, catalog);
    }

    #[test]
    fn native_auth_requirements_match_the_product_catalog() {
        let native = ProviderRegistry::native()
            .descriptors()
            .into_iter()
            .map(|provider| (provider.id, provider.anonymous))
            .collect::<BTreeMap<_, _>>();

        for site in crate::subscriptions::sites::SITES {
            assert_eq!(
                native.get(site.id).copied(),
                Some(!site.auth_strictly_required),
                "native auth boundary differs for {}",
                site.id
            );
        }
    }

    #[test]
    fn staging_names_do_not_expose_or_trust_provider_identity() {
        let post = SourcePost {
            site_id: "fixture".into(),
            partition: SourcePartition::new("feed"),
            stable_id: "../../unsafe".into(),
            canonical_url: None,
            creator: None,
            name: None,
            notes: None,
            created_at: None,
            tags: Vec::new(),
            media: Vec::new(),
            resume_cursor_after: None,
        };
        let name = staging_name(&post);
        assert!(name.starts_with("post-"));
        assert!(!name.contains('/'));
        assert!(!name.contains("unsafe"));
    }

    #[test]
    fn stored_credentials_map_directly_to_native_request_fields() {
        let api = stored_request_credentials(
            SiteCredential {
                site_category: "gelbooru".into(),
                credential_type: CredentialType::ApiKey,
                username: Some("123".into()),
                password: Some("secret".into()),
                cookies: Some(std::collections::HashMap::from([(
                    "session".into(),
                    "captured".into(),
                )])),
                headers: None,
                oauth_token: None,
            },
            "gelbooru.com",
        );
        assert_eq!(api.username.as_deref(), Some("123"));
        assert_eq!(api.api_key.as_deref(), Some("secret"));
        assert_eq!(
            api.cookies.get("session").map(String::as_str),
            Some("captured")
        );
        assert!(api.allowed_domains.contains("gelbooru.com"));

        let oauth = stored_request_credentials(
            SiteCredential {
                site_category: "baraag".into(),
                credential_type: CredentialType::OAuthToken,
                username: None,
                password: Some("token-secret".into()),
                cookies: None,
                headers: Some(std::collections::HashMap::from([(
                    "User-Agent".into(),
                    "captured-agent".into(),
                )])),
                oauth_token: Some("access-token".into()),
            },
            "baraag.net",
        );
        assert_eq!(oauth.oauth_token.as_deref(), Some("access-token"));
        assert_eq!(oauth.oauth_token_secret.as_deref(), Some("token-secret"));
        assert_eq!(
            oauth.headers.get("User-Agent").map(String::as_str),
            Some("captured-agent")
        );
    }

    #[test]
    fn staging_is_owned_by_subscription_query_and_execution() {
        let query = ClaimedQueryRun {
            run_query_id: 30,
            run_id: 20,
            query_id: 10,
            subscription_id: 5,
            site_id: "fixture".into(),
            domain_key: "fixture.test".into(),
            query_kind: "search".into(),
            query_text: "example".into(),
            group_posts: true,
            requested_by: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            run_post_limit: Some(1),
            initial_run_complete: false,
            resume_cursor: None,
            attempt_count: 1,
        };
        assert_eq!(
            run_staging_path(Path::new("/library"), &query),
            Path::new("/library/source-runners/native/subscription-5/query-10/run-query-30")
        );
    }

    #[test]
    fn traversal_safety_budget_scales_with_the_added_post_target() {
        assert_eq!(1_u32.saturating_mul(MAX_TRAVERSED_PER_ADDED_TARGET), 10);
        assert_eq!(
            100_u32.saturating_mul(MAX_TRAVERSED_PER_ADDED_TARGET),
            1_000
        );
    }

    #[test]
    fn partition_cursor_codec_keeps_single_streams_plain_and_multi_streams_independent() {
        let feed = SourcePartition::new("feed");
        let single = [feed.clone()];
        let decoded = decode_runtime_cursor(&single, Some("after-1")).unwrap();
        assert_eq!(decoded[&feed].as_deref(), Some("after-1"));
        assert_eq!(encode_runtime_cursor(&single, &decoded).unwrap(), "after-1");

        let purchases = SourcePartition::new("purchases");
        let messages = SourcePartition::new("messages");
        let partitions = [purchases.clone(), messages.clone(), feed.clone()];
        let cursors = BTreeMap::from([
            (purchases.clone(), Some("paid-2".into())),
            (messages.clone(), Some("message-4".into())),
            (feed.clone(), None),
        ]);
        let encoded = encode_runtime_cursor(&partitions, &cursors).unwrap();
        assert_eq!(
            decode_runtime_cursor(&partitions, Some(&encoded)).unwrap(),
            cursors
        );
    }

    #[tokio::test]
    #[ignore = "requires live provider network access"]
    async fn live_provider_settles_through_the_production_worker() {
        if std::env::var_os("PICTO_LIVE_USE_CREDENTIAL_STORE").is_none() {
            std::env::set_var("PICTO_EPHEMERAL_CREDENTIALS", "1");
        }
        let site_id = std::env::var("PICTO_LIVE_SOURCE_SITE").unwrap_or_else(|_| "e621".into());
        let query_text =
            std::env::var("PICTO_LIVE_SOURCE_QUERY").unwrap_or_else(|_| "rating:s".into());
        let directory = tempfile::tempdir().unwrap();
        let application = crate::library_application::LibraryApplication::create(
            directory.path().join("library"),
        )
        .unwrap();
        let definition = crate::subscription_catalog::NewSubscription {
            name: format!("{site_id} live smoke"),
            schedule: "manual".into(),
            initial_post_limit: Some(1),
            periodic_post_limit: Some(1),
            queries: vec![crate::subscription_catalog::NewSubscriptionQuery {
                site_id: site_id.clone(),
                query_text,
                display_name: None,
                notes: None,
                group_posts: true,
            }],
        };
        let now = chrono::Utc::now().to_rfc3339();
        let (subscription_id, _) = application
            .create_subscription_definition_library(&definition, &now)
            .unwrap();
        application
            .request_subscription_run_library(subscription_id, &now)
            .unwrap();
        let worker = crate::subscription_runtime::SubscriptionWorker::new(
            &application,
            NativeSourceRunner::open(application.root()),
        );
        worker.tick(&chrono::Utc::now().to_rfc3339()).await.unwrap();

        let subscription = crate::subscription_catalog::list_library(&application)
            .unwrap()
            .subscriptions
            .into_iter()
            .find(|entry| entry.subscription_id == subscription_id)
            .unwrap();
        let diagnostics = application
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    let mut statement = connection.prepare(
                        "SELECT run_query.status, COALESCE(run_query.failure_kind, ''),
                                COALESCE(run_query.error_message, ''),
                                COALESCE(attempt.state, ''),
                                COALESCE(attempt.terminal_reason, '')
                         FROM subscription_run_query run_query
                         LEFT JOIN source_post_attempt attempt USING(run_query_id)
                         ORDER BY attempt.attempt_id",
                    )?;
                    let rows = statement
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, String>(4)?,
                            ))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(rows)
                },
            )
            .unwrap();
        assert_eq!(
            subscription.status.as_deref(),
            Some("succeeded"),
            "native smoke did not settle: {diagnostics:?}"
        );
        assert_eq!(
            subscription.progress.posts_added, 1,
            "native smoke did not add a post: progress={:?}, diagnostics={diagnostics:?}",
            subscription.progress
        );
        assert!(subscription.progress.posts_traversed >= 1);
        assert!(subscription.progress.downloaded >= 1);
        assert_eq!(subscription.progress.failed, 0);
    }
}
