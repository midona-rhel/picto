//! Tags and tag namespaces: assignment, definitions, renames and merges.

use super::*;

impl Library {
    pub fn tags(&self) -> Result<Vec<TagRecord>> {
        self.tags_with_revision().map(|(tags, _)| tags)
    }

    pub fn tag_namespaces(&self) -> Result<Vec<crate::TagNamespaceRecord>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            let mut statement = connection.prepare(
                "SELECT namespace.namespace_id, namespace.display_name, COUNT(tag.tag_id)
                 FROM tag_namespace namespace
                 LEFT JOIN tag_definition tag ON tag.namespace_id = namespace.namespace_id
                 GROUP BY namespace.namespace_id, namespace.display_name
                 ORDER BY namespace.display_name COLLATE NOCASE, namespace.namespace_id",
            )?;
            let values = statement
                .query_map([], |row| {
                    Ok(crate::TagNamespaceRecord {
                        namespace_id: TagNamespaceId(row.get(0)?),
                        name: row.get(1)?,
                        tag_count: row.get::<_, i64>(2)? as u64,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(LibraryError::from)?;
            Ok(values)
        })
    }

    pub fn tags_with_revision(&self) -> Result<(Vec<TagRecord>, u64)> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let mut statement = connection.prepare(
                    "SELECT tag.tag_id, namespace.namespace_id,
                            namespace.display_name, tag.subname
                     FROM tag_definition tag
                     JOIN tag_namespace namespace
                       ON namespace.namespace_id = tag.namespace_id
                     ORDER BY namespace.display_name COLLATE NOCASE,
                              tag.subname COLLATE NOCASE, tag.tag_id",
                )?;
                let rows = statement.query_map([], |row| {
                    let tag_id = crate::TagId(row.get(0)?);
                    let members = snapshot.tags.get(&tag_id);
                    Ok(TagRecord {
                        tag_id,
                        namespace_id: TagNamespaceId(row.get(1)?),
                        namespace: row.get(2)?,
                        subname: row.get(3)?,
                        active_count: members.map_or(0, |roots| (roots & snapshot.active()).len()),
                        assignment_count: members.map_or(0, |roots| roots.len()),
                    })
                })?;
                let tags = rows
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(LibraryError::from)?;
                Ok((tags, snapshot.revision))
            },
        )
    }

    /// One page of tags, filtered by namespace and subname substring and sorted
    /// by subname, then namespace, then id (case-insensitive). Counts are
    /// computed only for the returned page, not for every tag in the library.
    pub fn tag_page(
        &self,
        namespace: Option<&str>,
        search: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<TagRecord>, Option<usize>, u64)> {
        let search = search
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let sorted = self.tag_list.sorted(connection, snapshot.revision)?;
                let mut matching = sorted.iter().filter(|entry| {
                    namespace.is_none_or(|namespace| entry.row.namespace == namespace)
                        && search
                            .as_ref()
                            .is_none_or(|search| entry.subname_key.contains(search.as_str()))
                });
                let page = matching
                    .by_ref()
                    .skip(offset)
                    .take(limit)
                    .map(|entry| entry.row.clone().with_counts(&snapshot))
                    .collect::<Vec<_>>();
                let next = offset.saturating_add(limit);
                let next = matching.next().is_some().then_some(next);
                Ok((page, next, snapshot.revision))
            },
        )
    }

    /// The requested tags with counts; unknown ids are skipped.
    pub fn tags_by_id(&self, tag_ids: &[crate::TagId]) -> Result<Vec<TagRecord>> {
        if tag_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                Ok(tag_definition_rows(connection, Some(tag_ids))?
                    .into_iter()
                    .map(|row| row.with_counts(&snapshot))
                    .collect())
            },
        )
    }

    /// Tags assigned to no root, active or not.
    pub fn unused_tag_count(&self) -> Result<u64> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let mut statement = connection.prepare_cached("SELECT tag_id FROM tag_definition")?;
                let ids = statement.query_map([], |row| row.get::<_, u32>(0))?;
                let mut unused = 0;
                for tag_id in ids {
                    let tag_id = crate::TagId(tag_id?);
                    if snapshot.tags.get(&tag_id).is_none_or(|members| members.is_empty()) {
                        unused += 1;
                    }
                }
                Ok(unused)
            },
        )
    }

    pub fn add_tag(&self, target: &SelectionTarget, name: &str) -> Result<MutationReceipt> {
        self.apply_tags(target, &[name.to_owned()], true)
    }

    /// Apply independently predicted tag sets to visible roots in one exact
    /// publication. Collections are one root, so callers union all member
    /// predictions into one assignment before entering the library kernel.
    /// Tags the user asked for (manual AI tagging); undoable.
    pub fn add_tag_assignments(
        &self,
        assignments: &[RootTagAssignment],
    ) -> Result<MutationReceipt> {
        self.assign_tags(assignments, true)
    }

    /// Tags written by background work (auto-tag on import). They never enter
    /// the undo stack, so Undo keeps meaning "undo my last edit", and roots
    /// deleted while inference ran are skipped instead of failing the batch.
    pub fn add_automatic_tag_assignments(
        &self,
        assignments: &[RootTagAssignment],
    ) -> Result<MutationReceipt> {
        self.assign_tags(assignments, false)
    }

    pub(super) fn assign_tags(
        &self,
        assignments: &[RootTagAssignment],
        user_requested: bool,
    ) -> Result<MutationReceipt> {
        if assignments.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one root tag assignment is required".into(),
            ));
        }
        let mut roots_by_tag = HashMap::<String, RoaringBitmap>::new();
        let mut requested_roots = RoaringBitmap::new();
        for assignment in assignments {
            requested_roots.insert(assignment.root_id.0);
            for tag in &assignment.tags {
                let tag = ingest::canonical_tag_name(tag)?;
                roots_by_tag
                    .entry(tag)
                    .or_default()
                    .insert(assignment.root_id.0);
            }
        }
        if roots_by_tag.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one tag is required".into(),
            ));
        }

        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            if user_requested {
                WorkPriority::ForegroundMutation
            } else {
                WorkPriority::CanonicalIngest
            },
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                // Check only the requested roots instead of unioning every root.
                let missing = requested_roots
                    .iter()
                    .filter(|root_id| {
                        !snapshot.root_kinds.values().any(|members| members.contains(*root_id))
                    })
                    .collect::<RoaringBitmap>();
                if user_requested {
                    if let Some(root_id) = missing.min() {
                        return Err(LibraryError::NotFound(format!("root {root_id}")));
                    }
                }

                let mut next = (*snapshot).clone();
                let mut changes = Vec::with_capacity(roots_by_tag.len());
                let mut affected = RoaringBitmap::new();
                let mut smart_changes = Vec::new();
                for (name, requested) in &roots_by_tag {
                    let tag_id = if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                        tag_id
                    } else {
                        let tag_id = ingest::ensure_tag(transaction, name)?;
                        Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                        tag_id
                    };
                    let before = next
                        .tags
                        .get(&tag_id)
                        .map(|members| members.to_bitmap())
                        .unwrap_or_default();
                    let mut after = before.clone();
                    after |= &(requested - &missing);
                    let changed = &after - &before;
                    if changed.is_empty() {
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
                        &changed,
                    )?;
                    Arc::make_mut(&mut next.tags).insert(tag_id, after.clone().into());
                    let counts = Arc::make_mut(&mut next.tag_count);
                    for root_id in &changed {
                        counts.insert(root_id, counts.value(root_id).unwrap_or(0) + 1);
                    }
                    affected |= &changed;
                    smart_changes.push(crate::predicate::DependencyChange::Tag(tag_id));
                    changes.push(SemanticChange::Bitmap {
                        key: BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        before: Arc::new(before),
                        after: Arc::new(after),
                    });
                }

                if !affected.is_empty() {
                    crate::smart::settle_affected_for(transaction, &mut next, &affected, &smart_changes)?;
                    insert_cloud_journal(
                        transaction,
                        revision,
                        "tag.ai_batch",
                        Some(&affected),
                        serde_json::json!({"assignment_count": assignments.len()}),
                        changed_at_ms,
                    )?;
                }
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    if affected.is_empty() {
                        Vec::new()
                    } else {
                        vec![
                            "roots".into(),
                            "tags".into(),
                            "sidebar".into(),
                            "smart-folders".into(),
                        ]
                    },
                    affected.iter().map(RootId),
                );
                let history = (user_requested && !changes.is_empty()).then(|| {
                    HistoryEntry::for_command(
                        "items.apply_ai_tags",
                        "Apply AI tags",
                        SemanticChange::Compound(changes),
                    )
                });
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    pub fn rename_tag(&self, tag_id: crate::TagId, name: &str) -> Result<MutationReceipt> {
        let name = ingest::canonical_tag_name(name)?;
        let changed_at_ms = now_ms();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let old_name = snapshot
                .tag_ids_by_name
                .iter()
                .find_map(|(name, id)| (*id == tag_id).then_some(name.clone()))
                .ok_or_else(|| LibraryError::NotFound(format!("tag {}", tag_id.0)))?;
            if snapshot
                .tag_ids_by_name
                .get(&name)
                .is_some_and(|existing| *existing != tag_id)
            {
                return Err(LibraryError::InvalidInput(format!(
                    "tag {name} already exists"
                )));
            }
            let mut next = (*snapshot).clone();
            rename_tag_definition(transaction, &mut next, tag_id, &name)?;
            insert_cloud_journal(
                transaction,
                revision,
                "tag.rename",
                None,
                serde_json::json!({"tag_id": tag_id.0, "name": name}),
                changed_at_ms,
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["tags".into(), "navigation".into(), "smart-folders".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (old_name != name).then(|| {
                        HistoryEntry::for_command(
                            "tags.rename_or_merge",
                            "Rename tag",
                            SemanticChange::TagName {
                                tag_id,
                                before: old_name,
                                after: name.clone(),
                            },
                        )
                    }),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn rename_tag_namespace(
        &self,
        namespace_id: TagNamespaceId,
        name: &str,
    ) -> Result<MutationReceipt> {
        let name = required_name("tag namespace", name)?;
        if name.eq_ignore_ascii_case("general") {
            return self.rename_or_merge_tag_namespace(namespace_id, "");
        }
        if name.contains(':') {
            return Err(LibraryError::InvalidInput(
                "tag namespace cannot contain a colon".into(),
            ));
        }
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let before = load_namespace_name(transaction, namespace_id)?;
            let mut next = (*snapshot).clone();
            apply_namespace_name(transaction, &mut next, namespace_id, &name)?;
            insert_cloud_journal(
                transaction,
                revision,
                "tag.namespace.rename",
                None,
                serde_json::json!({"namespace_id": namespace_id.0, "name": name}),
                now_ms(),
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["tags".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (before != name).then(|| {
                        HistoryEntry::for_command(
                            "tags.group.rename",
                            "Rename tag namespace",
                            SemanticChange::TagNamespaceName {
                                namespace_id,
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

    pub fn create_tag_namespace(&self, name: &str) -> Result<MutationReceipt> {
        let name = required_name("tag namespace", name)?;
        if name.eq_ignore_ascii_case("general") {
            return Err(LibraryError::InvalidInput(
                "General is the built-in unnamespaced tag group".into(),
            ));
        }
        if name.contains(':') {
            return Err(LibraryError::InvalidInput(
                "tag namespace cannot contain a colon".into(),
            ));
        }
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let duplicate = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM tag_namespace WHERE display_name = ?1)",
                [&name],
                |row| row.get::<_, bool>(0),
            )?;
            if duplicate {
                return Err(LibraryError::InvalidInput(format!(
                    "tag namespace {name} already exists"
                )));
            }
            let namespace_id = TagNamespaceId(LibraryDatabase::allocate_id(transaction)?);
            let state = TagNamespaceDefinitionState {
                namespace_id,
                stable_key: uuid::Uuid::new_v4().to_string(),
                display_name: name.clone(),
            };
            transaction.execute(
                "INSERT INTO tag_namespace(namespace_id, stable_key, display_name)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![state.namespace_id.0, state.stable_key, state.display_name,],
            )?;
            insert_cloud_journal(
                transaction,
                revision,
                "tag.namespace.create",
                None,
                serde_json::json!({
                    "namespace_id": namespace_id.0,
                    "name": name,
                }),
                now_ms(),
            )?;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["tags".into(), "navigation".into()],
                Vec::new(),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "tags.group.create",
                        "Create tag namespace",
                        SemanticChange::TagNamespaceDefinition {
                            before: None,
                            after: Some(state),
                        },
                    )),
                },
            ))
        })?;
        Ok(receipt)
    }

    /// Ensure portable metadata can reference tag definitions without adding
    /// synthetic assignments to a library item.
    pub fn ensure_tag_definitions(
        &self,
        names: &[String],
    ) -> Result<(Vec<crate::TagId>, MutationReceipt)> {
        let names = names
            .iter()
            .map(|name| ingest::canonical_tag_name(name))
            .collect::<Result<BTreeSet<_>>>()?;
        if names.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one tag definition is required".into(),
            ));
        }
        let (tag_ids, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let mut next = (*snapshot).clone();
            let mut tag_ids = Vec::with_capacity(names.len());
            let mut created = Vec::new();
            for name in &names {
                let tag_id = if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                    tag_id
                } else {
                    let tag_id = ingest::ensure_tag(transaction, name)?;
                    Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                    Arc::make_mut(&mut next.tags)
                        .entry(tag_id)
                        .or_insert_with(|| RoaringBitmap::new().into());
                    created.push(name.clone());
                    tag_id
                };
                tag_ids.push(tag_id);
            }
            if !created.is_empty() {
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.definitions.ensure",
                    None,
                    serde_json::json!({"names": created}),
                    now_ms(),
                )?;
            }
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                if created.is_empty() {
                    Vec::new()
                } else {
                    vec!["tags".into(), "navigation".into()]
                },
                Vec::new(),
            );
            Ok((
                (tag_ids, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        Ok((tag_ids, receipt))
    }

    pub fn rename_or_merge_tag_namespace(
        &self,
        namespace_id: TagNamespaceId,
        name: &str,
    ) -> Result<MutationReceipt> {
        let name = name.trim();
        let name = if name.eq_ignore_ascii_case("general") {
            String::new()
        } else {
            name.to_owned()
        };
        if name.contains(':') {
            return Err(LibraryError::InvalidInput(
                "tag namespace cannot contain a colon".into(),
            ));
        }
        if load_namespace_name_for_library(self, namespace_id)? == name {
            return self.rename_tag_namespace(namespace_id, &name);
        }
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            use rusqlite::OptionalExtension;

            let source_state = load_namespace_definition(transaction, namespace_id)?
                .ok_or_else(|| {
                    LibraryError::NotFound(format!("tag namespace {}", namespace_id.0))
                })?;
            if source_state.display_name.is_empty() {
                return Err(LibraryError::InvalidInput(
                    "the unnamespaced tag group cannot be renamed".into(),
                ));
            }
            let target_id = transaction
                .query_row(
                    "SELECT namespace_id FROM tag_namespace
                     WHERE display_name = ?1 AND namespace_id != ?2",
                    rusqlite::params![name, namespace_id.0],
                    |row| row.get::<_, u32>(0).map(TagNamespaceId),
                )
                .optional()?;
            let Some(target_id) = target_id else {
                let mut next = (*snapshot).clone();
                apply_namespace_name(transaction, &mut next, namespace_id, &name)?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.namespace.rename",
                    None,
                    serde_json::json!({"namespace_id": namespace_id.0, "name": name}),
                    now_ms(),
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["tags".into(), "navigation".into()],
                    Vec::new(),
                );
                return Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "tags.group.rename",
                            "Rename tag namespace",
                            SemanticChange::TagNamespaceName {
                                namespace_id,
                                before: source_state.display_name,
                                after: name.clone(),
                            },
                        )),
                    },
                ));
            };

            let source_tags = transaction
                .prepare(
                    "SELECT tag_id, subname FROM tag_definition
                     WHERE namespace_id = ?1 ORDER BY tag_id",
                )?
                .query_map([namespace_id.0], |row| {
                    Ok((crate::TagId(row.get(0)?), row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut next = (*snapshot).clone();
            let mut affected = RoaringBitmap::new();
            let mut changes = Vec::new();
            let mut smart_changes = Vec::new();
            for (source_tag_id, subname) in source_tags {
                let destination = transaction
                    .query_row(
                        "SELECT tag_id FROM tag_definition
                         WHERE namespace_id = ?1 AND subname = ?2",
                        rusqlite::params![target_id.0, subname],
                        |row| row.get::<_, u32>(0).map(crate::TagId),
                    )
                    .optional()?;
                if let Some(destination) = destination {
                    let (members, tag_changes) = remove_tag_in_transaction(
                        transaction,
                        &mut next,
                        source_tag_id,
                        Some(destination),
                        revision,
                    )?;
                    affected |= members;
                    changes.extend(tag_changes);
                    smart_changes.push(crate::predicate::DependencyChange::Tag(source_tag_id));
                    smart_changes.push(crate::predicate::DependencyChange::Tag(destination));
                } else {
                    let before = next
                        .tag_ids_by_name
                        .iter()
                        .find_map(|(name, id)| (*id == source_tag_id).then_some(name.clone()))
                        .ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "tag {} has no projection name",
                                source_tag_id.0
                            ))
                        })?;
                    let after = if name.is_empty() {
                        subname.clone()
                    } else {
                        format!("{name}:{subname}")
                    };
                    transaction.execute(
                        "UPDATE tag_definition SET namespace_id = ?2 WHERE tag_id = ?1",
                        rusqlite::params![source_tag_id.0, target_id.0],
                    )?;
                    let names = Arc::make_mut(&mut next.tag_ids_by_name);
                    names.remove(&before);
                    names.insert(after.clone(), source_tag_id);
                    changes.push(SemanticChange::TagName {
                        tag_id: source_tag_id,
                        before,
                        after,
                    });
                }
            }
            transaction.execute(
                "DELETE FROM tag_namespace WHERE namespace_id = ?1",
                [namespace_id.0],
            )?;
            crate::smart::settle_affected_for(transaction, &mut next, &affected, &smart_changes)?;
            changes.push(SemanticChange::TagNamespaceDefinition {
                before: Some(source_state),
                after: None,
            });
            insert_cloud_journal(
                transaction,
                revision,
                "tag.namespace.merge",
                Some(&affected),
                serde_json::json!({
                    "source_namespace_id": namespace_id.0,
                    "destination_namespace_id": target_id.0,
                }),
                now_ms(),
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec![
                    "roots".into(),
                    "tags".into(),
                    "navigation".into(),
                    "smart-folders".into(),
                ],
                affected.iter().map(RootId),
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "tags.group.rename",
                        "Merge tag namespaces",
                        SemanticChange::Compound(changes),
                    )),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn delete_tag_namespace(&self, namespace_id: TagNamespaceId) -> Result<MutationReceipt> {
        let general_id = self
            .database
            .read(WorkPriority::VisibleRead, |connection| {
                use rusqlite::OptionalExtension;
                connection
                    .query_row(
                        "SELECT namespace_id FROM tag_namespace WHERE display_name = ''",
                        [],
                        |row| row.get::<_, u32>(0).map(TagNamespaceId),
                    )
                    .optional()
                    .map_err(Into::into)
            })?;
        if general_id == Some(namespace_id) {
            return Err(LibraryError::InvalidInput(
                "the unnamespaced tag group cannot be deleted".into(),
            ));
        }
        self.rename_or_merge_tag_namespace(namespace_id, "")
    }

    pub fn delete_tag(&self, tag_id: crate::TagId, changed_at_ms: i64) -> Result<MutationReceipt> {
        self.remove_tag_definition(tag_id, None, changed_at_ms)
    }

    pub fn merge_tags(
        &self,
        source: crate::TagId,
        destination: crate::TagId,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if source == destination {
            return Err(LibraryError::InvalidInput(
                "source and destination tags are identical".into(),
            ));
        }
        self.remove_tag_definition(source, Some(destination), changed_at_ms)
    }

    pub fn delete_unused_tags(&self) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let tag_ids = transaction
                .prepare("SELECT tag_id FROM tag_definition ORDER BY tag_id")?
                .query_map([], |row| row.get::<_, u32>(0).map(crate::TagId))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let unused = tag_ids
                .into_iter()
                .filter(|tag_id| snapshot.tags.get(tag_id).is_none_or(|roots| roots.is_empty()))
                .collect::<Vec<_>>();
            let mut next = (*snapshot).clone();
            let mut changes = Vec::new();
            for tag_id in &unused {
                let (_, tag_changes) = remove_tag_in_transaction(
                    transaction,
                    &mut next,
                    *tag_id,
                    None,
                    revision,
                )?;
                changes.extend(tag_changes);
            }
            let settings_changed =
                if let Some(change) = prune_deleted_starred_tags(transaction, &next)? {
                    changes.push(change);
                    true
                } else {
                    false
                };
            insert_cloud_journal(
                transaction,
                revision,
                "tag.delete_unused",
                None,
                serde_json::json!({"tag_ids": unused.iter().map(|id| id.0).collect::<Vec<_>>() }),
                now_ms(),
            )?;
            next.revision = revision;
            let mut resources =
                vec!["tags".into(), "navigation".into(), "smart-folders".into()];
            if settings_changed {
                resources.push("settings".into());
            }
            let receipt = PublicationCoordinator::receipt(
                revision,
                resources,
                Vec::new(),
            );
            Ok((receipt.clone(), PublishedDelta {
                snapshot: next,
                receipt,
                history: (!changes.is_empty()).then(|| HistoryEntry::for_command(
                    "tags.delete_unused",
                    "Delete unused tags",
                    SemanticChange::Compound(changes),
                )),
            }))
        })?;
        Ok(receipt)
    }

    pub fn apply_tags(
        &self,
        target: &SelectionTarget,
        names: &[String],
        add: bool,
    ) -> Result<MutationReceipt> {
        let names = names
            .iter()
            .filter(|name| !name.trim().is_empty())
            .map(|name| ingest::canonical_tag_name(name))
            .collect::<Result<BTreeSet<_>>>()?
            .into_iter()
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one non-empty tag is required".into(),
            ));
        }
        self.tag_mutation(target, names, add)
    }

    pub(super) fn remove_tag_definition(
        &self,
        source: crate::TagId,
        destination: Option<crate::TagId>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let mut next = (*snapshot).clone();
            let (source_members, mut history_changes) = remove_tag_in_transaction(
                transaction,
                &mut next,
                source,
                destination,
                revision,
            )?;
            // Members moved to the destination (or lost a tag): folders on
            // either tag, and Exact-mode folders, must see the new membership.
            let mut smart_changes = vec![crate::predicate::DependencyChange::Tag(source)];
            smart_changes.extend(destination.map(crate::predicate::DependencyChange::Tag));
            crate::smart::settle_affected_for(
                transaction,
                &mut next,
                &source_members,
                &smart_changes,
            )?;
            let settings_changed = if destination.is_none() {
                if let Some(change) = prune_deleted_starred_tags(transaction, &next)? {
                    history_changes.push(change);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            transaction.execute(
                "INSERT INTO cloud_journal
                     (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    revision as i64,
                    if destination.is_some() {
                        "tag.merge"
                    } else {
                        "tag.delete"
                    },
                    crate::bitmap::encode(&source_members)?,
                    serde_json::json!({
                        "source_tag_id": source.0,
                        "destination_tag_id": destination.map(|tag| tag.0),
                    })
                    .to_string(),
                    changed_at_ms
                ],
            )?;
            next.revision = revision;
            let mut resources = vec![
                "roots".into(),
                "tags".into(),
                "navigation".into(),
                "smart-folders".into(),
            ];
            if settings_changed {
                resources.push("settings".into());
            }
            let receipt = PublicationCoordinator::receipt(
                revision,
                resources,
                source_members.iter().map(RootId),
            );
            let label = if destination.is_some() {
                "Merge tags"
            } else {
                "Delete tag"
            };
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        if destination.is_some() {
                            "tags.rename_or_merge"
                        } else {
                            "tags.delete"
                        },
                        label,
                        SemanticChange::Compound(history_changes),
                    )),
                },
            ))
        })?;
        Ok(receipt)
    }

    pub(super) fn tag_mutation(
        &self,
        target: &SelectionTarget,
        names: Vec<String>,
        add: bool,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let changed_at_ms = now_ms();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let selection = crate::selection::resolve_with(transaction, &snapshot, &target, Some((&self.match_cache, &self.order_cache)))?;
            let mut next = (*snapshot).clone();
            if selection.is_empty() {
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
                return Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ));
            }
            let mut affected = RoaringBitmap::new();
            let mut changes = Vec::new();
            let mut tag_ids = Vec::new();
            for name in &names {
                let tag_id = if add {
                    if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                        tag_id
                    } else {
                        let tag_id = ingest::ensure_tag(transaction, name)?;
                        Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                        tag_id
                    }
                } else if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                    tag_id
                } else {
                    continue;
                };
                let before = next
                    .tags
                    .get(&tag_id)
                    .map(|members| members.to_bitmap())
                    .unwrap_or_default();
                let mut after = before.clone();
                if add {
                    after |= &selection;
                } else {
                    after -= &selection;
                }
                let changed = &before ^ &after;
                if changed.is_empty() {
                    continue;
                }
                Arc::make_mut(&mut next.tags).insert(tag_id, after.clone().into());
                bitmap::replace_changed(
                    transaction,
                    revision,
                    BitmapKey {
                        domain: BitmapDomain::Tag,
                        key_id: tag_id.0,
                    },
                    &after,
                    &changed,
                )?;
                let counts = Arc::make_mut(&mut next.tag_count);
                for root_id in &changed {
                    let current = counts.value(root_id).unwrap_or(0);
                    counts.insert(
                        root_id,
                        if after.contains(root_id) {
                            current + 1
                        } else {
                            current.saturating_sub(1)
                        },
                    );
                }
                affected |= &changed;
                tag_ids.push(tag_id.0);
                changes.push(SemanticChange::Bitmap {
                    key: BitmapKey {
                        domain: BitmapDomain::Tag,
                        key_id: tag_id.0,
                    },
                    before: Arc::new(before),
                    after: Arc::new(after),
                });
            }
            if changes.is_empty() {
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
            let smart_changes = tag_ids
                .iter()
                .map(|tag_id| crate::predicate::DependencyChange::Tag(crate::TagId(*tag_id)))
                .collect::<Vec<_>>();
            crate::smart::settle_affected_for(transaction, &mut next, &affected, &smart_changes)?;
            insert_cloud_journal(
                transaction,
                revision,
                if add { "tag.add" } else { "tag.remove" },
                Some(&affected),
                serde_json::json!({"tag_ids": tag_ids}),
                changed_at_ms,
            )?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["roots".into(), "tags".into(), "sidebar".into()],
                affected.iter().map(RootId),
            );
            let history = HistoryEntry::for_command(
                "items.apply_tags",
                if add { "Add tags" } else { "Remove tags" },
                SemanticChange::Compound(changes),
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

pub(super) fn rename_tag_definition(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    tag_id: crate::TagId,
    name: &str,
) -> Result<()> {
    let name = ingest::canonical_tag_name(name)?;
    let (namespace, subname) = name.split_once(':').unwrap_or(("", name.as_str()));
    if subname.trim().is_empty() {
        return Err(LibraryError::InvalidInput("tag name is empty".into()));
    }
    if snapshot
        .tag_ids_by_name
        .get(&name)
        .is_some_and(|existing| *existing != tag_id)
    {
        return Err(LibraryError::InvalidInput(format!(
            "tag {name} already exists"
        )));
    }
    let old_name = snapshot
        .tag_ids_by_name
        .iter()
        .find_map(|(name, id)| (*id == tag_id).then_some(name.clone()))
        .ok_or_else(|| LibraryError::NotFound(format!("tag {}", tag_id.0)))?;
    let namespace_id = ingest::ensure_namespace(transaction, namespace)?;
    transaction.execute(
        "UPDATE tag_definition SET namespace_id = ?2, subname = ?3 WHERE tag_id = ?1",
        rusqlite::params![tag_id.0, namespace_id, subname],
    )?;
    let names = Arc::make_mut(&mut snapshot.tag_ids_by_name);
    names.remove(&old_name);
    names.insert(name, tag_id);
    Ok(())
}

pub(super) fn remove_tag_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    source: crate::TagId,
    destination: Option<crate::TagId>,
    revision: u64,
) -> Result<(RoaringBitmap, Vec<SemanticChange>)> {
    let source_state = load_tag_definition(transaction, source)?
        .ok_or_else(|| LibraryError::NotFound(format!("tag {}", source.0)))?;
    if let Some(destination) = destination {
        if load_tag_definition(transaction, destination)?.is_none() {
            return Err(LibraryError::NotFound(format!("tag {}", destination.0)));
        }
    }
    let source_members = snapshot
        .tags
        .get(&source)
        .map(|members| members.to_bitmap())
        .unwrap_or_default();
    let destination_before = destination
        .and_then(|tag_id| snapshot.tags.get(&tag_id))
        .map(|members| members.to_bitmap())
        .unwrap_or_default();
    let mut history_changes = Vec::new();

    let folder_ids = transaction
        .prepare("SELECT folder_id FROM folder_definition")?
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for folder_id in folder_ids.into_iter().map(FolderId) {
        let before = ingest::folder_auto_tags(transaction, folder_id)?;
        if !before.contains(source.0) {
            continue;
        }
        let mut after = before.clone();
        after.remove(source.0);
        if let Some(destination) = destination {
            after.insert(destination.0);
        }
        transaction.execute(
            "UPDATE folder_definition SET auto_tag_ids = ?2 WHERE folder_id = ?1",
            rusqlite::params![folder_id.0, ingest::encode_folder_auto_tags(&after)?],
        )?;
        history_changes.push(SemanticChange::FolderAutoTags {
            folder_id,
            before: Arc::new(before),
            after: Arc::new(after),
        });
    }

    if let Some(destination) = destination {
        let mut after = destination_before.clone();
        after |= &source_members;
        if destination_before != after {
            bitmap::replace_changed(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: destination.0,
                },
                &after,
                &source_members,
            )?;
            Arc::make_mut(&mut snapshot.tags).insert(destination, after.clone().into());
            history_changes.push(SemanticChange::Bitmap {
                key: BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: destination.0,
                },
                before: Arc::new(destination_before.clone()),
                after: Arc::new(after),
            });
        }
    }

    bitmap::replace(
        transaction,
        revision,
        BitmapKey {
            domain: BitmapDomain::Tag,
            key_id: source.0,
        },
        &RoaringBitmap::new(),
    )?;
    transaction.execute("DELETE FROM tag_definition WHERE tag_id = ?1", [source.0])?;
    Arc::make_mut(&mut snapshot.tags).remove(&source);
    Arc::make_mut(&mut snapshot.tag_ids_by_name).remove(&source_state.full_name);

    let counts = Arc::make_mut(&mut snapshot.tag_count);
    for root_id in &source_members {
        if destination.is_none() || destination_before.contains(root_id) {
            let current = counts.value(root_id).unwrap_or(0);
            counts.insert(root_id, current.saturating_sub(1));
        }
    }
    let query_changes =
        crate::smart::rewrite_tag_references(transaction, snapshot, source, destination)?;
    history_changes.insert(
        0,
        SemanticChange::TagDefinition {
            before: Some(source_state),
            after: None,
            before_members: Arc::new(source_members.clone()),
            after_members: Arc::new(RoaringBitmap::new()),
            queries: Arc::new(query_changes),
        },
    );
    Ok((source_members, history_changes))
}

