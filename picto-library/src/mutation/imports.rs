//! Importing prepared media and collections.

use super::*;

impl Library {
    pub fn ingest(&self, input: &PreparedImport) -> Result<(RootId, MutationReceipt)> {
        let mut outputs = self.ingest_batch(std::slice::from_ref(input))?;
        Ok(outputs.remove(0))
    }

    pub fn ingest_batch(
        &self,
        inputs: &[PreparedImport],
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        self.ingest_batch_with(inputs, false, None)
    }

    /// Import items for a foreground task, which records the roots it
    /// creates and the tags it adds to existing ones so it can be undone.
    pub fn ingest_batch_for_task(
        &self,
        inputs: &[PreparedImport],
        task: crate::TaskId,
        auto_tag: bool,
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        self.ingest_batch_with(inputs, auto_tag, Some(task))
    }

    pub fn ingest_batch_with_auto_tags(
        &self,
        inputs: &[PreparedImport],
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        self.ingest_batch_with(inputs, true, None)
    }

    fn ingest_batch_with(
        &self,
        inputs: &[PreparedImport],
        auto_tag: bool,
        task: Option<crate::TaskId>,
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        if inputs.len() > ingest::MAX_INGEST_BATCH {
            return Err(LibraryError::InvalidInput(format!(
                "one canonical ingest batch may contain at most {} items",
                ingest::MAX_INGEST_BATCH
            )));
        }
        let outputs = self.publish(WorkPriority::CanonicalIngest, |transaction, revision, snapshot| {
            if let Some(task) = task {
                super::tasks::require_running_task(transaction, task)?;
            }
            let mut next = (*snapshot).clone();
            let mut root_ids = Vec::with_capacity(inputs.len());
            let mut created_root_ids = Vec::with_capacity(inputs.len());
            let mut resources = BTreeSet::new();
            let mut bitmap_keys = HashSet::new();
            let mut folder_ids = HashSet::new();
            let mut affected = RoaringBitmap::new();
            for input in inputs {
                let output = ingest::insert_one(transaction, revision, next, input)?;
                next = output.snapshot;
                root_ids.push(output.root_id);
                if let Some(task) = task {
                    crate::mutation::tasks::record_task_ingest(
                        transaction,
                        task,
                        output.created_root.then_some(output.root_id),
                        &output.added_tags,
                        1,
                    )?;
                    resources.insert("tasks".to_owned());
                }
                if output.created_root {
                    created_root_ids.push(output.root_id);
                    if let Some(attempt_id) = input
                        .source_identity
                        .as_ref()
                        .and_then(|source| source.source_attempt_id)
                    {
                        record_source_attempt_root(
                            transaction,
                            attempt_id,
                            output.root_id,
                            true,
                        )?;
                        resources.insert("subscriptions".to_owned());
                    }
                }
                resources.extend(output.resources);
                bitmap_keys.extend(output.bitmap_keys);
                folder_ids.extend(output.folder_ids);
                affected |= &output.affected_roots;
            }
            ingest::persist_touched(
                transaction,
                revision,
                &next,
                bitmap_keys,
                folder_ids,
                affected.iter().map(RootId),
            )?;
            crate::smart::settle_affected(transaction, &mut next, &affected)?;
            if auto_tag && !created_root_ids.is_empty() {
                ingest::enqueue_ai_tag_roots(
                    transaction,
                    created_root_ids.iter().copied(),
                    inputs
                        .iter()
                        .map(|input| input.imported_at_ms)
                        .max()
                        .unwrap_or_else(now_ms),
                )?;
                resources.insert("tasks".to_owned());
            }
            insert_cloud_journal(
                transaction,
                revision,
                "root.ingest",
                Some(&affected),
                serde_json::json!({"count": root_ids.len()}),
                inputs
                    .iter()
                    .map(|input| input.imported_at_ms)
                    .max()
                    .unwrap_or_else(now_ms),
            )?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                resources,
                affected.iter().map(RootId),
            );
            let outputs = root_ids
                .into_iter()
                .map(|root_id| (root_id, receipt.clone()))
                .collect::<Vec<_>>();
            Ok((
                outputs,
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        Ok(outputs)
    }

    pub fn ingest_collection(
        &self,
        input: &PreparedCollectionImport,
    ) -> Result<(RootId, MutationReceipt)> {
        self.ingest_collection_with(input, false, None)
    }

    /// Import a collection for a foreground task; see [`Self::ingest_batch_for_task`].
    pub fn ingest_collection_for_task(
        &self,
        input: &PreparedCollectionImport,
        task: crate::TaskId,
        auto_tag: bool,
    ) -> Result<(RootId, MutationReceipt)> {
        self.ingest_collection_with(input, auto_tag, Some(task))
    }

    pub fn ingest_collection_with_auto_tags(
        &self,
        input: &PreparedCollectionImport,
    ) -> Result<(RootId, MutationReceipt)> {
        self.ingest_collection_with(input, true, None)
    }

    /// Import a post's files as one collection. A file the library already
    /// holds is never copied: a loose item joins the new collection (as
    /// grouping would). Existing collections sharing any of its files combine
    /// with the post, keeping each original set complete without copying media.
    fn ingest_collection_with(
        &self,
        input: &PreparedCollectionImport,
        auto_tag: bool,
        task: Option<crate::TaskId>,
    ) -> Result<(RootId, MutationReceipt)> {
        if input.members.is_empty() {
            return Err(LibraryError::InvalidInput(
                "a collection import requires at least one media member".into(),
            ));
        }
        if input.cover_index >= input.members.len() {
            return Err(LibraryError::InvalidInput(
                "collection cover index is outside the member list".into(),
            ));
        }
        let (collection_id, receipt) = self.publish(WorkPriority::CanonicalIngest, |transaction, revision, snapshot| {
            if let Some(task) = task {
                super::tasks::require_running_task(transaction, task)?;
            }
            let mut next = (*snapshot).clone();
            let mut root_ids = Vec::with_capacity(input.members.len() + 1);
            let mut created_root_ids = Vec::with_capacity(input.members.len());
            let mut requested_cover = input.existing_root_id;
            if let Some(existing_root_id) = input.existing_root_id {
                if !next
                    .root_kinds
                    .get(&RootKind::Collection)
                    .is_some_and(|roots| roots.contains(existing_root_id.0))
                    && !next
                        .root_kinds
                        .get(&RootKind::Media)
                        .is_some_and(|roots| roots.contains(existing_root_id.0))
                {
                    return Err(LibraryError::NotFound(format!(
                        "existing source root {existing_root_id}"
                    )));
                }
                root_ids.push(existing_root_id);
            }
            let mut resources = BTreeSet::new();
            let mut bitmap_keys = HashSet::new();
            let mut folder_ids = HashSet::new();
            let mut ingest_affected = RoaringBitmap::new();
            let mut outputs = Vec::with_capacity(input.members.len());
            for member in &input.members {
                let output = ingest::insert_one(transaction, revision, next, member)?;
                next = output.snapshot;
                if output.created_root {
                    created_root_ids.push(output.root_id);
                }
                if let Some(task) = task {
                    // Partial collections are ungrouped during rollback, exposing
                    // these created media roots for deletion.
                    crate::mutation::tasks::record_task_ingest(
                        transaction,
                        task,
                        output.created_root.then_some(output.root_id),
                        &output.added_tags,
                        0,
                    )?;
                }
                resources.extend(output.resources);
                bitmap_keys.extend(output.bitmap_keys);
                folder_ids.extend(output.folder_ids);
                ingest_affected |= &output.affected_roots;
                outputs.push(output.root_id);
            }
            for (index, root) in outputs.iter().copied().enumerate() {
                root_ids.push(root);
                if input.existing_root_id.is_none() && index == input.cover_index {
                    requested_cover = Some(root);
                }
            }
            let ordered_roots = root_ids
                .iter()
                .copied()
                .fold(Vec::new(), |mut roots, root| {
                    if !roots.contains(&root) {
                        roots.push(root);
                    }
                    roots
                });
            let existing_collections = ordered_roots
                .iter()
                .copied()
                .filter(|root| {
                    next.root_kinds
                        .get(&RootKind::Collection)
                        .is_some_and(|collections| collections.contains(root.0))
                })
                .collect::<Vec<_>>();
            let winning_collection_id = existing_collections.first().copied();
            let creates_collection = ordered_roots.len() >= 2 && winning_collection_id.is_none();
            let wholly_created = creates_collection && created_root_ids.len() == input.members.len();
            let before = task.filter(|_| !wholly_created && ordered_roots.len() >= 2).map(|_| {
                super::structure::capture_structure(
                    transaction, &next,
                    &ordered_roots.iter().map(|root| root.0).collect(), None,
                )
            }).transpose()?;
            let requested_cover = requested_cover.filter(|root| ordered_roots.contains(root));
            let (published_root, mut affected) = if let Some(winner) = winning_collection_id.filter(|_| ordered_roots.len() >= 2) {
                    let mut merge = Merge { record: task.is_some(), ..Default::default() };
                    combine_overlapping_roots(transaction, revision, &mut next,
                        &ordered_roots, winner, input.modified_at_ms, &mut merge)?;
                    if let Some(task) = task {
                        for change in merge.changes {
                            if let SemanticChange::Relink(state) = change {
                                super::tasks::record_task_undo(transaction, task, super::tasks::TaskUndo::Relink(state))?;
                            }
                        }
                    }
                    resources.extend(merge.resources);
                    (winner, merge.affected)
                } else if ordered_roots.len() >= 2 {
                    // Capture before lifecycle promotion so rollback restores each input's state.
                    settle_lifecycles(&mut next, &ordered_roots, &mut bitmap_keys);
                    let output = crate::group::organize(
                        transaction,
                        revision,
                        next,
                        &GroupRequest {
                            target: SelectionTarget::Explicit {
                                root_ids: ordered_roots.clone(),
                            },
                            cover_root_id: requested_cover.unwrap_or(ordered_roots[0]),
                            winning_collection_id,
                            // Existing collection titles are collection-owned. A later
                            // member refresh must not replace one with a member/source name.
                            name: winning_collection_id
                                .is_none()
                                .then(|| input.name.clone())
                                .flatten(),
                            notes: input.members[input.cover_index].notes.clone(),
                            modified_at_ms: input.modified_at_ms,
                        },
                        true,
                        None,
                    )?;
                    next = output.snapshot;
                    resources.insert("collections".to_owned());
                    (output.collection_id, output.affected)
                } else {
                    let published_root = ordered_roots[0];
                    (
                        published_root,
                        root_ids
                            .iter()
                            .map(|root| root.0)
                            .collect::<RoaringBitmap>(),
                    )
                };
            if let (Some(task), Some(before)) = (task, before) {
                let after = super::structure::capture_structure(transaction, &next, &affected, None)?;
                super::tasks::record_task_undo(transaction, task, super::tasks::TaskUndo::Structure {
                    affected: Arc::new(affected.clone()),
                    before: Box::new(before), after: Box::new(after),
                })?;
            }
            affected |= &ingest_affected;
            ingest::persist_touched(
                transaction,
                revision,
                &next,
                bitmap_keys,
                folder_ids,
                affected.iter().map(RootId),
            )?;
            crate::smart::settle_affected(transaction, &mut next, &affected)?;
            if auto_tag && !created_root_ids.is_empty() {
                ingest::enqueue_ai_tag_roots(
                    transaction,
                    std::iter::once(published_root),
                    input.modified_at_ms,
                )?;
                resources.insert("tasks".to_owned());
            }
            if !created_root_ids.is_empty() || creates_collection {
                for attempt_id in input
                    .members
                    .iter()
                    .filter_map(|member| member.source_identity.as_ref())
                    .filter_map(|source| source.source_attempt_id)
                    .collect::<BTreeSet<_>>()
                {
                    record_source_attempt_root(
                        transaction,
                        attempt_id,
                        published_root,
                        creates_collection || created_root_ids.contains(&published_root),
                    )?;
                    resources.insert("subscriptions".to_owned());
                }
            }
            if let Some(task) = task {
                crate::mutation::tasks::record_task_units(
                    transaction,
                    task,
                    created_root_ids.len(),
                    input.members.len() - created_root_ids.len(),
                    wholly_created.then_some(published_root),
                )?;
                resources.insert("tasks".to_owned());
            }
            resources.extend(["tags".to_owned(), "folders".to_owned()]);
            let receipt = PublicationCoordinator::receipt(
                revision,
                resources,
                affected.iter()
                    .filter(|id| *id == published_root.0 || snapshot.root_kinds.values().any(|roots| roots.contains(*id)))
                    .map(RootId),
            );
            Ok((
                (published_root, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        Ok((collection_id, receipt))
    }
}

/// Move `roots` into the lifecycle among them that keeps an item most (Active,
/// then Inbox, then Trash), so they can be grouped.
pub(super) fn settle_lifecycles(
    snapshot: &mut ProjectionSnapshot,
    roots: &[RootId],
    bitmap_keys: &mut HashSet<BitmapKey>,
) {
    let current = |snapshot: &ProjectionSnapshot, root: RootId| {
        Lifecycle::ALL
            .into_iter()
            .find(|value| snapshot.lifecycle(*value).contains(root.0))
    };
    let Some(target) = roots
        .iter()
        .filter_map(|root| current(snapshot, *root))
        .max_by_key(|value| value.keep_rank())
    else {
        return;
    };
    for root in roots {
        let Some(value) = current(snapshot, *root).filter(|value| *value != target) else {
            continue;
        };
        let lifecycle = Arc::make_mut(&mut snapshot.lifecycle);
        lifecycle.entry(value).or_default().remove(root.0);
        lifecycle.entry(target).or_default().insert(root.0);
        for changed in [value, target] {
            bitmap_keys.insert(BitmapKey {
                domain: BitmapDomain::Lifecycle,
                key_id: changed.bitmap_key(),
            });
        }
    }
}

pub(super) fn record_source_attempt_root(
    transaction: &rusqlite::Transaction<'_>,
    attempt_id: i64,
    root_id: RootId,
    created_root: bool,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO source_attempt_root(attempt_id, root_id, root_stable_key, created_root)
         SELECT ?1, item.local_id, item.stable_key, ?3
         FROM library_item item
         WHERE item.local_id = ?2
         ON CONFLICT(attempt_id, root_stable_key) DO UPDATE SET created_root = MAX(source_attempt_root.created_root, excluded.created_root)",
        rusqlite::params![attempt_id, root_id.0, created_root],
    )?;
    Ok(())
}
