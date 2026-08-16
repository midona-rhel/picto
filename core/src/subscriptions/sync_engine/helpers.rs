use std::collections::HashSet;
use std::path::Path;

use tracing::{info, warn};

use crate::subscriptions::import_policy::preferred_import_name;
use crate::subscriptions::source_adapter::ParsedMetadata;
use crate::tags::logging::{preview_tag_strings, summarize_tag_strings};

pub(super) fn build_subscription_ingest_request(
    subscription_id: i64,
    file_path: &Path,
    metadata: &ParsedMetadata,
    skip_thumbnail: bool,
    initial_status: i64,
) -> crate::ingest::SingleIngestRequest {
    crate::ingest::SingleIngestRequest {
        source_kind: crate::ingest::IngestSourceKind::Subscription,
        path: file_path.to_path_buf(),
        tag_strings: crate::ingest::normalize_subscription_tags(metadata),
        source_urls: crate::ingest::dedupe_urls(metadata.source_urls.clone()),
        name: preferred_import_name(metadata),
        notes: crate::ingest::metadata_notes_text(metadata),
        created_at: metadata.created_at.clone(),
        initial_status,
        skip_thumbnail,
        tag_provenance_mask: crate::db::types::TAG_PROVENANCE_UNKNOWN,
        subscription_id: Some(subscription_id),
    }
}

pub(super) fn log_subscription_ingest_request_shape(
    query_id: i64,
    subscription_id: i64,
    metadata: &ParsedMetadata,
    tag_strings: &[String],
) {
    let summary = summarize_tag_strings(tag_strings);
    info!(
        query_id,
        subscription_id,
        post_id = metadata.post_id.as_deref().unwrap_or("?"),
        category = metadata.category.as_deref().unwrap_or("?"),
        item_key = metadata.item_key.as_deref().unwrap_or("?"),
        request_tag_count = summary.total,
        request_creator_tag_count = summary.creator,
        request_character_tag_count = summary.character,
        request_series_tag_count = summary.series,
        request_general_tag_count = summary.general,
        request_meta_tag_count = summary.meta,
        request_other_namespaced_tag_count = summary.other_namespaced,
        request_tag_preview = ?preview_tag_strings(tag_strings, 5),
        "subscription ingest request built"
    );
}

pub(super) async fn cleanup_subscription_temp_root(temp_root: &Path) {
    crate::subscriptions::gallery_dl_runner::cleanup_temp_dir(temp_root).await;
}

pub(super) async fn release_producer_sources(source_paths: &[std::path::PathBuf]) {
    for source_path in source_paths {
        if let Err(error) = tokio::fs::remove_file(source_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %source_path.display(), error = %error, "Failed to release staged subscription source");
            }
        }
    }
}

pub(super) fn compute_committed_cursor(
    completed_cleanly: bool,
    resume_strategy: Option<&str>,
    range_start: u32,
    posts_this_run: usize,
    all_post_ids: &HashSet<String>,
    source_cursor: Option<&str>,
) -> Option<String> {
    if !completed_cleanly {
        return None;
    }
    if resume_strategy == Some("source_cursor") {
        return source_cursor
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string);
    }
    if resume_strategy == Some("range_offset") {
        if let Some(cursor) = source_cursor
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
        {
            return Some(cursor.to_string());
        }
    }
    if posts_this_run == 0 {
        return None;
    }
    match resume_strategy {
        Some("range_offset") => Some((range_start as usize + posts_this_run - 1).to_string()),
        Some("tag_id_lt") => {
            let mut min_id: Option<u64> = None;
            for pid in all_post_ids {
                if let Ok(n) = pid.parse::<u64>() {
                    min_id = Some(min_id.map_or(n, |cur| cur.min(n)));
                }
            }
            min_id.map(|id| id.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_committed_cursor, initial_history_has_more};
    use std::collections::HashSet;

    #[test]
    fn cursor_advances_only_after_the_whole_batch_commits() {
        let post_ids = HashSet::from(["105".to_string(), "104".to_string()]);

        assert_eq!(
            compute_committed_cursor(false, Some("range_offset"), 1, 1, &post_ids, None),
            None
        );
        assert_eq!(
            compute_committed_cursor(false, Some("tag_id_lt"), 1, 1, &post_ids, None),
            None
        );
        assert_eq!(
            compute_committed_cursor(true, Some("range_offset"), 1, 2, &post_ids, None),
            Some("2".to_string())
        );
        assert_eq!(
            compute_committed_cursor(true, Some("range_offset"), 1, 92, &post_ids, Some("100"),),
            Some("100".to_string())
        );
        assert_eq!(
            compute_committed_cursor(true, Some("tag_id_lt"), 1, 2, &post_ids, None),
            Some("104".to_string())
        );
        assert_eq!(
            compute_committed_cursor(
                true,
                Some("source_cursor"),
                1,
                0,
                &HashSet::new(),
                Some("opaque-next"),
            ),
            Some("opaque-next".to_string())
        );
    }

    #[test]
    fn full_initial_batch_keeps_history_cursor_for_the_next_run() {
        assert!(initial_history_has_more(
            false,
            true,
            Some(100),
            100,
            Some("12345"),
        ));
        assert!(!initial_history_has_more(
            false,
            true,
            Some(100),
            99,
            Some("12345"),
        ));
    }
}

pub(super) fn initial_history_has_more(
    completed_initial_run: bool,
    completed_cleanly: bool,
    post_limit: Option<u32>,
    fetched_items: usize,
    next_resume_cursor: Option<&str>,
) -> bool {
    if completed_initial_run || !completed_cleanly {
        return false;
    }
    let Some(limit) = post_limit else {
        return false;
    };
    if limit == 0 || fetched_items < limit as usize {
        return false;
    }
    next_resume_cursor.is_some_and(|cursor| !cursor.trim().is_empty())
}
