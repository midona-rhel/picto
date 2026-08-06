//! Subscription sync engine — downloads files via gallery-dl subprocess.
//!
//! Steps per query:
//! 1. Build URL from subscription's gallery-dl URL template + query text
//! 2. Load credentials from OS keychain (if configured for that site)
//! 3. Spawn gallery-dl subprocess with appropriate flags
//! 4. Queue each committed download for background ingest
//! 5. Merge metadata for already-imported files (tags, URLs, notes, name)
//! 6. Track `completed_initial_run` for smart-stop behavior

mod credentials;
mod helpers;
mod persistence;
mod progress;
mod query;
mod retry;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::AppSettings;
use crate::subscriptions::gallery_dl_runner::{FailureKind, GalleryDlRunner};
use crate::subscriptions::import_policy::preferred_import_name;
use crate::subscriptions::source_adapter::ParsedMetadata;

#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    pub files_downloaded: usize,
    pub files_skipped: usize,
    pub queued_for_ingest: usize,
    pub pages_fetched: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub failure_kind: Option<String>,
    pub metadata_validated: usize,
    pub metadata_invalid: usize,
    pub last_metadata_error: Option<String>,
    pub current_post_id: Option<String>,
    pub current_post_items: usize,
    pub posts_processed: usize,
    pub resume_cursor: Option<String>,
}

fn query_run_completion(
    status: &str,
    progress: &SyncProgress,
) -> crate::subscriptions::types::SubscriptionQueryRunCompletion {
    crate::subscriptions::types::SubscriptionQueryRunCompletion {
        status: status.to_string(),
        failure_kind: progress.failure_kind.clone(),
        error_message: progress.errors.last().cloned(),
        posts_processed: progress.posts_processed as i64,
        files_downloaded: progress.files_downloaded as i64,
        files_skipped: progress.files_skipped as i64,
        metadata_validated: progress.metadata_validated as i64,
        metadata_invalid: progress.metadata_invalid as i64,
    }
}

pub struct SubscriptionSyncEngine<'a> {
    db: &'a LibraryDatabase,
    library_root: PathBuf,
    rate_limiter: Option<RateLimiter>,
    runner: GalleryDlRunner,
    settings: AppSettings,
    subscription_name: String,
    progress_mode: String,
    group_name: Option<String>,
    current_query_id: Option<i64>,
    current_query_name: Option<String>,
    current_phase: String,
    last_progress_emit: std::time::Instant,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
    auto_merge_require_matching_dimensions: bool,
    auto_collections: bool,
}

pub(super) struct PendingMember {
    pub file_path: PathBuf,
    pub metadata: ParsedMetadata,
    pub page_num: u32,
}

pub(super) struct PendingCollection {
    pub category: String,
    pub post_id: String,
    pub preferred_name: String,
    pub expected_count: Option<u32>,
    pub members: Vec<PendingMember>,
}

impl PendingCollection {
    fn push_member(&mut self, member: PendingMember) -> (usize, bool) {
        let advertised_count = member.metadata.page_count.unwrap_or(0);
        self.expected_count =
            Some(self.expected_count.unwrap_or(0).max(advertised_count)).filter(|count| *count > 0);
        self.members.push(member);
        (self.members.len(), self.is_complete(false))
    }

    fn is_complete(&self, source_finished: bool) -> bool {
        match self.expected_count {
            Some(expected) => self.members.len() >= expected as usize,
            None => source_finished && !self.members.is_empty(),
        }
    }
}

fn incomplete_post_detail(post: &PendingCollection) -> String {
    match post.expected_count {
        Some(expected) => format!(
            "Post {} downloaded {} of {expected} expected files",
            post.post_id,
            post.members.len(),
        ),
        None => format!(
            "Post {} ended before its file count was known; downloaded {} files",
            post.post_id,
            post.members.len(),
        ),
    }
}

