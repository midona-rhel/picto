//! Subscription policy helpers shared across orchestration and execution.
//!
//! Keeps naming, batch-limit, and terminal-status behavior in one place so
//! controllers and the sync engine stop re-implementing those rules.

use crate::subscriptions::gallery_dl_runner::FailureKind;

pub fn effective_inbox_limit(_configured: u32) -> u32 {
    // Product decision: inbox ingest is always hard-capped to 1000 items.
    1000
}

pub fn resolve_query_name(query_id: i64, query_text: &str, display_name: Option<&str>) -> String {
    if let Some(name) = display_name.map(str::trim).filter(|name| !name.is_empty()) {
        let is_numeric_placeholder = name.chars().all(|c| c.is_ascii_digit());
        if !is_numeric_placeholder || query_text.trim().is_empty() {
            return name.to_string();
        }
    }
    let trimmed = query_text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    format!("Query {query_id}")
}

pub fn effective_query_post_limit(global_batch_size: u32, subscription_limit: u32) -> Option<u32> {
    match (global_batch_size, subscription_limit) {
        (0, 0) => None, // no limits → gallery-dl runs until source is exhausted
        (0, local) => Some(local),
        (global, 0) => Some(global),
        (global, local) => Some(local.min(global)),
    }
}

pub fn resolve_finished_status_text(status: &str, failure_kind: Option<&str>) -> &'static str {
    if status == "cancelled" && failure_kind == Some(FailureKind::InboxFull.as_str()) {
        return "Paused (Inbox full)";
    }
    if status == "cancelled" && failure_kind == Some(FailureKind::DownloadFailure.as_str()) {
        return "Stopped (Download failed)";
    }
    match (status, failure_kind) {
        ("succeeded", _) => "Completed",
        ("cancelled", _) => "Cancelled",
        (_, Some("unauthorized")) => "Failed (Unauthorized)",
        (_, Some("expired")) => "Failed (Credentials expired)",
        (_, Some("rate_limited")) => "Failed (Rate limited)",
        (_, Some("network")) => "Failed (Network error)",
        _ => "Failed",
    }
}

pub fn default_resume_strategy_for_site(site_id: &str) -> Option<&'static str> {
    match site_id {
        "danbooru" | "gelbooru" | "rule34" | "e621" | "yandere" | "konachan" | "safebooru" => {
            Some("tag_id_lt")
        }
        "pixiv" | "pixivuser" | "webtoons" => Some("range_offset"),
        // The runner converts this depth into ArtStation's cumulative
        // `max-posts`; it does not apply gallery-dl's asset-level post range.
        "artstation" | "hentaifoundry" | "baraag" | "deviantart" | "tumblr" | "furaffinity" => {
            Some("range_offset")
        }
        "idolcomplex" | "sankaku" => Some("source_cursor"),
        _ => None,
    }
}

pub fn apply_resume_to_query(
    query_text: &str,
    resume_cursor: &str,
    resume_strategy: &str,
) -> String {
    match resume_strategy {
        "tag_id_lt" => {
            if query_text
                .split_whitespace()
                .any(|token| token.starts_with("id:<"))
            {
                return query_text.to_string();
            }
            let suffix = format!("id:<{resume_cursor}");
            if query_text.trim().is_empty() {
                suffix
            } else {
                format!("{} {}", query_text.trim(), suffix)
            }
        }
        _ => query_text.to_string(),
    }
}

pub fn derive_resume_cursor(
    items: &[crate::subscriptions::source_adapter::DownloadedItem],
    strategy: &str,
    range_end: Option<u32>,
) -> Option<String> {
    match strategy {
        "tag_id_lt" => {
            let mut min_id: Option<u64> = None;
            for item in items {
                if let Some(pid) = item
                    .metadata
                    .post_id
                    .as_deref()
                    .and_then(|raw| raw.parse::<u64>().ok())
                {
                    min_id = Some(min_id.map_or(pid, |cur| cur.min(pid)));
                }
            }
            min_id.map(|id| id.to_string())
        }
        "range_offset" => range_end.map(|end| end.to_string()),
        _ => None,
    }
}

