//! Collections, permanent deletion and the structural state undo restores.

use super::*;

impl Library {
    pub fn organize_into_collection(
        &self,
        request: &GroupRequest,
    ) -> Result<(RootId, MutationReceipt)> {
        let request = request.clone();
        let (collection_id, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let selected = crate::selection::resolve_with(transaction, &snapshot, &request.target, Some((&self.match_cache, &self.order_cache)))?;
            let before = capture_structure(transaction, &snapshot, &selected, None)?;
            // Grouping only moves the selection's tags onto the collection, so
            // the tags found now are the only ones to settle and capture after.
            let tags = captured_tags(&before);
            let output = crate::group::organize(
                transaction,
                revision,
                (*snapshot).clone(),
                &request,
                false,
                Some(&tags),
            )?;
            let mut next = output.snapshot;
            crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
            let after = capture_structure(transaction, &next, &output.affected, Some(&tags))?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec![
                    "roots".into(),
                    "collections".into(),
                    "tags".into(),
                    "folders".into(),
                    "sidebar".into(),
                ],
                output.affected.iter().map(RootId),
            );
            Ok((
                (output.collection_id, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "collections.organize",
                        "Organize collection",
                        SemanticChange::Structure {
                            affected: Arc::new(output.affected),
                            before,
                            after,
                        },
                    )
                    .about(selected.len(), None)),
                },
            ))
        })?;
        Ok((collection_id, receipt))
    }

    pub fn ungroup_collection(
        &self,
        collection_id: RootId,
        modified_at_ms: i64,
    ) -> Result<(Vec<RootId>, MutationReceipt)> {
        let (roots, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let mut affected = snapshot
                .collection_orders
                .get(&collection_id)
                .ok_or_else(|| {
                    LibraryError::InvalidInput(format!(
                        "root {collection_id} is not a collection"
                    ))
                })?
                .iter()
                .map(|media| media.0)
                .collect::<RoaringBitmap>();
            affected.insert(collection_id.0);
            let before = capture_structure(transaction, &snapshot, &affected, None)?;
            // Members inherit the collection's tags and gain no others.
            let tags = captured_tags(&before);
            let output = crate::group::ungroup(
                transaction,
                revision,
                (*snapshot).clone(),
                collection_id,
                modified_at_ms,
                Some(&tags),
            )?;
            let mut next = output.snapshot;
            crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
            let after = capture_structure(transaction, &next, &output.affected, Some(&tags))?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec![
                    "roots".into(),
                    "collections".into(),
                    "tags".into(),
                    "folders".into(),
                    "sidebar".into(),
                ],
                output.affected.iter().map(RootId),
            );
            Ok((
                (output.roots, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "collections.ungroup",
                        "Ungroup collection",
                        SemanticChange::Structure {
                            affected: Arc::new(output.affected),
                            before,
                            after,
                        },
                    )),
                },
            ))
        })?;
        Ok((roots, receipt))
    }

    pub fn detach_collection_member(
        &self,
        collection_id: RootId,
        media_id: MediaId,
        modified_at_ms: i64,
    ) -> Result<(RootId, MutationReceipt)> {
        let (mut roots, receipt) =
            self.detach_collection_members(collection_id, vec![media_id], None, modified_at_ms)?;
        let root_id = roots
            .pop()
            .ok_or_else(|| LibraryError::InvalidState("detach produced no root".into()))?;
        Ok((root_id, receipt))
    }

    pub fn detach_collection_members(
        &self,
        collection_id: RootId,
        media_ids: Vec<MediaId>,
        target_lifecycle: Option<Lifecycle>,
        modified_at_ms: i64,
    ) -> Result<(Vec<RootId>, MutationReceipt)> {
        let (root_ids, receipt) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let mut affected = media_ids
                .iter()
                .map(|media_id| media_id.0)
                .collect::<RoaringBitmap>();
            affected.insert(collection_id.0);
            let before = capture_structure(transaction, &snapshot, &affected, None)?;
            // Detached members inherit the collection's tags and gain no others.
            let tags = captured_tags(&before);
            let output = crate::group::detach_many(
                transaction,
                revision,
                (*snapshot).clone(),
                collection_id,
                &media_ids,
                target_lifecycle,
                modified_at_ms,
                Some(&tags),
            )?;
            let mut next = output.snapshot;
            crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
            let after = capture_structure(transaction, &next, &output.affected, Some(&tags))?;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec![
                    "roots".into(),
                    "collections".into(),
                    "tags".into(),
                    "folders".into(),
                    "sidebar".into(),
                    "search".into(),
                ],
                output.affected.iter().map(RootId),
            );
            let detached = output.root_ids.len() as u64;
            Ok((
                (output.root_ids, receipt.clone()),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        "collections.detach",
                        "Detach collection member",
                        SemanticChange::Structure {
                            affected: Arc::new(output.affected),
                            before,
                            after,
                        },
                    )
                    .about(detached, None)),
                },
            ))
        })?;
        Ok((root_ids, receipt))
    }

    pub fn reorder_collection(
        &self,
        collection_id: RootId,
        ordered_media_ids: Vec<MediaId>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let before = snapshot
                .collection_orders
                .get(&collection_id)
                .ok_or_else(|| {
                    LibraryError::InvalidInput(format!(
                        "root {collection_id} is not a collection"
                    ))
                })?
                .iter()
                .map(|media| media.0)
                .collect::<Vec<_>>();
            let after = ordered_media_ids
                .iter()
                .map(|media| media.0)
                .collect::<Vec<_>>();
            let before_members = before.iter().copied().collect::<RoaringBitmap>();
            let after_members = after.iter().copied().collect::<RoaringBitmap>();
            if before.len() != after.len() || before_members != after_members {
                return Err(LibraryError::InvalidInput(
                    "collection order must contain every member exactly once".into(),
                ));
            }
            let mut next = (*snapshot).clone();
            let history_entry = if before == after {
                None
            } else {
                ordering::replace(
                    transaction,
                    revision,
                    OrderOwnerKind::Collection,
                    collection_id.0,
                    &after,
                )?;
                Arc::make_mut(&mut next.collection_orders)
                    .insert(collection_id, Arc::new(ordered_media_ids.clone()));
                transaction.execute(
                    "INSERT INTO cloud_journal
                         (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                     VALUES (?1, 'collection.reorder', ?2, '{}', ?3)",
                    rusqlite::params![
                        revision as i64,
                        crate::bitmap::encode(&[collection_id.0].into_iter().collect())?,
                        changed_at_ms
                    ],
                )?;
                Some(HistoryEntry::for_command(
                    "collections.reorder",
                    "Reorder collection",
                    SemanticChange::Order {
                        owner_kind: OrderOwnerKind::Collection,
                        owner_id: collection_id.0,
                        before: Arc::new(before),
                        after: Arc::new(after),
                    },
                ))
            };
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["roots".into(), "collections".into()],
                [collection_id],
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: history_entry,
                },
            ))
        })?;
        Ok(receipt)
    }

    pub fn set_collection_cover(
        &self,
        collection_id: RootId,
        cover_media_id: MediaId,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let (before, before_notes) = transaction
                .query_row(
                    "SELECT cover_media_id, notes FROM library_root WHERE root_id = ?1",
                    [collection_id.0],
                    |row| Ok((MediaId(row.get(0)?), row.get::<_, Option<String>>(1)?)),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("collection {collection_id}"))
                    }
                    error => error.into(),
                })?;
            let mut next = (*snapshot).clone();
            let history_entry = if before == cover_media_id {
                None
            } else {
                crate::group::set_cover(transaction, &mut next, collection_id, cover_media_id)?;
                let affected = [collection_id.0].into_iter().collect();
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                let after_notes = transaction.query_row(
                    "SELECT notes FROM library_root WHERE root_id = ?1",
                    [collection_id.0],
                    |row| row.get::<_, Option<String>>(0),
                )?;
                transaction.execute(
                    "INSERT INTO cloud_journal
                         (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                     VALUES (?1, 'collection.cover', ?2, ?3, ?4)",
                    rusqlite::params![
                        revision as i64,
                        crate::bitmap::encode(&affected)?,
                        serde_json::json!({"cover_media_id": cover_media_id.0}).to_string(),
                        changed_at_ms
                    ],
                )?;
                Some(HistoryEntry::for_command(
                    "collections.set_cover",
                    "Set collection cover",
                    SemanticChange::CollectionCover {
                        root_id: collection_id,
                        before,
                        after: cover_media_id,
                        before_notes,
                        after_notes,
                    },
                ))
            };
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["roots".into(), "collections".into(), "smart-folders".into()],
                [collection_id],
            );
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: history_entry,
                },
            ))
        })?;
        Ok(receipt)
    }

    /// Delete Trash roots for good. A delete of at most `undo_limit` roots is
    /// undoable: its rows are kept in history and its files stay on disk
    /// until the entry leaves history (when the app closes, or it falls off
    /// the stack). A larger one cannot be undone and frees its files at once.
    pub fn permanently_delete(
        &self,
        target: &SelectionTarget,
        deleted_at_ms: i64,
        undo_limit: usize,
    ) -> Result<(MutationReceipt, Vec<crate::PendingBlobCleanup>)> {
        self.delete_permanently_from(
            target,
            deleted_at_ms,
            undo_limit,
            Lifecycle::Trash,
            "items.delete",
            "Delete permanently",
        )
    }

    /// Reject Inbox roots straight out of the library (the setting that
    /// skips Trash for rejected items). Undoable like `permanently_delete`.
    pub fn delete_rejected(
        &self,
        target: &SelectionTarget,
        deleted_at_ms: i64,
        undo_limit: usize,
    ) -> Result<(MutationReceipt, Vec<crate::PendingBlobCleanup>)> {
        self.delete_permanently_from(
            target,
            deleted_at_ms,
            undo_limit,
            Lifecycle::Inbox,
            "items.reject",
            "Reject",
        )
    }

    fn delete_permanently_from(
        &self,
        target: &SelectionTarget,
        deleted_at_ms: i64,
        undo_limit: usize,
        lifecycle: Lifecycle,
        command: &'static str,
        label: &'static str,
    ) -> Result<(MutationReceipt, Vec<crate::PendingBlobCleanup>)> {
        let target = target.clone();
        let protected_cleanup_files = self.history.protected_cleanup_files();
        let (receipt, cleanup) = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let roots = crate::selection::resolve_with(transaction, &snapshot, &target, Some((&self.match_cache, &self.order_cache)))?;
            if roots.is_empty() {
                return Err(LibraryError::InvalidInput(
                    "permanent deletion requires at least one root".into(),
                ));
            }
            if !roots.is_subset(snapshot.lifecycle(lifecycle)) {
                return Err(LibraryError::InvalidInput(match lifecycle {
                    Lifecycle::Inbox => "only Inbox roots can be rejected".into(),
                    _ => "only Trash roots can be permanently deleted".into(),
                }));
            }
            let undoable = usize::try_from(roots.len()).is_ok_and(|count| count <= undo_limit);
            let state = undoable
                .then(|| capture_deleted_roots(transaction, &snapshot, &roots))
                .transpose()?;
            // Files an undoable delete keeps are not freed now.
            let mut protected = protected_cleanup_files.clone();
            if let Some(state) = &state {
                protected.extend(state.files.iter().copied());
            }
            let (next, cleanup, receipt) = delete_roots(
                transaction,
                revision,
                &snapshot,
                &roots,
                deleted_at_ms,
                &protected,
            )?;
            let history = state.map(|state| {
                HistoryEntry::for_command(
                    command,
                    label,
                    SemanticChange::PermanentDelete(Arc::new(state)),
                )
                .about(roots.len(), None)
            });
            Ok((
                (receipt.clone(), cleanup),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history,
                },
            ))
        })?;
        Ok((receipt, cleanup))
    }

    pub fn pending_blob_cleanup(&self, limit: usize) -> Result<Vec<PendingBlobCleanup>> {
        self.database.read_consistent(
            WorkPriority::Maintenance,
            |_| Ok(self.history.protected_cleanup_files()),
            |connection, protected| crate::duplicate::ready_cleanup(connection, &protected, limit),
        )
    }

    /// Delete unreferenced physical files while retaining the serialized writer.
    /// Readers continue through WAL; canonical ingestion cannot race a cleanup
    /// by attaching a new media item to the file being removed.
    pub fn clean_pending_blobs(
        &self,
        limit: usize,
        mut delete: impl FnMut(&PendingBlobCleanup) -> Result<()>,
    ) -> Result<usize> {
        let protected = self.history.protected_cleanup_files();
        self.database
            .maintenance_write(WorkPriority::Maintenance, |transaction| {
                let cleanup = crate::duplicate::ready_cleanup(transaction, &protected, limit)?;
                let mut removed = 0;
                for pending in cleanup {
                    delete(&pending)?;
                    let deleted = transaction.execute(
                        "DELETE FROM media_file
                         WHERE file_id = ?1
                           AND NOT EXISTS(
                               SELECT 1 FROM media_item WHERE file_id = ?1
                           )",
                        [pending.file_id.0],
                    )?;
                    if deleted != 1 {
                        return Err(LibraryError::InvalidState(format!(
                            "blob cleanup file {} became referenced",
                            pending.file_id.0
                        )));
                    }
                    removed += 1;
                }
                Ok(removed)
            })
    }
}

