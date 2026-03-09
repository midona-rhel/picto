//! Subscription policy helpers shared across orchestration and execution.
//!
//! Keeps naming, batch-limit, and terminal-status behavior in one place so
//! controllers and the sync engine stop re-implementing those rules.

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

#[cfg(test)]
mod tests {
    use super::{effective_query_file_limit, resolve_finished_status_text};

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
        assert_eq!(
            resolve_finished_status_text("succeeded", None),
            "Completed"
        );
    }
}
