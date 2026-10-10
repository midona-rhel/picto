//! Smart folder definitions and their tree.

use super::*;

impl Library {
    pub fn smart_folders(&self) -> Result<Vec<SmartFolderRecord>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::smart::list(connection, &snapshot),
        )
    }

    pub fn create_smart_folder(
        &self,
        input: SmartFolderInput,
    ) -> Result<(SmartFolderId, MutationReceipt)> {
        let name = required_name("smart folder", &input.name)?;
        let parent_id = input.parent_id;
        let icon = normalized_optional(input.icon.as_deref());
        let color = normalized_optional(input.color.as_deref());
        let notes = normalized_optional(input.notes.as_deref());
        let view = input.view;
        crate::smart::validate_view(&view)?;
        let changed_at_ms = now_ms();
        let (smart_folder_id, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            crate::smart::validate_capacity(transaction)?;
            validate_smart_parent(transaction, parent_id, None)?;
            let smart_folder_id = SmartFolderId(LibraryDatabase::allocate_id(transaction)?);
            let display_order = transaction.query_row(
                "SELECT COALESCE(MAX(display_order) + 1, 0)
                 FROM smart_folder_definition WHERE parent_id IS ?1",
                [parent_id.map(|id| id.0)],
                |row| row.get::<_, i64>(0),
            )?;
            transaction.execute(
                "INSERT INTO smart_folder_definition
                     (smart_folder_id, stable_key, parent_id, name, icon, color, notes,
                      view_query_json, display_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    smart_folder_id.0,
                    uuid::Uuid::new_v4().to_string(),
                    parent_id.map(|id| id.0),
                    name,
                    icon,
                    color,
                    notes,
                    serde_json::to_string(&view)?,
                    display_order
                ],
            )?;
            insert_cloud_journal(
                transaction,
                revision,
                "smart_folder.create",
                None,
                serde_json::json!({"smart_folder_id": smart_folder_id.0}),
                changed_at_ms,
            )?;
            let mut next = (*snapshot).clone();
            crate::smart::refresh_subtree(transaction, &mut next, smart_folder_id)?;
            next.revision = revision;
            let after = load_smart_folder_definition(transaction, smart_folder_id)?
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "new smart folder {} disappeared before publication",
                        smart_folder_id.0
                    ))
                })?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["smart-folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                (smart_folder_id, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "smart_folders.create",
                        "Create smart folder",
                        SemanticChange::SmartFolderDefinition {
                            smart_folder_id,
                            before: None,
                            after: Some(Box::new(after)),
                        },
                    )),
                },
            ))
        })?;
        Ok((smart_folder_id, receipt))
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: SmartFolderId,
        input: SmartFolderInput,
    ) -> Result<MutationReceipt> {
        let name = required_name("smart folder", &input.name)?;
        let parent_id = input.parent_id;
        let icon = normalized_optional(input.icon.as_deref());
        let color = normalized_optional(input.color.as_deref());
        let notes = normalized_optional(input.notes.as_deref());
        let view = input.view;
        crate::smart::validate_view(&view)?;
        let changed_at_ms = now_ms();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            validate_smart_parent(transaction, parent_id, Some(smart_folder_id))?;
            let before = load_smart_folder_definition(transaction, smart_folder_id)?
                .ok_or_else(|| {
                    LibraryError::NotFound(format!("smart folder {smart_folder_id}"))
                })?;
            let query_changed = before.parent_id != parent_id || before.view != view;
            if transaction.execute(
                "UPDATE smart_folder_definition
                 SET parent_id = ?2, name = ?3, icon = ?4, color = ?5, notes = ?6,
                     view_query_json = ?7
                 WHERE smart_folder_id = ?1",
                rusqlite::params![
                    smart_folder_id.0,
                    parent_id.map(|id| id.0),
                    name,
                    icon,
                    color,
                    notes,
                    serde_json::to_string(&view)?
                ],
            )? == 0
            {
                return Err(LibraryError::NotFound(format!(
                    "smart folder {}",
                    smart_folder_id.0
                )));
            }
            insert_cloud_journal(
                transaction,
                revision,
                "smart_folder.update",
                None,
                serde_json::json!({"smart_folder_id": smart_folder_id.0}),
                changed_at_ms,
            )?;
            let mut next = (*snapshot).clone();
            if query_changed {
                crate::smart::refresh_subtree(transaction, &mut next, smart_folder_id)?;
            }
            next.revision = revision;
            let after = load_smart_folder_definition(transaction, smart_folder_id)?
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "smart folder {} disappeared during update",
                        smart_folder_id.0
                    ))
                })?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["smart-folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (before != after).then(|| {
                        HistoryEntry::for_command(
                            "smart_folders.update",
                            "Update smart folder",
                            SemanticChange::SmartFolderDefinition {
                                smart_folder_id,
                                before: Some(Box::new(before)),
                                after: Some(Box::new(after)),
                            },
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn delete_smart_folder(
        &self,
        smart_folder_id: SmartFolderId,
    ) -> Result<SmartFolderDeleteResult> {
        let changed_at_ms = now_ms();
        let result = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let mut definitions = load_smart_folder_subtree(transaction, smart_folder_id)?;
            let fallback_smart_folder_id = definitions
                .iter()
                .find(|(definition, _)| definition.smart_folder_id == smart_folder_id)
                .and_then(|(definition, _)| definition.parent_id);
            definitions.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
            let mut next = (*snapshot).clone();
            let mut changes = Vec::with_capacity(definitions.len());
            for (definition, _) in &definitions {
                transaction.execute(
                    "DELETE FROM smart_folder_definition WHERE smart_folder_id = ?1",
                    [definition.smart_folder_id.0],
                )?;
                crate::smart::remove(&mut next, definition.smart_folder_id);
                changes.push(SemanticChange::SmartFolderDefinition {
                    smart_folder_id: definition.smart_folder_id,
                    before: Some(Box::new(definition.clone())),
                    after: None,
                });
            }
            insert_cloud_journal(
                transaction,
                revision,
                "smart_folder.delete",
                None,
                serde_json::json!({
                    "smart_folder_ids": definitions.iter()
                        .map(|(definition, _)| definition.smart_folder_id.0)
                        .collect::<Vec<_>>()
                }),
                changed_at_ms,
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["smart-folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                SmartFolderDeleteResult {
                    deleted_smart_folder_ids: definitions
                        .iter()
                        .map(|(definition, _)| definition.smart_folder_id)
                        .collect(),
                    fallback_smart_folder_id,
                    receipt: receipt.clone(),
                },
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "smart_folders.delete",
                        "Delete smart folder",
                        SemanticChange::Compound(changes),
                    )),
                },
            ))
        })?;
        Ok(result)
    }

    pub fn reorder_smart_folder_children(
        &self,
        parent_id: Option<SmartFolderId>,
        smart_folder_ids: &[SmartFolderId],
    ) -> Result<MutationReceipt> {
        let requested = smart_folder_ids.iter().map(|id| id.0).collect::<Vec<_>>();
        if requested.iter().copied().collect::<HashSet<_>>().len() != requested.len() {
            return Err(LibraryError::InvalidInput(
                "smart-folder reorder contains duplicate IDs".into(),
            ));
        }
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            validate_smart_parent(transaction, parent_id, None)?;
            let mut statement = transaction.prepare(
                "SELECT smart_folder_id FROM smart_folder_definition
                 WHERE parent_id IS ?1 ORDER BY display_order, smart_folder_id",
            )?;
            let current = statement
                .query_map([parent_id.map(|id| id.0)], |row| {
                    row.get::<_, u32>(0).map(SmartFolderId)
                })?
                .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
            if current.iter().map(|id| id.0).collect::<BTreeSet<_>>()
                != requested.iter().copied().collect::<BTreeSet<_>>()
                || current.len() != requested.len()
            {
                return Err(LibraryError::InvalidInput(
                    "smart-folder reorder must contain every sibling exactly once".into(),
                ));
            }
            let before = current
                .iter()
                .map(|id| {
                    load_smart_folder_definition(transaction, *id)?.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "smart folder {} disappeared during reorder",
                            id.0
                        ))
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut update = transaction.prepare_cached(
                "UPDATE smart_folder_definition SET display_order = ?2
                 WHERE smart_folder_id = ?1",
            )?;
            for (display_order, id) in smart_folder_ids.iter().enumerate() {
                update.execute(rusqlite::params![id.0, display_order as i64])?;
            }
            let mut after_by_id = HashMap::new();
            for id in smart_folder_ids {
                let after =
                    load_smart_folder_definition(transaction, *id)?.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "smart folder {} disappeared during reorder",
                            id.0
                        ))
                    })?;
                after_by_id.insert(*id, after);
            }
            let changes = before
                .into_iter()
                .filter_map(|before| {
                    let after = after_by_id.remove(&before.smart_folder_id)?;
                    (before != after).then_some(SemanticChange::SmartFolderDefinition {
                        smart_folder_id: before.smart_folder_id,
                        before: Some(Box::new(before)),
                        after: Some(Box::new(after)),
                    })
                })
                .collect::<Vec<_>>();
            insert_cloud_journal(
                transaction,
                revision,
                "smart_folder.reorder",
                None,
                serde_json::json!({"parent_id": parent_id.map(|id| id.0)}),
                now_ms(),
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["smart-folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (!changes.is_empty()).then(|| {
                        HistoryEntry::for_command(
                            "smart_folders.reorder",
                            "Reorder smart folders",
                            SemanticChange::Compound(changes),
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }
}

pub(super) fn validate_smart_parent(
    transaction: &rusqlite::Transaction<'_>,
    parent_id: Option<SmartFolderId>,
    child_id: Option<SmartFolderId>,
) -> Result<()> {
    if parent_id.is_some() && parent_id == child_id {
        return Err(LibraryError::InvalidInput(
            "a smart folder cannot be its own parent".into(),
        ));
    }
    if let Some(parent_id) = parent_id {
        let exists = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM smart_folder_definition WHERE smart_folder_id = ?1
             )",
            [parent_id.0],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(LibraryError::NotFound(format!(
                "smart folder {}",
                parent_id.0
            )));
        }
        if let Some(child_id) = child_id {
            let creates_cycle = transaction.query_row(
                "WITH RECURSIVE descendants(smart_folder_id) AS (
                     SELECT smart_folder_id FROM smart_folder_definition WHERE parent_id = ?1
                     UNION ALL
                     SELECT child.smart_folder_id
                     FROM smart_folder_definition child
                     JOIN descendants parent ON child.parent_id = parent.smart_folder_id
                 )
                 SELECT EXISTS(
                     SELECT 1 FROM descendants WHERE smart_folder_id = ?2
                 )",
                rusqlite::params![child_id.0, parent_id.0],
                |row| row.get::<_, bool>(0),
            )?;
            if creates_cycle {
                return Err(LibraryError::InvalidInput(
                    "a smart folder cannot move below its descendant".into(),
                ));
            }
        }
    }
    let parent_depth = if let Some(parent_id) = parent_id {
        transaction.query_row(
            "WITH RECURSIVE ancestors(smart_folder_id, parent_id, depth) AS (
                 SELECT smart_folder_id, parent_id, 1
                 FROM smart_folder_definition WHERE smart_folder_id = ?1
                 UNION ALL
                 SELECT parent.smart_folder_id, parent.parent_id, child.depth + 1
                 FROM smart_folder_definition parent
                 JOIN ancestors child ON parent.smart_folder_id = child.parent_id
             )
             SELECT COALESCE(MAX(depth), 0) FROM ancestors",
            [parent_id.0],
            |row| row.get::<_, u32>(0),
        )? as usize
    } else {
        0
    };
    let subtree_height = if let Some(child_id) = child_id {
        transaction.query_row(
            "WITH RECURSIVE descendants(smart_folder_id, depth) AS (
                 SELECT smart_folder_id, 1 FROM smart_folder_definition
                 WHERE smart_folder_id = ?1
                 UNION ALL
                 SELECT child.smart_folder_id, parent.depth + 1
                 FROM smart_folder_definition child
                 JOIN descendants parent ON child.parent_id = parent.smart_folder_id
             )
             SELECT COALESCE(MAX(depth), 1) FROM descendants",
            [child_id.0],
            |row| row.get::<_, u32>(0),
        )? as usize
    } else {
        1
    };
    if parent_depth + subtree_height > crate::smart::MAX_SMART_FOLDER_DEPTH {
        return Err(LibraryError::InvalidInput(format!(
            "smart folders may be nested at most {} levels deep",
            crate::smart::MAX_SMART_FOLDER_DEPTH
        )));
    }
    Ok(())
}

