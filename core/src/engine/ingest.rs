use crate::blob_store::BlobStore;
use crate::sqlite::SqliteDatabase;
use crate::types::ImportBatchResult;

use super::ApplicationEngine;

impl ApplicationEngine {
    pub async fn import_files(
        &self,
        legacy_db: &SqliteDatabase,
        _blob_store: &BlobStore,
        paths: Vec<String>,
        tag_strings: Option<Vec<String>>,
        source_urls: Option<Vec<String>>,
        initial_status: i64,
        library_root: Option<&std::path::Path>,
    ) -> Result<ImportBatchResult, String> {
        crate::ingest_queue::enqueue_manual_files(
            legacy_db,
            paths,
            tag_strings,
            source_urls,
            initial_status,
            library_root,
        )
        .await
    }

    pub async fn import_folder(
        &self,
        legacy_db: &SqliteDatabase,
        _blob_store: &BlobStore,
        path: String,
        preserve_structure: bool,
        parent_folder_id: Option<i64>,
        initial_status: i64,
    ) -> Result<ImportBatchResult, String> {
        crate::ingest_queue::enqueue_folder_import(
            self.db(),
            legacy_db,
            path,
            preserve_structure,
            parent_folder_id,
            initial_status,
        )
        .await
    }
}
