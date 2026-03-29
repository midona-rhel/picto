use crate::blob_store::BlobStore;
use crate::sqlite::SqliteDatabase;
use crate::types::ImportBatchResult;

use super::ApplicationEngine;

impl ApplicationEngine {
    pub async fn import_files(
        &self,
        legacy_db: &SqliteDatabase,
        blob_store: &BlobStore,
        paths: Vec<String>,
        tag_strings: Option<Vec<String>>,
        source_urls: Option<Vec<String>>,
        initial_status: i64,
        library_root: Option<&std::path::Path>,
    ) -> Result<ImportBatchResult, String> {
        let (batch, summary) = crate::ingest::import_files(
            self.db(),
            Some(legacy_db),
            blob_store,
            paths,
            tag_strings,
            source_urls,
            initial_status,
            library_root,
        )
        .await?;

        crate::ingest::apply_compiler_plan(self.db(), &summary.flags, &summary.folder_ids);
        if !summary.imported_hashes.is_empty()
            || !summary.skipped_hashes.is_empty()
            || summary.flags.status_changed
            || summary.flags.tags_changed
            || summary.flags.metadata_changed
        {
            crate::events::emit_state_changed(
                "manual_import",
                crate::ingest::build_ingest_change_impact(
                    &summary,
                    vec!["system:active".into(), "system:inbox".into()],
                ),
            );
        }

        Ok(batch)
    }

    pub async fn import_folder(
        &self,
        legacy_db: &SqliteDatabase,
        blob_store: &BlobStore,
        path: String,
        preserve_structure: bool,
        parent_folder_id: Option<i64>,
        initial_status: i64,
    ) -> Result<ImportBatchResult, String> {
        let (batch, summary) = crate::ingest::import_folder(
            self.db(),
            Some(legacy_db),
            blob_store,
            path,
            preserve_structure,
            parent_folder_id,
            initial_status,
        )
        .await?;

        crate::ingest::apply_compiler_plan(self.db(), &summary.flags, &summary.folder_ids);
        if !summary.imported_hashes.is_empty()
            || !summary.skipped_hashes.is_empty()
            || !summary.folder_ids.is_empty()
            || summary.flags.status_changed
            || summary.flags.tags_changed
            || summary.flags.metadata_changed
        {
            crate::events::emit_state_changed(
                "import_folder",
                crate::ingest::build_ingest_change_impact(
                    &summary,
                    vec!["system:active".into(), "system:inbox".into()],
                ),
            );
        }

        Ok(batch)
    }
}