pub(super) fn prune_deleted_starred_tags(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
) -> Result<Option<SemanticChange>> {
    use rusqlite::OptionalExtension;

    let before = transaction
        .query_row(
            "SELECT value_json FROM setting WHERE key = 'application'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(before) = before else {
        return Ok(None);
    };
    let mut settings = serde_json::from_str::<serde_json::Value>(&before).map_err(|error| {
        LibraryError::InvalidState(format!(
            "application settings contain invalid JSON: {error}"
        ))
    })?;
    let original = settings.clone();
    let Some(starred_tags) = settings
        .as_object_mut()
        .and_then(|object| object.get_mut("starredTags"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(None);
    };
    starred_tags.retain(|entry| {
        entry
            .as_str()
            .is_some_and(|name| snapshot.tag_ids_by_name.contains_key(name))
    });
    if settings == original {
        return Ok(None);
    }

    let after = serde_json::to_string(&settings).map_err(|error| {
        LibraryError::InvalidState(format!("application settings cannot be encoded: {error}"))
    })?;
    transaction.execute(
        "UPDATE setting SET value_json = ?1 WHERE key = 'application'",
        [&after],
    )?;
    Ok(Some(SemanticChange::AuxiliaryJson {
        table: "setting",
        key: "application".into(),
        before: Some(before),
        after: Some(after),
        resource: "settings",
    }))
}

pub(super) fn load_namespace_name(
    connection: &rusqlite::Connection,
    namespace_id: TagNamespaceId,
) -> Result<String> {
    connection
        .query_row(
            "SELECT display_name FROM tag_namespace WHERE namespace_id = ?1",
            [namespace_id.0],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                LibraryError::NotFound(format!("tag namespace {namespace_id}"))
            }
            error => error.into(),
        })
}

