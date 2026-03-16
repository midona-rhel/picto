//! Subscription sync engine — downloads files via gallery-dl subprocess.
//!
//! Steps per query:
//! 1. Build URL from subscription's gallery-dl URL template + query text
//! 2. Load credentials from OS keychain (if configured for that site)
//! 3. Spawn gallery-dl subprocess with appropriate flags
//! 4. Import each downloaded file via the existing ImportPipeline
//! 5. Merge metadata for already-imported files (tags, URLs, notes, name)
//! 6. Track `completed_initial_run` for smart-stop behavior

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::blob_store::BlobStore;
use crate::credential_store;
use crate::import::existing::{merge_existing_import_target, ExistingImportMergeRequest};
use crate::import::pipeline::{ImportOptions, ImportPipeline};
use crate::settings::store::AppSettings;
use crate::subscriptions::archive::subscription_query_archive_prefix;
use crate::subscriptions::gallery_dl_runner::{self, FailureKind, GalleryDlRunner, ParsedMetadata, RunOptions};
use crate::subscriptions::import_policy::{
    collection_group_parts, generated_subscription_name, normalized_title,
    preferred_import_name, should_replace_existing_name, validate_metadata_for_site,
};
use crate::subscriptions::policy::{
    apply_resume_to_query, default_resume_strategy_for_site, derive_resume_cursor,
    effective_inbox_limit, resolve_query_name,
};
use crate::subscriptions::runtime_tasks::publish_running_progress;
use crate::sqlite::SqliteDatabase;
use crate::tags::normalize;

#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    pub files_downloaded: usize,
    pub files_skipped: usize,
    pub pages_fetched: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub failure_kind: Option<String>,
    pub metadata_validated: usize,
    pub metadata_invalid: usize,
    pub last_metadata_error: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportOutcome {
    hex_hash: String,
    imported_new: bool,
}

#[derive(Debug, Clone)]
struct CollectionGroup {
    category: String,
    post_id: String,
    preferred_name: String,
    hashes: Vec<String>,
}

pub struct SubscriptionSyncEngine<'a> {
    db: &'a SqliteDatabase,
    blob_store: &'a BlobStore,
    runner: GalleryDlRunner,
    settings: AppSettings,
    subscription_name: String,
    current_query_id: Option<i64>,
    current_query_name: Option<String>,
    last_progress_emit: std::time::Instant,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
}

impl<'a> SubscriptionSyncEngine<'a> {
    pub fn new(
        db: &'a SqliteDatabase,
        blob_store: &'a BlobStore,
        settings: &AppSettings,
    ) -> Result<Self, String> {
        let binary_path = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
        let runner = GalleryDlRunner::new(binary_path);

        Ok(Self {
            db,
            blob_store,
            runner,
            settings: settings.clone(),
            subscription_name: String::new(),
            current_query_id: None,
            current_query_name: None,
            last_progress_emit: std::time::Instant::now(),
            auto_merge_enabled: false,
            auto_merge_distance: crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD,
        })
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.subscription_name = name;
        self
    }

    pub fn with_auto_merge(mut self, enabled: bool, distance: u32) -> Self {
        self.auto_merge_enabled = enabled;
        self.auto_merge_distance = distance;
        self
    }

