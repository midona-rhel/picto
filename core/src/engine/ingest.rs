use super::ApplicationEngine;

impl ApplicationEngine {
    pub async fn add_media(
        &self,
        paths: Vec<String>,
        tag_strings: Option<Vec<String>>,
        source_urls: Option<Vec<String>>,
        initial_status: i64,
        parent_folder_id: Option<i64>,
        preserve_structure: bool,
        library_root: Option<&std::path::Path>,
    ) -> Result<(), String> {
        if preserve_structure {
            let [path] = paths.as_slice() else {
                return Err(
                    "Preserving folder structure requires exactly one folder path".to_string(),
                );
            };
            if !std::path::Path::new(path).is_dir() {
                return Err("Preserving folder structure requires a directory path".to_string());
            }
            return crate::ingest_queue::enqueue_folder_import(
                self.db(),
                path.clone(),
                parent_folder_id,
                tag_strings,
                source_urls,
                initial_status,
            )
            .await;
        }

        crate::ingest_queue::enqueue_manual_files(
            self.db(),
            paths,
            tag_strings,
            source_urls,
            initial_status,
            parent_folder_id,
            library_root,
        )
        .await
    }
}