/// Parse range_offset cursor into the starting post-range index.
pub fn range_start_from_cursor(resume_cursor: Option<&str>, strategy: Option<&str>) -> u32 {
    match strategy {
        Some("range_offset") => resume_cursor
            .and_then(|c| c.trim().parse::<u32>().ok())
            .map(|end| end + 1)
            .unwrap_or(1),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_resume_to_query, default_resume_strategy_for_site, derive_resume_cursor,
        effective_inbox_limit, effective_query_post_limit, range_start_from_cursor,
        resolve_finished_status_text,
    };

    #[test]
    fn public_boorus_resume_from_the_oldest_committed_post_id() {
        for site_id in [
            "danbooru",
            "gelbooru",
            "rule34",
            "e621",
            "yandere",
            "konachan",
            "safebooru",
        ] {
            assert_eq!(
                default_resume_strategy_for_site(site_id),
                Some("tag_id_lt"),
                "{site_id}"
            );
        }
    }

    #[test]
    fn artstation_grows_a_cumulative_project_scan_depth() {
        assert_eq!(
            default_resume_strategy_for_site("artstation"),
            Some("range_offset")
        );
        assert_eq!(
            range_start_from_cursor(Some("25"), Some("range_offset")),
            26
        );
    }

    #[test]
    fn webtoons_resumes_at_the_next_child_episode() {
        assert_eq!(
            default_resume_strategy_for_site("webtoons"),
            Some("range_offset")
        );
        assert_eq!(range_start_from_cursor(Some("1"), Some("range_offset")), 2);
    }

    #[test]
    fn hentaifoundry_resumes_with_a_post_safe_range_offset() {
        assert_eq!(
            default_resume_strategy_for_site("hentaifoundry"),
            Some("range_offset")
        );
        assert_eq!(
            range_start_from_cursor(Some("24"), Some("range_offset")),
            25
        );
    }

    #[test]
    fn baraag_resumes_by_status_without_splitting_attachments() {
        assert_eq!(
            default_resume_strategy_for_site("baraag"),
            Some("range_offset")
        );
        assert_eq!(range_start_from_cursor(Some("2"), Some("range_offset")), 3);
    }

    #[test]
    fn deviantart_resumes_with_a_gallery_range_offset() {
        assert_eq!(
            default_resume_strategy_for_site("deviantart"),
            Some("range_offset")
        );
        assert_eq!(
            range_start_from_cursor(Some("24"), Some("range_offset")),
            25
        );
    }

    #[test]
    fn tumblr_resumes_at_the_next_whole_post() {
        assert_eq!(
            default_resume_strategy_for_site("tumblr"),
            Some("range_offset")
        );
        assert_eq!(
            range_start_from_cursor(Some("24"), Some("range_offset")),
            25
        );
    }

    #[test]
    fn idolcomplex_resumes_by_source_cursor() {
        assert_eq!(
            default_resume_strategy_for_site("idolcomplex"),
            Some("source_cursor")
        );
    }

    #[test]
    fn sankaku_resumes_by_source_cursor() {
        assert_eq!(
            default_resume_strategy_for_site("sankaku"),
            Some("source_cursor")
        );
    }

    #[test]
    fn effective_query_post_limit_clamps_to_global_cap_when_enabled() {
        assert_eq!(effective_query_post_limit(100, 0), Some(100));
        assert_eq!(effective_query_post_limit(100, 50), Some(50));
        assert_eq!(effective_query_post_limit(100, 500), Some(100));
    }

    #[test]
    fn effective_query_post_limit_none_when_both_zero() {
        // Both 0 = unlimited: gallery-dl runs until source exhaustion / inbox cap
        assert_eq!(effective_query_post_limit(0, 0), None);
    }

    #[test]
    fn effective_query_post_limit_uses_subscription_limit_without_global_cap() {
        assert_eq!(effective_query_post_limit(0, 50), Some(50));
        assert_eq!(effective_query_post_limit(0, 500), Some(500));
    }

    #[test]
    fn resolve_finished_status_text_marks_inbox_full_as_paused() {
        assert_eq!(
            resolve_finished_status_text("cancelled", Some("inbox_full")),
            "Paused (Inbox full)"
        );
        assert_eq!(
            resolve_finished_status_text("cancelled", Some("unknown")),
            "Cancelled"
        );
        assert_eq!(resolve_finished_status_text("succeeded", None), "Completed");
    }

    #[test]
    fn effective_inbox_limit_is_hard_capped() {
        assert_eq!(effective_inbox_limit(0), 1000);
        assert_eq!(effective_inbox_limit(10_000), 1000);
    }

    #[test]
    fn apply_resume_to_query_adds_id_lt_clause_once() {
        let q = apply_resume_to_query("1girl solo", "12345", "tag_id_lt");
        assert_eq!(q, "1girl solo id:<12345");
        let q2 = apply_resume_to_query(&q, "99999", "tag_id_lt");
        assert_eq!(q2, q);
    }

    #[test]
    fn derive_resume_cursor_uses_min_numeric_post_id() {
        let items = vec![
            crate::subscriptions::source_adapter::DownloadedItem {
                file_path: std::path::PathBuf::from("/tmp/a"),
                metadata: crate::subscriptions::source_adapter::ParsedMetadata {
                    post_id: Some("100".to_string()),
                    ..Default::default()
                },
            },
            crate::subscriptions::source_adapter::DownloadedItem {
                file_path: std::path::PathBuf::from("/tmp/b"),
                metadata: crate::subscriptions::source_adapter::ParsedMetadata {
                    post_id: Some("93".to_string()),
                    ..Default::default()
                },
            },
        ];
        assert_eq!(
            derive_resume_cursor(&items, "tag_id_lt", None),
            Some("93".to_string())
        );
    }

    #[test]
    fn derive_resume_cursor_range_offset_returns_range_end() {
        let items = vec![];
        assert_eq!(
            derive_resume_cursor(&items, "range_offset", Some(100)),
            Some("100".to_string())
        );
        assert_eq!(derive_resume_cursor(&items, "range_offset", None), None);
    }

    #[test]
    fn range_start_from_cursor_computes_next_offset() {
        assert_eq!(range_start_from_cursor(None, Some("range_offset")), 1);
        assert_eq!(
            range_start_from_cursor(Some("100"), Some("range_offset")),
            101
        );
        assert_eq!(
            range_start_from_cursor(Some("200"), Some("range_offset")),
            201
        );
        // Non range_offset strategies always start at 1
        assert_eq!(range_start_from_cursor(Some("100"), Some("tag_id_lt")), 1);
        assert_eq!(range_start_from_cursor(Some("100"), None), 1);
    }
}
