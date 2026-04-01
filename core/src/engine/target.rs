//! EntityTarget resolution.
//!
//! Resolves an EntityTarget into either concrete entity ids (small targets)
//! or a DB-backed bulk reference (large query_results targets).

use crate::db::types::{EntityTarget, EntityTargetKind, EntityViewQuery};
use crate::db::LibraryDatabase;

/// Resolved form of an EntityTarget.
///
/// Small targets resolve to concrete ids in memory.
/// Large query_results targets stay as a query reference so the db layer
/// can execute against them directly without materializing millions of ids.
#[derive(Debug)]
pub enum ResolvedTarget {
    /// Concrete entity ids (from entity_hashes lookups or small result sets).
    Ids(Vec<i64>),
    /// DB-backed bulk target — the query will be executed by the db layer
    /// directly (e.g. via temp table + JOIN). Exclusions are entity_hashes
    /// to subtract from the result.
    Query {
        view_query: EntityViewQuery,
        exclusions: Vec<String>,
    },
}

impl ResolvedTarget {
    /// Returns true if this target resolved to zero entities.
    pub fn is_empty(&self) -> bool {
        match self {
            ResolvedTarget::Ids(ids) => ids.is_empty(),
            ResolvedTarget::Query { .. } => false, // can't know without executing
        }
    }
}

/// Resolve an EntityTarget.
///
/// - `entity_hashes` targets: look up each hash → entity_id via the db.
/// - `query_results` targets with a manageable result set: materialize ids.
/// - `query_results` targets with large/unbounded scopes: keep as Query reference.
pub fn resolve(db: &LibraryDatabase, target: &EntityTarget) -> Result<ResolvedTarget, String> {
    match target.kind {
        EntityTargetKind::EntityHashes => {
            let hashes = target.entity_hashes.as_deref().unwrap_or(&[]);
            if hashes.is_empty() {
                return Ok(ResolvedTarget::Ids(Vec::new()));
            }
            let ids = db.resolve_entity_hashes(hashes)?;
            Ok(ResolvedTarget::Ids(ids))
        }
        EntityTargetKind::QueryResults => {
            let query = target
                .query
                .as_ref()
                .ok_or_else(|| "query_results target missing query field".to_string())?;
            let exclusions = target
                .excluded_entity_hashes
                .as_deref()
                .unwrap_or(&[])
                .to_vec();
            // Keep as a query reference — the db layer handles bulk execution.
            Ok(ResolvedTarget::Query {
                view_query: query.clone(),
                exclusions,
            })
        }
    }
}
