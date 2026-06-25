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

pub fn classify_failure(stderr: &str) -> FailureKind {
    let lower = stderr.to_ascii_lowercase();
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
    fn auth_wins_over_not_found() {
        assert_eq!(
            classify_failure("403 Forbidden: page not found"),
            FailureKind::Unauthorized
        );
    }
}