pub(super) fn load_namespace_name_for_library(
    library: &Library,
    namespace_id: TagNamespaceId,
) -> Result<String> {
    library
        .database
        .read(WorkPriority::VisibleRead, |connection| {
            load_namespace_name(connection, namespace_id)
        })
}

pub(super) fn load_namespace_definition(
    connection: &rusqlite::Connection,
    namespace_id: TagNamespaceId,
) -> Result<Option<TagNamespaceDefinitionState>> {
    use rusqlite::OptionalExtension;

    connection
        .query_row(
            "SELECT stable_key, display_name FROM tag_namespace WHERE namespace_id = ?1",
            [namespace_id.0],
            |row| {
                Ok(TagNamespaceDefinitionState {
                    namespace_id,
                    stable_key: row.get(0)?,
                    display_name: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(super) fn apply_namespace_name(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    namespace_id: TagNamespaceId,
    replacement: &str,
) -> Result<()> {
    let before = load_namespace_name(transaction, namespace_id)?;
    let duplicate = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM tag_namespace
             WHERE display_name = ?1 AND namespace_id != ?2
         )",
        rusqlite::params![replacement, namespace_id.0],
        |row| row.get::<_, bool>(0),
    )?;
    if duplicate {
        return Err(LibraryError::InvalidInput(format!(
            "tag namespace {replacement} already exists"
        )));
    }
    let mut statement = transaction
        .prepare("SELECT tag_id, subname FROM tag_definition WHERE namespace_id = ?1")?;
    let tags = statement
        .query_map([namespace_id.0], |row| {
            Ok((crate::TagId(row.get(0)?), row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    transaction.execute(
        "UPDATE tag_namespace SET display_name = ?2 WHERE namespace_id = ?1",
        rusqlite::params![namespace_id.0, replacement],
    )?;
    let names = Arc::make_mut(&mut snapshot.tag_ids_by_name);
    for (tag_id, subname) in tags {
        let old_name = if before.is_empty() {
            subname.clone()
        } else {
            format!("{before}:{subname}")
        };
        let new_name = if replacement.is_empty() {
            subname
        } else {
            format!("{replacement}:{subname}")
        };
        names.remove(&old_name);
        names.insert(new_name, tag_id);
    }
    Ok(())
}

pub(super) fn load_tag_definition(
    transaction: &rusqlite::Transaction<'_>,
    tag_id: crate::TagId,
) -> Result<Option<TagDefinitionState>> {
    use rusqlite::OptionalExtension;

    transaction
        .query_row(
            "SELECT tag.stable_key, tag.namespace_id, namespace.display_name, tag.subname
             FROM tag_definition tag
             JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
             WHERE tag.tag_id = ?1",
            [tag_id.0],
            |row| {
                let namespace = row.get::<_, String>(2)?;
                let subname = row.get::<_, String>(3)?;
                Ok(TagDefinitionState {
                    tag_id,
                    stable_key: row.get(0)?,
                    namespace_id: row.get(1)?,
                    full_name: if namespace.is_empty() {
                        subname
                    } else {
                        format!("{namespace}:{subname}")
                    },
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(super) fn tag_subname(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, subname)| subname)
}

/// Every tag definition, sorted the way the tag lists show them, with
/// lowercased keys computed once. Rebuilt only after the library changes, so
/// typing in a tag search filters a resident list instead of reading,
/// lowercasing and sorting every definition on each keystroke.
#[derive(Default)]
pub(crate) struct TagListCache(parking_lot::Mutex<Option<(u64, Arc<Vec<SortedTag>>)>>);

pub(super) struct SortedTag {
    subname_key: String,
    row: TagDefinitionRow,
}

impl TagListCache {
    fn sorted(&self, connection: &rusqlite::Connection, revision: u64) -> Result<Arc<Vec<SortedTag>>> {
        if let Some((cached, rows)) = self.0.lock().as_ref() {
            if *cached == revision {
                return Ok(rows.clone());
            }
        }
        let mut rows = tag_definition_rows(connection, None)?
            .into_iter()
            .map(|row| (row.namespace.to_lowercase(), SortedTag { subname_key: row.subname.to_lowercase(), row }))
            .collect::<Vec<_>>();
        rows.sort_by(|(left_namespace, left), (right_namespace, right)| {
            (&left.subname_key, left_namespace, left.row.tag_id.0)
                .cmp(&(&right.subname_key, right_namespace, right.row.tag_id.0))
        });
        let rows = Arc::new(rows.into_iter().map(|(_, entry)| entry).collect::<Vec<_>>());
        *self.0.lock() = Some((revision, rows.clone()));
        Ok(rows)
    }
}

#[derive(Clone)]
pub(super) struct TagDefinitionRow {
    tag_id: crate::TagId,
    namespace_id: TagNamespaceId,
    namespace: String,
    subname: String,
}

impl TagDefinitionRow {
    fn with_counts(self, snapshot: &ProjectionSnapshot) -> TagRecord {
        let members = snapshot.tags.get(&self.tag_id);
        TagRecord {
            tag_id: self.tag_id,
            namespace_id: self.namespace_id,
            namespace: self.namespace,
            subname: self.subname,
            active_count: members.map_or(0, |roots| roots.intersection_len(snapshot.active())),
            assignment_count: members.map_or(0, |roots| roots.len()),
        }
    }
}

/// Tag definitions without counts; `only` restricts to the given ids.
pub(super) fn tag_definition_rows(
    connection: &rusqlite::Connection,
    only: Option<&[crate::TagId]>,
) -> Result<Vec<TagDefinitionRow>> {
    let base = "SELECT tag.tag_id, namespace.namespace_id, namespace.display_name, tag.subname
         FROM tag_definition tag
         JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id";
    let map = |row: &rusqlite::Row<'_>| {
        Ok(TagDefinitionRow {
            tag_id: crate::TagId(row.get(0)?),
            namespace_id: TagNamespaceId(row.get(1)?),
            namespace: row.get(2)?,
            subname: row.get(3)?,
        })
    };
    let rows = match only {
        None => connection
            .prepare_cached(base)?
            .query_map([], map)?
            .collect::<std::result::Result<Vec<_>, _>>(),
        Some(ids) => {
            let ids = serde_json::to_string(&ids.iter().map(|id| id.0).collect::<Vec<_>>())?;
            connection
                .prepare_cached(&format!(
                    "{base} WHERE tag.tag_id IN (SELECT value FROM json_each(?1))
                     ORDER BY namespace.display_name COLLATE NOCASE,
                              tag.subname COLLATE NOCASE, tag.tag_id"
                ))?
                .query_map([ids], map)?
                .collect::<std::result::Result<Vec<_>, _>>()
        }
    };
    rows.map_err(LibraryError::from)
}
