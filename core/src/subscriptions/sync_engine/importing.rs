use std::collections::{HashMap, HashSet};
use std::path::Path;

use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::import::existing::{merge_existing_import_target, ExistingImportMergeRequest};
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
    pub _hex_hash: String,
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
        self.import_item_inner(
            file_path,
            metadata,
            subscription_id,
            gallery_url,
            is_collection_member,
            false,
        )
        .await
    }

    async fn import_item_inner(
        &self,
        file_path: &Path,
        metadata: &ParsedMetadata,
        subscription_id: i64,
        gallery_url: &str,
        is_collection_member: bool,
        skip_thumbnail: bool,
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
                _hex_hash: hex_hash,
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
        options.skip_thumbnail = skip_thumbnail;

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
            Ok((imported, deferred)) => {
                // Run deferred work (dominant colors, phash, thumbnail generation).
                if let Some(work) = deferred {
                    pipeline.process_deferred(work).await;
                }
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

                    crate::events::emit_state_changed(
                        "subscription_import",
                        crate::runtime_contract::change_builder::ChangeImpact::file_lifecycle(
                            self.db,
                        )
                        .file_hashes(vec![surviving_hash.clone()])
                        .extra_grid_scopes(vec!["system:inbox".into()]),
                    );
                }

                Ok(ImportOutcome {
                    _hex_hash: surviving_hash,
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
                    _hex_hash: hash,
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

    /// Import all stashed members and group them into a collection atomically.
    /// Prepares all files first (hash + MIME + blob write), then commits everything
    /// in a single DB transaction. The grid never sees individual loose files.
    pub(super) async fn materialize_collection(
        &mut self,
        mut pc: super::PendingCollection,
        subscription_id: i64,
        sub_id_str: &str,
        progress: &mut super::SyncProgress,
        changed_collection_ids: &mut Vec<i64>,
    ) {
        pc.members.sort_by_key(|m| m.page_num);
        let member_count = pc.members.len();

        self.set_phase("importing");
        self.emit_progress_force(
            sub_id_str,
            progress,
            &format!(
                "Importing {} files for '{}'...",
                member_count, pc.preferred_name
            ),
        );

        let pipeline = ImportPipeline::new(self.db, self.blob_store);
        let mut prepared: Vec<crate::import::pipeline::PreparedFile> =
            Vec::with_capacity(pc.members.len());

        // Phase 1: prepare all files (hash, MIME, blob write — no DB)
        for (i, member) in pc.members.iter().enumerate() {
            let mut options = ImportOptions::default();
            options.tags = member.metadata.tags.clone();
            options.source_urls = member.metadata.source_urls.clone();
            options.created_at = member.metadata.created_at.clone();
            options.name = preferred_import_name(&member.metadata);
            options.skip_thumbnail = i > 0; // only cover gets thumbnail
            {
                let mut notes = HashMap::new();
                if let Some(ref desc) = member.metadata.description {
                    notes.insert("description".to_string(), desc.clone());
                }
                if let Some(ref title) = member.metadata.title {
                    notes.insert("title".to_string(), title.clone());
                }
                if !notes.is_empty() {
                    options.notes = Some(notes);
                }
            }
            let mut seen_urls = HashSet::new();
            options.source_urls.retain(|url| {
                let trimmed = url.trim();
                !trimmed.is_empty() && seen_urls.insert(trimmed.to_string())
            });

            match pipeline.prepare_file(&member.file_path, &options).await {
                Ok(pf) => {
                    // files_downloaded already incremented during stashing
                    prepared.push(pf);
                }
                Err(crate::import::pipeline::ImportError::AlreadyImported(_)) => {
                    // Was counted as downloaded during stash, move to skipped
                    progress.files_skipped += 1;
                    if progress.files_downloaded > 0 {
                        progress.files_downloaded -= 1;
                    }
                }
                Err(e) => {
                    progress
                        .errors
                        .push(format!("Prepare error for post {}: {e}", pc.post_id));
                }
            }
        }

        if prepared.is_empty() {
            return;
        }

        // Single-member "collection" — import as standalone file
        if prepared.len() < 2 {
            let pf = prepared.remove(0);
            let file_hash = pf.hex_hash.clone();
            let _ = self.db.import_file(pf.db_opts).await;
            crate::events::emit_state_changed(
                "subscription_import",
                crate::runtime_contract::change_builder::ChangeImpact::file_lifecycle(self.db)
                    .file_hashes(vec![file_hash])
                    .extra_grid_scopes(vec!["system:inbox".into()]),
            );
            return;
        }

        // Phase 2: one atomic DB transaction — insert all files + create collection
        self.set_phase("creating_collection");
        self.emit_progress_force(
            sub_id_str,
            progress,
            &format!(
                "Creating collection '{}' ({} items)",
                pc.preferred_name,
                prepared.len()
            ),
        );
        let db_opts: Vec<_> = prepared.into_iter().map(|pf| pf.db_opts).collect();
        match self
            .db
            .import_collection_batch(db_opts, &pc.preferred_name)
            .await
        {
            Ok(result) => {
                let _ = self
                    .db
                    .upsert_subscription_post_collection(
                        subscription_id,
                        &pc.category,
                        &pc.post_id,
                        result.collection_id,
                    )
                    .await;
                changed_collection_ids.push(result.collection_id);

                // Mark download queue entry as complete
                if let Some(qid) = pc.queue_id {
                    let _ = self.db.mark_queue_complete(qid).await;
                }

                let impact = crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
                    result.collection_id,
                )
                .file_hashes(result.hashes)
                .extra_grid_scopes(vec!["system:inbox".into()])
                .merge(crate::runtime_contract::change_builder::ChangeImpact::file_lifecycle(
                    self.db,
                ));
                crate::events::emit_state_changed("subscription_collection_import", impact);
            }
            Err(e) => {
                progress
                    .errors
                    .push(format!("Collection batch commit failed: {e}"));
            }
        }
    }

    async fn merge_existing_metadata(
        &self,
        hex_hash: &str,
        existing: &crate::sqlite::files::FileRecord,
        metadata: &ParsedMetadata,
        _gallery_url: &str,
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
                change_origin: "subscription_import",
            },
        )
        .await
        .map(|_| ())
    }
}
