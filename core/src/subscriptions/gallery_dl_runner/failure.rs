use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Unauthorized,
    Expired,
    RateLimited,
    Network,
    /// The query/user/post doesn't exist or the site rejected it (404/422).
    NotFound,
    /// Local environment breakage — python bootstrap, missing modules, bad paths.
    Environment,
    Unknown,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Expired => "expired",
            Self::RateLimited => "rate_limited",
            Self::Network => "network",
            Self::NotFound => "not_found",
            Self::Environment => "environment",
            Self::Unknown => "unknown",
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
    // Twitter/X stale-session signature: when the stored cookies are stale, X
    // serves a page format whose script hash can't be extracted and gallery-dl
    // 404s on a malformed "ondemand.s.Nonea.js" URL (upstream gallery-dl #9602).
    // The remedy is re-exporting fresh browser cookies, so this is Expired —
    // without this check the 404 would misclassify as NotFound.
    if lower.contains("ondemand.s.none") {
        return FailureKind::Expired;
    }
    // Auth patterns first — a 403 must stay Unauthorized, not NotFound.
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("unauthorized")
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
    fn debug_noise_does_not_poison_classification() {
        // Regression: a twitter user-not-found run whose DEBUG stderr is full
        // of incidental 403s must classify from the final exception, not the
        // noise.
        let mut stderr = String::new();
        for i in 0..200 {
            stderr.push_str(&format!(
                "[urllib3.connectionpool][DEBUG] https://x.com:443 \"GET /i/api/{i} HTTP/1.1\" 403 None\n"
            ));
        }
        stderr.push_str("Traceback (most recent call last):\n");
        stderr.push_str("  File \"twitter.py\", line 100, in _user\n");
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
                      [twitter][error] 403 Forbidden\n";
        assert_eq!(classify_failure(stderr), FailureKind::Unauthorized);
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
    fn twitter_stale_session_classifies_as_expired() {
        assert_eq!(
            classify_failure(
                "gallery_dl.exception.HttpError: '404 Not Found' for \
                 'https://abs.twimg.com/responsive-web/client-web/ondemand.s.Nonea.js'"
            ),
            FailureKind::Expired
        );
    }

    #[test]
    fn auth_wins_over_not_found() {
        assert_eq!(
            classify_failure("403 Forbidden: page not found"),
            FailureKind::Unauthorized
        );
    }
}
