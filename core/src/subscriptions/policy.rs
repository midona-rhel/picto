//! Subscription policy helpers shared across orchestration and execution.
//!
//! Keeps naming, batch-limit, and terminal-status behavior in one place so
//! controllers and the sync engine stop re-implementing those rules.

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

pub fn effective_query_file_limit(global_batch_size: u32, subscription_limit: u32) -> Option<u32> {
    if global_batch_size == 0 {
        return None;
    }
    let local = if subscription_limit == 0 {
        global_batch_size
    } else {
        subscription_limit
    };
    Some(local.min(global_batch_size).max(1))
}

pub fn resolve_finished_status_text(status: &str, failure_kind: Option<&str>) -> &'static str {
    if status == "cancelled" && failure_kind == Some("inbox_full") {
        return "Paused (Inbox full)";
    }
    match status {
        "succeeded" => "Completed",
        "cancelled" => "Cancelled",
        _ => "Failed",
    }
}

pub fn default_resume_strategy_for_site(site_id: &str) -> Option<&'static str> {
    match crate::subscriptions::gallery_dl_runner::canonical_site_id(site_id) {
        "danbooru" | "gelbooru" | "3dbooru" | "safebooru" | "rule34" | "yandere" | "e621"
        | "konachan" | "lolibooru" => Some("tag_id_lt"),
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
    items: &[crate::subscriptions::gallery_dl_runner::DownloadedItem],
    strategy: &str,
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_resume_to_query, derive_resume_cursor, effective_inbox_limit,
        effective_query_file_limit, resolve_finished_status_text,
    };

    #[test]
    fn effective_query_file_limit_clamps_to_global_cap_when_enabled() {
        assert_eq!(effective_query_file_limit(100, 0), Some(100));
        assert_eq!(effective_query_file_limit(100, 50), Some(50));
        assert_eq!(effective_query_file_limit(100, 500), Some(100));
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
            crate::subscriptions::gallery_dl_runner::DownloadedItem {
                file_path: std::path::PathBuf::from("/tmp/a"),
                metadata: crate::subscriptions::gallery_dl_runner::ParsedMetadata {
                    post_id: Some("100".to_string()),
                    ..Default::default()
                },
            },
            crate::subscriptions::gallery_dl_runner::DownloadedItem {
                file_path: std::path::PathBuf::from("/tmp/b"),
                metadata: crate::subscriptions::gallery_dl_runner::ParsedMetadata {
                    post_id: Some("93".to_string()),
                    ..Default::default()
                },
            },
        ];
        assert_eq!(
            derive_resume_cursor(&items, "tag_id_lt"),
            Some("93".to_string())
        );
    }
}
