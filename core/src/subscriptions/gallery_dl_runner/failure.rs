use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Unauthorized,
    Expired,
    RateLimited,
    Network,
    Unknown,
}

pub fn classify_failure(stderr: &str) -> FailureKind {
    let lower = stderr.to_ascii_lowercase();
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
    FailureKind::Unknown
}