    /// Sync a single subscription query via gallery-dl.
    pub async fn sync_query(
        &mut self,
        subscription_id: i64,
        query_id: i64,
        query_text: &str,
        query_display_name: Option<&str>,
        site_id: &str,
        file_limit: Option<u32>,
        completed_initial_run: bool,
        resume_cursor: Option<&str>,
        resume_strategy: Option<&str>,
        cancel: CancellationToken,
    ) -> SyncProgress {
        self.current_query_id = Some(query_id);
        self.current_query_name =
            Some(resolve_query_name(query_id, query_text, query_display_name));
        let mut progress = SyncProgress::default();
        let sub_id_str = subscription_id.to_string();
        let inbox_limit = effective_inbox_limit(self.settings.sub_inbox_pause_limit);

        let inbox_count = self
            .db
            .bitmaps
            .len(&crate::sqlite::bitmaps::BitmapKey::Status(0));
        if inbox_count >= inbox_limit as u64 {
            progress.failure_kind = Some("inbox_full".to_string());
            self.emit_progress(
                &sub_id_str,
                &progress,
                &format!("Inbox cap reached ({inbox_limit}); waiting for review"),
            );
            progress.cancelled = true;
            return progress;
        }

        // 1. Build URL from site registry + query (+ optional best-effort resume cursor)
        let resume_strategy = resume_strategy
            .map(str::to_string)
            .or_else(|| default_resume_strategy_for_site(site_id).map(str::to_string));
        let query_for_run = match (
            resume_cursor,
            resume_strategy.as_deref(),
            completed_initial_run,
        ) {
            (Some(cursor), Some(strategy), false) if !cursor.trim().is_empty() => {
                apply_resume_to_query(query_text, cursor.trim(), strategy)
            }
            _ => query_text.to_string(),
        };
        let url = match gallery_dl_runner::build_url(site_id, &query_for_run) {
            Some(u) => u,
            None => {
                progress.errors.push(format!("Unknown site: {site_id}"));
                return progress;
            }
        };

        // 2. Load credentials (site id first, then domain-based fallbacks)
        let site_entry = gallery_dl_runner::site_by_id(site_id);
        let auth_supported = site_entry.is_some_and(|site| site.auth_supported);
        let auth_required = site_entry.is_some_and(|site| site.auth_required_for_full_access);
        let mut credential = None;
        if auth_supported {
            let mut credential_categories = vec![site_id.to_string()];
            if let Some(site) = site_entry {
                credential_categories.push(site.domain.to_string());
                credential_categories.push(site.domain.trim_start_matches("www.").to_string());
            }
            if let Some(domain) = gallery_dl_runner::extract_domain(&url) {
                credential_categories.push(domain.clone());
                credential_categories.push(domain.trim_start_matches("www.").to_string());
            }
            credential_categories.sort();
            credential_categories.dedup();

            for category in credential_categories {
                match credential_store::get_credential(&category) {
                    Ok(Some(c)) => {
                        credential = Some(c);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(site = %category, error = %e, "Failed to load credential");
                    }
                }
            }
        }
        if auth_supported && credential.is_none() && auth_required {
            self.emit_progress(
                &sub_id_str,
                &progress,
                "No credential configured for this site; some content may be inaccessible",
            );
            self.update_credential_health(
                site_id,
                "missing",
                Some("No credential configured for a site that commonly requires auth"),
            )
            .await;
        }

        // 3. Build archive path (per-library, shared across all subscriptions)
        // db_dir() returns the `db/` subdirectory; library root is one level up.
        let archive_path = self
            .db
            .db_dir()
            .parent()
            .map(|r| r.join("gdl-archive.sqlite3"))
            .unwrap_or_else(|| PathBuf::from("gdl-archive.sqlite3"));
        let archive_prefix = subscription_query_archive_prefix(subscription_id, query_id);

        // 4. Determine abort threshold: only on subsequent runs (smart stop)
        let abort_threshold = if completed_initial_run {
            Some(self.settings.sub_abort_threshold)
        } else {
            None
        };

        self.emit_progress(
            &sub_id_str,
            &progress,
            &format!("Starting gallery-dl for '{}'...", query_text),
        );
        let has_credential = credential.is_some();

        // 5. Run gallery-dl
        let opts = RunOptions {
            url: url.clone(),
            file_limit,
            abort_threshold,
            sleep_request: self.settings.sub_rate_limit_secs,
            credential,
            archive_path,
            archive_prefix: Some(archive_prefix),
            cancel: cancel.clone(),
        };

        let run_result = match self.runner.run(&opts).await {
            Ok(r) => r,
            Err(e) => {
                progress.errors.push(format!("gallery-dl failed: {e}"));
                progress.failure_kind = Some("unknown".to_string());
                self.update_credential_health(site_id, "error", Some(&e))
                    .await;
                return progress;
            }
        };

        if cancel.is_cancelled() {
            progress.cancelled = true;
        }
        if run_result.exit_code != 0 && !progress.cancelled {
            let failure_kind = gallery_dl_runner::classify_failure(&run_result.stderr_output);
            let failure_kind_str = match failure_kind {
                FailureKind::Unauthorized => "unauthorized",
                FailureKind::Expired => "expired",
                FailureKind::RateLimited => "rate_limited",
                FailureKind::Network => "network",
                FailureKind::Unknown => "unknown",
            };
            progress.failure_kind = Some(failure_kind_str.to_string());
            let summary = format!(
                "gallery-dl exited with code {} ({failure_kind_str})",
                run_result.exit_code
            );
            progress.errors.push(summary.clone());
            let health_status = match failure_kind {
                FailureKind::Unauthorized => "unauthorized",
                FailureKind::Expired => "expired",
                _ => "error",
            };
            let err = if run_result.stderr_output.trim().is_empty() {
                summary
            } else {
                run_result
                    .stderr_output
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or(summary.as_str())
                    .trim()
                    .to_string()
            };
            warn!(
                site_id = %site_id,
                query_id,
                failure_kind = failure_kind_str,
                error = %err,
                "gallery-dl query execution failed"
            );
            self.update_credential_health(site_id, health_status, Some(&err))
                .await;
        } else if run_result.exit_code == 0 && has_credential {
            self.update_credential_health(site_id, "valid", None).await;
        }

        // 6. Import each downloaded file
        let temp_dir = run_result
            .items
            .first()
            .and_then(|item| item.file_path.parent())
            .map(|p| p.to_path_buf());
        let mut collection_groups: HashMap<String, CollectionGroup> = HashMap::new();

        for item in &run_result.items {
            if cancel.is_cancelled() {
                progress.cancelled = true;
                break;
            }
            let inbox_count = self
                .db
                .bitmaps
                .len(&crate::sqlite::bitmaps::BitmapKey::Status(0));
            if inbox_count >= inbox_limit as u64 {
                progress.failure_kind = Some("inbox_full".to_string());
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Inbox cap reached ({inbox_limit}); pausing download"),
                );
                progress.cancelled = true;
                break;
            }
            progress.pages_fetched += 1;
            let post_id = item.metadata.post_id.as_deref().unwrap_or("unknown");
            self.emit_progress(
                &sub_id_str,
                &progress,
                &format!("Importing post {post_id}..."),
            );

            if let Err(metadata_error) = validate_metadata_for_site(site_id, &item.metadata) {
                progress.metadata_invalid += 1;
                progress.last_metadata_error = Some(metadata_error.clone());
                self.emit_progress(
                    &sub_id_str,
                    &progress,
                    &format!("Skipping invalid metadata: {metadata_error}"),
                );
                continue;
            }
            progress.metadata_validated += 1;

            match self
                .import_item(&item.file_path, &item.metadata, subscription_id, &url)
                .await
            {
                Ok(outcome) => {
                    if outcome.imported_new {
                        progress.files_downloaded += 1;
                    } else {
                        progress.files_skipped += 1;
                    }

                    if let Some((category, post_id, preferred_name)) =
                        collection_group_parts(site_id, &item.metadata)
                    {
                        let key = format!("{category}:{post_id}");
                        let group =
                            collection_groups
                                .entry(key)
                                .or_insert_with(|| CollectionGroup {
                                    category,
                                    post_id,
                                    preferred_name,
                                    hashes: Vec::new(),
                                });
                        if !group.hashes.iter().any(|h| h == &outcome.hex_hash) {
                            group.hashes.push(outcome.hex_hash.clone());
                        }
                    }

                    if outcome.imported_new {
                        self.emit_progress(
                            &sub_id_str,
                            &progress,
                            &format!("Downloaded {} files", progress.files_downloaded),
                        );
                    } else {
                        self.emit_progress(
                            &sub_id_str,
                            &progress,
                            &format!("Checking... ({} existing)", progress.files_skipped),
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        post_id = %post_id,
                        path = %item.file_path.display(),
                        error = %e,
                        "Subscription item import failed"
                    );
                    progress
                        .errors
                        .push(format!("Import error for post {post_id}: {e}"));
                }
            }
        }

        if !collection_groups.is_empty() {
            self.materialize_collection_groups(
                subscription_id,
                &sub_id_str,
                &cancel,
                &mut progress,
                collection_groups,
            )
            .await;
        }

        // 7. Clean up temp directory
        if let Some(ref dir) = temp_dir {
            // Go up to the actual temp root (gallery-dl creates subdirs)
            let temp_root = dir.parent().unwrap_or(dir);
            gallery_dl_runner::cleanup_temp_dir(temp_root).await;
        }

        // 8. Update query progress only on clean completion.
        // If a run is cancelled/failed we preserve prior "last successful" progress
        // so interrupted runs are not treated as completed.
        self.emit_progress_force(&sub_id_str, &progress, "Finalizing...");
        let completed_cleanly = run_result.exit_code == 0 && !progress.cancelled;
        let next_resume_cursor = resume_strategy
            .as_deref()
            .and_then(|strategy| derive_resume_cursor(&run_result.items, strategy));
        let continue_initial_pagination = should_continue_initial_pagination(
            completed_initial_run,
            completed_cleanly,
            file_limit,
            run_result.items.len(),
            next_resume_cursor.as_deref(),
        );

        // Persist resume state for initial-run pagination and interrupted runs.
        if !completed_initial_run {
            let persisted_cursor = if completed_cleanly {
                if continue_initial_pagination {
                    next_resume_cursor.clone()
                } else {
                    None
                }
            } else {
                next_resume_cursor
                    .clone()
                    .or_else(|| resume_cursor.map(|s| s.to_string()))
            };
            if let Err(e) = self
                .db
                .set_query_resume_state(query_id, persisted_cursor, resume_strategy.clone())
                .await
            {
                progress
                    .errors
                    .push(format!("Failed to persist query resume state: {e}"));
            }
        }

        if completed_cleanly {
            let now = Utc::now().to_rfc3339();
            if let Err(e) = self
                .db
                .update_query_progress(query_id, &now, progress.files_downloaded as i64)
                .await
            {
                progress
                    .errors
                    .push(format!("Failed to update query progress: {e}"));
            }
        } else {
            info!(
                query_id,
                exit_code = run_result.exit_code,
                cancelled = progress.cancelled,
                "Skipping progress checkpoint update: run did not complete cleanly"
            );
        }

        // 9. Mark completed_initial_run when initial pagination has drained.
        if !completed_initial_run && completed_cleanly && !continue_initial_pagination {
            if let Err(e) = self
                .db
                .set_query_completed_initial_run(query_id, true)
                .await
            {
                progress
                    .errors
                    .push(format!("Failed to mark initial run complete: {e}"));
            }
        } else if continue_initial_pagination {
            info!(
                query_id,
                next_resume_cursor = ?next_resume_cursor,
                fetched_items = run_result.items.len(),
                file_limit = ?file_limit,
                "Initial run continues; resuming next chunk"
            );
        }

        info!(
            query_id,
            downloaded = progress.files_downloaded,
            skipped = progress.files_skipped,
            errors = progress.errors.len(),
            exit_code = run_result.exit_code,
            cancelled = progress.cancelled,
            "Sync query finished"
        );

        progress
    }

