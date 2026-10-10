//! Folders: the tree, ordered membership, covers, watches and auto-tags.

use super::*;

impl Library {
    pub fn folders(&self) -> Result<Vec<FolderRecord>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| list_folders(connection, &snapshot),
        )
    }

    pub fn navigation(&self) -> Result<(Vec<FolderRecord>, Vec<SmartFolderRecord>, u64)> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                Ok((
                    list_folders(connection, &snapshot)?,
                    crate::smart::list(connection, &snapshot)?,
                    snapshot.revision,
                ))
            },
        )
    }

    pub fn folder_auto_tags(&self, folder_id: FolderId) -> Result<Vec<String>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                require_folder(connection, folder_id)?;
                let ids = ingest::folder_auto_tags(connection, folder_id)?;
                let names_by_id = snapshot
                    .tag_ids_by_name
                    .iter()
                    .map(|(name, tag_id)| (*tag_id, name.as_str()))
                    .collect::<HashMap<_, _>>();
                ids.iter()
                    .map(|tag_id| {
                        names_by_id.get(&crate::TagId(tag_id)).map_or_else(
                            || {
                                Err(LibraryError::InvalidState(format!(
                                    "folder {} references missing auto-tag {tag_id}",
                                    folder_id.0
                                )))
                            },
                            |name| Ok((*name).to_owned()),
                        )
                    })
                    .collect()
            },
        )
    }

    pub fn create_folder(
        &self,
        name: &str,
        parent_id: Option<FolderId>,
    ) -> Result<(FolderId, MutationReceipt)> {
        self.create_folder_with_stable_key(name, parent_id, uuid::Uuid::new_v4().to_string())
    }

    pub fn create_folder_with_stable_key(
        &self,
        name: &str,
        parent_id: Option<FolderId>,
        stable_key: String,
    ) -> Result<(FolderId, MutationReceipt)> {
        if name.trim().is_empty() {
            return Err(LibraryError::InvalidInput("folder name is empty".into()));
        }
        if stable_key.trim().is_empty() {
            return Err(LibraryError::InvalidInput(
                "folder stable key is empty".into(),
            ));
        }
        let name = name.trim().to_owned();
        let changed_at_ms = now_ms();
        let (folder_id, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            validate_folder_parent(transaction, parent_id, None)?;
            let folder_id = FolderId(LibraryDatabase::allocate_id(transaction)?);
            let display_order = transaction.query_row(
                "SELECT COALESCE(MAX(display_order) + 1, 0)
                 FROM folder_definition WHERE parent_id IS ?1",
                [parent_id.map(|id| id.0)],
                |row| row.get::<_, i64>(0),
            )?;
            transaction.execute(
                "INSERT INTO folder_definition
                     (folder_id, stable_key, parent_id, name, display_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    folder_id.0,
                    stable_key,
                    parent_id.map(|id| id.0),
                    name,
                    display_order
                ],
            )?;
            insert_cloud_journal(
                transaction,
                revision,
                "folder.create",
                None,
                serde_json::json!({
                    "folder_id": folder_id.0,
                    "parent_id": parent_id.map(|id| id.0),
                    "name": name,
                }),
                changed_at_ms,
            )?;
            let mut next = (*snapshot).clone();
            Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(Vec::new()));
            Arc::make_mut(&mut next.folders).insert(folder_id, RoaringBitmap::new().into());
            next.revision = revision;
            let after = load_folder_definition(transaction, folder_id)?.ok_or_else(|| {
                LibraryError::InvalidState(format!(
                    "new folder {} disappeared before publication",
                    folder_id.0
                ))
            })?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                (folder_id, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "folders.create",
                        "Create folder",
                        SemanticChange::FolderDefinition {
                            folder_id,
                            before: None,
                            after: Some(Box::new(after)),
                        },
                    )),
                },
            ))
        })?;
        Ok((folder_id, receipt))
    }

    pub fn duplicate_folder(&self, source_id: FolderId) -> Result<(FolderId, MutationReceipt)> {
        let (duplicate_id, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let source = load_folder_definition(transaction, source_id)?
                .ok_or_else(|| LibraryError::NotFound(format!("folder {source_id}")))?;
            let duplicate_name = unique_folder_copy_name(
                transaction,
                source.parent_id,
                &format!("{} copy", source.name),
            )?;
            let root_display_order = transaction.query_row(
                "SELECT COALESCE(MAX(display_order) + 1, 0)
                 FROM folder_definition WHERE parent_id IS ?1",
                [source.parent_id.map(|id| id.0)],
                |row| row.get::<_, i64>(0),
            )?;
            let rows = load_folder_clone_rows(transaction, source_id)?;
            validate_folder_nesting(
                transaction,
                source.parent_id,
                folder_subtree_height(transaction, source_id)?,
            )?;
            let mut replacements = HashMap::<FolderId, FolderId>::new();
            let mut changes = Vec::with_capacity(rows.len());
            let mut next = (*snapshot).clone();
            let mut duplicate_id = None;
            for row in rows {
                let folder_id = FolderId(LibraryDatabase::allocate_id(transaction)?);
                let parent_id = if row.folder_id == source_id {
                    source.parent_id
                } else {
                    row.parent_id.and_then(|id| replacements.get(&id).copied())
                };
                let name = if row.folder_id == source_id {
                    duplicate_name.clone()
                } else {
                    row.name.clone()
                };
                let display_order = if row.folder_id == source_id {
                    root_display_order
                } else {
                    row.display_order
                };
                transaction.execute(
                    "INSERT INTO folder_definition
                         (folder_id, stable_key, parent_id, name, icon, color, notes,
                          auto_tag_ids, display_order)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        folder_id.0,
                        uuid::Uuid::new_v4().to_string(),
                        parent_id.map(|id| id.0),
                        name,
                        row.icon,
                        row.color,
                        row.notes,
                        row.auto_tag_ids,
                        display_order,
                    ],
                )?;
                let after =
                    load_folder_definition(transaction, folder_id)?.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "duplicated folder {} disappeared",
                            folder_id.0
                        ))
                    })?;
                changes.push(SemanticChange::FolderDefinition {
                    folder_id,
                    before: None,
                    after: Some(Box::new(after)),
                });
                replacements.insert(row.folder_id, folder_id);
                Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(Vec::new()));
                Arc::make_mut(&mut next.folders).insert(folder_id, RoaringBitmap::new().into());
                if row.folder_id == source_id {
                    duplicate_id = Some(folder_id);
                }
            }
            let duplicate_id = duplicate_id.ok_or_else(|| {
                LibraryError::InvalidState("folder clone produced no root".into())
            })?;
            insert_cloud_journal(
                transaction,
                revision,
                "folder.duplicate",
                None,
                serde_json::json!({"source_id": source_id.0, "folder_id": duplicate_id.0}),
                now_ms(),
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                (duplicate_id, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "folders.duplicate",
                        "Duplicate folder",
                        SemanticChange::Compound(changes),
                    )),
                },
            ))
        })?;
        Ok((duplicate_id, receipt))
    }

    pub fn set_folder_metadata(
        &self,
        folder_id: FolderId,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
    ) -> Result<MutationReceipt> {
        let icon = normalized_optional(icon);
        let color = normalized_optional(color);
        let notes = normalized_optional(notes);
        self.update_folder_definition(
            "folders.set_metadata",
            "Edit folder",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition SET icon = ?2, color = ?3, notes = ?4
                 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, icon, color, notes],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn folder_cover(&self, folder_id: FolderId) -> Result<Option<crate::FolderCover>> {
        use rusqlite::OptionalExtension;

        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let cover_root_id = connection
                    .query_row(
                        "SELECT cover_root_id FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                        |row| row.get::<_, Option<u32>>(0),
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LibraryError::NotFound(format!("folder {folder_id}"))
                        }
                        error => error.into(),
                    })?;
                let Some(root_id) = cover_root_id.map(RootId) else {
                    return Ok(None);
                };
                if !snapshot
                    .folders
                    .get(&folder_id)
                    .is_some_and(|members| members.contains(root_id.0))
                {
                    return Ok(None);
                }
                connection
                    .query_row(
                        "SELECT file.content_hash, file.mime
                         FROM library_root root
                         JOIN media_item media ON media.media_id = root.cover_media_id
                         JOIN media_file file ON file.file_id = media.file_id
                         WHERE root.root_id = ?1",
                        [root_id.0],
                        |row| {
                            Ok(crate::FolderCover {
                                root_id,
                                content_hash: row.get(0)?,
                                mime: row.get(1)?,
                            })
                        },
                    )
                    .optional()
                    .map_err(Into::into)
            },
        )
    }

    pub fn set_folder_cover(
        &self,
        folder_id: FolderId,
        root_id: RootId,
    ) -> Result<MutationReceipt> {
        let snapshot = self.projections.snapshot();
        if !snapshot
            .folders
            .get(&folder_id)
            .is_some_and(|members| members.contains(root_id.0))
        {
            return Err(LibraryError::InvalidInput(
                "folder cover root must belong to the folder".into(),
            ));
        }
        drop(snapshot);
        self.update_folder_definition(
            "folders.set_cover",
            "Set folder cover",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition SET cover_root_id = ?2 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, root_id.0],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn set_folder_watch(
        &self,
        folder_id: FolderId,
        path: &str,
        include_subfolders: bool,
        lifecycle: Lifecycle,
    ) -> Result<MutationReceipt> {
        let path = required_name("watch path", path)?;
        if lifecycle == Lifecycle::Trash {
            return Err(LibraryError::InvalidInput(
                "watched folders import into Inbox or Active".into(),
            ));
        }
        self.update_folder_definition(
            "folders.set_watch",
            "Set watched folder",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition
                     SET watch_path = ?2, watch_enabled = 1, watch_subfolders = ?3,
                         watch_lifecycle = ?4
                     WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, path, include_subfolders, lifecycle.watch_code()],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn clear_folder_watch(&self, folder_id: FolderId) -> Result<MutationReceipt> {
        self.update_folder_definition(
            "folders.clear_watch",
            "Clear watched folder",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition
                     SET watch_path = NULL, watch_enabled = 0, watch_subfolders = 0
                     WHERE folder_id = ?1",
                    [folder_id.0],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn move_folder(
        &self,
        folder_id: FolderId,
        parent_id: Option<FolderId>,
    ) -> Result<MutationReceipt> {
        self.update_folder_definition(
            "folders.move",
            "Move folder",
            folder_id,
            move |transaction| {
                validate_folder_parent(transaction, parent_id, Some(folder_id))?;
                let display_order = transaction.query_row(
                    "SELECT COALESCE(MAX(display_order) + 1, 0)
                 FROM folder_definition WHERE parent_id IS ?1 AND folder_id != ?2",
                    rusqlite::params![parent_id.map(|id| id.0), folder_id.0],
                    |row| row.get::<_, i64>(0),
                )?;
                transaction.execute(
                    "UPDATE folder_definition SET parent_id = ?2, display_order = ?3
                 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, parent_id.map(|id| id.0), display_order],
                )?;
                Ok(())
            },
        )
    }

    pub fn folder_child_capacity(&self, parent_id: Option<FolderId>) -> Result<usize> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            Ok(crate::model::MAX_FOLDER_DEPTH - folder_depth(connection, parent_id)?)
        })
    }

    pub fn reorder_folder_children(
        &self,
        parent_id: Option<FolderId>,
        folder_ids: &[FolderId],
    ) -> Result<MutationReceipt> {
        let requested = folder_ids
            .iter()
            .map(|folder_id| folder_id.0)
            .collect::<Vec<_>>();
        if requested.iter().copied().collect::<HashSet<_>>().len() != requested.len() {
            return Err(LibraryError::InvalidInput(
                "folder reorder contains duplicate IDs".into(),
            ));
        }
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            validate_folder_parent(transaction, parent_id, None)?;
            let current = load_folder_children(transaction, parent_id)?;
            let current_set = current
                .iter()
                .map(|folder| folder.0)
                .collect::<BTreeSet<_>>();
            let requested_set = requested.iter().copied().collect::<BTreeSet<_>>();
            if current_set != requested_set || current.len() != requested.len() {
                return Err(LibraryError::InvalidInput(
                    "folder reorder must contain every sibling exactly once".into(),
                ));
            }
            let before = current
                .iter()
                .map(|folder_id| {
                    load_folder_definition(transaction, *folder_id)?.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "folder {} disappeared during reorder",
                            folder_id.0
                        ))
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut update = transaction.prepare_cached(
                "UPDATE folder_definition SET display_order = ?2 WHERE folder_id = ?1",
            )?;
            for (display_order, folder_id) in folder_ids.iter().enumerate() {
                update.execute(rusqlite::params![folder_id.0, display_order as i64])?;
            }
            let after = folder_ids
                .iter()
                .map(|folder_id| {
                    load_folder_definition(transaction, *folder_id)?.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "folder {} disappeared during reorder",
                            folder_id.0
                        ))
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut after_by_id = after
                .into_iter()
                .map(|state| (state.folder_id, state))
                .collect::<HashMap<_, _>>();
            let changes = before
                .into_iter()
                .filter_map(|before| {
                    let after = after_by_id.remove(&before.folder_id)?;
                    (before != after).then_some(SemanticChange::FolderDefinition {
                        folder_id: before.folder_id,
                        before: Some(Box::new(before)),
                        after: Some(Box::new(after)),
                    })
                })
                .collect::<Vec<_>>();
            insert_cloud_journal(
                transaction,
                revision,
                "folder.reorder",
                None,
                serde_json::json!({"parent_id": parent_id.map(|id| id.0)}),
                now_ms(),
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (!changes.is_empty()).then(|| {
                        HistoryEntry::for_command(
                            "folders.reorder",
                            "Reorder folders",
                            SemanticChange::Compound(changes),
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn sort_folder_tree(
        &self,
        folder_id: FolderId,
        descending: bool,
        recursive: bool,
    ) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            require_folder(transaction, folder_id)?;
            let parent_ids = if recursive {
                load_folder_subtree(transaction, &[folder_id])?
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>()
            } else {
                vec![folder_id]
            };
            let mut changes = Vec::new();
            let mut update = transaction.prepare_cached(
                "UPDATE folder_definition SET display_order = ?2 WHERE folder_id = ?1",
            )?;
            for parent_id in parent_ids {
                let children = load_folder_children(transaction, Some(parent_id))?;
                let mut states = children
                    .iter()
                    .map(|id| {
                        load_folder_definition(transaction, *id)?.ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "folder {} disappeared during sort",
                                id.0
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                states.sort_by(|left, right| {
                    let ordering = left
                        .name
                        .to_lowercase()
                        .cmp(&right.name.to_lowercase())
                        .then_with(|| left.folder_id.cmp(&right.folder_id));
                    if descending {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
                for (display_order, before) in states.into_iter().enumerate() {
                    if before.display_order == display_order as i64 {
                        continue;
                    }
                    update
                        .execute(rusqlite::params![before.folder_id.0, display_order as i64])?;
                    let after = load_folder_definition(transaction, before.folder_id)?
                        .ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "folder {} disappeared during sort",
                                before.folder_id.0
                            ))
                        })?;
                    changes.push(SemanticChange::FolderDefinition {
                        folder_id: before.folder_id,
                        before: Some(Box::new(before)),
                        after: Some(Box::new(after)),
                    });
                }
            }
            insert_cloud_journal(
                transaction,
                revision,
                "folder.sort_tree",
                None,
                serde_json::json!({
                    "folder_id": folder_id.0,
                    "descending": descending,
                    "recursive": recursive,
                }),
                now_ms(),
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (!changes.is_empty()).then(|| {
                        HistoryEntry::for_command(
                            "folders.sort_tree",
                            "Sort folders",
                            SemanticChange::Compound(changes),
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn reorder_folder_items(
        &self,
        folder_id: FolderId,
        root_ids: &[RootId],
    ) -> Result<MutationReceipt> {
        let values = root_ids.iter().map(|root_id| root_id.0).collect::<Vec<_>>();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            require_folder(transaction, folder_id)?;
            let before = snapshot
                .folder_orders
                .get(&folder_id)
                .map(|order| order.iter().map(|root| root.0).collect::<Vec<_>>())
                .unwrap_or_default();
            let requested = values.iter().copied().collect::<BTreeSet<_>>();
            if requested.len() != values.len()
                || !requested.is_subset(&before.iter().copied().collect())
            {
                return Err(LibraryError::InvalidInput(
                    "folder item reorder must contain distinct existing members".into(),
                ));
            }
            // A grid window sends only its loaded members. Permute their
            // canonical slots, leaving unloaded/filtered members in place.
            let mut replacements = values.iter();
            let values = before
                .iter()
                .map(|id| {
                    if requested.contains(id) {
                        *replacements.next().expect("validated permutation")
                    } else {
                        *id
                    }
                })
                .collect::<Vec<_>>();
            ordering::replace(
                transaction,
                revision,
                OrderOwnerKind::Folder,
                folder_id.0,
                &values,
            )?;
            insert_cloud_journal(
                transaction,
                revision,
                "folder.items.reorder",
                None,
                serde_json::json!({"folder_id": folder_id.0}),
                now_ms(),
            )?;
            let mut next = (*snapshot).clone();
            Arc::make_mut(&mut next.folder_orders).insert(
                folder_id,
                Arc::new(values.iter().copied().map(RootId).collect()),
            );
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), format!("folder:{}", folder_id.0)],
                if values.len() <= 256 {
                    values.iter().copied().map(RootId).collect()
                } else {
                    Vec::new()
                },
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (before != values).then(|| {
                        HistoryEntry::for_command(
                            "folders.sort_items",
                            "Reorder folder items",
                            SemanticChange::Order {
                                owner_kind: OrderOwnerKind::Folder,
                                owner_id: folder_id.0,
                                before: Arc::new(before),
                                after: Arc::new(values.clone()),
                            },
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn sort_folder_items(
        &self,
        folder_id: FolderId,
        field: crate::model::ContentSortField,
    ) -> Result<MutationReceipt> {
        let ordered = self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                require_folder(connection, folder_id)?;
                let roots = snapshot
                    .folder_orders
                    .get(&folder_id)
                    .map(AsRef::as_ref)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let root_ids = serde_json::to_string(
                    &roots.iter().map(|root_id| root_id.0).collect::<Vec<_>>(),
                )?;
                let order = match field {
                    crate::model::ContentSortField::Name => {
                        "lower(root.name), root.root_id"
                    }
                    crate::model::ContentSortField::ImportedAt => {
                        "root.imported_at_ms DESC, root.root_id"
                    }
                    crate::model::ContentSortField::CreatedAt => {
                        "root.captured_at_ms IS NULL, root.captured_at_ms DESC, root.root_id"
                    }
                    crate::model::ContentSortField::ModifiedAt => {
                        "root.modified_at_ms DESC, root.root_id"
                    }
                    crate::model::ContentSortField::Size => {
                        "root.total_size_bytes DESC, root.root_id"
                    }
                    crate::model::ContentSortField::Notes => {
                        "NULLIF(trim(root.notes), '') IS NULL, lower(root.notes), lower(root.name), root.root_id"
                    }
                };
                let sql = format!(
                    "SELECT root.root_id
                     FROM json_each(?1) selected
                     JOIN library_root root
                       ON root.root_id = CAST(selected.value AS INTEGER)
                     ORDER BY {order}"
                );
                let mut statement = connection.prepare(&sql)?;
                let ordered = statement
                    .query_map([root_ids], |row| row.get::<_, u32>(0))?
                    .map(|value| value.map(RootId))
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(ordered)
            },
        )?;
        self.reorder_folder_items(folder_id, &ordered)
    }

    pub fn delete_folders(&self, folder_ids: &[FolderId]) -> Result<FolderDeleteResult> {
        if folder_ids.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one folder is required".into(),
            ));
        }
        let requested = folder_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let result = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let subtree = load_folder_subtree(transaction, &requested)?;
            let present = subtree
                .iter()
                .map(|(folder_id, _)| *folder_id)
                .collect::<BTreeSet<_>>();
            if requested
                .iter()
                .any(|folder_id| !present.contains(folder_id))
            {
                return Err(LibraryError::NotFound(
                    "one or more selected folders do not exist".into(),
                ));
            }
            let definitions = subtree
                .iter()
                .map(|(folder_id, depth)| {
                    load_folder_definition(transaction, *folder_id)?
                        .map(|definition| (definition, *depth))
                        .ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "folder {} disappeared during deletion",
                                folder_id.0
                            ))
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            let fallback_folder_id = if requested.len() == 1 {
                let mut parent = definitions
                    .iter()
                    .find(|(definition, _)| definition.folder_id == requested[0])
                    .and_then(|(definition, _)| definition.parent_id);
                while parent.is_some_and(|folder_id| present.contains(&folder_id)) {
                    parent = definitions
                        .iter()
                        .find(|(definition, _)| Some(definition.folder_id) == parent)
                        .and_then(|(definition, _)| definition.parent_id);
                }
                parent
            } else {
                None
            };

            let mut next = (*snapshot).clone();
            let mut affected = RoaringBitmap::new();
            let mut history_changes = Vec::new();
            for (definition, _) in &definitions {
                let before = next
                    .folder_orders
                    .get(&definition.folder_id)
                    .map(|order| order.iter().map(|root| root.0).collect::<Vec<_>>())
                    .unwrap_or_default();
                if !before.is_empty() {
                    history_changes.push(SemanticChange::Order {
                        owner_kind: OrderOwnerKind::Folder,
                        owner_id: definition.folder_id.0,
                        before: Arc::new(before.clone()),
                        after: Arc::new(Vec::new()),
                    });
                }
                let members = before.iter().copied().collect::<RoaringBitmap>();
                affected |= &members;
                let counts = Arc::make_mut(&mut next.folder_count);
                for root_id in &members {
                    let current = counts.value(root_id).unwrap_or(0);
                    counts.insert(root_id, current.saturating_sub(1));
                }
                ordering::delete(transaction, OrderOwnerKind::Folder, definition.folder_id.0)?;
                Arc::make_mut(&mut next.folder_orders).remove(&definition.folder_id);
                Arc::make_mut(&mut next.folders).remove(&definition.folder_id);
            }

            let mut child_first = definitions.clone();
            child_first.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
            for (definition, _) in &child_first {
                transaction.execute(
                    "DELETE FROM folder_definition WHERE folder_id = ?1",
                    [definition.folder_id.0],
                )?;
                history_changes.push(SemanticChange::FolderDefinition {
                    folder_id: definition.folder_id,
                    before: Some(Box::new(definition.clone())),
                    after: None,
                });
            }
            crate::smart::settle_affected_for(
                transaction,
                &mut next,
                &affected,
                &[crate::predicate::DependencyChange::Folders],
            )?;
            next.revision = revision;
            let deleted_folder_ids = definitions
                .iter()
                .map(|(definition, _)| definition.folder_id)
                .collect::<Vec<_>>();
            insert_cloud_journal(
                transaction,
                revision,
                "folder.delete",
                None,
                serde_json::json!({
                    "folder_ids": deleted_folder_ids.iter().map(|id| id.0).collect::<Vec<_>>()
                }),
                now_ms(),
            )?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec![
                    "folders".into(),
                    "navigation".into(),
                    "sidebar".into(),
                    "smart-folders".into(),
                ],
                if affected.len() <= 256 {
                    affected.iter().map(RootId).collect()
                } else {
                    Vec::new()
                },
            );
            Ok((
                FolderDeleteResult {
                    deleted_folder_ids,
                    fallback_folder_id,
                    receipt: receipt.clone(),
                },
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "folders.delete",
                        "Delete folders",
                        SemanticChange::Compound(history_changes),
                    )),
                },
            ))
        })?;
        Ok(result)
    }

    pub(super) fn update_folder_definition<F>(
        &self,
        command: &'static str,
        label: &'static str,
        folder_id: FolderId,
        update: F,
    ) -> Result<MutationReceipt>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<()>,
    {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let before = load_folder_definition(transaction, folder_id)?
                .ok_or_else(|| LibraryError::NotFound(format!("folder {folder_id}")))?;
            update(transaction)?;
            let after = load_folder_definition(transaction, folder_id)?.ok_or_else(|| {
                LibraryError::InvalidState(format!(
                    "folder {} disappeared during update",
                    folder_id.0
                ))
            })?;
            insert_cloud_journal(
                transaction,
                revision,
                "folder.update",
                None,
                serde_json::json!({"folder_id": folder_id.0}),
                now_ms(),
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (before != after).then(|| {
                        HistoryEntry::for_command(
                            command,
                            label,
                            SemanticChange::FolderDefinition {
                                folder_id,
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

    pub fn rename_folder(&self, folder_id: FolderId, name: &str) -> Result<MutationReceipt> {
        let name = required_name("folder", name)?;
        let changed_at_ms = now_ms();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let before = transaction
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
            rename_folder_definition(transaction, folder_id, &name)?;
            insert_cloud_journal(
                transaction,
                revision,
                "folder.rename",
                None,
                serde_json::json!({"folder_id": folder_id.0, "name": name}),
                changed_at_ms,
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["folders".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (before != name).then(|| {
                        HistoryEntry::for_command(
                            "folders.rename",
                            "Rename folder",
                            SemanticChange::FolderName {
                                folder_id,
                                before,
                                after: name.clone(),
                            },
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn set_folder_auto_tags(
        &self,
        folder_id: FolderId,
        tag_ids: Vec<crate::TagId>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let after = tag_ids
            .into_iter()
            .map(|tag_id| tag_id.0)
            .collect::<RoaringBitmap>();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let before = ingest::folder_auto_tags(transaction, folder_id)?;
            for tag_id in &after {
                let exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tag_definition WHERE tag_id = ?1)",
                    [tag_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if !exists {
                    return Err(LibraryError::NotFound(format!("tag {tag_id}")));
                }
            }
            transaction.execute(
                "UPDATE folder_definition SET auto_tag_ids = ?2 WHERE folder_id = ?1",
                rusqlite::params![folder_id.0, ingest::encode_folder_auto_tags(&after)?],
            )?;
            transaction.execute(
                "INSERT INTO cloud_journal
                     (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                 VALUES (?1, 'folder.auto_tags', NULL, ?2, ?3)",
                rusqlite::params![
                    revision as i64,
                    serde_json::json!({"folder_id": folder_id.0}).to_string(),
                    changed_at_ms
                ],
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt =
                PublicationCoordinator::receipt(revision, vec!["folders".into()], Vec::new());
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (before != after).then(|| {
                        HistoryEntry::for_command(
                            "folders.set_auto_tags",
                            "Change folder auto-tags",
                            SemanticChange::FolderAutoTags {
                                folder_id,
                                before: Arc::new(before),
                                after: Arc::new(after.clone()),
                            },
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn add_to_folder(
        &self,
        target: &SelectionTarget,
        folder_id: FolderId,
    ) -> Result<MutationReceipt> {
        self.folder_membership_mutation(target, folder_id, true)
    }

    pub fn remove_from_folder(
        &self,
        target: &SelectionTarget,
        folder_id: FolderId,
    ) -> Result<MutationReceipt> {
        self.folder_membership_mutation(target, folder_id, false)
    }

    pub(super) fn folder_membership_mutation(
        &self,
        target: &SelectionTarget,
        folder_id: FolderId,
        add: bool,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let changed_at_ms = now_ms();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let exists = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM folder_definition WHERE folder_id = ?1)",
                [folder_id.0],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(LibraryError::NotFound(format!("folder {}", folder_id.0)));
            }
            let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
            let mut next = (*snapshot).clone();
            let before = next
                .folder_orders
                .get(&folder_id)
                .cloned()
                .unwrap_or_else(|| Arc::new(Vec::new()));
            let mut after = before.as_ref().clone();
            if add {
                let existing = after.iter().map(|root| root.0).collect::<RoaringBitmap>();
                after.extend((&selection - &existing).iter().map(RootId));
            } else {
                after.retain(|root| !selection.contains(root.0));
            }
            if before.as_ref() == &after {
                let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
                next.revision = revision;
                return Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ));
            }
            ordering::replace(
                transaction,
                revision,
                OrderOwnerKind::Folder,
                folder_id.0,
                &after.iter().map(|root| root.0).collect::<Vec<_>>(),
            )?;
            let after_bitmap = after.iter().map(|root| root.0).collect::<RoaringBitmap>();
            let before_bitmap = before.iter().map(|root| root.0).collect::<RoaringBitmap>();
            let changed = &before_bitmap ^ &after_bitmap;
            let added = &after_bitmap - &before_bitmap;
            Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(after.clone()));
            Arc::make_mut(&mut next.folders).insert(folder_id, after_bitmap.clone().into());
            let counts = Arc::make_mut(&mut next.folder_count);
            for root_id in &changed {
                let current = counts.value(root_id).unwrap_or(0);
                counts.insert(
                    root_id,
                    if after_bitmap.contains(root_id) {
                        current + 1
                    } else {
                        current.saturating_sub(1)
                    },
                );
            }
            let mut history_changes = vec![SemanticChange::Order {
                owner_kind: OrderOwnerKind::Folder,
                owner_id: folder_id.0,
                before: Arc::new(before.iter().map(|root| root.0).collect()),
                after: Arc::new(after.iter().map(|root| root.0).collect()),
            }];
            let mut auto_tagged = false;
            if add && !added.is_empty() {
                for tag_id in ingest::folder_auto_tags(transaction, folder_id)? {
                    let tag_id = crate::TagId(tag_id);
                    let before = next
                        .tags
                        .get(&tag_id)
                        .map(|members| members.to_bitmap())
                        .unwrap_or_default();
                    let mut after = before.clone();
                    after |= &added;
                    let tag_changed = &before ^ &after;
                    if tag_changed.is_empty() {
                        continue;
                    }
                    bitmap::replace_changed(
                        transaction,
                        revision,
                        BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        &after,
                        &tag_changed,
                    )?;
                    Arc::make_mut(&mut next.tags).insert(tag_id, after.clone().into());
                    let counts = Arc::make_mut(&mut next.tag_count);
                    for root_id in &tag_changed {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(root_id, current + 1);
                    }
                    history_changes.push(SemanticChange::Bitmap {
                        key: BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        before: Arc::new(before),
                        after: Arc::new(after),
                    });
                    auto_tagged = true;
                }
            }
            if auto_tagged {
                crate::smart::settle_affected(transaction, &mut next, &changed)?;
            } else {
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &changed,
                    &[crate::predicate::DependencyChange::Folder(folder_id)],
                )?;
            }
            insert_cloud_journal(
                transaction,
                revision,
                if add { "folder.add" } else { "folder.remove" },
                Some(&changed),
                serde_json::json!({"folder_id": folder_id.0}),
                changed_at_ms,
            )?;
            next.revision = revision;
            let mut resources = vec!["roots".into(), "folders".into(), "sidebar".into()];
            if auto_tagged {
                resources.push("tags".into());
            }
            let receipt = PublicationCoordinator::receipt(
                revision,
                resources,
                changed.iter().map(RootId),
            );
            let history = HistoryEntry::for_command(
                "items.set_folder",
                if add {
                    "Add to folder"
                } else {
                    "Remove from folder"
                },
                SemanticChange::Compound(history_changes),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(history),
                },
            ))
        })?;
        Ok(receipt)
    }
}

pub(super) fn rename_folder_definition(
    transaction: &rusqlite::Transaction<'_>,
    folder_id: FolderId,
    name: &str,
) -> Result<()> {
    if transaction.execute(
        "UPDATE folder_definition SET name = ?2 WHERE folder_id = ?1",
        rusqlite::params![folder_id.0, name],
    )? == 0
    {
        return Err(LibraryError::NotFound(format!("folder {}", folder_id.0)));
    }
    Ok(())
}

pub(super) fn load_folder_definition(
    connection: &rusqlite::Connection,
    folder_id: FolderId,
) -> Result<Option<FolderDefinitionState>> {
    use rusqlite::OptionalExtension;

    connection
        .query_row(
            "SELECT stable_key, parent_id, name, icon, color, notes, auto_tag_ids,
                    cover_root_id, watch_path, watch_enabled, watch_subfolders, display_order,
                    watch_lifecycle
             FROM folder_definition WHERE folder_id = ?1",
            [folder_id.0],
            |row| {
                Ok(FolderDefinitionState {
                    folder_id,
                    stable_key: row.get(0)?,
                    parent_id: row.get::<_, Option<u32>>(1)?.map(FolderId),
                    name: row.get(2)?,
                    icon: row.get(3)?,
                    color: row.get(4)?,
                    notes: row.get(5)?,
                    auto_tag_ids: row.get(6)?,
                    cover_root_id: row.get::<_, Option<u32>>(7)?.map(RootId),
                    watch_path: row.get(8)?,
                    watch_enabled: row.get(9)?,
                    watch_subfolders: row.get(10)?,
                    display_order: row.get(11)?,
                    watch_lifecycle: Lifecycle::from_watch_code(row.get(12)?),
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(super) fn list_folders(
    connection: &rusqlite::Connection,
    snapshot: &ProjectionSnapshot,
) -> Result<Vec<FolderRecord>> {
    let mut statement = connection.prepare(
        "SELECT folder_id, stable_key, parent_id, name, icon, color, notes,
                cover_root_id, watch_path, watch_enabled, watch_subfolders, display_order,
                watch_lifecycle
         FROM folder_definition
         ORDER BY parent_id, display_order, folder_id",
    )?;
    let rows = statement.query_map([], |row| {
        let folder_id = FolderId(row.get(0)?);
        Ok(FolderRecord {
            folder_id,
            stable_key: row.get(1)?,
            parent_id: row.get::<_, Option<u32>>(2)?.map(FolderId),
            name: row.get(3)?,
            icon: row.get(4)?,
            color: row.get(5)?,
            notes: row.get(6)?,
            cover_root_id: row.get::<_, Option<u32>>(7)?.map(RootId),
            watch_path: row.get(8)?,
            watch_enabled: row.get(9)?,
            watch_subfolders: row.get(10)?,
            watch_lifecycle: Lifecycle::from_watch_code(row.get(12)?),
            display_order: row.get(11)?,
            count: snapshot
                .folders
                .get(&folder_id)
                .map_or(0, |roots| (roots & snapshot.active()).len()),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(super) fn require_folder(connection: &rusqlite::Connection, folder_id: FolderId) -> Result<()> {
    if load_folder_definition(connection, folder_id)?.is_none() {
        return Err(LibraryError::NotFound(format!("folder {folder_id}")));
    }
    Ok(())
}

pub(super) fn load_folder_children(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
) -> Result<Vec<FolderId>> {
    let mut statement = connection.prepare(
        "SELECT folder_id FROM folder_definition
         WHERE parent_id IS ?1 ORDER BY display_order, folder_id",
    )?;
    let values = statement
        .query_map([parent_id.map(|id| id.0)], |row| {
            row.get::<_, u32>(0).map(FolderId)
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

pub(super) fn unique_folder_copy_name(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
    base: &str,
) -> Result<String> {
    for suffix in 1..=10_000 {
        let candidate = if suffix == 1 {
            base.to_owned()
        } else {
            format!("{base} {suffix}")
        };
        let exists = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM folder_definition
                 WHERE parent_id IS ?1 AND name = ?2
             )",
            rusqlite::params![parent_id.map(|id| id.0), candidate],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Ok(candidate);
        }
    }
    Err(LibraryError::InvalidState(
        "could not allocate a unique folder copy name".into(),
    ))
}

pub(super) fn load_folder_clone_rows(
    connection: &rusqlite::Connection,
    root_id: FolderId,
) -> Result<Vec<FolderDefinitionState>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE subtree(folder_id, depth) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT child.folder_id, subtree.depth + 1
             FROM folder_definition child
             JOIN subtree ON child.parent_id = subtree.folder_id
         )
         SELECT folder.stable_key, folder.parent_id, folder.name, folder.icon,
                folder.color, folder.notes, folder.auto_tag_ids, folder.cover_root_id,
                folder.watch_path, folder.watch_enabled, folder.watch_subfolders,
                folder.display_order, folder.folder_id
         FROM subtree
         JOIN folder_definition folder ON folder.folder_id = subtree.folder_id
         ORDER BY subtree.depth, folder.parent_id, folder.display_order, folder.folder_id",
    )?;
    let rows = statement
        .query_map([root_id.0], |row| {
            Ok(FolderDefinitionState {
                folder_id: FolderId(row.get(12)?),
                stable_key: row.get(0)?,
                parent_id: row.get::<_, Option<u32>>(1)?.map(FolderId),
                name: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                notes: row.get(5)?,
                auto_tag_ids: row.get(6)?,
                // Clones have no memberships, so covers and watches are intentionally not copied.
                cover_root_id: None,
                watch_path: None,
                watch_enabled: false,
                watch_subfolders: false,
                watch_lifecycle: Lifecycle::Inbox,
                display_order: row.get(11)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    if rows.is_empty() {
        return Err(LibraryError::NotFound(format!("folder {root_id}")));
    }
    Ok(rows)
}

pub(super) fn load_folder_subtree(
    transaction: &rusqlite::Transaction<'_>,
    requested: &[FolderId],
) -> Result<Vec<(FolderId, u32)>> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_selected_folder (
             folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_selected_folder;",
    )?;
    {
        let mut insert = transaction
            .prepare_cached("INSERT INTO picto_selected_folder(folder_id) VALUES (?1)")?;
        for folder_id in requested {
            insert.execute([folder_id.0])?;
        }
    }
    let mut statement = transaction.prepare(
        "WITH RECURSIVE subtree(folder_id, depth) AS (
             SELECT folder.folder_id, 0
             FROM folder_definition folder
             JOIN picto_selected_folder selected
               ON selected.folder_id = folder.folder_id
             UNION
             SELECT child.folder_id, parent.depth + 1
             FROM folder_definition child
             JOIN subtree parent ON child.parent_id = parent.folder_id
         )
         SELECT folder_id, MAX(depth) FROM subtree
         GROUP BY folder_id ORDER BY folder_id",
    )?;
    let values = statement
        .query_map([], |row| Ok((FolderId(row.get(0)?), row.get::<_, u32>(1)?)))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

pub(super) fn validate_folder_parent(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
    child_id: Option<FolderId>,
) -> Result<()> {
    if parent_id.is_some() && parent_id == child_id {
        return Err(LibraryError::InvalidInput(
            "a folder cannot be its own parent".into(),
        ));
    }
    if let Some(parent_id) = parent_id {
        require_folder(connection, parent_id)?;
    }
    if let (Some(parent_id), Some(child_id)) = (parent_id, child_id) {
        let creates_cycle = connection.query_row(
            "WITH RECURSIVE descendants(folder_id) AS (
                 SELECT folder_id FROM folder_definition WHERE parent_id = ?1
                 UNION ALL
                 SELECT child.folder_id
                 FROM folder_definition child
                 JOIN descendants parent ON child.parent_id = parent.folder_id
             )
             SELECT EXISTS(SELECT 1 FROM descendants WHERE folder_id = ?2)",
            rusqlite::params![child_id.0, parent_id.0],
            |row| row.get::<_, bool>(0),
        )?;
        if creates_cycle {
            return Err(LibraryError::InvalidInput(
                "a folder cannot move below its descendant".into(),
            ));
        }
    }
    let subtree_height = child_id
        .map(|child_id| folder_subtree_height(connection, child_id))
        .transpose()?
        .unwrap_or(1);
    validate_folder_nesting(connection, parent_id, subtree_height)
}

pub(super) fn folder_subtree_height(connection: &rusqlite::Connection, folder_id: FolderId) -> Result<usize> {
    require_folder(connection, folder_id)?;
    connection
        .query_row(
            "WITH RECURSIVE descendants(folder_id, depth) AS (
                 SELECT folder_id, 1 FROM folder_definition WHERE folder_id = ?1
                 UNION ALL
                 SELECT child.folder_id, parent.depth + 1
                 FROM folder_definition child
                 JOIN descendants parent ON child.parent_id = parent.folder_id
             )
             SELECT COALESCE(MAX(depth), 1) FROM descendants",
            [folder_id.0],
            |row| row.get::<_, u32>(0),
        )
        .map(|height| height as usize)
        .map_err(Into::into)
}

pub(super) fn validate_folder_nesting(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
    subtree_height: usize,
) -> Result<()> {
    let parent_depth = folder_depth(connection, parent_id)?;
    if parent_depth + subtree_height > crate::model::MAX_FOLDER_DEPTH {
        return Err(LibraryError::InvalidInput(format!(
            "folders may be nested at most {} levels deep",
            crate::model::MAX_FOLDER_DEPTH
        )));
    }
    Ok(())
}

pub(super) fn folder_depth(connection: &rusqlite::Connection, folder_id: Option<FolderId>) -> Result<usize> {
    let depth = if let Some(folder_id) = folder_id {
        require_folder(connection, folder_id)?;
        connection.query_row(
            "WITH RECURSIVE ancestors(folder_id, parent_id, depth) AS (
                 SELECT folder_id, parent_id, 1
                 FROM folder_definition WHERE folder_id = ?1
                 UNION ALL
                 SELECT parent.folder_id, parent.parent_id, child.depth + 1
                 FROM folder_definition parent
                 JOIN ancestors child ON parent.folder_id = child.parent_id
             )
             SELECT COALESCE(MAX(depth), 0) FROM ancestors",
            [folder_id.0],
            |row| row.get::<_, u32>(0),
        )? as usize
    } else {
        0
    };
    Ok(depth)
}
