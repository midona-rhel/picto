use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    CredentialMissing,
    CredentialBlocked,
    Unauthorized,
    Expired,
    RateLimited,
    Network,
    /// The query/user/post doesn't exist or the site rejected it (404/422).
    NotFound,
    /// Local environment breakage — python bootstrap, missing modules, bad paths.
    Environment,
    InvalidQueryKind,
    InboxFull,
    MalformedMetadata,
    IngestQueueFailure,
    DownloadFailure,
    BridgeNoDownloads,
    UnexpectedRetryItem,
    MissingRetry,
    MissingSubscription,
    MissingQuery,
    Runtime,
    Panic,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    FixCredentials,
    RetryAutomatically,
    RetryNow,
    ReviewQuery,
    None,
}

impl RecoveryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FixCredentials => "fix_credentials",
            Self::RetryAutomatically => "retry_automatically",
            Self::RetryNow => "retry_now",
            Self::ReviewQuery => "review_query",
            Self::None => "none",
        }
    }
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CredentialMissing => "credential_missing",
            Self::CredentialBlocked => "credential_blocked",
            Self::Unauthorized => "unauthorized",
            Self::Expired => "expired",
            Self::RateLimited => "rate_limited",
            Self::Network => "network",
            Self::NotFound => "not_found",
            Self::Environment => "environment",
            Self::InvalidQueryKind => "invalid_query_kind",
            Self::InboxFull => "inbox_full",
            Self::MalformedMetadata => "malformed_metadata",
            Self::IngestQueueFailure => "ingest_queue_failure",
            Self::DownloadFailure => "download_failure",
            Self::BridgeNoDownloads => "bridge_no_downloads",
            Self::UnexpectedRetryItem => "unexpected_retry_item",
            Self::MissingRetry => "missing_retry",
            Self::MissingSubscription => "missing_subscription",
            Self::MissingQuery => "missing_query",
            Self::Runtime => "runtime",
            Self::Panic => "panic",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }

    pub fn recovery_action(self) -> RecoveryAction {
        match self {
            Self::CredentialMissing
            | Self::CredentialBlocked
            | Self::Unauthorized
            | Self::Expired => RecoveryAction::FixCredentials,
            Self::RateLimited | Self::Network => RecoveryAction::RetryAutomatically,
            Self::DownloadFailure | Self::IngestQueueFailure => RecoveryAction::RetryNow,
            Self::InvalidQueryKind
            | Self::NotFound
            | Self::MalformedMetadata
            | Self::BridgeNoDownloads
            | Self::UnexpectedRetryItem => RecoveryAction::ReviewQuery,
            Self::Environment
            | Self::Runtime
            | Self::Panic
            | Self::Unknown
            | Self::MissingRetry
            | Self::MissingSubscription
            | Self::MissingQuery
            | Self::InboxFull
            | Self::Stale => RecoveryAction::None,
        }
    }

    pub fn creates_issue(self) -> bool {
        !matches!(
            self,
            Self::InboxFull | Self::Stale | Self::MissingSubscription | Self::MissingQuery
        )
    }

    /// Auth failures share one durable issue identity regardless of whether
    /// the runner reported an HTTP auth failure or an expired session.
    pub fn issue_kind(self) -> &'static str {
        match self {
            Self::Unauthorized | Self::Expired => FailureKind::CredentialBlocked.as_str(),
            _ => self.as_str(),
        }
    }
}

/// True for lines that carry an actual failure rather than DEBUG noise:
/// `[error]`-tagged log lines, `gallery_dl.exception.*` lines, and Python
/// traceback terminators (`SomeError: message`).
fn is_error_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("[error]") {
        return true;
    }
    if trimmed.starts_with("gallery_dl.exception.") {
        return true;
    }
    // "NotFoundError: Requested user could not be found"
    if let Some(colon) = trimmed.find(": ") {
        let head = &trimmed[..colon];
        if head.ends_with("Error")
            && !head.is_empty()
            && head
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return true;
        }
    }
    false
}

pub fn has_error_lines(stderr: &str) -> bool {
    stderr.lines().any(is_error_line)
}

