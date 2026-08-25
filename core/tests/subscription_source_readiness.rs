use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use picto_core::app::Application;
use picto_core::blob_store::{mime_to_extension, BlobStore};
use picto_core::onlyfans_source_v2::SubscriptionSourceRouter;
use picto_core::store::Store;
use picto_core::subscription_catalog_v2::{NewSubscription, NewSubscriptionQuery};
use picto_core::subscription_runtime_v2::SubscriptionWorker;
use picto_core::subscriptions::gallery_dl_runner::site_by_id;
use picto_core::subscriptions::source_adapter::describe_site;

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
}

#[derive(Debug, Clone, Copy)]
struct RunEvidence {
    run_id: i64,
    status: &'static str,
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
    picto_core::state_v2::init_tracing();
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
    let application = open_application(root)?;
    let (subscription_id, _) = application.create_subscription_definition(
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
    require_success(first_run, application.store())?;
    let first = read_evidence(application.store(), subscription_id)?;
    validate_evidence(root, &site_id, &first)?;
    if first.posts.is_empty() {
        return Err("source run succeeded without materializing any media posts".into());
    }
    if first.traversed_post_count > batch_size as usize {
        return Err(format!(
            "source traversed {} posts for a requested batch of {batch_size}",
            first.traversed_post_count
        ));
    }
    let checkpoint = read_checkpoint(application.store(), subscription_id)?;
    drop(application);

    let reopened = open_application(root)?;
    let after_restart = read_evidence(reopened.store(), subscription_id)?;
    if first != after_restart {
        return Err("closing and reopening changed persisted source or media identity".into());
    }
    if checkpoint != read_checkpoint(reopened.store(), subscription_id)? {
        return Err("closing and reopening changed the durable continuation cursor".into());
    }

    // One more source post proves that the next run continues from persisted
    // state instead of replaying the first source window.
    let second_run = execute_run(&reopened, subscription_id, 1).await?;
    require_success(second_run, reopened.store())?;
    let continued = read_evidence(reopened.store(), subscription_id)?;
    validate_evidence(root, &site_id, &continued)?;
    require_prefix_preserved(&first, &continued)?;
    let continued_checkpoint = read_checkpoint(reopened.store(), subscription_id)?;
    if continued.posts.len() == first.posts.len() && continued_checkpoint == checkpoint {
        return Err("continuation neither materialized media nor advanced source history".into());
    }

    write_report(
        &site_id,
        &query_text,
        batch_size,
        auth_mode,
        &first,
        &continued,
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

fn open_application(root: &Path) -> Result<Application, String> {
    Ok(Application::try_new(Arc::new(Store::open(root)?))?)
}

async fn execute_run(
    application: &Application,
    subscription_id: i64,
    _batch_size: u32,
) -> Result<RunEvidence, String> {
    let now = Utc::now().to_rfc3339();
    let (created, _) = application.request_subscription_run(subscription_id, &now)?;
    if !created.created {
        return Err("subscription already had an active run".into());
    }
    let runner = SubscriptionSourceRouter::open(application.store().library_root());
    let worker = SubscriptionWorker::new(application, runner);
    worker.tick(&Utc::now().to_rfc3339()).await?;
    Ok(RunEvidence {
        run_id: created.run_id,
        status: "finished",
    })
}

fn require_success(run: RunEvidence, store: &Store) -> Result<(), String> {
    let (status, query_status, failure_kind, error): (
        String,
        String,
        Option<String>,
        Option<String>,
    ) = store.read(|connection| {
        connection.query_row(
            "SELECT sr.status, srq.status, srq.failure_kind, srq.error_message
             FROM subscription_run sr
             JOIN subscription_run_query srq ON srq.run_id = sr.run_id
             WHERE sr.run_id = ?1",
            [run.run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
    })?;
    if status != "succeeded" || query_status != "succeeded" {
        return Err(format!(
            "run {} did not succeed: run={status}, query={query_status}, kind={failure_kind:?}, error={error:?}",
            run.run_id
        ));
    }
    let _ = run.status;
    Ok(())
}

fn read_evidence(store: &Store, subscription_id: i64) -> Result<Evidence, String> {
    store.read_result(|connection| {
        let traversed_post_count = connection
            .query_row(
                "SELECT COUNT(DISTINCT ssp.source_post_id)
                 FROM subscription_source_post ssp
                 WHERE ssp.subscription_id = ?1",
                [subscription_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())? as usize;
        let unsettled_items: i64 = connection
            .query_row(
                "SELECT COUNT(*)
                 FROM subscription_source_post ssp
                 JOIN source_item si ON si.source_post_id = ssp.source_post_id
                 WHERE ssp.subscription_id = ?1 AND si.state <> 'ingested'",
                [subscription_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if unsettled_items != 0 {
            return Err(format!(
                "successful run retained {unsettled_items} non-ingested source items"
            ));
        }
        let mut statement = connection
            .prepare(
                "SELECT sp.source_post_id, sp.post_key, sp.canonical_url,
                        sp.creator_name, sp.title, sp.description, sp.captured_at,
                        sp.root_item_id, li.kind, lr.lifecycle
                 FROM subscription_source_post ssp
                 JOIN source_post sp ON sp.source_post_id = ssp.source_post_id
                 LEFT JOIN library_item li ON li.item_id = sp.root_item_id
                 LEFT JOIN library_root lr ON lr.item_id = sp.root_item_id
                 WHERE ssp.subscription_id = ?1
                   AND sp.root_item_id IS NOT NULL
                 ORDER BY sp.source_post_id",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([subscription_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            })
            .map_err(|error| error.to_string())?;
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
                root_kind,
                lifecycle,
            ) = row.map_err(|error| error.to_string())?;
            let mut item_statement = connection
                .prepare(
                    "SELECT si.source_item_id, si.item_key, si.position, si.media_item_id,
                            mf.file_hash, mf.mime_type, mf.size_bytes,
                            COUNT(mt.tag_id)
                     FROM source_item si
                     JOIN media_asset ma ON ma.item_id = si.media_item_id
                     JOIN media_file mf ON mf.file_id = ma.file_id
                     LEFT JOIN media_tag mt ON mt.media_item_id = ma.item_id
                     WHERE si.source_post_id = ?1 AND si.state = 'ingested'
                     GROUP BY si.source_item_id
                     ORDER BY si.position, si.source_item_id",
                )
                .map_err(|error| error.to_string())?;
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
                        tag_count: row.get(7)?,
                    })
                })
                .map_err(|error| error.to_string())?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?;
            let root_item_id = root_item_id
                .ok_or_else(|| format!("source post {source_post_id} has no visible root"))?;
            let collection_member_order = connection
                .prepare(
                    "SELECT media_item_id FROM collection_member
                     WHERE collection_id = ?1
                     ORDER BY position_rank, media_item_id",
                )
                .map_err(|error| error.to_string())?
                .query_map([root_item_id], |row| row.get(0))
                .map_err(|error| error.to_string())?
                .collect::<rusqlite::Result<Vec<i64>>>()
                .map_err(|error| error.to_string())?;
            let rooted_media_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM library_root
                     WHERE item_id IN (
                         SELECT media_item_id FROM source_item
                         WHERE source_post_id = ?1 AND state = 'ingested'
                     )",
                    [source_post_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            posts.push(PostEvidence {
                source_post_id,
                post_key,
                canonical_url: canonical_url
                    .ok_or_else(|| format!("source post {source_post_id} has no canonical URL"))?,
                creator_name,
                title,
                description,
                captured_at,
                root_item_id,
                root_kind: root_kind
                    .ok_or_else(|| format!("source post {source_post_id} root is missing"))?,
                lifecycle: lifecycle
                    .ok_or_else(|| format!("source post {source_post_id} root is hidden"))?,
                items,
                collection_member_order,
                rooted_media_count,
            });
        }
        Ok(Evidence {
            traversed_post_count,
            posts,
        })
    })
}

fn validate_evidence(root: &Path, site_id: &str, evidence: &Evidence) -> Result<(), String> {
    let blob_store = BlobStore::open(root).map_err(|error| error.to_string())?;
    let creator_source = matches!(
        site_id,
        "pixiv"
            | "pixivuser"
            | "webtoons"
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
    let tagged_source = !matches!(
        site_id,
        "patreon" | "fanbox" | "subscribestar" | "webtoons" | "onlyfans"
    );
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
            if !media.mime_type.starts_with("image/") && !media.mime_type.starts_with("video/") {
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
    Ok(())
}

fn read_checkpoint(store: &Store, subscription_id: i64) -> Result<String, String> {
    store.read(|connection| {
        connection.query_row(
            "SELECT COALESCE(resume_cursor, '<null>')
             FROM subscription_query WHERE subscription_id = ?1",
            [subscription_id],
            |row| row.get(0),
        )
    })
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

fn write_report(
    site_id: &str,
    query: &str,
    batch_size: u32,
    auth_mode: &'static str,
    first: &Evidence,
    final_evidence: &Evidence,
) -> Result<(), String> {
    let Some(path) = std::env::var_os("PICTO_LIVE_SUBSCRIPTION_REPORT") else {
        return Ok(());
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let report = serde_json::json!({
        "schema_version": 3,
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
        },
        "final_state": {
            "traversed_posts": final_evidence.traversed_post_count,
            "materialized_posts": final_evidence.posts.len(),
            "media_items": final_evidence.media_count(),
            "collections": final_evidence.collection_count(),
        },
        "checks": {
            "source_identity_persisted": true,
            "canonical_urls_persisted": true,
            "metadata_text_sanitized": true,
            "tags_and_creator_metadata_checked": true,
            "image_video_mime_only": true,
            "blob_sizes_match_sqlite": true,
            "single_posts_are_media_roots": true,
            "multi_media_posts_are_collection_roots": true,
            "collection_members_follow_source_order": true,
            "restart_is_stable": true,
            "next_run_continues": true,
        }
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    println!("subscription_certification_report={}", path.display());
    Ok(())
}

fn configure_credential(site_id: &str) -> Result<&'static str, String> {
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
    keyring::set_default_credential_builder(Box::new(SharedMemoryCredentialBuilder::default()));
    picto_core::credential_store::set_credential(credential)
}

type SharedSecrets = Arc<Mutex<BTreeMap<(String, String), Vec<u8>>>>;

#[derive(Default)]
struct SharedMemoryCredentialBuilder {
    secrets: SharedSecrets,
}

impl keyring::credential::CredentialBuilderApi for SharedMemoryCredentialBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<keyring::credential::Credential>> {
        Ok(Box::new(SharedMemoryCredential {
            key: (service.to_string(), user.to_string()),
            secrets: Arc::clone(&self.secrets),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn persistence(&self) -> keyring::credential::CredentialPersistence {
        keyring::credential::CredentialPersistence::ProcessOnly
    }
}

struct SharedMemoryCredential {
    key: (String, String),
    secrets: SharedSecrets,
}

impl keyring::credential::CredentialApi for SharedMemoryCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        self.secrets
            .lock()
            .expect("credential fixture lock poisoned")
            .insert(self.key.clone(), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.secrets
            .lock()
            .expect("credential fixture lock poisoned")
            .get(&self.key)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        self.secrets
            .lock()
            .expect("credential fixture lock poisoned")
            .remove(&self.key)
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
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
