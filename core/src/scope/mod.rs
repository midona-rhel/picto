//! Canonical scope resolution engine.
//!
//! Single source of truth for converting user view intent (status, tags,
//! folders, smart folders) into a `RoaringBitmap` of matching file IDs.
//! Both grid and selection code paths consume this module.

pub mod resolver;

/// Match mode for include/exclude set operations on tags or folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncludeMatchMode {
    Any,
    All,
    Exact,
}

/// Parse an optional match mode string into an `IncludeMatchMode`.
pub fn parse_include_match_mode(
    raw: Option<&str>,
    default_mode: IncludeMatchMode,
) -> IncludeMatchMode {
    match raw {
        Some("any") => IncludeMatchMode::Any,
        Some("exact") => IncludeMatchMode::Exact,
        Some("all") => IncludeMatchMode::All,
        _ => default_mode,
    }
}