/// Remove `roots` (and their media) from the library inside a publication:
/// tombstones, projections, orders, smart folders and the cloud journal.
/// Files no other media item uses are queued for cleanup; those an undo entry
/// still needs (`protected`) are queued but not returned for immediate removal.
pub(super) fn delete_roots(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &ProjectionSnapshot,
    roots: &RoaringBitmap,
    deleted_at_ms: i64,
    protected_cleanup_files: &std::collections::BTreeSet<crate::FileId>,
) -> Result<(ProjectionSnapshot, Vec<crate::PendingBlobCleanup>, MutationReceipt)> {
    let roots = roots.clone();
    let mut media_ids = RoaringBitmap::new();
    let mut collection_ids = Vec::new();
    for root_id in &roots {
        let root_id = RootId(root_id);
        if let Some(members) = snapshot.collection_orders.get(&root_id) {
            collection_ids.push(root_id);
            media_ids.extend(members.iter().map(|media| media.0));
        } else {
            media_ids.insert(root_id.0);
        }
    }
    let mut item_ids = roots.clone();
    item_ids |= &media_ids;
    stage_delete_ids(transaction, "delete_root", &roots)?;
    stage_delete_ids(transaction, "delete_media", &media_ids)?;
    stage_delete_ids(transaction, "delete_item", &item_ids)?;

    // One set-based read of the deleted media's files. CROSS JOIN keeps
    // SQLite walking the staged ids rather than every media item.
    let mut files = std::collections::HashMap::new();
    {
        let mut statement = transaction.prepare_cached(
            "SELECT file.file_id, file.content_hash, file.file_path,
                    CASE WHEN cloud.remote_present = 1
                         THEN cloud.remote_extension ELSE NULL END
             FROM temp.delete_media selected
             CROSS JOIN media_item media ON media.media_id = selected.local_id
             JOIN media_file file ON file.file_id = media.file_id
             LEFT JOIN cloud_blob_state cloud
               ON cloud.file_hash = file.content_hash",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (file_id, content_hash, file_path, remote_extension) = row?;
            files
                .entry(file_id)
                .or_insert((content_hash, file_path, remote_extension));
        }
    }
    transaction.execute(
        "INSERT INTO deletion_tombstone(stable_key, revision, deleted_at_ms)
         SELECT item.stable_key, ?1, ?2
         FROM temp.delete_item selected CROSS JOIN library_item item
             ON item.local_id = selected.local_id
         WHERE true
         ON CONFLICT(stable_key) DO UPDATE SET
             revision = excluded.revision,
             deleted_at_ms = excluded.deleted_at_ms",
        rusqlite::params![revision as i64, deleted_at_ms],
    )?;
    transaction.execute(
        "DELETE FROM root_fts WHERE rowid IN
             (SELECT local_id FROM temp.delete_root)",
        [],
    )?;
    transaction.execute(
        "DELETE FROM library_root WHERE root_id IN
             (SELECT local_id FROM temp.delete_root)",
        [],
    )?;
    for collection_id in &collection_ids {
        ordering::delete(transaction, OrderOwnerKind::Collection, collection_id.0)?;
    }
    transaction.execute(
        "DELETE FROM media_item WHERE media_id IN
             (SELECT local_id FROM temp.delete_media)",
        [],
    )?;
    transaction.execute(
        "DELETE FROM library_item WHERE local_id IN
             (SELECT local_id FROM temp.delete_item)",
        [],
    )?;

    let mut cleanup = Vec::new();
    let (device_id, retention_json) = transaction.query_row(
        "SELECT device_id, retention_json FROM cloud_state WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    let retention_days = serde_json::from_str::<serde_json::Value>(&retention_json)
        .ok()
        .and_then(|value| value.get("deleted_blobs_days")?.as_i64())
        .unwrap_or(7)
        .clamp(0, 3_650);
    let deleted_at =
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(deleted_at_ms)
            .ok_or_else(|| {
                LibraryError::InvalidInput("deletion timestamp is outside range".into())
            })?;
    let purge_after = deleted_at + chrono::Duration::days(retention_days);

    // Files another media item still uses survive; one query finds them.
    let still_referenced = {
        let ids = serde_json::to_string(&files.keys().collect::<Vec<_>>())?;
        let mut statement = transaction.prepare_cached(
            "SELECT DISTINCT media.file_id FROM media_item media
             WHERE media.file_id IN (SELECT value FROM json_each(?1))",
        )?;
        let rows = statement.query_map([ids], |row| row.get::<_, u32>(0))?;
        rows.collect::<std::result::Result<std::collections::HashSet<_>, _>>()?
    };
    for (file_id, (content_hash, file_path, remote_extension)) in files {
        if !still_referenced.contains(&file_id) {
            if let Some(remote_extension) = remote_extension {
                transaction.execute(
                    "INSERT INTO cloud_tombstone
                         (object_kind, object_key, mutation_id, hlc_physical_ms,
                          hlc_logical, device_id, causal_frontier_json,
                          deleted_at, purge_after)
                     VALUES ('blob', ?1, ?2, ?3, 0, ?4, '{}', ?5, ?6)
                     ON CONFLICT(object_kind, object_key) DO UPDATE SET
                         mutation_id = excluded.mutation_id,
                         hlc_physical_ms = excluded.hlc_physical_ms,
                         hlc_logical = excluded.hlc_logical,
                         device_id = excluded.device_id,
                         causal_frontier_json = excluded.causal_frontier_json,
                         deleted_at = excluded.deleted_at,
                         purge_after = excluded.purge_after",
                    rusqlite::params![
                        format!("{content_hash}.{remote_extension}"),
                        uuid::Uuid::new_v4().to_string(),
                        deleted_at_ms,
                        device_id,
                        deleted_at.to_rfc3339(),
                        purge_after.to_rfc3339(),
                    ],
                )?;
            }
            transaction.execute(
                "INSERT INTO blob_cleanup_queue(file_id, file_path, enqueued_revision)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(file_id) DO NOTHING",
                rusqlite::params![file_id, file_path, revision as i64],
            )?;
            let file_id = crate::FileId(file_id);
            if !protected_cleanup_files.contains(&file_id) {
                cleanup.push(crate::PendingBlobCleanup {
                    file_id,
                    content_hash,
                    file_path,
                });
            }
        }
    }

    let mut next = (*snapshot).clone();
    for lifecycle in Lifecycle::ALL {
        let values = Arc::make_mut(&mut next.lifecycle)
            .entry(lifecycle)
            .or_default();
        if !values.is_disjoint(&roots) {
            *values -= &roots;
            bitmap::replace_changed(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Lifecycle,
                    key_id: lifecycle.bitmap_key(),
                },
                values,
                &roots,
            )?;
        }
    }
    for rating in Rating::ALL {
        let values = Arc::make_mut(&mut next.ratings).entry(rating).or_default();
        if !values.is_disjoint(&roots) {
            *values -= &roots;
            bitmap::replace_changed(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Rating,
                    key_id: rating.bitmap_key(),
                },
                values,
                &roots,
            )?;
        }
    }
    let changed_tags = next
        .tags
        .iter()
        .filter_map(|(tag_id, members)| {
            (!members.is_disjoint(&roots)).then_some(*tag_id)
        })
        .collect::<Vec<_>>();
    for tag_id in changed_tags {
        let members = Arc::make_mut(&mut next.tags).get_mut(&tag_id).unwrap();
        *members -= &roots;
        bitmap::replace_changed(
            transaction,
            revision,
            BitmapKey {
                domain: BitmapDomain::Tag,
                key_id: tag_id.0,
            },
            members,
            &roots,
        )?;
    }
    // Folder bitmaps say which orders hold a deleted root; scanning
    // every order instead walked every membership in the library.
    let changed_folders = next
        .folders
        .iter()
        .filter_map(|(folder_id, members)| {
            (!members.is_disjoint(&roots)).then_some(*folder_id)
        })
        .collect::<Vec<_>>();
    for folder_id in changed_folders {
        let mut order = next.folder_orders[&folder_id].as_ref().clone();
        order.retain(|root| !roots.contains(root.0));
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Folder,
            folder_id.0,
            &order.iter().map(|root| root.0).collect::<Vec<_>>(),
        )?;
        Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(order));
        if let Some(members) = Arc::make_mut(&mut next.folders).get_mut(&folder_id) {
            members.subtract(&roots);
        }
    }
    for collection_id in &collection_ids {
        Arc::make_mut(&mut next.collection_orders).remove(collection_id);
    }
    for media_id in &media_ids {
        Arc::make_mut(&mut next.media_owner).remove(media_id);
        Arc::make_mut(&mut next.image_media).remove(media_id);
    }
    crate::group::remove_root_projections(&mut next, &roots);
    crate::smart::settle_affected(transaction, &mut next, &roots)?;
    transaction.execute(
        "INSERT INTO cloud_journal
             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
         VALUES (?1, 'root.permanent_delete', ?2, '{}', ?3)",
        rusqlite::params![
            revision as i64,
            crate::bitmap::encode(&roots)?,
            deleted_at_ms
        ],
    )?;
    next.revision = revision;
    let receipt = PublicationCoordinator::receipt(
        revision,
        vec![
            "roots".into(),
            "sidebar".into(),
            "tags".into(),
            "folders".into(),
            "collections".into(),
            "search".into(),
        ],
        roots.iter().map(RootId),
    );
    Ok((next, cleanup, receipt))
}

