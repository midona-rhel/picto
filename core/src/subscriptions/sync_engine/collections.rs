use std::collections::{HashMap, HashSet};

use tokio_util::sync::CancellationToken;

use super::{CollectionGroup, SubscriptionSyncEngine, SyncProgress};

impl<'a> SubscriptionSyncEngine<'a> {
    pub(super) async fn materialize_collection_groups(
        &mut self,
        subscription_id: i64,
        subscription_id_str: &str,
        cancel: &CancellationToken,
        progress: &mut SyncProgress,
        groups: HashMap<String, CollectionGroup>,
    ) {
        let mut group_values: Vec<CollectionGroup> = groups
            .into_values()
            .filter_map(|mut group| {
                let mut seen = HashSet::new();
                group.hashes.retain(|hash| seen.insert(hash.clone()));
                if group.hashes.len() < 2 {
                    None
                } else {
                    Some(group)
                }
            })
            .collect();
        if group_values.is_empty() {
            return;
        }
        group_values.sort_by(|a, b| a.category.cmp(&b.category).then(a.post_id.cmp(&b.post_id)));

        let total_groups = group_values.len();
        let mut changed_collection_ids = Vec::new();
        for (idx, group) in group_values.into_iter().enumerate() {
            if cancel.is_cancelled() {
                progress.cancelled = true;
                break;
            }

            self.emit_progress(
                subscription_id_str,
                progress,
                &format!("Organizing collections ({}/{})", idx + 1, total_groups),
            );

            let mapped_collection_id = match self
                .db
                .get_subscription_post_collection(subscription_id, &group.category, &group.post_id)
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    progress.errors.push(format!(
                        "Collection map lookup failed for {}:{}: {e}",
                        group.category, group.post_id
                    ));
                    None
                }
            };
            let existing_collection_id = if mapped_collection_id.is_some() {
                mapped_collection_id
            } else {
                match self.find_collection_for_hashes(&group.hashes).await {
                    Ok(id) => id,
                    Err(e) => {
                        progress.errors.push(format!(
                            "Collection lookup failed for {}:{}: {e}",
                            group.category, group.post_id
                        ));
                        continue;
                    }
                }
            };
            let collection_id = match existing_collection_id {
                Some(id) => id,
                None => match self
                    .db
                    .create_collection(&group.preferred_name)
                    .await
                {
                    Ok(id) => id,
                    Err(e) => {
                        progress.errors.push(format!(
                            "Collection create failed for {}:{}: {e}",
                            group.category, group.post_id
                        ));
                        continue;
                    }
                },
            };
            let add_result = self
                .db
                .add_collection_members_by_hashes(collection_id, &group.hashes)
                .await;
            if let Err(e) = self
                .db
                .upsert_subscription_post_collection(
                    subscription_id,
                    &group.category,
                    &group.post_id,
                    collection_id,
                )
                .await
            {
                progress.errors.push(format!(
                    "Collection map update failed for {}:{}: {e}",
                    group.category, group.post_id
                ));
            }
            match add_result {
                Ok(added) => {
                    if added > 0 {
                        changed_collection_ids.push(collection_id);
                    }
                }
                Err(e) => {
                    progress.errors.push(format!(
                        "Collection member update failed for {}:{}: {e}",
                        group.category, group.post_id
                    ));
                }
            }
        }

        if changed_collection_ids.is_empty() {
            return;
        }
        changed_collection_ids.sort_unstable();
        changed_collection_ids.dedup();

        self.db.scope_cache_invalidate_all();
        let mut scopes: Vec<String> = vec!["system:all".to_string()];
        scopes.extend(
            changed_collection_ids
                .iter()
                .map(|id| format!("collection:{id}")),
        );
        crate::events::emit_mutation(
            "subscription_import_collections",
            crate::runtime_contract::mutation_builder::MutationImpact::new()
                .folder_membership_changed(changed_collection_ids)
                .extra_grid_scopes(scopes),
        );
    }

    async fn find_collection_for_hashes(&self, hashes: &[String]) -> Result<Option<i64>, String> {
        let probe = hashes.to_vec();
        self.db
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT me.parent_collection_id
                     FROM file f
                     JOIN entity_file ef ON ef.file_id = f.file_id
                     JOIN media_entity me ON me.entity_id = ef.entity_id
                     WHERE f.hash = ?1
                       AND me.kind = 'single'
                       AND me.parent_collection_id IS NOT NULL
                     LIMIT 1",
                )?;
                for hash in &probe {
                    let mut rows = stmt.query([hash])?;
                    if let Some(row) = rows.next()? {
                        return Ok(Some(row.get::<_, i64>(0)?));
                    }
                }
                Ok(None)
            })
            .await
    }
}
