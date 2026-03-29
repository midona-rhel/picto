use std::path::Path;

use tracing::info;

use crate::subscriptions::gallery_dl_runner::ParsedMetadata;

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
        _gallery_url: &str,
        is_collection_member: bool,
        skip_thumbnail: bool,
    ) -> Result<ImportOutcome, String> {
        info!(
            post_id = metadata.post_id.as_deref().unwrap_or("?"),
            tags = metadata.tags.len(),
            "Importing file"
        );
        let state = crate::state::get_state()?;
        let outcome = crate::ingest::ingest_subscription_item(
            state.engine.db(),
            self.db,
            self.blob_store,
            file_path,
            metadata,
            subscription_id,
            skip_thumbnail,
            0,
        )
        .await?;

        if !is_collection_member {
            let mut summary = crate::ingest::IngestBatchSummary::default();
            summary.flags.merge(&outcome.flags);
            if outcome.imported_new {
                summary.imported_hashes.push(outcome.entity_hash.clone());
            } else {
                summary.skipped_hashes.push(outcome.entity_hash.clone());
            }
            crate::ingest::apply_compiler_plan(state.engine.db(), &summary.flags, &summary.folder_ids);
            crate::events::emit_state_changed(
                "subscription_import",
                crate::ingest::build_ingest_change_impact(
                    &summary,
                    vec!["system:inbox".into()],
                ),
            );
        }

        Ok(ImportOutcome {
            _hex_hash: outcome.entity_hash,
            imported_new: outcome.imported_new,
        })
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

        let state = match crate::state::get_state() {
            Ok(state) => state,
            Err(error) => {
                progress.errors.push(error);
                return;
            }
        };

        self.set_phase("creating_collection");
        self.emit_progress_force(
            sub_id_str,
            progress,
            &format!(
                "Creating collection '{}' ({} items)",
                pc.preferred_name,
                pc.members.len()
            ),
        );
        let members: Vec<crate::ingest::SubscriptionCollectionMember> = pc
            .members
            .iter()
            .enumerate()
            .map(|(index, member)| crate::ingest::SubscriptionCollectionMember {
                path: member.file_path.clone(),
                metadata: member.metadata.clone(),
                skip_thumbnail: index > 0,
            })
            .collect();

        match crate::ingest::materialize_subscription_collection(
            state.engine.db(),
            self.db,
            self.blob_store,
            subscription_id,
            &pc.category,
            &pc.post_id,
            &pc.preferred_name,
            &members,
        )
        .await {
            Ok(result) => {
                crate::ingest::apply_compiler_plan(state.engine.db(), &result.flags, &[]);
                if let Some(collection_id) = result.collection_id {
                    changed_collection_ids.push(collection_id);
                }
                if let Some(qid) = pc.queue_id {
                    let _ = self.db.mark_queue_complete(qid).await;
                }
                if let Some(collection_hash) = result.collection_hash {
                    let mut summary = crate::ingest::IngestBatchSummary::default();
                    summary.flags.merge(&result.flags);
                    summary.imported_hashes.push(collection_hash);
                    crate::events::emit_state_changed(
                        "subscription_collection_import",
                        crate::ingest::build_ingest_change_impact(
                            &summary,
                            vec!["system:inbox".into()],
                        ),
                    );
                }
            }
            Err(error) => {
                progress
                    .errors
                    .push(format!("Collection batch commit failed: {error}"));
            }
        }
    }
}