/// The lines that carry the actual failure. The bridge logs everything at
/// DEBUG (urllib3 request lines full of incidental 403s/404s), so classifying
/// the whole blob misattributes failures — only error-bearing lines count.
/// Falls back to the last 5 non-empty lines when nothing is error-tagged.
pub fn error_tail(stderr: &str) -> String {
    let error_lines: Vec<&str> = stderr.lines().filter(|line| is_error_line(line)).collect();
    if !error_lines.is_empty() {
        let start = error_lines.len().saturating_sub(8);
        return error_lines[start..].join("\n");
    }
    let non_empty: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = non_empty.len().saturating_sub(5);
    non_empty[start..].join("\n")
}

/// The single best human-readable error line for storage and display.
pub fn final_error_line(stderr: &str) -> Option<String> {
    let tail = error_tail(stderr);
    let line = tail.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    let mut out = line.to_string();
    if out.len() > 500 {
        out.truncate(500);
    }
    Some(out)
}

pub fn classify_failure(stderr: &str) -> FailureKind {
    let lower = error_tail(stderr).to_ascii_lowercase();
    // Auth patterns first — a 403 must stay Unauthorized, not NotFound.
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("unauthorized")
        || lower.contains("authorizationerror")
        || lower.contains("authenticationerror")
        || lower.contains("sad panda")
        || lower.contains("authrequired")
        || lower.contains("missing authentication")
        || lower.contains("login required")
        || lower.contains("authentication required")
    {
        return FailureKind::Unauthorized;
    }
    if lower.contains("expired")
        || lower.contains("token invalid")
        || lower.contains("session invalid")
        || lower.contains("session has expired")
    {
        return FailureKind::Expired;
    }
    if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests")
    {
        return FailureKind::RateLimited;
    }
    if lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("nameresolutionerror")
        || lower.contains("failed to resolve")
        || lower.contains("nodename nor servname provided")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("network is unreachable")
        || lower.contains("dns")
    {
        return FailureKind::Network;
    }
    if lower.contains("modulenotfounderror")
        || lower.contains("importerror")
        || lower.contains("bridge_import_error")
        || lower.contains("no such file or directory")
    {
        return FailureKind::Environment;
    }
    if lower.contains("404")
        || lower.contains("not found")
        || lower.contains("notfounderror")
        || lower.contains("could not be found")
        || lower.contains("no results")
        || lower.contains("422")
        || lower.contains("unprocessable entity")
        || lower.contains("400")
        || lower.contains("bad request")
        || lower.contains("user deleted")
    {
        return FailureKind::NotFound;
    }
    FailureKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_actions_and_issue_creation_are_deterministic() {
        let cases = [
            (
                FailureKind::CredentialMissing,
                RecoveryAction::FixCredentials,
                true,
            ),
            (
                FailureKind::CredentialBlocked,
                RecoveryAction::FixCredentials,
                true,
            ),
            (
                FailureKind::Unauthorized,
                RecoveryAction::FixCredentials,
                true,
            ),
            (FailureKind::Expired, RecoveryAction::FixCredentials, true),
            (
                FailureKind::RateLimited,
                RecoveryAction::RetryAutomatically,
                true,
            ),
            (
                FailureKind::Network,
                RecoveryAction::RetryAutomatically,
                true,
            ),
            (FailureKind::DownloadFailure, RecoveryAction::RetryNow, true),
            (
                FailureKind::IngestQueueFailure,
                RecoveryAction::RetryNow,
                true,
            ),
            (
                FailureKind::InvalidQueryKind,
                RecoveryAction::ReviewQuery,
                true,
            ),
            (FailureKind::NotFound, RecoveryAction::ReviewQuery, true),
            (
                FailureKind::MalformedMetadata,
                RecoveryAction::ReviewQuery,
                true,
            ),
            (
                FailureKind::BridgeNoDownloads,
                RecoveryAction::ReviewQuery,
                true,
            ),
            (
                FailureKind::UnexpectedRetryItem,
                RecoveryAction::ReviewQuery,
                true,
            ),
            (FailureKind::Environment, RecoveryAction::None, true),
            (FailureKind::Runtime, RecoveryAction::None, true),
            (FailureKind::Panic, RecoveryAction::None, true),
            (FailureKind::Unknown, RecoveryAction::None, true),
            (FailureKind::MissingRetry, RecoveryAction::None, true),
            (
                FailureKind::MissingSubscription,
                RecoveryAction::None,
                false,
            ),
            (FailureKind::MissingQuery, RecoveryAction::None, false),
            (FailureKind::InboxFull, RecoveryAction::None, false),
            (FailureKind::Stale, RecoveryAction::None, false),
        ];

        for (kind, action, creates_issue) in cases {
            assert_eq!(kind.recovery_action(), action, "{}", kind.as_str());
            assert_eq!(kind.creates_issue(), creates_issue, "{}", kind.as_str());
        }
    }

    #[test]
    fn warning_only_skips_are_not_extractor_errors() {
        assert!(!has_error_lines(
            "[fanbox][WARNING] Skipping post 123 (HttpError: '403 Forbidden')"
        ));
        assert!(has_error_lines(
            "[fanbox][ERROR] AuthenticationError: Login required"
        ));
    }

    #[test]
    fn auth_failures_share_the_credential_blocked_issue_key() {
        assert_eq!(FailureKind::Unauthorized.issue_kind(), "credential_blocked");
        assert_eq!(FailureKind::Expired.issue_kind(), "credential_blocked");
    }

    #[test]
    fn classifies_not_found() {
        assert_eq!(
            classify_failure("HttpError: '422 Unprocessable Entity' for url"),
            FailureKind::NotFound
        );
        assert_eq!(classify_failure("404 not found"), FailureKind::NotFound);
    }

    #[test]
    fn classifies_environment() {
        assert_eq!(
            classify_failure("ModuleNotFoundError: No module named 'gallery_dl'"),
            FailureKind::Environment
        );
    }

    #[test]
    fn classifies_python_name_resolution_failures_as_network() {
        let stderr = "gallery_dl.exception.HttpError: NameResolutionError: \
                      Failed to resolve 'gelbooru.com' ([Errno 8] nodename nor servname provided)";
        assert_eq!(classify_failure(stderr), FailureKind::Network);
    }

    #[test]
    fn debug_noise_does_not_poison_classification() {
        // Incidental HTTP failures in DEBUG output must not override the final
        // extractor error.
        let mut stderr = String::new();
        for i in 0..200 {
            stderr.push_str(&format!(
                "[urllib3.connectionpool][DEBUG] https://gelbooru.com:443 \"GET /index.php?page={i} HTTP/1.1\" 403 None\n"
            ));
        }
        stderr.push_str("Traceback (most recent call last):\n");
        stderr.push_str("  File \"gelbooru.py\", line 100, in items\n");
        stderr.push_str("gallery_dl.exception.NotFoundError: Requested user could not be found\n");
        assert_eq!(classify_failure(&stderr), FailureKind::NotFound);
        assert_eq!(
            final_error_line(&stderr).unwrap(),
            "gallery_dl.exception.NotFoundError: Requested user could not be found"
        );
    }

    #[test]
    fn error_tagged_auth_line_still_classifies_unauthorized() {
        let stderr = "[urllib3.connectionpool][DEBUG] GET /posts 200\n\
                      [pixiv][error] 403 Forbidden\n";
        assert_eq!(classify_failure(stderr), FailureKind::Unauthorized);
    }

    #[test]
    fn gallery_dl_authentication_error_is_unauthorized() {
        assert_eq!(
            classify_failure(
                "gallery_dl.exception.AuthenticationError: FANBOX session was rejected"
            ),
            FailureKind::Unauthorized
        );
    }

    #[test]
    fn exhentai_expired_session_is_unauthorized() {
        assert_eq!(
            classify_failure("gallery_dl.exception.AuthorizationError: Sad Panda"),
            FailureKind::Unauthorized
        );
    }

    #[test]
    fn python_error_terminator_counts_as_error_line() {
        let stderr = "some debug\nNotFoundError: Requested user could not be found\n";
        assert_eq!(classify_failure(stderr), FailureKind::NotFound);
    }

    #[test]
    fn tail_falls_back_to_last_lines_without_error_tags() {
        let stderr = "line one\n404 not found\n";
        assert_eq!(classify_failure(stderr), FailureKind::NotFound);
    }

    #[test]
    fn auth_wins_over_not_found() {
        assert_eq!(
            classify_failure("403 Forbidden: page not found"),
            FailureKind::Unauthorized
        );
    }
}
