use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use picto_core::blob_store::{mime_to_extension, BlobStore};
use picto_core::db::LibraryDatabase;
use picto_core::ingest_queue::IngestQueueItemPayload;
use picto_core::subscriptions::credential_service::SubscriptionCredentialService;
use picto_core::subscriptions::gallery_dl_runner::{build_url, site_by_id};
use picto_core::subscriptions::runtime_service::SubscriptionRuntimeService;
use picto_core::subscriptions::source_adapter::{describe_site, validate_query_kind};
use picto_core::subscriptions::types::{SubscriptionQuery, SubscriptionQueryRunRecord};

const RUN_TIMEOUT: Duration = Duration::from_secs(240);
const DEFAULT_POST_LIMIT: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MemberEvidence {
    site_id: String,
    post_id: String,
    item_key: String,
    entity_id: i64,
    entity_hash: String,
    entity_status: i64,
    member_status: String,
    page_num: Option<i64>,
    canonical_post_url: String,
    media_url: String,
    source_urls_json: String,
    date_created: String,
    tag_count: i64,
    creator_tag_count: i64,
    expected_tags: BTreeSet<(String, String)>,
    persisted_tags: BTreeSet<(String, String)>,
    file_hash: String,
    mime_type: String,
    size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryEvidence {
    members: BTreeSet<MemberEvidence>,
    linked_entity_ids: BTreeSet<i64>,
    post_order: Vec<(String, String)>,
    expected_member_counts: BTreeMap<(String, String), i64>,
}

#[derive(Debug, Clone, Copy)]
struct QueueEvidence {
    complete: i64,
    pending: i64,
    running: i64,
    failed: i64,
}

/// This is deliberately one selected-source test rather than a fixture matrix.
/// The source probe owns extractor-specific expectations; this test certifies
/// that a real download survives Picto's complete persistence lifecycle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PICTO_LIVE_SUBSCRIPTION_SITE/QUERY and explicit local credential access for credential-capable sources"]
async fn live_subscription_source_persistence_certification() {
    let result = certify_selected_source().await;
    let _ = picto_core::state::close_library().await;
    result.unwrap_or_else(|error| panic!("subscription certification failed: {error}"));
}

async fn certify_selected_source() -> Result<(), String> {
    let site_id = required_env("PICTO_LIVE_SUBSCRIPTION_SITE")?;
    let query_text = required_env("PICTO_LIVE_SUBSCRIPTION_QUERY")?;
    let post_limit = requested_post_limit()?;
    let site = site_by_id(&site_id).ok_or_else(|| format!("unknown site '{site_id}'"))?;
    let descriptor = describe_site(&site_id)
        .ok_or_else(|| format!("site '{site_id}' has no source descriptor"))?;
    if descriptor.query_kinds.len() != 1 {
        return Err(format!(
            "site '{site_id}' exposes {} query behaviors; certification requires one",
            descriptor.query_kinds.len()
        ));
    }
    let query_kind = descriptor.query_kinds[0].id;
    validate_query_kind(&site_id, query_kind)?;
    let source_url = build_url(&site_id, &query_text)
        .ok_or_else(|| format!("site '{site_id}' could not build a source URL"))?;

    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = temp.path();
    let state = picto_core::state::open_library(root.to_path_buf()).await?;
    let mut settings = state.settings.get();
    settings.sub_batch_size = post_limit;
    settings.sub_abort_threshold = 1;
    state.settings.update(settings);

    let (subscription_id, query_id, first_run, first_query, first_evidence) = {
        let db = LibraryDatabase::open(root)?;
        let runtime = SubscriptionRuntimeService::new(&db, root);

        if site.auth_supported {
            let credential_file = std::env::var_os("PICTO_LIVE_SUBSCRIPTION_CREDENTIAL_FILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
            let credential_file_configured = credential_file.is_some();
            let allow_keychain = std::env::var("PICTO_LIVE_SUBSCRIPTION_ALLOW_KEYCHAIN")
                .ok()
                .as_deref()
                == Some("1");
            if site.auth_strictly_required && credential_file.is_none() && !allow_keychain {
                return Err(format!(
                    "site '{site_id}' requires credentials; pass a verifier-only --credential-file or explicitly allow an attended --allow-keychain run"
                ));
            }

            if let Some(path) = credential_file.as_deref() {
                install_mock_credential(path, site.credential_owner_site_id)?;
            }
            if allow_keychain || credential_file_configured {
                attach_existing_credential(root, site.credential_owner_site_id)?;
                let credential = SubscriptionCredentialService::new(&db)
                    .resolve_credential(&site_id, &source_url)
                    .await;
                if site.auth_strictly_required && !credential.has_credential() {
                    return Err(format!(
                        "site '{site_id}' requires a real credential; missing credentials are not a skip"
                    ));
                }
            }
        }

        let subscription = runtime
            .create_subscription(format!("certify-{site_id}"), Some(post_limit), Some(1))
            .await?;
        let subscription_id = parse_id("subscription", &subscription.id)?;
        let query = runtime
            .add_subscription_query(
                subscription.id.clone(),
                site_id.clone(),
                Some(query_kind.to_string()),
                query_text.clone(),
                Some("release source certification".to_string()),
            )
            .await?;
        let query_id = parse_id("query", &query.id)?;

        run_query(&subscription.id, &query.id).await?;
        let first_run = wait_for_finished_query_run(&runtime, query_id, 0).await?;
        require_successful_download(&first_run, Some(post_limit))?;
        let first_query = runtime
            .get_subscription_query(query_id)
            .await?
            .ok_or_else(|| "query disappeared after its first run".to_string())?;
        let queue = read_queue_evidence(root, subscription_id, query_id)?;
        if queue.pending != 0 || queue.running != 0 || queue.failed != 0 {
            return Err(format!(
                "first run left non-terminal ingest work: {queue:?}"
            ));
        }
        if queue.complete < first_run.files_downloaded {
            return Err(format!(
                "first run reports {} downloads but only {} completed ingest items",
                first_run.files_downloaded, queue.complete
            ));
        }
        let evidence = read_library_evidence(root, subscription_id)?;
        validate_library_evidence(root, &evidence, true, true)?;
        validate_first_fetch(&evidence, post_limit)?;
        (subscription_id, query_id, first_run, first_query, evidence)
    };

    picto_core::state::close_library().await?;
    let state = picto_core::state::open_library(root.to_path_buf()).await?;
    let mut settings = state.settings.get();
    settings.sub_batch_size = post_limit;
    settings.sub_abort_threshold = 1;
    state.settings.update(settings);

    let db = LibraryDatabase::open(root)?;
    let runtime = SubscriptionRuntimeService::new(&db, root);
    let reopened_query = runtime
        .get_subscription_query(query_id)
        .await?
        .ok_or_else(|| "query disappeared after reopening the library".to_string())?;
    verify_query_checkpoint(&first_query, &reopened_query)?;
    let reopened_evidence = read_library_evidence(root, subscription_id)?;
    verify_evidence_preserved(&first_evidence, &reopened_evidence, "restart")?;
    if reopened_evidence.members.len() != first_evidence.members.len()
        || reopened_evidence.linked_entity_ids != first_evidence.linked_entity_ids
        || reopened_evidence.post_order != first_evidence.post_order
    {
        return Err("restart added or removed durable subscription lineage".into());
    }
    validate_library_evidence(root, &reopened_evidence, true, false)?;

    // Continue from the durable cursor after restart. This may add the next
    // source post, or add nothing if the initial history was already complete.
    // The first-fetch proof already covered the full requested batch, so one
    // post is enough to prove cursor continuation without downloading a second
    // 100-post batch.
    set_subscription_initial_post_limit(root, subscription_id, 1)?;
    run_query(&subscription_id.to_string(), &query_id.to_string()).await?;
    let resumed_run =
        wait_for_finished_query_run(&runtime, query_id, first_run.query_run_id).await?;
    require_successful_download(&resumed_run, None)?;
    let resumed_evidence = read_library_evidence(root, subscription_id)?;
    validate_library_evidence(root, &resumed_evidence, true, false)?;
    verify_evidence_preserved(&reopened_evidence, &resumed_evidence, "resume")?;
    if !reopened_query.completed_initial_run
        && resumed_evidence.members.len() == reopened_evidence.members.len()
    {
        return Err(
            "query reported more initial history, but resume imported no additional media".into(),
        );
    }

    // Revisit the beginning without deleting the gallery-dl archive. Every
    // already-seen item must be skipped before ingest, so no entity or lineage
    // row can be duplicated.
    runtime
        .set_query_completed_initial_run(query_id, false)
        .await?;
    runtime.set_query_resume_state(query_id, None, None).await?;
    set_subscription_initial_post_limit(root, subscription_id, post_limit)?;
    run_query(&subscription_id.to_string(), &query_id.to_string()).await?;
    let replay_run =
        wait_for_finished_query_run(&runtime, query_id, resumed_run.query_run_id).await?;
    require_successful_download(&replay_run, None)?;
    let replay_evidence = read_library_evidence(root, subscription_id)?;
    validate_library_evidence(root, &replay_evidence, true, false)?;
    verify_evidence_preserved(&resumed_evidence, &replay_evidence, "replay")?;
    if replay_evidence.members.len() != resumed_evidence.members.len()
        || replay_evidence.linked_entity_ids != resumed_evidence.linked_entity_ids
        || replay_evidence.post_order != resumed_evidence.post_order
    {
        return Err(format!(
            "replaying the first source page changed media identity: before={} members, after={} members",
            resumed_evidence.members.len(),
            replay_evidence.members.len()
        ));
    }
    if replay_run.files_downloaded != 0 {
        return Err(format!(
            "archive-backed replay downloaded {} already-seen files",
            replay_run.files_downloaded
        ));
    }

    write_certification_report(
        &site_id,
        &query_text,
        post_limit,
        first_run.posts_processed as u32,
        &first_evidence,
        &replay_evidence,
    )?;
    println!(
        "subscription_certification: site={} first_fetch_source_posts={} first_fetch_materialized_posts={} first_fetch_members={} final_posts={} final_members={} restart=passed resume=passed replay=passed",
        site_id,
        first_run.posts_processed,
        first_evidence.post_order.len(),
        first_evidence.members.len(),
        replay_evidence.post_order.len(),
        replay_evidence.members.len(),
    );
    Ok(())
}

fn install_mock_credential(path: &Path, site_category: &str) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct CredentialFile {
        credentials: BTreeMap<String, picto_core::credential_store::SiteCredential>,
    }

    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read verifier credential file: {error}"))?;
    let mut file: CredentialFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid verifier credential file: {error}"))?;
    let credential = file
        .credentials
        .remove(site_category)
        .ok_or_else(|| format!("verifier credential file has no entry for '{site_category}'"))?;
    if credential.site_category != site_category {
        return Err(format!(
            "verifier credential key '{site_category}' contains credential for '{}'",
            credential.site_category
        ));
    }

    keyring::set_default_credential_builder(Box::new(SharedMemoryCredentialBuilder::default()));
    picto_core::credential_store::set_credential(&credential)
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
            .expect("verifier credential store lock poisoned")
            .insert(self.key.clone(), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.secrets
            .lock()
            .expect("verifier credential store lock poisoned")
            .get(&self.key)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        self.secrets
            .lock()
            .expect("verifier credential store lock poisoned")
            .remove(&self.key)
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn attach_existing_credential(root: &Path, site_category: &str) -> Result<(), String> {
    let credential = picto_core::credential_store::get_credential(site_category)?
        .ok_or_else(|| format!("no stored credential exists for '{site_category}'"))?;
    let conn = rusqlite::Connection::open(root.join("library.db")).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO credential_domain (site_category, credential_type, display_name, date_added)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(site_category) DO UPDATE SET
             credential_type = excluded.credential_type,
             display_name = excluded.display_name",
        rusqlite::params![
            site_category,
            credential.credential_type.as_str(),
            site_category,
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn run_query(subscription_id: &str, query_id: &str) -> Result<(), String> {
    picto_core::dispatch::dispatch(
        "run_subscription_query",
        &serde_json::json!({
            "subscription_id": subscription_id,
            "query_id": query_id,
        })
        .to_string(),
    )
    .await
    .map(|_| ())
}

async fn wait_for_finished_query_run(
    runtime: &SubscriptionRuntimeService<'_>,
    query_id: i64,
    after_query_run_id: i64,
) -> Result<SubscriptionQueryRunRecord, String> {
    let started = Instant::now();
    loop {
        let runs = runtime.list_subscription_query_runs(query_id, 10).await?;
        if let Some(run) = runs
            .into_iter()
            .filter(|run| run.query_run_id > after_query_run_id)
            .find(|run| run.finished_at.is_some())
        {
            return Ok(run);
        }
        if started.elapsed() > run_timeout() {
            return Err(format!("timed out waiting for query {query_id} to settle"));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn require_successful_download(
    run: &SubscriptionQueryRunRecord,
    expected_posts: Option<u32>,
) -> Result<(), String> {
    if run.status != "succeeded" {
        return Err(format!(
            "query run {} ended status={} kind={:?} error={:?}",
            run.query_run_id, run.status, run.failure_kind, run.error_message
        ));
    }
    if run.metadata_invalid != 0 {
        return Err(format!(
            "query run {} accepted {} invalid metadata records",
            run.query_run_id, run.metadata_invalid
        ));
    }
    if let Some(expected_posts) = expected_posts {
        if run.posts_processed != i64::from(expected_posts) {
            return Err(format!(
                "initial run processed {} posts instead of {}",
                run.posts_processed, expected_posts
            ));
        }
        if run.files_downloaded < 1 {
            return Err("initial run succeeded without downloading media".into());
        }
    }
    Ok(())
}

fn read_queue_evidence(
    root: &Path,
    subscription_id: i64,
    query_id: i64,
) -> Result<QueueEvidence, String> {
    let conn = open_read_connection(root)?;
    conn.query_row(
        "SELECT
             COALESCE(SUM(i.status = 'complete'), 0),
             COALESCE(SUM(i.status = 'pending'), 0),
             COALESCE(SUM(i.status = 'running'), 0),
             COALESCE(SUM(i.status = 'failed'), 0)
         FROM ingest_queue_item i
         JOIN ingest_queue q ON q.queue_id = i.queue_id
         WHERE q.subscription_id = ?1 AND q.query_id = ?2",
        rusqlite::params![subscription_id, query_id],
        |row| {
            Ok(QueueEvidence {
                complete: row.get(0)?,
                pending: row.get(1)?,
                running: row.get(2)?,
                failed: row.get(3)?,
            })
        },
    )
    .map_err(|error| error.to_string())
}

fn read_library_evidence(root: &Path, subscription_id: i64) -> Result<LibraryEvidence, String> {
    let conn = open_read_connection(root)?;
    let (expected_tags, expected_member_counts) = {
        let mut expected = BTreeMap::<(String, String, i64), BTreeSet<(String, String)>>::new();
        let mut expected_counts = BTreeMap::<(String, String), i64>::new();
        let mut stmt = conn
            .prepare(
                "SELECT i.page_num, i.payload_json
                 FROM ingest_queue_item i
                 JOIN ingest_queue q ON q.queue_id = i.queue_id
                 WHERE q.subscription_id = ?1 AND i.status = 'complete'",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([subscription_id], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (page_num, payload_json) = row.map_err(|error| error.to_string())?;
            let payload: IngestQueueItemPayload =
                serde_json::from_str(&payload_json).map_err(|error| error.to_string())?;
            let Some(metadata) = payload.subscription_metadata else {
                continue;
            };
            let page_count = metadata.page_count;
            let (Some(site_id), Some(post_id)) = (metadata.category, metadata.post_id) else {
                continue;
            };
            if let Some(page_count) = page_count {
                expected_counts.insert((site_id.clone(), post_id.clone()), i64::from(page_count));
            }
            let tags = expected
                .entry((site_id, post_id, page_num.unwrap_or(0)))
                .or_default();
            for tag in payload.request.tag_strings {
                let Some((namespace, subtag)) = picto_core::tags::normalize::parse_tag(&tag) else {
                    return Err(format!(
                        "ingest queue contains invalid normalized tag '{tag}'"
                    ));
                };
                tags.insert((namespace, subtag));
            }
        }
        (expected, expected_counts)
    };
    let persisted_tags = {
        let mut persisted = BTreeMap::<i64, BTreeSet<(String, String)>>::new();
        let mut stmt = conn
            .prepare(
                "SELECT et.entity_id, t.namespace, t.subtag
                 FROM subscription_post_member spm
                 JOIN media_entity me ON me.entity_id = spm.entity_id
                 JOIN entity_tag et ON et.entity_id = me.entity_id
                 JOIN tag t ON t.tag_id = et.tag_id
                 WHERE spm.subscription_id = ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([subscription_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (entity_id, namespace, subtag) = row.map_err(|error| error.to_string())?;
            persisted
                .entry(entity_id)
                .or_default()
                .insert((namespace, subtag));
        }
        persisted
    };
    let members = {
        let mut stmt = conn
            .prepare(
                "SELECT spm.site_id, spm.post_id, spm.item_key,
                        me.entity_id, me.entity_hash, me.status,
                        spm.status, spm.page_num, spm.canonical_post_url, spm.media_url,
                        me.source_urls_json, me.date_created,
                        (SELECT COUNT(*) FROM entity_tag et WHERE et.entity_id = me.entity_id),
                        (SELECT COUNT(*)
                           FROM entity_tag et
                           JOIN tag t ON t.tag_id = et.tag_id
                          WHERE et.entity_id = me.entity_id AND t.namespace = 'creator'),
                        mf.file_hash, mf.mime_type, mf.size_bytes
                 FROM subscription_post_member spm
                 LEFT JOIN media_entity me ON me.entity_id = spm.entity_id
                 LEFT JOIN media_file mf ON mf.file_id = me.file_id
                 WHERE spm.subscription_id = ?1
                 ORDER BY spm.site_id, spm.post_id, spm.item_key",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([subscription_id], |row| {
                Ok(MemberEvidence {
                    site_id: row.get(0)?,
                    post_id: row.get(1)?,
                    item_key: row.get(2)?,
                    entity_id: required_column(row, 3, "entity_id")?,
                    entity_hash: required_column(row, 4, "entity_hash")?,
                    entity_status: required_column(row, 5, "entity_status")?,
                    member_status: row.get(6)?,
                    page_num: row.get(7)?,
                    canonical_post_url: required_column(row, 8, "canonical_post_url")?,
                    media_url: required_column(row, 9, "media_url")?,
                    source_urls_json: required_column(row, 10, "source_urls_json")?,
                    date_created: required_column(row, 11, "date_created")?,
                    tag_count: row.get(12)?,
                    creator_tag_count: row.get(13)?,
                    expected_tags: BTreeSet::new(),
                    persisted_tags: BTreeSet::new(),
                    file_hash: required_column(row, 14, "file_hash")?,
                    mime_type: required_column(row, 15, "mime_type")?,
                    size_bytes: required_column(row, 16, "size_bytes")?,
                })
            })
            .map_err(|error| error.to_string())?;
        let mut members = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;
        for member in &mut members {
            member.expected_tags = expected_tags
                .get(&(
                    member.site_id.clone(),
                    member.post_id.clone(),
                    member.page_num.unwrap_or(0),
                ))
                .cloned()
                .unwrap_or_default();
            member.persisted_tags = persisted_tags
                .get(&member.entity_id)
                .cloned()
                .unwrap_or_default();
        }
        members.into_iter().collect()
    };
    let linked_entity_ids = {
        let mut stmt = conn
            .prepare(
                "SELECT entity_id FROM subscription_entity
                 WHERE subscription_id = ?1 ORDER BY entity_id",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([subscription_id], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<BTreeSet<_>>>()
            .map_err(|error| error.to_string())?
    };
    let post_order = {
        let mut stmt = conn
            .prepare(
                "SELECT site_id, post_id
                 FROM subscription_post_member
                 WHERE subscription_id = ?1
                 GROUP BY site_id, post_id
                 ORDER BY MIN(rowid)",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([subscription_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };
    Ok(LibraryEvidence {
        members,
        linked_entity_ids,
        post_order,
        expected_member_counts,
    })
}

fn validate_library_evidence(
    root: &Path,
    evidence: &LibraryEvidence,
    require_media: bool,
    require_source_plan: bool,
) -> Result<(), String> {
    if require_media && evidence.members.is_empty() {
        return Err("subscription has no durable post members".into());
    }
    let blob_store = BlobStore::open(root).map_err(|error| error.to_string())?;
    let mut posts = BTreeMap::<(&str, &str), Vec<&MemberEvidence>>::new();
    for member in &evidence.members {
        if !matches!(member.member_status.as_str(), "imported" | "reused") {
            return Err(format!(
                "post {} item {} remained in status '{}'",
                member.post_id, member.item_key, member.member_status
            ));
        }
        if member.entity_status != 0 {
            return Err(format!(
                "post {} imported with lifecycle status {} instead of Inbox",
                member.post_id, member.entity_status
            ));
        }
        if member.entity_hash != member.file_hash {
            return Err(format!(
                "post {} resolves entity {} to different file {}",
                member.post_id, member.entity_hash, member.file_hash
            ));
        }
        if member.canonical_post_url.trim().is_empty() || member.media_url.trim().is_empty() {
            return Err(format!(
                "post {} is missing durable source URLs",
                member.post_id
            ));
        }
        if member.canonical_post_url == member.media_url {
            return Err(format!(
                "post {} persisted its media URL as the canonical post URL",
                member.post_id
            ));
        }
        if member.date_created.trim().is_empty() {
            return Err(format!(
                "post {} is missing its source timestamp",
                member.post_id
            ));
        }
        if require_source_plan && member.expected_tags.is_empty() {
            return Err(format!(
                "post {} item {} has no normalized source tags in its durable ingest plan",
                member.post_id, member.item_key
            ));
        }
        if !member.expected_tags.is_empty()
            && !member.persisted_tags.is_superset(&member.expected_tags)
        {
            let missing = member
                .expected_tags
                .difference(&member.persisted_tags)
                .cloned()
                .collect::<Vec<_>>();
            return Err(format!(
                "post {} item {} lost source tags during ingest: {missing:?}",
                member.post_id, member.item_key
            ));
        }
        let source_urls: Vec<String> = serde_json::from_str(&member.source_urls_json)
            .map_err(|error| format!("post {} has invalid source URLs: {error}", member.post_id))?;
        if source_urls.is_empty() || !source_urls.contains(&member.canonical_post_url) {
            return Err(format!(
                "post {} did not persist its canonical URL on the media entity",
                member.post_id
            ));
        }
        let extension = mime_to_extension(&member.mime_type);
        let blob_path = blob_store
            .original_path_with_ext(&member.file_hash, Some(extension))
            .map_err(|error| error.to_string())?;
        let blob_size = std::fs::metadata(&blob_path)
            .map_err(|error| format!("missing blob {}: {error}", blob_path.display()))?
            .len() as i64;
        if blob_size != member.size_bytes || blob_size == 0 {
            return Err(format!(
                "blob {} size {} does not match database size {}",
                member.file_hash, blob_size, member.size_bytes
            ));
        }
        posts
            .entry((&member.site_id, &member.post_id))
            .or_default()
            .push(member);
    }

    for (post, members) in posts {
        for member in members {
            if !evidence.linked_entity_ids.contains(&member.entity_id) {
                return Err(format!(
                    "source post {}:{} media {} is not linked to its subscription",
                    post.0, post.1, member.entity_hash
                ));
            }
        }
    }
    Ok(())
}

fn validate_first_fetch(evidence: &LibraryEvidence, expected_posts: u32) -> Result<(), String> {
    let expected_posts = expected_posts as usize;
    if evidence.post_order.is_empty() || evidence.post_order.len() > expected_posts {
        return Err(format!(
            "the first fetch materialized {} image-bearing posts from {expected_posts} checked source posts",
            evidence.post_order.len()
        ));
    }
    if evidence.post_order.iter().collect::<BTreeSet<_>>().len() != evidence.post_order.len() {
        return Err("the first fetch contains duplicate source-post identities".into());
    }

    let mut members_by_post = BTreeMap::<(&str, &str), Vec<&MemberEvidence>>::new();
    for member in &evidence.members {
        members_by_post
            .entry((&member.site_id, &member.post_id))
            .or_default()
            .push(member);
    }

    for (ordinal, (site_id, post_id)) in evidence.post_order.iter().enumerate() {
        let members = members_by_post
            .get(&(site_id.as_str(), post_id.as_str()))
            .ok_or_else(|| {
                format!(
                    "first-fetch post {} ({site_id}:{post_id}) has no durable members",
                    ordinal + 1
                )
            })?;
        let expected_members = evidence
            .expected_member_counts
            .get(&(site_id.clone(), post_id.clone()))
            .or_else(|| {
                evidence
                    .expected_member_counts
                    .get(&(String::new(), post_id.clone()))
            })
            .copied();
        if let Some(expected_members) = expected_members {
            if members.len() as i64 != expected_members {
                return Err(format!(
                    "first-fetch post {} ({site_id}:{post_id}) persisted {} of {expected_members} advertised members",
                    ordinal + 1,
                    members.len()
                ));
            }
        }

        if members.len() > 1 {
            let mut pages = members
                .iter()
                .map(|member| member.page_num)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    format!(
                        "first-fetch post {} ({site_id}:{post_id}) is missing child order",
                        ordinal + 1
                    )
                })?;
            pages.sort_unstable();
            pages.dedup();
            if pages.len() != members.len() || pages.windows(2).any(|pair| pair[1] != pair[0] + 1) {
                return Err(format!(
                    "first-fetch post {} ({site_id}:{post_id}) has incomplete or duplicate child order {pages:?}",
                    ordinal + 1
                ));
            }
        }
    }

    // These boundary lookups deliberately fail if the report cannot prove the
    // first and last image-bearing posts from the requested source window.
    let _ = evidence
        .post_order
        .first()
        .ok_or_else(|| "the first-fetch boundary is missing post 1".to_string())?;
    let _ = evidence
        .post_order
        .last()
        .ok_or_else(|| "the first-fetch materialized boundary is missing".to_string())?;
    Ok(())
}

fn write_certification_report(
    site_id: &str,
    query_text: &str,
    requested_posts: u32,
    source_posts_processed: u32,
    first_fetch: &LibraryEvidence,
    final_evidence: &LibraryEvidence,
) -> Result<(), String> {
    let Some(report_path) = std::env::var_os("PICTO_LIVE_SUBSCRIPTION_REPORT") else {
        return Ok(());
    };
    let report_path = std::path::PathBuf::from(report_path);
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create report directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let posts = post_reports(first_fetch)?;
    let first = posts
        .first()
        .cloned()
        .ok_or_else(|| "cannot report an empty first fetch".to_string())?;
    let last = posts
        .last()
        .cloned()
        .ok_or_else(|| "cannot report an empty first fetch".to_string())?;
    let report = serde_json::json!({
        "schema_version": 2,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "site_id": site_id,
        "query": query_text,
        "requested_first_fetch_source_posts": requested_posts,
        "first_fetch": {
            "source_posts_processed": source_posts_processed,
            "materialized_post_count": first_fetch.post_order.len(),
            "member_count": first_fetch.members.len(),
            "first_materialized_post": first,
            "post_100": (requested_posts == 100 && first_fetch.post_order.len() >= 100).then_some(last.clone()),
            "last_materialized_post": last,
            "posts": posts,
        },
        "final_state": {
            "post_count": final_evidence.post_order.len(),
            "member_count": final_evidence.members.len(),
        },
        "checks": {
            "all_advertised_children_persisted": true,
            "all_blobs_match_database_sizes": true,
            "canonical_post_urls_persisted": true,
            "source_timestamps_persisted": true,
            "tags_and_creator_metadata_persisted": true,
            "child_order_is_complete": true,
            "restart_is_stable": true,
            "resume_preserves_identity": true,
            "replay_is_idempotent": true,
        },
    });
    let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    std::fs::write(&report_path, bytes)
        .map_err(|error| format!("could not write report {}: {error}", report_path.display()))?;
    println!(
        "subscription_certification_report={}",
        report_path.display()
    );
    Ok(())
}

fn post_reports(evidence: &LibraryEvidence) -> Result<Vec<serde_json::Value>, String> {
    let mut members_by_post = BTreeMap::<(&str, &str), Vec<&MemberEvidence>>::new();
    for member in &evidence.members {
        members_by_post
            .entry((&member.site_id, &member.post_id))
            .or_default()
            .push(member);
    }
    evidence
        .post_order
        .iter()
        .enumerate()
        .map(|(ordinal, (site_id, post_id))| {
            let mut members = members_by_post
                .get(&(site_id.as_str(), post_id.as_str()))
                .cloned()
                .ok_or_else(|| format!("missing report members for {site_id}:{post_id}"))?;
            members.sort_by_key(|member| (member.page_num, member.item_key.as_str()));
            let expected_members = evidence
                .expected_member_counts
                .get(&(site_id.clone(), post_id.clone()))
                .or_else(|| {
                    evidence
                        .expected_member_counts
                        .get(&(String::new(), post_id.clone()))
                })
                .copied()
                .unwrap_or(members.len() as i64);
            Ok(serde_json::json!({
                "ordinal": ordinal + 1,
                "site_id": site_id,
                "post_id": post_id,
                "expected_members": expected_members,
                "persisted_members": members.len(),
                "members": members.iter().map(|member| serde_json::json!({
                    "item_key": member.item_key,
                    "page_num": member.page_num,
                    "entity_hash": member.entity_hash,
                    "file_hash": member.file_hash,
                    "canonical_post_url": member.canonical_post_url,
                    "media_url": member.media_url,
                    "date_created": member.date_created,
                    "tag_count": member.tag_count,
                    "creator_tag_count": member.creator_tag_count,
                    "expected_tag_count": member.expected_tags.len(),
                    "mime_type": member.mime_type,
                    "size_bytes": member.size_bytes,
                })).collect::<Vec<_>>(),
            }))
        })
        .collect()
}

fn verify_evidence_preserved(
    before: &LibraryEvidence,
    after: &LibraryEvidence,
    phase: &str,
) -> Result<(), String> {
    let after_members = after
        .members
        .iter()
        .map(|member| {
            (
                (
                    member.site_id.as_str(),
                    member.post_id.as_str(),
                    member.item_key.as_str(),
                ),
                member,
            )
        })
        .collect::<BTreeMap<_, _>>();

    for old in &before.members {
        let key = (
            old.site_id.as_str(),
            old.post_id.as_str(),
            old.item_key.as_str(),
        );
        let Some(new) = after_members.get(&key) else {
            return Err(format!("{phase} removed post member {key:?}"));
        };
        if old.entity_id != new.entity_id
            || old.entity_hash != new.entity_hash
            || old.entity_status != new.entity_status
            || old.member_status != new.member_status
            || old.page_num != new.page_num
            || old.canonical_post_url != new.canonical_post_url
            || old.media_url != new.media_url
            || old.date_created != new.date_created
            || old.tag_count != new.tag_count
            || old.creator_tag_count != new.creator_tag_count
            || old.persisted_tags != new.persisted_tags
            || old.file_hash != new.file_hash
            || old.mime_type != new.mime_type
            || old.size_bytes != new.size_bytes
        {
            return Err(format!(
                "{phase} rewrote stable post-member evidence: before={old:?} after={new:?}"
            ));
        }

        let old_sources = parse_source_urls(old)?;
        let new_sources = parse_source_urls(new)?;
        if !new_sources.is_superset(&old_sources) {
            return Err(format!(
                "{phase} removed source URLs from {}:{}:{}: before={old_sources:?} after={new_sources:?}",
                old.site_id, old.post_id, old.item_key
            ));
        }
    }

    if !after
        .linked_entity_ids
        .is_superset(&before.linked_entity_ids)
    {
        return Err(format!(
            "{phase} removed an existing subscription entity link"
        ));
    }
    if !after.post_order.starts_with(&before.post_order) {
        return Err(format!(
            "{phase} changed source-post order: before={:?} after={:?}",
            before.post_order, after.post_order
        ));
    }
    // Expected counts belong to the consumed ingest plan. Startup prunes
    // completed queue rows, while durable member identity is verified above.
    Ok(())
}

fn parse_source_urls(member: &MemberEvidence) -> Result<BTreeSet<String>, String> {
    serde_json::from_str::<Vec<String>>(&member.source_urls_json)
        .map(|urls| urls.into_iter().collect())
        .map_err(|error| {
            format!(
                "post {} item {} has invalid source URLs: {error}",
                member.post_id, member.item_key
            )
        })
}

fn verify_query_checkpoint(
    before: &SubscriptionQuery,
    after: &SubscriptionQuery,
) -> Result<(), String> {
    if before.query_id != after.query_id
        || before.subscription_id != after.subscription_id
        || before.completed_initial_run != after.completed_initial_run
        || before.resume_cursor != after.resume_cursor
        || before.resume_strategy != after.resume_strategy
        || before.files_found != after.files_found
        || before.posts_found != after.posts_found
    {
        return Err(format!(
            "query checkpoint changed across restart: before={before:?} after={after:?}"
        ));
    }
    Ok(())
}

fn open_read_connection(root: &Path) -> Result<rusqlite::Connection, String> {
    let conn =
        rusqlite::Connection::open(root.join("library.db")).map_err(|error| error.to_string())?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    Ok(conn)
}

fn set_subscription_initial_post_limit(
    root: &Path,
    subscription_id: i64,
    limit: u32,
) -> Result<(), String> {
    let conn = open_read_connection(root)?;
    conn.execute(
        "UPDATE subscription SET initial_post_limit = ?1 WHERE subscription_id = ?2",
        rusqlite::params![i64::from(limit), subscription_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn required_column<T: rusqlite::types::FromSql>(
    row: &rusqlite::Row<'_>,
    index: usize,
    name: &str,
) -> rusqlite::Result<T> {
    row.get::<_, Option<T>>(index)?.ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Null,
            format!("missing {name}").into(),
        )
    })
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} must name the real source case to certify"))
}

fn requested_post_limit() -> Result<u32, String> {
    match std::env::var("PICTO_LIVE_SUBSCRIPTION_POST_LIMIT") {
        Ok(value) => value
            .trim()
            .parse::<u32>()
            .map_err(|error| format!("invalid PICTO_LIVE_SUBSCRIPTION_POST_LIMIT: {error}"))
            .and_then(|value| {
                if value == 0 {
                    Err("PICTO_LIVE_SUBSCRIPTION_POST_LIMIT must be greater than zero".into())
                } else {
                    Ok(value)
                }
            }),
        Err(_) => Ok(DEFAULT_POST_LIMIT),
    }
}

fn run_timeout() -> Duration {
    std::env::var("PICTO_LIVE_SUBSCRIPTION_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or(RUN_TIMEOUT)
}

fn parse_id(kind: &str, value: &str) -> Result<i64, String> {
    value
        .parse()
        .map_err(|error| format!("invalid {kind} id '{value}': {error}"))
}
