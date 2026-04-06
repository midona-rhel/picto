//! Shared selection summary helper types.

/// Rating stats for a selection.
pub struct RatingStats {
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// If every item in the selection has the same rating, this is that value.
    pub shared: Option<i64>,
}
