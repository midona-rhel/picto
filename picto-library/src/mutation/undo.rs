//! Undo and redo: replaying recorded semantic changes.

use super::*;

impl Library {
    pub fn undo(&self) -> Result<Option<MutationReceipt>> {
        let Some(entry) = self.history.take_undo() else {
            return Ok(None);
        };
        match self.replay_history(&entry, false) {
            Ok(receipt) => {
                self.history.complete_undo(entry);
                Ok(Some(receipt))
            }
            Err(error) => {
                self.history.restore_undo(entry);
                Err(error)
            }
        }
    }

    pub fn redo(&self) -> Result<Option<MutationReceipt>> {
        let Some(entry) = self.history.take_redo() else {
            return Ok(None);
        };
        match self.replay_history(&entry, true) {
            Ok(receipt) => {
                self.history.complete_redo(entry);
                Ok(Some(receipt))
            }
            Err(error) => {
                self.history.restore_redo(entry);
                Err(error)
            }
        }
    }

    pub(super) fn replay_history(&self, entry: &HistoryEntry, use_after: bool) -> Result<MutationReceipt> {
        let change = entry.change.clone();
        let changed_at_ms = now_ms();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let mut next = (*snapshot).clone();
            let mut affected = RoaringBitmap::new();
            let mut resources = BTreeSet::new();
            apply_semantic_change(
                transaction,
                revision,
                &mut next,
                &change,
                use_after,
                &mut affected,
                &mut resources,
            )?;
            // Re-check only the smart folders the replayed change can
            // affect, exactly as the original edit did.
            let mut changes = Vec::new();
            history_dependencies(&change, &mut changes);
            crate::smart::settle_affected_for(transaction, &mut next, &affected, &changes)?;
            insert_cloud_journal(
                transaction,
                revision,
                if use_after {
                    "history.redo"
                } else {
                    "history.undo"
                },
                (!affected.is_empty()).then_some(&affected),
                serde_json::json!({"label": entry.label}),
                changed_at_ms,
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                resources,
                affected.iter().map(RootId),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        Ok(receipt)
    }
}

pub(super) fn history_dependencies(
    change: &SemanticChange,
    out: &mut Vec<crate::predicate::DependencyChange>,
) {
    use crate::predicate::DependencyChange;
    let dependency = match change {
        SemanticChange::Compound(changes) => {
            for change in changes {
                history_dependencies(change, out);
            }
            return;
        }
        SemanticChange::Bitmap { key, .. } => match key.domain {
            BitmapDomain::Tag => DependencyChange::Tag(crate::TagId(key.key_id)),
            BitmapDomain::Lifecycle => DependencyChange::Lifecycle,
            BitmapDomain::Rating => DependencyChange::Rating,
        },
        SemanticChange::Order {
            owner_kind: crate::ordering::OrderOwnerKind::Folder,
            owner_id,
            ..
        } => DependencyChange::Folder(crate::FolderId(*owner_id)),
        SemanticChange::RootText { .. } => DependencyChange::RootText,
        _ => DependencyChange::All,
    };
    if !out.contains(&dependency) {
        out.push(dependency);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_semantic_change(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    change: &SemanticChange,
    use_after: bool,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<()> {
    match change {
        SemanticChange::AuxiliaryJson {
            table,
            key,
            before,
            after,
            resource,
        } => {
            use rusqlite::OptionalExtension;
            let (table, key_column) = auxiliary_json_spec(table)?;
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = transaction
                .query_row(
                    &format!("SELECT value_json FROM {table} WHERE {key_column} = ?1"),
                    [key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because {table}/{key} changed"
                )));
            }
            if let Some(value) = replacement {
                transaction.execute(
                    &format!(
                        "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
                         ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
                    ),
                    rusqlite::params![key, value],
                )?;
            } else {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE {key_column} = ?1"),
                    [key],
                )?;
            }
            resources.insert((*resource).into());
        }
        SemanticChange::Bitmap { key, before, after } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            // Replay only the roots this entry changed. Edits to other roots
            // since then (background AI tags, imports, another selection) do
            // not block undo; a later change to these same roots does.
            let changed = expected ^ replacement;
            let current = projection_bitmap(snapshot, *key);
            if (&current & &changed) != (expected & &changed) {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because {:?}/{} changed",
                    key.domain, key.key_id
                )));
            }
            let mut next = &current - &changed;
            next |= &(replacement & &changed);
            bitmap::replace_changed(transaction, revision, *key, &next, &changed)?;
            set_projection_bitmap(snapshot, *key, next);
            *affected |= &changed;
            resources.insert("roots".into());
            match key.domain {
                BitmapDomain::Tag => {
                    let counts = Arc::make_mut(&mut snapshot.tag_count);
                    for root_id in &changed {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(
                            root_id,
                            if replacement.contains(root_id) {
                                current + 1
                            } else {
                                current.saturating_sub(1)
                            },
                        );
                    }
                    resources.insert("tags".into());
                }
                BitmapDomain::Lifecycle => {
                    resources.insert("sidebar".into());
                    resources.insert("folders".into());
                    resources.insert("tags".into());
                }
                BitmapDomain::Rating => {
                    resources.insert("ratings".into());
                }
            }
        }
        SemanticChange::Order {
            owner_kind,
            owner_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            match owner_kind {
                OrderOwnerKind::Folder => {
                    let folder_id = FolderId(*owner_id);
                    let current = snapshot
                        .folder_orders
                        .get(&folder_id)
                        .map(|roots| roots.iter().map(|root| root.0).collect::<Vec<_>>())
                        .unwrap_or_default();
                    if &current != expected {
                        return Err(LibraryError::InvalidState(format!(
                            "cannot replay history because folder {owner_id} changed"
                        )));
                    }
                    ordering::replace(transaction, revision, *owner_kind, *owner_id, replacement)?;
                    let old_bitmap = expected.iter().copied().collect::<RoaringBitmap>();
                    let new_bitmap = replacement.iter().copied().collect::<RoaringBitmap>();
                    let changed = &old_bitmap ^ &new_bitmap;
                    Arc::make_mut(&mut snapshot.folder_orders).insert(
                        folder_id,
                        Arc::new(replacement.iter().copied().map(RootId).collect()),
                    );
                    Arc::make_mut(&mut snapshot.folders)
                        .insert(folder_id, new_bitmap.clone().into());
                    let counts = Arc::make_mut(&mut snapshot.folder_count);
                    for root_id in &changed {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(
                            root_id,
                            if new_bitmap.contains(root_id) {
                                current + 1
                            } else {
                                current.saturating_sub(1)
                            },
                        );
                    }
                    *affected |= &changed;
                    resources.insert("roots".into());
                    resources.insert("folders".into());
                    resources.insert("sidebar".into());
                }
                OrderOwnerKind::Collection => {
                    let root_id = RootId(*owner_id);
                    let current = snapshot
                        .collection_orders
                        .get(&root_id)
                        .map(|members| members.iter().map(|media| media.0).collect::<Vec<_>>())
                        .unwrap_or_default();
                    if &current != expected {
                        return Err(LibraryError::InvalidState(format!(
                            "cannot replay history because collection {owner_id} changed"
                        )));
                    }
                    let old_members = expected.iter().copied().collect::<RoaringBitmap>();
                    let new_members = replacement.iter().copied().collect::<RoaringBitmap>();
                    if old_members != new_members {
                        return Err(LibraryError::InvalidState(
                            "structural collection history requires a structural snapshot".into(),
                        ));
                    }
                    ordering::replace(transaction, revision, *owner_kind, *owner_id, replacement)?;
                    Arc::make_mut(&mut snapshot.collection_orders).insert(
                        root_id,
                        Arc::new(replacement.iter().copied().map(crate::MediaId).collect()),
                    );
                    affected.insert(root_id.0);
                    resources.insert("roots".into());
                    resources.insert("collections".into());
                }
            }
        }
        SemanticChange::RootText { before, after } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            if expected.len() != replacement.len() {
                return Err(LibraryError::InvalidState(
                    "text history root sets do not match".into(),
                ));
            }
            for (expected, replacement) in expected.iter().zip(replacement.iter()) {
                if expected.root_id != replacement.root_id {
                    return Err(LibraryError::InvalidState(
                        "text history root order does not match".into(),
                    ));
                }
                let current = transaction.query_row(
                    "SELECT name, notes, source_urls_json, modified_at_ms
                     FROM library_root WHERE root_id = ?1",
                    [expected.root_id.0],
                    |row| {
                        Ok(crate::history::RootTextState {
                            root_id: expected.root_id,
                            name: row.get(0)?,
                            notes: row.get(1)?,
                            source_urls: serde_json::from_str(&row.get::<_, String>(2)?)
                                .unwrap_or_default(),
                            modified_at_ms: row.get(3)?,
                        })
                    },
                )?;
                if current != *expected {
                    return Err(LibraryError::InvalidState(format!(
                        "cannot replay history because root {} text changed",
                        expected.root_id.0
                    )));
                }
                transaction.execute(
                    "UPDATE library_root
                     SET name = ?2, notes = ?3, source_urls_json = ?4, modified_at_ms = ?5
                     WHERE root_id = ?1",
                    rusqlite::params![
                        replacement.root_id.0,
                        replacement.name,
                        replacement.notes,
                        serde_json::to_string(&replacement.source_urls)?,
                        replacement.modified_at_ms
                    ],
                )?;
                transaction.execute(
                    "UPDATE media_item SET media_name = ?2
                     WHERE media_id = ?1 AND EXISTS(
                         SELECT 1 FROM library_item
                         WHERE local_id = ?1 AND item_kind = 1
                     )",
                    rusqlite::params![replacement.root_id.0, replacement.name],
                )?;
                crate::fts::mark_one(transaction, replacement.root_id, revision as i64)?;
                Arc::make_mut(&mut snapshot.name_index)
                    .insert(replacement.root_id.0, &replacement.name);
                Arc::make_mut(&mut snapshot.modified_at).insert(
                    replacement.root_id.0,
                    replacement.modified_at_ms.max(0) as u64,
                );
                if replacement.notes.is_some() {
                    Arc::make_mut(&mut snapshot.notes_present).insert(replacement.root_id.0);
                } else {
                    Arc::make_mut(&mut snapshot.notes_present).remove(replacement.root_id.0);
                }
                if replacement.source_urls.is_empty() {
                    Arc::make_mut(&mut snapshot.urls_present).remove(replacement.root_id.0);
                } else {
                    Arc::make_mut(&mut snapshot.urls_present).insert(replacement.root_id.0);
                }
                affected.insert(replacement.root_id.0);
            }
            resources.insert("roots".into());
            resources.insert("search".into());
        }
        SemanticChange::CollectionCover {
            root_id,
            before,
            after,
            before_notes,
            after_notes,
        } => {
            let (expected, replacement, expected_notes, replacement_notes) = if use_after {
                (*before, *after, before_notes, after_notes)
            } else {
                (*after, *before, after_notes, before_notes)
            };
            let (current, current_notes) = transaction
                .query_row(
                    "SELECT cover_media_id, notes FROM library_root WHERE root_id = ?1",
                    [root_id.0],
                    |row| Ok((MediaId(row.get(0)?), row.get::<_, Option<String>>(1)?)),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("collection {root_id}"))
                    }
                    error => error.into(),
                })?;
            if current != expected || &current_notes != expected_notes {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because collection {root_id} cover changed"
                )));
            }
            crate::group::set_cover(transaction, snapshot, *root_id, replacement)?;
            transaction.execute(
                "UPDATE library_root SET notes = ?2 WHERE root_id = ?1",
                rusqlite::params![root_id.0, replacement_notes],
            )?;
            if replacement_notes.is_some() {
                Arc::make_mut(&mut snapshot.notes_present).insert(root_id.0);
            } else {
                Arc::make_mut(&mut snapshot.notes_present).remove(root_id.0);
            }
            affected.insert(root_id.0);
            resources.insert("roots".into());
            resources.insert("collections".into());
        }
        SemanticChange::TagName {
            tag_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = snapshot
                .tag_ids_by_name
                .iter()
                .find_map(|(name, id)| (*id == *tag_id).then_some(name.as_str()))
                .ok_or_else(|| LibraryError::NotFound(format!("tag {}", tag_id.0)))?;
            if current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag {} was renamed",
                    tag_id.0
                )));
            }
            rename_tag_definition(transaction, snapshot, *tag_id, replacement)?;
            resources.insert("tags".into());
            resources.insert("navigation".into());
            resources.insert("smart-folders".into());
        }
        SemanticChange::TagNamespaceName {
            namespace_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            if load_namespace_name(transaction, *namespace_id)? != *expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag namespace {} was renamed",
                    namespace_id.0
                )));
            }
            apply_namespace_name(transaction, snapshot, *namespace_id, replacement)?;
            resources.insert("tags".into());
            resources.insert("navigation".into());
        }
        SemanticChange::TagNamespaceDefinition { before, after } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let namespace_id = expected
                .as_ref()
                .or(replacement.as_ref())
                .expect("namespace definition history has one state")
                .namespace_id;
            if load_namespace_definition(transaction, namespace_id)? != *expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag namespace {} changed",
                    namespace_id.0
                )));
            }
            if let Some(state) = replacement {
                transaction.execute(
                    "INSERT INTO tag_namespace(namespace_id, stable_key, display_name)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(namespace_id) DO UPDATE SET
                         stable_key = excluded.stable_key,
                         display_name = excluded.display_name",
                    rusqlite::params![state.namespace_id.0, state.stable_key, state.display_name,],
                )?;
            } else {
                transaction.execute(
                    "DELETE FROM tag_namespace WHERE namespace_id = ?1",
                    [namespace_id.0],
                )?;
            }
            resources.insert("tags".into());
            resources.insert("navigation".into());
        }
        SemanticChange::TagDefinition {
            before,
            after,
            before_members,
            after_members,
            queries,
        } => {
            let (expected, replacement, expected_members, replacement_members) = if use_after {
                (
                    before,
                    after,
                    before_members.as_ref(),
                    after_members.as_ref(),
                )
            } else {
                (
                    after,
                    before,
                    after_members.as_ref(),
                    before_members.as_ref(),
                )
            };
            let tag_id = expected
                .as_ref()
                .or(replacement.as_ref())
                .expect("tag definition history has one state")
                .tag_id;
            if load_tag_definition(transaction, tag_id)? != *expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag {} definition changed",
                    tag_id.0
                )));
            }
            let current_members = snapshot
                .tags
                .get(&tag_id)
                .map(|members| members.to_bitmap())
                .unwrap_or_default();
            if &current_members != expected_members {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag {} membership changed",
                    tag_id.0
                )));
            }
            for query in queries.iter() {
                let expected_query = if use_after {
                    &query.before
                } else {
                    &query.after
                };
                if snapshot.smart_queries.get(&query.smart_folder_id.0) != Some(expected_query) {
                    return Err(LibraryError::InvalidState(format!(
                        "cannot replay history because smart folder {} changed",
                        query.smart_folder_id.0
                    )));
                }
            }

            if let Some(state) = replacement {
                transaction.execute(
                    "INSERT INTO tag_definition
                         (tag_id, stable_key, namespace_id, subname)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(tag_id) DO UPDATE SET
                         stable_key = excluded.stable_key,
                         namespace_id = excluded.namespace_id,
                         subname = excluded.subname",
                    rusqlite::params![
                        state.tag_id.0,
                        state.stable_key,
                        state.namespace_id,
                        tag_subname(&state.full_name)
                    ],
                )?;
            } else {
                transaction.execute("DELETE FROM tag_definition WHERE tag_id = ?1", [tag_id.0])?;
            }
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                },
                replacement_members,
            )?;
            let names = Arc::make_mut(&mut snapshot.tag_ids_by_name);
            names.retain(|_, id| *id != tag_id);
            if let Some(state) = replacement {
                names.insert(state.full_name.clone(), tag_id);
                Arc::make_mut(&mut snapshot.tags)
                    .insert(tag_id, replacement_members.clone().into());
            } else {
                Arc::make_mut(&mut snapshot.tags).remove(&tag_id);
            }
            let changed = expected_members ^ replacement_members;
            let counts = Arc::make_mut(&mut snapshot.tag_count);
            for root_id in &changed {
                let current = counts.value(root_id).unwrap_or(0);
                counts.insert(
                    root_id,
                    if replacement_members.contains(root_id) {
                        current + 1
                    } else {
                        current.saturating_sub(1)
                    },
                );
            }
            for query in queries.iter() {
                let replacement_query = if use_after {
                    &query.after
                } else {
                    &query.before
                };
                transaction.execute(
                    "UPDATE smart_folder_definition SET view_query_json = ?2
                     WHERE smart_folder_id = ?1",
                    rusqlite::params![
                        query.smart_folder_id.0,
                        serde_json::to_string(replacement_query)?
                    ],
                )?;
                Arc::make_mut(&mut snapshot.smart_queries)
                    .insert(query.smart_folder_id.0, replacement_query.clone());
            }
            if !queries.is_empty() {
                crate::smart::refresh_all(transaction, snapshot)?;
            }
            *affected |= &changed;
            if !queries.is_empty() {
                *affected |= snapshot.active();
            }
            resources.insert("roots".into());
            resources.insert("tags".into());
            resources.insert("navigation".into());
            resources.insert("smart-folders".into());
        }
        SemanticChange::FolderAutoTags {
            folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            let current = ingest::folder_auto_tags(transaction, *folder_id)?;
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because folder {} auto-tags changed",
                    folder_id.0
                )));
            }
            transaction.execute(
                "UPDATE folder_definition SET auto_tag_ids = ?2 WHERE folder_id = ?1",
                rusqlite::params![folder_id.0, ingest::encode_folder_auto_tags(replacement)?],
            )?;
            resources.insert("folders".into());
        }
        SemanticChange::FolderName {
            folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = transaction
                .query_row(
                    "SELECT name FROM folder_definition WHERE folder_id = ?1",
                    [folder_id.0],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("folder {}", folder_id.0))
                    }
                    error => error.into(),
                })?;
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because folder {} was renamed",
                    folder_id.0
                )));
            }
            rename_folder_definition(transaction, *folder_id, replacement)?;
            resources.insert("folders".into());
            resources.insert("navigation".into());
        }
        SemanticChange::FolderDefinition {
            folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = load_folder_definition(transaction, *folder_id)?;
            if current.as_ref() != expected.as_deref() {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because folder {} changed",
                    folder_id.0
                )));
            }
            match replacement {
                Some(folder) => {
                    transaction.execute(
                        "INSERT INTO folder_definition
                             (folder_id, stable_key, parent_id, name, icon, color, notes,
                              auto_tag_ids, cover_root_id, watch_path, watch_enabled,
                              watch_subfolders, display_order, watch_lifecycle)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                         ON CONFLICT(folder_id) DO UPDATE SET
                             stable_key = excluded.stable_key,
                             parent_id = excluded.parent_id,
                             name = excluded.name,
                             icon = excluded.icon,
                             color = excluded.color,
                             notes = excluded.notes,
                             auto_tag_ids = excluded.auto_tag_ids,
                             cover_root_id = excluded.cover_root_id,
                             watch_path = excluded.watch_path,
                             watch_enabled = excluded.watch_enabled,
                             watch_subfolders = excluded.watch_subfolders,
                             display_order = excluded.display_order,
                             watch_lifecycle = excluded.watch_lifecycle",
                        rusqlite::params![
                            folder.folder_id.0,
                            folder.stable_key,
                            folder.parent_id.map(|id| id.0),
                            folder.name,
                            folder.icon,
                            folder.color,
                            folder.notes,
                            folder.auto_tag_ids,
                            folder.cover_root_id.map(|id| id.0),
                            folder.watch_path,
                            folder.watch_enabled,
                            folder.watch_subfolders,
                            folder.display_order,
                            folder.watch_lifecycle.watch_code()
                        ],
                    )?;
                    Arc::make_mut(&mut snapshot.folders)
                        .entry(*folder_id)
                        .or_default();
                    Arc::make_mut(&mut snapshot.folder_orders)
                        .entry(*folder_id)
                        .or_insert_with(|| Arc::new(Vec::new()));
                }
                None => {
                    let members = snapshot
                        .folder_orders
                        .get(folder_id)
                        .map_or(0, |order| order.len());
                    if members != 0 {
                        return Err(LibraryError::InvalidState(format!(
                            "cannot remove folder {} with members while replaying history",
                            folder_id.0
                        )));
                    }
                    ordering::delete(transaction, OrderOwnerKind::Folder, folder_id.0)?;
                    transaction.execute(
                        "DELETE FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                    )?;
                    Arc::make_mut(&mut snapshot.folders).remove(folder_id);
                    Arc::make_mut(&mut snapshot.folder_orders).remove(folder_id);
                }
            }
            resources.insert("folders".into());
            resources.insert("navigation".into());
        }
        SemanticChange::SmartFolderDefinition {
            smart_folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = load_smart_folder_definition(transaction, *smart_folder_id)?;
            if current.as_ref() != expected.as_deref() {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because smart folder {} changed",
                    smart_folder_id.0
                )));
            }
            match replacement {
                Some(folder) => {
                    transaction.execute(
                        "INSERT INTO smart_folder_definition
                             (smart_folder_id, stable_key, parent_id, name, icon, color, notes,
                              view_query_json, display_order)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                         ON CONFLICT(smart_folder_id) DO UPDATE SET
                             stable_key = excluded.stable_key,
                             parent_id = excluded.parent_id,
                             name = excluded.name,
                             icon = excluded.icon,
                             color = excluded.color,
                             notes = excluded.notes,
                             view_query_json = excluded.view_query_json,
                             display_order = excluded.display_order",
                        rusqlite::params![
                            folder.smart_folder_id.0,
                            folder.stable_key,
                            folder.parent_id.map(|id| id.0),
                            folder.name,
                            folder.icon,
                            folder.color,
                            folder.notes,
                            serde_json::to_string(&folder.view)?,
                            folder.display_order
                        ],
                    )?;
                    crate::smart::refresh_subtree(transaction, snapshot, *smart_folder_id)?;
                }
                None => {
                    transaction.execute(
                        "DELETE FROM smart_folder_definition WHERE smart_folder_id = ?1",
                        [smart_folder_id.0],
                    )?;
                    crate::smart::remove(snapshot, *smart_folder_id);
                }
            }
            resources.insert("smart-folders".into());
            resources.insert("navigation".into());
        }
        SemanticChange::RecentViews { before, after } => {
            let replacement = if use_after { after } else { before };
            transaction.execute("DELETE FROM recent_view", [])?;
            let mut statement = transaction.prepare_cached(
                "INSERT INTO recent_view (root_id, viewed_at_ms) VALUES (?1, ?2)",
            )?;
            for (root_id, viewed_at_ms) in replacement.iter() {
                statement.execute(rusqlite::params![root_id.0, viewed_at_ms])?;
            }
            resources.insert("recently-viewed".into());
            resources.insert("navigation".into());
        }
        SemanticChange::Structure {
            affected: changed_roots,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            restore_structure(
                transaction,
                revision,
                snapshot,
                changed_roots,
                expected,
                replacement,
            )?;
            *affected |= changed_roots.as_ref();
            resources.extend([
                "roots".into(),
                "collections".into(),
                "tags".into(),
                "folders".into(),
                "sidebar".into(),
                "search".into(),
            ]);
        }
        SemanticChange::DuplicateResolution(state) => {
            let changed =
                crate::duplicate::replay(transaction, revision, snapshot, state, use_after)?;
            *affected |= &changed;
            resources.insert("duplicates".into());
            if state.rewires_file() {
                resources.insert("roots".into());
                resources.insert("media".into());
                resources.insert("smart-folders".into());
            }
            if state.changes_names() {
                resources.insert("search".into());
            }
        }
        SemanticChange::Compound(changes) => {
            if use_after {
                for change in changes {
                    apply_semantic_change(
                        transaction,
                        revision,
                        snapshot,
                        change,
                        use_after,
                        affected,
                        resources,
                    )?;
                }
            } else {
                for change in changes.iter().rev() {
                    apply_semantic_change(
                        transaction,
                        revision,
                        snapshot,
                        change,
                        use_after,
                        affected,
                        resources,
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn load_recent_views(connection: &rusqlite::Connection) -> Result<Vec<(RootId, i64)>> {
    crate::projection::load_recent_views(connection)
}