impl<'a> SubscriptionSyncEngine<'a> {
    pub fn new(
        db: &'a LibraryDatabase,
        settings: &AppSettings,
        library_root: &Path,
    ) -> Result<Self, String> {
        let binary_path = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
        let runner = GalleryDlRunner::new(binary_path);

        Ok(Self {
            db,
            library_root: library_root.to_path_buf(),
            rate_limiter: None,
            runner,
            settings: settings.clone(),
            subscription_name: String::new(),
            progress_mode: "subscription".to_string(),
            group_name: None,
            current_query_id: None,
            current_query_name: None,
            current_phase: "starting".to_string(),
            last_progress_emit: std::time::Instant::now(),
            auto_merge_enabled: false,
            auto_merge_distance: crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD,
            auto_merge_require_matching_dimensions: false,
            auto_collections: true,
        })
    }

    pub(super) fn runtime_service(
        &self,
    ) -> crate::subscriptions::runtime_service::SubscriptionRuntimeService<'_> {
        crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            self.db,
            self.library_root.as_path(),
        )
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.subscription_name = name;
        self
    }

    pub fn with_progress_mode(mut self, mode: &str) -> Self {
        self.progress_mode = mode.to_string();
        self
    }

    pub fn with_group_name(mut self, group_name: Option<String>) -> Self {
        self.group_name = group_name;
        self
    }

    pub fn with_auto_collections(mut self, auto_collections: bool) -> Self {
        self.auto_collections = auto_collections;
        self
    }

    pub fn with_rate_limiter(mut self, rate_limiter: RateLimiter) -> Self {
        self.rate_limiter = Some(rate_limiter);
        self
    }

    pub fn with_auto_merge(
        mut self,
        enabled: bool,
        distance: u32,
        require_matching_dimensions: bool,
    ) -> Self {
        self.auto_merge_enabled = enabled;
        self.auto_merge_distance = distance;
        self.auto_merge_require_matching_dimensions = require_matching_dimensions;
        self
    }

    fn finalize_current_post_progress(
        &self,
        progress: &mut SyncProgress,
        posts_processed_this_run: &mut usize,
    ) {
        progress.posts_processed += 1;
        *posts_processed_this_run += 1;

        progress.current_post_id = None;
        progress.current_post_items = 0;
    }

    async fn flush_pending_collection(
        &mut self,
        pending_key: &str,
        pending_collections: &mut HashMap<String, PendingCollection>,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        sub_id_str: &str,
        progress: &mut SyncProgress,
    ) -> Result<Option<(String, usize)>, String> {
        let Some(pc) = pending_collections.remove(pending_key) else {
            return Ok(None);
        };
        let post_id = pc.post_id.clone();
        let file_count = pc.members.len();
        self.enqueue_pending_collection(pc, subscription_id, query_id, query_run_id)
            .await?;
        progress.queued_for_ingest += 1;
        self.set_phase("queueing");
        self.emit_progress_force(sub_id_str, progress, "Queued post for ingest");
        Ok(Some((post_id, file_count)))
    }

    async fn enqueue_pending_collection(
        &self,
        pc: PendingCollection,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
    ) -> Result<i64, String> {
        let source_paths = pc
            .members
            .iter()
            .map(|member| member.file_path.clone())
            .collect::<Vec<_>>();
        let staged =
            crate::ingest_queue::stage_ingest_sources(&self.library_root, &source_paths).await?;
        let items: Vec<(
            PathBuf,
            Option<i64>,
            crate::ingest_queue::IngestQueueItemPayload,
            bool,
        )> = pc
            .members
            .into_iter()
            .zip(staged.paths.iter().cloned())
            .map(|(member, staged_path)| {
                let request = helpers::build_subscription_ingest_request(
                    subscription_id,
                    &staged_path,
                    &member.metadata,
                    false,
                    0,
                );
                helpers::log_subscription_ingest_request_shape(
                    query_id,
                    subscription_id,
                    &member.metadata,
                    &request.tag_strings,
                );
                (
                    staged_path,
                    Some(member.page_num as i64),
                    crate::ingest_queue::IngestQueueItemPayload {
                        request,
                        subscription_metadata: Some(member.metadata),
                        target_folder_id: None,
                    },
                    true,
                )
            })
            .collect();
        let result = self
            .db
            .enqueue_ingest_queue(
                crate::ingest_queue::IngestQueueKind::Collection,
                "subscription",
                Some(subscription_id),
                Some(query_id),
                query_run_id,
                Some(&staged.root),
                Some(&pc.post_id),
                Some(&pc.category),
                Some(&pc.preferred_name),
                pc.expected_count.map(i64::from),
                items,
            )
            .await;
        if result.is_ok() {
            helpers::release_producer_sources(&source_paths).await;
        } else {
            let _ = tokio::fs::remove_dir_all(&staged.root).await;
        }
        result
    }

    async fn enqueue_single_subscription_item(
        &self,
        subscription_id: i64,
        query_id: i64,
        query_run_id: Option<i64>,
        file_path: &std::path::Path,
        metadata: &ParsedMetadata,
    ) -> Result<i64, String> {
        let staged = crate::ingest_queue::stage_ingest_sources(
            &self.library_root,
            &[file_path.to_path_buf()],
        )
        .await?;
        let staged_path = staged.paths[0].clone();
        let request = helpers::build_subscription_ingest_request(
            subscription_id,
            &staged_path,
            metadata,
            false,
            0,
        );
        helpers::log_subscription_ingest_request_shape(
            query_id,
            subscription_id,
            metadata,
            &request.tag_strings,
        );
        let result = self
            .db
            .enqueue_ingest_queue(
                crate::ingest_queue::IngestQueueKind::Single,
                "subscription",
                Some(subscription_id),
                Some(query_id),
                query_run_id,
                Some(&staged.root),
                metadata.post_id.as_deref(),
                metadata.category.as_deref(),
                preferred_import_name(metadata).as_deref(),
                metadata.page_count.map(i64::from),
                vec![(
                    staged_path,
                    metadata.page_num.map(i64::from),
                    crate::ingest_queue::IngestQueueItemPayload {
                        request,
                        subscription_metadata: Some(metadata.clone()),
                        target_folder_id: None,
                    },
                    true,
                )],
            )
            .await;
        if result.is_ok() {
            helpers::release_producer_sources(&[file_path.to_path_buf()]).await;
        } else {
            let _ = tokio::fs::remove_dir_all(&staged.root).await;
        }
        result
    }

    async fn record_issue(
        &self,
        subscription_id: i64,
        query_id: Option<i64>,
        failure_kind: FailureKind,
        message: &str,
        detail: Option<&str>,
    ) {
        let _ = self
            .runtime_service()
            .upsert_subscription_issue(subscription_id, query_id, failure_kind, message, detail)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingCollection, PendingMember};
    use crate::subscriptions::source_adapter::ParsedMetadata;
    use std::path::PathBuf;

    fn member(page_num: u32, page_count: Option<u32>) -> PendingMember {
        PendingMember {
            file_path: PathBuf::from(format!("{page_num}.jpg")),
            metadata: ParsedMetadata {
                page_num: Some(page_num),
                page_count,
                ..Default::default()
            },
            page_num,
        }
    }

    fn pending(expected_count: Option<u32>, member_count: usize) -> PendingCollection {
        PendingCollection {
            category: "pixiv".to_string(),
            post_id: "42".to_string(),
            preferred_name: "post".to_string(),
            expected_count,
            members: (0..member_count)
                .map(|page_num| member(page_num as u32, expected_count))
                .collect(),
        }
    }

    #[test]
    fn pending_collection_requires_every_advertised_member() {
        assert!(!pending(Some(3), 2).is_complete(false));
        assert!(pending(Some(3), 3).is_complete(false));
    }

    #[test]
    fn unknown_member_count_waits_for_source_completion() {
        assert!(!pending(None, 2).is_complete(false));
        assert!(pending(None, 2).is_complete(true));
    }

    #[test]
    fn interleaved_posts_complete_independently() {
        let mut first = pending(None, 0);
        let mut second = pending(None, 0);

        assert!(!first.push_member(member(0, Some(2))).1);
        assert!(!second.push_member(member(0, Some(2))).1);
        assert!(first.push_member(member(1, Some(2))).1);
        assert!(second.push_member(member(1, Some(2))).1);
    }
}
