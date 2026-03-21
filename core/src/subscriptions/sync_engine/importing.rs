use std::collections::{HashMap, HashSet};
use std::path::Path;

use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::import::existing::{ExistingImportMergeRequest, merge_existing_import_target};
use crate::import::pipeline::{ImportError, ImportOptions, ImportPipeline};
use crate::subscriptions::gallery_dl_runner::ParsedMetadata;
use crate::subscriptions::import_policy::{
    generated_subscription_name, normalized_title, preferred_import_name,
    should_replace_existing_name,
};
use crate::tags::normalize;

use super::SubscriptionSyncEngine;

#[derive(Debug, Clone)]
pub(super) struct ImportOutcome {
    pub hex_hash: String,
    pub imported_new: bool,
}

impl<'a> SubscriptionSyncEngine<'a> {
    pub(super) async fn import_item(
        &self,
        file_path: &Path,
        metadata: &ParsedMetadata,
        subscription_id: i64,
        gallery_url: &str,
        is_collection_member: bool,
    ) -> Result<ImportOutcome, String> {
        let file_data = tokio::fs::read(file_path)
            .await
            .map_err(|e| format!("Read error: {e}"))?;
        let hex_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&file_data);
            hex::encode(hasher.finalize())
        };

        if let Ok(Some(existing)) = self.db.get_file_by_hash(&hex_hash).await {
            self.merge_existing_metadata(
                &hex_hash,
                &existing,
                metadata,
                gallery_url,
                subscription_id,
            )
            .await?;
            return Ok(ImportOutcome {
                hex_hash,
                imported_new: false,
            });
        }

        let mut options = ImportOptions::default();
        options.tags = metadata.tags.clone();
        options.source_urls = metadata.source_urls.clone();
        options.created_at = metadata.created_at.clone();
        let mut seen_urls = HashSet::new();
        options.source_urls.retain(|url| {
            let trimmed = url.trim();
            !trimmed.is_empty() && seen_urls.insert(trimmed.to_string())
        });

        options.name = preferred_import_name(metadata);

        {
            let mut notes = HashMap::new();
            if let Some(ref description) = metadata.description {
                notes.insert("description".to_string(), description.clone());
            }
            if let Some(ref title) = metadata.title {
                notes.insert("title".to_string(), title.clone());
            }
            if !notes.is_empty() {
                options.notes = Some(notes);
            }
        }

        info!(
            post_id = metadata.post_id.as_deref().unwrap_or("?"),
            tags = metadata.tags.len(),
            "Importing file"
        );

        let pipeline = ImportPipeline::new(self.db, self.blob_store);
        match pipeline.import_file(file_path, &options).await {
            Ok(imported) => {
                info!(hash = %imported.hex_hash, tags = options.tags.len(), "Import success");

                let mut surviving_hash = imported.hex_hash.clone();
                if self.auto_merge_enabled && imported.mime.starts_with("image/") {
                    match crate::duplicates::orchestrator::DuplicateOrchestrator::check_and_auto_merge(
                        self.db,
                        self.blob_store,
                        &imported.hex_hash,
                        self.auto_merge_distance,
                        self.auto_merge_require_matching_dimensions,
                    )
                    .await
                    {
                        Ok(Some(merge_result)) => {
                            surviving_hash = merge_result.winner_hash.clone();
                            info!(
                                winner = %merge_result.winner_hash,
                                loser = %merge_result.loser_hash,
                                tags_merged = merge_result.tags_merged,
                                "Auto-merged duplicate during subscription import"
                            );
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!(hash = %imported.hex_hash, error = %e, "Auto-merge failed (non-fatal)");
                        }
                    }
                }

                if let Err(e) = self
                    .db
                    .add_subscription_entity(subscription_id, &surviving_hash)
                    .await
                {
                    warn!(error = %e, "Failed to record subscription-file mapping");
                }

                // Collection members: suppress individual events — the collection
                // materialization step emits its own event once all pages are grouped.
                if !is_collection_member {
                    if surviving_hash == imported.hex_hash {
                        if let Ok(Some(record)) = self.db.get_file_by_hash(&surviving_hash).await {
                            let slim = crate::types::FileInfoSlim::from(record);
                            crate::events::emit(crate::events::event_names::FILE_IMPORTED, &slim);
                        }
                    }

                    crate::events::emit_mutation(
                        "subscription_import",
                        crate::runtime_contract::mutation_builder::MutationImpact::file_lifecycle(
                            self.db,
                        ),
                    );
                }

                Ok(ImportOutcome {
                    hex_hash: surviving_hash,
                    imported_new: true,
                })
            }
            Err(ImportError::AlreadyImported(hash)) => {
                info!(hash = %hash, "Already imported (skipped)");
                if let Ok(Some(existing)) = self.db.get_file_by_hash(&hash).await {
                    self.merge_existing_metadata(
                        &hash,
                        &existing,
                        metadata,
                        gallery_url,
                        subscription_id,
                    )
                    .await?;
                }
                Ok(ImportOutcome {
                    hex_hash: hash,
                    imported_new: false,
                })
            }
            Err(e) => {
                warn!(
                    path = %file_path.display(),
                    error = %e,
                    "Import pipeline failed"
                );
                Err(format!("{e}"))
            }
        }
    }

    async fn merge_existing_metadata(
        &self,
        hex_hash: &str,
        existing: &crate::sqlite::files::FileRecord,
        metadata: &ParsedMetadata,
        gallery_url: &str,
        subscription_id: i64,
    ) -> Result<(), String> {
        let existing_name = existing.name.as_deref().unwrap_or("").trim();
        let desired_name = if let Some(title) = normalized_title(metadata) {
            if should_replace_existing_name(existing_name, metadata) {
                Some(title)
            } else {
                None
            }
        } else if existing_name.is_empty() {
            generated_subscription_name(metadata)
        } else {
            None
        };

        let mut note_entries = HashMap::new();
        if let Some(ref description) = metadata.description {
            note_entries.insert("description".to_string(), description.clone());
        }
        if let Some(ref title) = metadata.title {
            note_entries.insert("title".to_string(), title.clone());
        }

        let mut source_urls = metadata.source_urls.clone();
        source_urls.retain(|url| !url.trim().is_empty());
        let mut deduped = Vec::with_capacity(source_urls.len());
        for url in source_urls {
            if !deduped.iter().any(|existing| existing == &url) {
                deduped.push(url);
            }
        }

        merge_existing_import_target(
            self.db,
            hex_hash,
            ExistingImportMergeRequest {
                restore_status: Some(1),
                tag_strings: metadata
                    .tags
                    .iter()
                    .map(|(ns, st)| normalize::combine_tag(ns, st))
                    .collect(),
                source_urls: deduped,
                created_at: metadata.created_at.clone(),
                name: desired_name,
                note_entries,
                subscription_id: Some(subscription_id),
                mutation_name: "subscription_import",
            },
        )
        .await
        .map(|_| ())
    }
}
