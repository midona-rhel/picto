//! Grid page query — resolves paginated file queries for the main image grid.
//!
//! Handles scope resolution, sorting, pagination, and color filtering.

mod collection;
mod common;
mod cursor;
mod scope;
mod status;

use common::QueryInputs;

use crate::sqlite::SqliteDatabase;
use crate::types::{GridPageSlimQuery, GridPageSlimResponse, GridScopeKind};

pub use common::GridOutlineResponse;

pub async fn get_grid_outline(
    db: &SqliteDatabase,
    query: GridPageSlimQuery,
) -> Result<GridOutlineResponse, String> {
    let inputs = QueryInputs::build(db, &query).await?;

    if query.scope.kind == GridScopeKind::Collection {
        return collection::get_collection_outline(db, &query, &inputs).await;
    }

    if scope::needs_scope(&query) {
        return scope::get_scoped_outline(db, &query, &inputs).await;
    }

    status::get_status_outline(db, &query, &inputs).await
}

pub async fn get_grid_page_slim(
    db: &SqliteDatabase,
    query: GridPageSlimQuery,
) -> Result<GridPageSlimResponse, String> {
    let inputs = QueryInputs::build(db, &query).await?;

    if query.scope.kind == GridScopeKind::Collection {
        return collection::get_collection_page(db, &query, &inputs).await;
    }

    if scope::needs_scope(&query) {
        return scope::get_scoped_page(db, &query, &inputs).await;
    }

    status::get_status_page(db, &query, &inputs).await
}
