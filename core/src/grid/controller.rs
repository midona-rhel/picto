//! Grid controller — thin entry point delegating to focused query modules.

use crate::ptr::db::PtrSqliteDatabase;
use crate::sqlite::SqliteDatabase;
use crate::types::{EntityMetadataBatchResponse, GridPageSlimQuery, GridPageSlimResponse};

pub struct GridController;

impl GridController {
    pub async fn get_grid_page_slim(
        db: &SqliteDatabase,
        query: GridPageSlimQuery,
    ) -> Result<GridPageSlimResponse, String> {
        crate::grid::query::get_grid_page_slim(db, query).await
    }

    pub async fn get_files_metadata_batch(
        db: &SqliteDatabase,
        ptr_db: &PtrSqliteDatabase,
        hashes: Vec<String>,
    ) -> Result<EntityMetadataBatchResponse, String> {
        crate::grid::metadata::get_files_metadata_batch(db, ptr_db, hashes).await
    }
}