    async fn update_credential_health(
        &self,
        site_category: &str,
        health_status: &str,
        last_error: Option<&str>,
    ) {
        if let Err(e) = self
            .db
            .upsert_credential_health(site_category, health_status, last_error)
            .await
        {
            warn!(
                site = %site_category,
                status = %health_status,
                error = %e,
                "Failed to persist credential health"
            );
        }
    }

    async fn materialize_collection_groups(
        &mut self,
        subscription_id: i64,
        subscription_id_str: &str,
        cancel: &CancellationToken,
        progress: &mut SyncProgress,
        groups: HashMap<String, CollectionGroup>,
    ) {
        let mut group_values: Vec<CollectionGroup> = groups
            .into_values()
            .filter_map(|mut group| {
                let mut seen = HashSet::new();
                group.hashes.retain(|hash| seen.insert(hash.clone()));
                if group.hashes.len() < 2 {
                    None
                } else {
                    Some(group)
                }
            })
            .collect();
        if group_values.is_empty() {
            return;
        }
        group_values.sort_by(|a, b| a.category.cmp(&b.category).then(a.post_id.cmp(&b.post_id)));

        let total_groups = group_values.len();
        let mut changed_collection_ids = Vec::new();
        for (idx, group) in group_values.into_iter().enumerate() {
            if cancel.is_cancelled() {
                progress.cancelled = true;
                break;
            }

            self.emit_progress(
                subscription_id_str,
                progress,
                &format!("Organizing collections ({}/{})", idx + 1, total_groups),
            );

            let mapped_collection_id = match self
                .db
                .get_subscription_post_collection(subscription_id, &group.category, &group.post_id)
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    progress.errors.push(format!(
                        "Collection map lookup failed for {}:{}: {e}",
                        group.category, group.post_id
                    ));
                    None
                }
            };
            let existing_collection_id = if mapped_collection_id.is_some() {
                mapped_collection_id
            } else {
                match self.find_collection_for_hashes(&group.hashes).await {
                    Ok(id) => id,
                    Err(e) => {
                        progress.errors.push(format!(
                            "Collection lookup failed for {}:{}: {e}",
                            group.category, group.post_id
                        ));
                        continue;
                    }
                }
            };
            let collection_id = match existing_collection_id {
                Some(id) => id,
                None => match self
                    .db
                    .create_collection(&group.preferred_name, None, &[])
                    .await
                {
                    Ok(id) => id,
                    Err(e) => {
                        progress.errors.push(format!(
                            "Collection create failed for {}:{}: {e}",
                            group.category, group.post_id
                        ));
                        continue;
                    }
                },
            };
            let add_result = self
                .db
                .add_collection_members_by_hashes(collection_id, &group.hashes)
                .await;
            if let Err(e) = self
                .db
                .upsert_subscription_post_collection(
                    subscription_id,
                    &group.category,
                    &group.post_id,
                    collection_id,
                )
                .await
            {
                progress.errors.push(format!(
                    "Collection map update failed for {}:{}: {e}",
                    group.category, group.post_id
                ));
            }
            match add_result {
                Ok(added) => {
                    if added > 0 {
                        changed_collection_ids.push(collection_id);
                    }
                }
                Err(e) => {
                    progress.errors.push(format!(
                        "Collection member update failed for {}:{}: {e}",
                        group.category, group.post_id
                    ));
                }
            }
        }

        if changed_collection_ids.is_empty() {
            return;
        }
        changed_collection_ids.sort_unstable();
        changed_collection_ids.dedup();

        self.db.scope_cache_invalidate_all();
        let mut scopes: Vec<String> = vec!["system:all".to_string()];
        scopes.extend(
            changed_collection_ids
                .iter()
                .map(|id| format!("collection:{id}")),
        );
        crate::events::emit_mutation(
            "subscription_import_collections",
            crate::events::MutationImpact::new()
                .folder_membership_changed(changed_collection_ids)
                .extra_grid_scopes(scopes),
        );
    }

    async fn find_collection_for_hashes(&self, hashes: &[String]) -> Result<Option<i64>, String> {
        let probe = hashes.to_vec();
        self.db
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT me.parent_collection_id
                     FROM file f
                     JOIN entity_file ef ON ef.file_id = f.file_id
                     JOIN media_entity me ON me.entity_id = ef.entity_id
                     WHERE f.hash = ?1
                       AND me.kind = 'single'
                       AND me.parent_collection_id IS NOT NULL
                     LIMIT 1",
                )?;
                for hash in &probe {
                    let mut rows = stmt.query([hash])?;
                    if let Some(row) = rows.next()? {
                        return Ok(Some(row.get::<_, i64>(0)?));
                    }
                }
                Ok(None)
            })
            .await
    }

    /// Import a single downloaded file and return the resolved hash plus whether
    /// this run imported new content for it.
    async fn import_item(
        &self,
        file_path: &std::path::Path,
        metadata: &ParsedMetadata,
        subscription_id: i64,
        gallery_url: &str,
    ) -> Result<ImportOutcome, String> {
        // Quick SHA256 — skip pipeline for known files
        let file_data = tokio::fs::read(file_path)
            .await
            .map_err(|e| format!("Read error: {e}"))?;
        let hex_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&file_data);
            hex::encode(hasher.finalize())
        };

        // Check if already imported
        if let Ok(Some(existing)) = self.db.get_file_by_hash(&hex_hash).await {
            self.merge_existing_metadata(&hex_hash, &existing, metadata, gallery_url, subscription_id)
                .await?;
            return Ok(ImportOutcome {
                hex_hash,
                imported_new: false,
            });
        }

        // Build import options from gallery-dl metadata
        let mut options = ImportOptions::default();
        options.tags = metadata.tags.clone();

        // Source URLs
        if let Some(ref source) = metadata.source_url {
            options.source_urls.push(source.clone());
        }
        if !gallery_url.is_empty() {
            options.source_urls.push(gallery_url.to_string());
        }

        options.name = preferred_import_name(metadata);

        // Description as note
        if let Some(ref description) = metadata.description {
            let mut notes = HashMap::new();
            notes.insert("description".to_string(), description.clone());
            options.notes = Some(notes);
        }

        info!(
            post_id = metadata.post_id.as_deref().unwrap_or("?"),
            tags = metadata.tags.len(),
            "Importing file"
        );

        // Import via pipeline
        let pipeline = ImportPipeline::new(self.db, self.blob_store);
        match pipeline.import_file(file_path, &options).await {
            Ok(imported) => {
                info!(hash = %imported.hex_hash, tags = options.tags.len(), "Import success");

                let mut surviving_hash = imported.hex_hash.clone();
                // Auto-merge duplicate detection
                if self.auto_merge_enabled && imported.mime.starts_with("image/") {
                    match crate::duplicates::controller::DuplicateController::check_and_auto_merge(
                        self.db,
                        &imported.hex_hash,
                        self.auto_merge_distance,
                    )
                    .await
                    {
                        Ok(Some(merge_result)) => {
                            surviving_hash = merge_result.winner_hash.clone();
                            info!(
                                winner = %merge_result.winner_hash,
                                loser = %merge_result.loser_hash,
                                tags_merged = merge_result.tags_merged,
                                "Auto-merged duplicate during subscription import"
                            );
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!(hash = %imported.hex_hash, error = %e, "Auto-merge failed (non-fatal)");
                        }
                    }
                }

                if let Err(e) = self
                    .db
                    .add_subscription_entity(subscription_id, &surviving_hash)
                    .await
                {
                    warn!(error = %e, "Failed to record subscription-file mapping");
                }

                // Only emit a live insert when the imported file survived as a distinct item.
                if surviving_hash == imported.hex_hash {
                    if let Ok(Some(record)) = self.db.get_file_by_hash(&surviving_hash).await {
                        let slim = crate::types::FileInfoSlim::from(record);
                        crate::events::emit(crate::events::event_names::FILE_IMPORTED, &slim);
                    }
                }

                // Emit state-changed for sidebar counts
                self.db.scope_cache_invalidate_all();
                crate::events::emit_mutation(
                    "subscription_import",
                    crate::events::MutationImpact::file_lifecycle(self.db),
                );

                Ok(ImportOutcome {
                    hex_hash: surviving_hash,
                    imported_new: true,
                })
            }
            Err(crate::import::pipeline::ImportError::AlreadyImported(hash)) => {
                info!(hash = %hash, "Already imported (skipped)");
                if let Ok(Some(existing)) = self.db.get_file_by_hash(&hash).await {
                    self.merge_existing_metadata(&hash, &existing, metadata, gallery_url, subscription_id)
                        .await?;
                }
                Ok(ImportOutcome {
                    hex_hash: hash,
                    imported_new: false,
                })
            }
            Err(e) => {
                warn!(
                    path = %file_path.display(),
                    error = %e,
                    "Import pipeline failed"
                );
                Err(format!("{e}"))
            }
        }
    }

    /// Merge metadata from gallery-dl into an already-existing file record.
    async fn merge_existing_metadata(
        &self,
        hex_hash: &str,
        existing: &crate::sqlite::files::FileRecord,
        metadata: &ParsedMetadata,
        gallery_url: &str,
        subscription_id: i64,
    ) -> Result<(), String> {
        let existing_name = existing.name.as_deref().unwrap_or("").trim();
        let desired_name = if let Some(title) = normalized_title(metadata) {
            if should_replace_existing_name(existing_name, metadata) {
                Some(title)
            } else {
                None
            }
        } else if existing_name.is_empty() {
            generated_subscription_name(metadata)
        } else {
            None
        };

        let mut note_entries = HashMap::new();
        if let Some(ref description) = metadata.description {
            note_entries.insert("description".to_string(), description.clone());
        }

        let mut source_urls = Vec::new();
        if let Some(ref source) = metadata.source_url {
            source_urls.push(source.clone());
        }
        if !gallery_url.is_empty() {
            source_urls.push(gallery_url.to_string());
        }

        merge_existing_import_target(
            self.db,
            hex_hash,
            ExistingImportMergeRequest {
                restore_status: Some(1),
                tag_strings: metadata
                    .tags
                    .iter()
                    .map(|(ns, st)| normalize::combine_tag(ns, st))
                    .collect(),
                source_urls,
                name: desired_name,
                note_entries,
                subscription_id: Some(subscription_id),
                mutation_name: "subscription_import",
            },
        )
        .await
        .map(|_| ())
    }

    fn emit_progress(&mut self, subscription_id: &str, progress: &SyncProgress, status_text: &str) {
        self.emit_progress_inner(subscription_id, progress, status_text, false);
    }

    fn emit_progress_force(
        &mut self,
        subscription_id: &str,
        progress: &SyncProgress,
        status_text: &str,
    ) {
        self.emit_progress_inner(subscription_id, progress, status_text, true);
    }

    fn emit_progress_inner(
        &mut self,
        subscription_id: &str,
        progress: &SyncProgress,
        status_text: &str,
        force: bool,
    ) {
        let now = std::time::Instant::now();
        if !force && now.duration_since(self.last_progress_emit).as_millis() < 300 {
            return;
        }
        self.last_progress_emit = now;
        publish_running_progress(
            subscription_id,
            &self.subscription_name,
            "subscription",
            self.current_query_id.map(|id| id.to_string()),
            self.current_query_name.clone(),
            progress,
            status_text,
        );
    }
}

fn should_continue_initial_pagination(
    completed_initial_run: bool,
    completed_cleanly: bool,
    file_limit: Option<u32>,
    fetched_items: usize,
    next_resume_cursor: Option<&str>,
) -> bool {
    if completed_initial_run || !completed_cleanly {
        return false;
    }
    let Some(limit) = file_limit else {
        return false;
    };
    if limit == 0 || fetched_items < limit as usize {
        return false;
    }
    next_resume_cursor.is_some_and(|cursor| !cursor.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

}