pub(super) fn load_smart_folder_definition(
    connection: &rusqlite::Connection,
    smart_folder_id: SmartFolderId,
) -> Result<Option<SmartFolderDefinitionState>> {
    use rusqlite::OptionalExtension;

    let row = connection
        .query_row(
            "SELECT stable_key, parent_id, name, icon, color, notes, view_query_json,
                    display_order
             FROM smart_folder_definition WHERE smart_folder_id = ?1",
            [smart_folder_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<u32>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(stable_key, parent_id, name, icon, color, notes, view_json, display_order)| {
            Ok(SmartFolderDefinitionState {
                smart_folder_id,
                stable_key,
                parent_id: parent_id.map(SmartFolderId),
                name,
                icon,
                color,
                notes,
                view: serde_json::from_str(&view_json)?,
                display_order,
            })
        },
    )
    .transpose()
}

pub(super) fn load_smart_folder_subtree(
    connection: &rusqlite::Connection,
    smart_folder_id: SmartFolderId,
) -> Result<Vec<(SmartFolderDefinitionState, u32)>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE subtree(smart_folder_id, depth) AS (
             SELECT smart_folder_id, 0 FROM smart_folder_definition
             WHERE smart_folder_id = ?1
             UNION ALL
             SELECT child.smart_folder_id, parent.depth + 1
             FROM smart_folder_definition child
             JOIN subtree parent ON child.parent_id = parent.smart_folder_id
         )
         SELECT smart_folder_id, depth FROM subtree ORDER BY smart_folder_id",
    )?;
    let ids = statement
        .query_map([smart_folder_id.0], |row| {
            Ok((SmartFolderId(row.get(0)?), row.get::<_, u32>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    if ids.is_empty() {
        return Err(LibraryError::NotFound(format!(
            "smart folder {smart_folder_id}"
        )));
    }
    ids.into_iter()
        .map(|(id, depth)| {
            load_smart_folder_definition(connection, id)?
                .map(|definition| (definition, depth))
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "smart folder {} disappeared during subtree load",
                        id.0
                    ))
                })
        })
        .collect()
}