fn ids_json(ids: impl IntoIterator<Item = u32>) -> Result<String> {
    Ok(serde_json::to_string(&ids.into_iter().collect::<Vec<_>>())?)
}

/// What a permanent delete of `roots` removes, read before it does.
pub(super) fn capture_deleted_roots(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    roots: &RoaringBitmap,
) -> Result<crate::history::DeletedRootsState> {
    let mut media = RoaringBitmap::new();
    for root_id in roots {
        match snapshot.collection_orders.get(&RootId(root_id)) {
            Some(members) => media.extend(members.iter().map(|member| member.0)),
            None => {
                media.insert(root_id);
            }
        }
    }
    let roots_json = ids_json(roots.iter())?;
    let media_json = ids_json(media.iter())?;
    let items_json = ids_json((roots | &media).iter())?;
    let items = transaction
        .prepare(
            "SELECT local_id, stable_key, item_kind FROM library_item
             WHERE local_id IN (SELECT value FROM json_each(?1)) ORDER BY local_id",
        )?
        .query_map([&items_json], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<Vec<(u32, String, u8)>>>()?;
    let media_rows = transaction
        .prepare(
            "SELECT media_id, media_name, media_notes, file_id FROM media_item
             WHERE media_id IN (SELECT value FROM json_each(?1)) ORDER BY media_id",
        )?
        .query_map([&media_json], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
        .collect::<rusqlite::Result<Vec<crate::history::DeletedMediaRow>>>()?;
    let provenance = transaction
        .prepare(
            "SELECT source_key, source_item_key, media_id, source_text FROM source_provenance
             WHERE media_id IN (SELECT value FROM json_each(?1))",
        )?
        .query_map([&media_json], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let source_posts = transaction
        .prepare(
            "SELECT source_post_id, root_item_id FROM source_post
             WHERE root_item_id IN (SELECT value FROM json_each(?1))",
        )?
        .query_map([&roots_json], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let source_items = transaction
        .prepare(
            "SELECT source_item_id, media_item_id FROM source_item
             WHERE media_item_id IN (SELECT value FROM json_each(?1))",
        )?
        .query_map([&media_json], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let folder_covers = transaction
        .prepare(
            "SELECT folder_id, cover_root_id FROM folder_definition
             WHERE cover_root_id IN (SELECT value FROM json_each(?1))",
        )?
        .query_map([&roots_json], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let files = media_rows
        .iter()
        .map(|(_, _, _, file_id)| crate::FileId(*file_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(crate::history::DeletedRootsState {
        roots: Arc::new(roots.clone()),
        structure: capture_structure(transaction, snapshot, roots, None)?,
        items: Arc::new(items),
        media: Arc::new(media_rows),
        image_media: Arc::new(snapshot.image_media.as_ref() & &media),
        provenance: Arc::new(provenance),
        source_posts: Arc::new(source_posts),
        source_items: Arc::new(source_items),
        folder_covers: Arc::new(folder_covers),
        files: Arc::new(files),
    })
}

/// Undo a permanent delete: put back every row it removed and rebuild the
/// roots' projections. Fails if the roots came back some other way or a file
/// is already gone.
pub(super) fn restore_deleted_roots(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    state: &crate::history::DeletedRootsState,
) -> Result<()> {
    let roots = state.roots.as_ref();
    let items_json = ids_json(state.items.iter().map(|(id, _, _)| *id))?;
    let present: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM library_item WHERE local_id IN (SELECT value FROM json_each(?1))",
        [&items_json],
        |row| row.get(0),
    )?;
    if present != 0 {
        return Err(LibraryError::InvalidState(
            "cannot undo the delete because its items exist again".into(),
        ));
    }
    let files_json = ids_json(state.files.iter().map(|file| file.0))?;
    let files: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM media_file WHERE file_id IN (SELECT value FROM json_each(?1))",
        [&files_json],
        |row| row.get(0),
    )?;
    if usize::try_from(files).ok() != Some(state.files.len()) {
        return Err(LibraryError::InvalidState(
            "cannot undo the delete because its files were already removed".into(),
        ));
    }
    // One file is one item: a file imported again since cannot come back
    // as a second one.
    let reimported: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM media_item WHERE file_id IN (SELECT value FROM json_each(?1))",
        [&files_json],
        |row| row.get(0),
    )?;
    if reimported != 0 {
        return Err(LibraryError::InvalidState(
            "cannot undo the delete because its files are in the library again".into(),
        ));
    }

    let mut insert_item = transaction
        .prepare_cached("INSERT INTO library_item(local_id, stable_key, item_kind) VALUES (?1, ?2, ?3)")?;
    for (id, key, kind) in state.items.iter() {
        insert_item.execute(rusqlite::params![id, key, kind])?;
    }
    drop(insert_item);
    let mut insert_media = transaction.prepare_cached(
        "INSERT INTO media_item(media_id, media_name, media_notes, file_id) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (id, name, notes, file_id) in state.media.iter() {
        insert_media.execute(rusqlite::params![id, name, notes, file_id])?;
    }
    drop(insert_media);
    for root in state.structure.roots.iter() {
        transaction.execute(
            "INSERT INTO library_root
                 (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
                  captured_at_ms, modified_at_ms, media_count, total_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                root.root_id.0,
                root.name,
                root.notes,
                root.source_urls_json,
                root.cover_media_id.0,
                root.imported_at_ms,
                root.captured_at_ms,
                root.modified_at_ms,
                root.media_count,
                i64::try_from(root.total_size_bytes).map_err(|_| {
                    LibraryError::InvalidState("root size exceeds SQLite range".into())
                })?
            ],
        )?;
        crate::fts::mark_one(transaction, root.root_id, root.modified_at_ms)?;
    }
    let mut provenance = transaction.prepare_cached(
        "INSERT INTO source_provenance (source_key, source_item_key, media_id, source_text)
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
    )?;
    for (key, item, media_id, text) in state.provenance.iter() {
        provenance.execute(rusqlite::params![key, item, media_id, text])?;
    }
    drop(provenance);
    for (post, root) in state.source_posts.iter() {
        transaction.execute(
            "UPDATE source_post SET root_item_id = ?2 WHERE source_post_id = ?1 AND root_item_id IS NULL",
            rusqlite::params![post, root],
        )?;
    }
    for (item, media_id) in state.source_items.iter() {
        transaction.execute(
            "UPDATE source_item SET media_item_id = ?2 WHERE source_item_id = ?1 AND media_item_id IS NULL",
            rusqlite::params![item, media_id],
        )?;
    }
    for (folder, root) in state.folder_covers.iter() {
        transaction.execute(
            "UPDATE folder_definition SET cover_root_id = ?2 WHERE folder_id = ?1 AND cover_root_id IS NULL",
            rusqlite::params![folder, root],
        )?;
    }
    transaction.execute(
        "UPDATE source_attempt_root
         SET root_id = (SELECT local_id FROM library_item WHERE stable_key = root_stable_key)
         WHERE root_id IS NULL
           AND root_stable_key IN (
               SELECT stable_key FROM library_item WHERE local_id IN (SELECT value FROM json_each(?1))
           )",
        [&items_json],
    )?;
    // The delete's tombstones and queued file removals no longer apply.
    transaction.execute(
        "DELETE FROM deletion_tombstone WHERE stable_key IN (
             SELECT stable_key FROM library_item WHERE local_id IN (SELECT value FROM json_each(?1))
         )",
        [&items_json],
    )?;
    transaction.execute(
        "DELETE FROM cloud_tombstone
         WHERE object_kind = 'blob' AND EXISTS (
             SELECT 1 FROM media_file file
             WHERE file.file_id IN (SELECT value FROM json_each(?1))
               AND (cloud_tombstone.object_key = file.content_hash
                    OR cloud_tombstone.object_key LIKE file.content_hash || '.%')
         )",
        [&files_json],
    )?;
    transaction.execute(
        "DELETE FROM blob_cleanup_queue WHERE file_id IN (SELECT value FROM json_each(?1))",
        [&files_json],
    )?;

    restore_structure_bitmaps(transaction, revision, snapshot, roots, &state.structure)?;
    restore_structure_orders(transaction, revision, snapshot, roots, &state.structure)?;
    for root in state.structure.roots.iter().filter(|root| root.kind == RootKind::Media) {
        Arc::make_mut(&mut snapshot.media_owner).insert(root.root_id.0, root.root_id);
    }
    for (root, members) in &state.structure.collections {
        for media in members {
            Arc::make_mut(&mut snapshot.media_owner).insert(media.0, *root);
        }
    }
    *Arc::make_mut(&mut snapshot.image_media) |= state.image_media.as_ref();
    crate::group::refresh_structural_projections(transaction, snapshot, roots)?;
    snapshot.revision = revision;
    Ok(())
}

pub(super) fn restore_structure(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
    expected: &StructuralState,
    replacement: &StructuralState,
) -> Result<()> {
    if load_structural_roots(transaction, affected)? != *expected.roots {
        return Err(LibraryError::InvalidState(
            "cannot replay history because collection structure changed".into(),
        ));
    }
    if load_structural_media_notes(transaction, snapshot, affected)? != *expected.media_notes {
        return Err(LibraryError::InvalidState(
            "cannot replay history because retained media notes changed".into(),
        ));
    }

    let current_collections = affected
        .iter()
        .filter_map(|id| {
            snapshot
                .collection_orders
                .get(&RootId(id))
                .map(|members| (RootId(id), members.as_ref().clone()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    if structural_memberships(snapshot, affected, None) != expected.memberships
        || current_collections != expected.collections
    {
        return Err(LibraryError::InvalidState(
            "cannot replay history because affected memberships changed".into(),
        ));
    }

    for root_id in affected {
        if snapshot.collection_orders.contains_key(&RootId(root_id)) {
            ordering::delete(transaction, OrderOwnerKind::Collection, root_id)?;
        }
        crate::fts::remove_root(transaction, root_id)?;
        transaction.execute("DELETE FROM library_root WHERE root_id = ?1", [root_id])?;
        transaction.execute(
            "DELETE FROM library_item WHERE local_id = ?1 AND item_kind = 2",
            [root_id],
        )?;
    }

    for root in replacement.roots.iter() {
        match root.kind {
            RootKind::Media => {
                let valid = transaction.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM library_item
                         WHERE local_id = ?1 AND item_kind = 1
                     )",
                    [root.root_id.0],
                    |row| row.get::<_, bool>(0),
                )?;
                if !valid {
                    return Err(LibraryError::InvalidState(format!(
                        "media {} lost its canonical item",
                        root.root_id.0
                    )));
                }
            }
            RootKind::Collection => {
                transaction.execute(
                    "INSERT INTO library_item(local_id, stable_key, item_kind)
                     VALUES (?1, ?2, 2)",
                    rusqlite::params![root.root_id.0, root.stable_key],
                )?;
            }
        }
        transaction.execute(
            "INSERT INTO library_root
                 (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
                  captured_at_ms, modified_at_ms, media_count, total_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                root.root_id.0,
                root.name,
                root.notes,
                root.source_urls_json,
                root.cover_media_id.0,
                root.imported_at_ms,
                root.captured_at_ms,
                root.modified_at_ms,
                root.media_count,
                i64::try_from(root.total_size_bytes).map_err(|_| {
                    LibraryError::InvalidState("root size exceeds SQLite range".into())
                })?
            ],
        )?;
        crate::fts::mark_one(transaction, root.root_id, root.modified_at_ms)?;
    }

    let mut restore_media_notes =
        transaction.prepare_cached("UPDATE media_item SET media_notes = ?2 WHERE media_id = ?1")?;
    for media in replacement.media_notes.iter() {
        restore_media_notes.execute(rusqlite::params![media.media_id.0, media.notes])?;
    }
    drop(restore_media_notes);

    restore_structure_bitmaps(transaction, revision, snapshot, affected, replacement)?;
    restore_structure_orders(transaction, revision, snapshot, affected, replacement)?;
    for media in expected.media_notes.iter() {
        Arc::make_mut(&mut snapshot.media_owner).remove(media.media_id.0);
    }
    for root in replacement
        .roots
        .iter()
        .filter(|root| root.kind == RootKind::Media)
    {
        Arc::make_mut(&mut snapshot.media_owner).insert(root.root_id.0, root.root_id);
    }
    for (root, members) in &replacement.collections {
        for media in members {
            Arc::make_mut(&mut snapshot.media_owner).insert(media.0, *root);
        }
    }
    crate::group::refresh_structural_projections(transaction, snapshot, affected)?;
    snapshot.revision = revision;
    Ok(())
}

pub(super) fn restore_structure_bitmaps(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    current: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
    replacement: &StructuralState,
) -> Result<()> {
    let keys = structural_memberships(current, affected, None)
        .keys()
        .chain(replacement.memberships.keys())
        .copied()
        .collect::<HashSet<_>>();
    for key in keys {
        let before = projection_bitmap(current, key);
        let mut after = &before - affected;
        if let Some(ids) = replacement.memberships.get(&key) {
            after |= ids;
        }
        if before != after {
            bitmap::replace_changed(transaction, revision, key, &after, &(&before ^ &after))?;
            set_projection_bitmap(current, key, after);
        }
    }
    Ok(())
}

pub(super) fn restore_structure_orders(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    current: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
    replacement: &StructuralState,
) -> Result<()> {
    let folders = current
        .folders
        .iter()
        .filter(|(_, members)| !members.is_disjoint(affected))
        .map(|(id, _)| *id)
        .chain(replacement.folders.keys().copied())
        .collect::<BTreeSet<_>>();
    for folder_id in folders {
        let retained = current
            .folder_orders
            .get(&folder_id)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter(|id| !affected.contains(id.0))
            .copied()
            .collect::<Vec<_>>();
        let after = merge_structural_folder_positions(
            &retained,
            replacement
                .folders
                .get(&folder_id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        );
        ordering::replace(
            transaction,
            revision,
            OrderOwnerKind::Folder,
            folder_id.0,
            &after.iter().map(|root| root.0).collect::<Vec<_>>(),
        )?;
        Arc::make_mut(&mut current.folders)
            .insert(folder_id, after.iter().map(|root| root.0).collect());
        Arc::make_mut(&mut current.folder_orders).insert(folder_id, Arc::new(after));
    }

    for id in affected {
        let root_id = RootId(id);
        if let Some(after) = replacement.collections.get(&root_id) {
            ordering::replace(
                transaction,
                revision,
                OrderOwnerKind::Collection,
                root_id.0,
                &after.iter().map(|media| media.0).collect::<Vec<_>>(),
            )?;
            Arc::make_mut(&mut current.collection_orders).insert(root_id, Arc::new(after.clone()));
        } else {
            ordering::delete(transaction, OrderOwnerKind::Collection, root_id.0)?;
            Arc::make_mut(&mut current.collection_orders).remove(&root_id);
        }
    }
    Ok(())
}

pub(super) fn merge_structural_folder_positions(
    retained: &[RootId],
    positions: &[crate::history::StructuralFolderPosition],
) -> Vec<RootId> {
    // Reinsert blocks in one pass rather than repeatedly shifting a large folder.
    let anchors = retained
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect::<HashMap<_, _>>();
    let mut restored = HashMap::new();
    let mut buckets: std::collections::BTreeMap<usize, Vec<RootId>> =
        std::collections::BTreeMap::new();
    for position in positions {
        let slot = position
            .previous
            .and_then(|id| {
                restored
                    .get(&id)
                    .copied()
                    .or_else(|| anchors.get(&id).map(|index| index + 1))
            })
            .or_else(|| {
                position
                    .next_unaffected
                    .and_then(|id| anchors.get(&id).copied())
            })
            .unwrap_or(position.ordinal.min(retained.len()));
        buckets.entry(slot).or_default().push(position.root_id);
        restored.insert(position.root_id, slot);
    }
    let mut result = Vec::with_capacity(retained.len() + positions.len());
    for (index, id) in retained.iter().enumerate() {
        if let Some(roots) = buckets.remove(&index) {
            result.extend(roots);
        }
        result.push(*id);
    }
    if let Some(roots) = buckets.remove(&retained.len()) {
        result.extend(roots);
    }
    result
}

pub(super) fn capture_structure(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    affected: &RoaringBitmap,
    tags: Option<&[crate::TagId]>,
) -> Result<StructuralState> {
    let mut folders = std::collections::BTreeMap::new();
    let dense = crate::bitmap::DenseIds::new(affected);
    for (folder, order) in snapshot.folder_orders.iter() {
        if snapshot.folders.get(folder).is_none_or(|members| members.is_disjoint(affected)) {
            continue;
        }
        let mut next_unaffected = None;
        let mut positions = Vec::new();
        for (ordinal, root_id) in order.iter().enumerate().rev() {
            if dense.contains(root_id.0) {
                positions.push(crate::history::StructuralFolderPosition {
                    root_id: *root_id,
                    previous: ordinal.checked_sub(1).map(|index| order[index]),
                    next_unaffected,
                    ordinal,
                });
            } else {
                next_unaffected = Some(*root_id);
            }
        }
        if !positions.is_empty() {
            positions.reverse();
            folders.insert(*folder, positions);
        }
    }
    Ok(StructuralState {
        roots: Arc::new(load_structural_roots(transaction, affected)?),
        media_notes: Arc::new(load_structural_media_notes(
            transaction,
            snapshot,
            affected,
        )?),
        memberships: structural_memberships(snapshot, affected, tags),
        folders,
        collections: affected
            .iter()
            .filter_map(|id| {
                snapshot
                    .collection_orders
                    .get(&RootId(id))
                    .map(|members| (RootId(id), members.as_ref().clone()))
            })
            .collect(),
    })
}

/// Every lifecycle, rating and tag membership of `affected`. With `tags`, only
/// those tags are checked: callers pass the tags captured before an operation
/// that cannot add a root to any other tag. Without it every tag is checked,
/// in parallel on large tag sets.
pub(super) fn structural_memberships(
    snapshot: &ProjectionSnapshot,
    affected: &RoaringBitmap,
    tags: Option<&[crate::TagId]>,
) -> HashMap<BitmapKey, RoaringBitmap> {
    let membership = |key: BitmapKey, members: &RoaringBitmap| {
        if members.is_disjoint(affected) {
            return None;
        }
        Some((key, members & affected))
    };
    let tag_key = |tag_id: crate::TagId| BitmapKey {
        domain: BitmapDomain::Tag,
        key_id: tag_id.0,
    };
    let mut memberships = match tags {
        Some(tags) => tags
            .iter()
            .filter_map(|tag_id| {
                snapshot
                    .tags
                    .get(tag_id)
                    .and_then(|members| membership(tag_key(*tag_id), members))
            })
            .collect::<HashMap<_, _>>(),
        None => {
            let entries = snapshot.tags.iter().collect::<Vec<_>>();
            let scan = |(tag_id, members): &(&crate::TagId, &crate::projection::SharedBitmap)| {
                membership(tag_key(**tag_id), members)
            };
            match crate::smart::smart_folder_pool() {
                Ok(pool) if entries.len() >= 1_024 => {
                    use rayon::prelude::*;
                    pool.install(|| entries.par_iter().filter_map(scan).collect())
                }
                _ => entries.iter().filter_map(scan).collect(),
            }
        }
    };
    let entries = snapshot
        .lifecycle
        .iter()
        .map(|(value, ids)| {
            (
                BitmapKey {
                    domain: BitmapDomain::Lifecycle,
                    key_id: value.bitmap_key(),
                },
                ids,
            )
        })
        .chain(snapshot.ratings.iter().map(|(value, ids)| {
            (
                BitmapKey {
                    domain: BitmapDomain::Rating,
                    key_id: value.bitmap_key(),
                },
                ids,
            )
        }))
        ;
    memberships.extend(entries.filter_map(|(key, members)| membership(key, members)));
    memberships
}

/// The tags `state` found holding any of its roots.
pub(super) fn captured_tags(state: &StructuralState) -> Vec<crate::TagId> {
    state
        .memberships
        .keys()
        .filter(|key| key.domain == BitmapDomain::Tag)
        .map(|key| crate::TagId(key.key_id))
        .collect()
}

pub(super) fn load_structural_media_notes(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    affected: &RoaringBitmap,
) -> Result<Vec<StructuralMediaNoteState>> {
    let mut media_ids = BTreeSet::new();
    for id in affected {
        if let Some(members) = snapshot.collection_orders.get(&RootId(id)) {
            media_ids.extend(members.iter().map(|media| media.0));
        } else if snapshot.media_owner.get(id) == Some(&RootId(id)) {
            media_ids.insert(id);
        }
    }
    let mut statement =
        transaction.prepare_cached("SELECT media_notes FROM media_item WHERE media_id = ?1")?;
    media_ids
        .into_iter()
        .map(|media_id| {
            statement
                .query_row([media_id], |row| {
                    Ok(StructuralMediaNoteState {
                        media_id: MediaId(media_id),
                        notes: row.get(0)?,
                    })
                })
                .map_err(Into::into)
        })
        .collect()
}

pub(super) fn load_structural_roots(
    transaction: &rusqlite::Transaction<'_>,
    affected: &RoaringBitmap,
) -> Result<Vec<StructuralRootState>> {
    let mut statement = transaction.prepare_cached(
        "SELECT item.stable_key, item.item_kind, root.name, root.notes,
                root.source_urls_json, root.cover_media_id, root.imported_at_ms,
                root.captured_at_ms, root.modified_at_ms, root.media_count,
                root.total_size_bytes
         FROM library_root root
         JOIN library_item item ON item.local_id = root.root_id
         WHERE root.root_id = ?1",
    )?;
    let mut roots = Vec::new();
    for root_id in affected {
        let state = statement.query_row([root_id], |row| {
            Ok(StructuralRootState {
                root_id: RootId(root_id),
                stable_key: row.get(0)?,
                kind: match row.get::<_, u8>(1)? {
                    1 => RootKind::Media,
                    2 => RootKind::Collection,
                    _ => unreachable!("schema constrains root kinds"),
                },
                name: row.get(2)?,
                notes: row.get(3)?,
                source_urls_json: row.get(4)?,
                cover_media_id: MediaId(row.get(5)?),
                imported_at_ms: row.get(6)?,
                captured_at_ms: row.get(7)?,
                modified_at_ms: row.get(8)?,
                media_count: row.get(9)?,
                total_size_bytes: row.get::<_, i64>(10)? as u64,
            })
        });
        match state {
            Ok(state) => roots.push(state),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(roots)
}

pub(super) fn stage_delete_ids(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    ids: &RoaringBitmap,
) -> Result<()> {
    let table = match table {
        "delete_root" | "delete_media" | "delete_item" => table,
        _ => {
            return Err(LibraryError::InvalidInput(format!(
                "unsupported deletion staging table {table}"
            )))
        }
    };
    transaction.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table} (
             local_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM {table};"
    ))?;
    let mut insert =
        transaction.prepare_cached(&format!("INSERT INTO temp.{table}(local_id) VALUES (?1)"))?;
    for id in ids {
        insert.execute([id])?;
    }
    Ok(())
}
