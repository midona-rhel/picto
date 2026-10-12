//! One file, one media item: merging two items that hold the same file (or a
//! duplicate's file) into the one that stays.
//!
//! A merge is a sequence of ordinary undoable changes, so a duplicate
//! resolution can be undone exactly:
//! 1. a merged-away collection member leaves its collection (a structural
//!    detach); a collection it leaves empty is removed, and what pointed at the
//!    collection points at the item that stays;
//! 2. a loose item hands its tags, folders, rating, lifecycle, notes and source
//!    links to the root that stays;
//! 3. its source links and stable key move to the item that stays;
//! 4. it is deleted (its file stays when the item that stays uses it).

use super::*;
use crate::history::{RelinkState, RelinkedProvenance, RootTextState};

/// What merges changed, in the order they were applied.
#[derive(Default)]
pub(super) struct Merge {
    /// Whether to keep what undo needs. A repair that clears history skips it.
    pub record: bool,
    pub changes: Vec<SemanticChange>,
    pub affected: RoaringBitmap,
    pub resources: BTreeSet<String>,
    /// Collections a merge emptied and removed.
    pub removed_collections: u64,
}

fn owner(snapshot: &ProjectionSnapshot, media: MediaId) -> Result<RootId> {
    snapshot
        .media_owner
        .get(media.0)
        .copied()
        .ok_or_else(|| LibraryError::InvalidState(format!("media {} has no owning root", media.0)))
}

fn lifecycle_of(snapshot: &ProjectionSnapshot, root: RootId) -> Result<Lifecycle> {
    Lifecycle::ALL
        .into_iter()
        .find(|value| snapshot.lifecycle(*value).contains(root.0))
        .ok_or_else(|| LibraryError::InvalidState(format!("root {root} has no lifecycle")))
}

fn rating_of(snapshot: &ProjectionSnapshot, root: RootId) -> Result<Rating> {
    Rating::ALL
        .into_iter()
        .find(|value| snapshot.rating(*value).contains(root.0))
        .ok_or_else(|| LibraryError::InvalidState(format!("root {root} has no rating")))
}

fn rating_rank(rating: Rating) -> usize {
    Rating::ALL
        .iter()
        .position(|value| *value == rating)
        .unwrap_or(0)
}

/// Merge `loser` into `survivor`, inside a publication. `now_ms` stamps what
/// the merge modifies.
pub(super) fn merge_media(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    survivor: MediaId,
    loser: MediaId,
    now_ms: i64,
    merge: &mut Merge,
) -> Result<()> {
    if survivor == loser {
        return Err(LibraryError::InvalidInput(
            "an item cannot be merged into itself".into(),
        ));
    }
    let survivor_root = owner(snapshot, survivor)?;
    let mut loser_root = owner(snapshot, loser)?;
    let mut hands_over = true;
    if loser_root.0 != loser.0 {
        let collection = loser_root;
        let members = snapshot
            .collection_orders
            .get(&collection)
            .map_or(0, |members| members.len());
        // A member leaves its collection alone; only a collection it empties
        // hands its tags, folders and links on to the item that stays.
        let empties = members == 1;
        hands_over = empties;
        if empties {
            let relink = relink_root(transaction, collection, survivor_root)?;
            push_relink(transaction, relink, merge)?;
            merge.removed_collections += 1;
        }
        let affected = RoaringBitmap::from_iter([collection.0, loser.0]);
        let before = merge
            .record
            .then(|| capture_structure(transaction, snapshot, &affected, None))
            .transpose()?;
        let tags = before.as_ref().map(captured_tags);
        let output = crate::group::detach_many(
            transaction,
            revision,
            snapshot.clone(),
            collection,
            &[loser],
            None,
            now_ms,
            tags.as_deref(),
        )?;
        *snapshot = output.snapshot;
        merge.affected |= &output.affected;
        if let Some(before) = before {
            let after = capture_structure(transaction, snapshot, &output.affected, tags.as_deref())?;
            merge.changes.push(SemanticChange::Structure {
                affected: Arc::new(output.affected),
                before,
                after,
            });
        }
        merge.resources.extend(
            ["roots", "collections", "tags", "folders", "sidebar", "search"].map(str::to_owned),
        );
        loser_root = RootId(loser.0);
    }

    if hands_over && loser_root != survivor_root {
        for change in hand_over(transaction, snapshot, loser_root, survivor_root, now_ms)? {
            apply_semantic_change(
                transaction,
                revision,
                snapshot,
                &change,
                true,
                &mut merge.affected,
                &mut merge.resources,
            )?;
            if merge.record {
                merge.changes.push(change);
            }
        }
    }

    let relink = relink_media(transaction, loser, survivor, loser_root, survivor_root)?;
    push_relink(transaction, relink, merge)?;

    let roots = RoaringBitmap::from_iter([loser_root.0]);
    let state = merge
        .record
        .then(|| capture_deleted_roots(transaction, snapshot, &roots))
        .transpose()?;
    // The merged-away file (a duplicate's) is freed by the cleanup worker once
    // no history entry needs it; nothing is removed here.
    let protected = std::collections::BTreeSet::new();
    let (next, _, receipt) = delete_roots(transaction, revision, snapshot, &roots, now_ms, &protected)?;
    *snapshot = next;
    merge.affected |= &roots;
    merge.affected.insert(survivor_root.0);
    merge.resources.extend(receipt.resources);
    if let Some(state) = state {
        merge.changes.push(SemanticChange::PermanentDelete(Arc::new(state)));
    }
    Ok(())
}

/// The changes that give `target` what `source` has and it lacks: tags,
/// folder places, the higher rating, the lifecycle that keeps it most, notes
/// and source links.
fn hand_over(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    source: RootId,
    target: RootId,
    now_ms: i64,
) -> Result<Vec<SemanticChange>> {
    let mut changes = Vec::new();
    let only_target = || Arc::new(RoaringBitmap::from_iter([target.0]));
    let nothing = || Arc::new(RoaringBitmap::new());
    for (tag_id, members) in snapshot.tags.iter() {
        if members.contains(source.0) && !members.contains(target.0) {
            changes.push(SemanticChange::Bitmap {
                key: BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                },
                before: nothing(),
                after: only_target(),
            });
        }
    }
    let (source_rating, target_rating) = (rating_of(snapshot, source)?, rating_of(snapshot, target)?);
    if rating_rank(source_rating) > rating_rank(target_rating) {
        for (rating, before, after) in [
            (target_rating, only_target(), nothing()),
            (source_rating, nothing(), only_target()),
        ] {
            changes.push(SemanticChange::Bitmap {
                key: BitmapKey {
                    domain: BitmapDomain::Rating,
                    key_id: rating.bitmap_key(),
                },
                before,
                after,
            });
        }
    }
    let (source_lifecycle, target_lifecycle) =
        (lifecycle_of(snapshot, source)?, lifecycle_of(snapshot, target)?);
    if source_lifecycle.keep_rank() > target_lifecycle.keep_rank() {
        for (lifecycle, before, after) in [
            (target_lifecycle, only_target(), nothing()),
            (source_lifecycle, nothing(), only_target()),
        ] {
            changes.push(SemanticChange::Bitmap {
                key: BitmapKey {
                    domain: BitmapDomain::Lifecycle,
                    key_id: lifecycle.bitmap_key(),
                },
                before,
                after,
            });
        }
    }
    // The target takes the source's place in folders it is not in yet; where
    // it already is, it keeps its own place.
    for (folder_id, members) in snapshot.folders.iter() {
        if !members.contains(source.0) || members.contains(target.0) {
            continue;
        }
        let before = snapshot
            .folder_orders
            .get(folder_id)
            .map(|order| order.iter().map(|root| root.0).collect::<Vec<_>>())
            .unwrap_or_default();
        let after = before
            .iter()
            .map(|root| if *root == source.0 { target.0 } else { *root })
            .collect::<Vec<_>>();
        changes.push(SemanticChange::Order {
            owner_kind: OrderOwnerKind::Folder,
            owner_id: folder_id.0,
            before: Arc::new(before),
            after: Arc::new(after),
        });
    }
    let source_text = root_text(transaction, source)?;
    let target_text = root_text(transaction, target)?;
    let mut merged = target_text.clone();
    for url in &source_text.source_urls {
        if !merged.source_urls.contains(url) {
            merged.source_urls.push(url.clone());
        }
    }
    let present = |notes: &Option<String>| {
        notes
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
            .map(str::to_owned)
    };
    if let Some(source_notes) = present(&source_text.notes) {
        merged.notes = match present(&target_text.notes) {
            None => Some(source_notes),
            Some(target_notes) if target_notes.contains(&source_notes) => target_text.notes.clone(),
            Some(target_notes) => Some(format!("{target_notes}\n\n{source_notes}")),
        };
    }
    if merged != target_text {
        merged.modified_at_ms = now_ms;
        changes.push(SemanticChange::RootText {
            before: Arc::new(vec![target_text]),
            after: Arc::new(vec![merged]),
        });
    }
    Ok(changes)
}

fn root_text(transaction: &rusqlite::Transaction<'_>, root: RootId) -> Result<RootTextState> {
    transaction
        .query_row(
            "SELECT name, notes, source_urls_json, modified_at_ms FROM library_root WHERE root_id = ?1",
            [root.0],
            |row| {
                Ok(RootTextState {
                    root_id: root,
                    name: row.get(0)?,
                    notes: row.get(1)?,
                    source_urls: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                    modified_at_ms: row.get(3)?,
                })
            },
        )
        .map_err(Into::into)
}

fn push_relink(
    transaction: &rusqlite::Transaction<'_>,
    relink: RelinkState,
    merge: &mut Merge,
) -> Result<()> {
    if relink.is_empty() {
        return Ok(());
    }
    apply_relink(transaction, &relink, true)?;
    merge.resources.insert("subscriptions".into());
    if merge.record {
        merge.changes.push(SemanticChange::Relink(Arc::new(relink)));
    }
    Ok(())
}

/// The source posts and folder covers that point at `from`, moved to `to`.
fn relink_root(
    transaction: &rusqlite::Transaction<'_>,
    from: RootId,
    to: RootId,
) -> Result<RelinkState> {
    let source_posts = transaction
        .prepare_cached("SELECT source_post_id FROM source_post WHERE root_item_id = ?1")?
        .query_map([from.0], |row| row.get::<_, i64>(0))?
        .map(|post| post.map(|post| (post, from.0, to.0)))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let folder_covers = transaction
        .prepare_cached("SELECT folder_id FROM folder_definition WHERE cover_root_id = ?1")?
        .query_map([from.0], |row| row.get::<_, u32>(0))?
        .map(|folder| folder.map(|folder| (folder, from.0, to.0)))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(RelinkState {
        source_posts,
        folder_covers,
        ..RelinkState::default()
    })
}

/// Everything that names the merged-away item, moved to the one that stays:
/// its source posts, source items, provenance, folder covers, and its stable
/// key (and any it inherited) as aliases.
fn relink_media(
    transaction: &rusqlite::Transaction<'_>,
    loser: MediaId,
    survivor: MediaId,
    loser_root: RootId,
    survivor_root: RootId,
) -> Result<RelinkState> {
    let mut state = relink_root(transaction, loser_root, survivor_root)?;
    state.source_items = transaction
        .prepare_cached("SELECT source_item_id FROM source_item WHERE media_item_id = ?1")?
        .query_map([loser.0], |row| row.get::<_, i64>(0))?
        .map(|item| item.map(|item| (item, loser.0, survivor.0)))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    state.provenance = transaction
        .prepare_cached(
            "SELECT source_key, source_item_key, source_text,
                    EXISTS(SELECT 1 FROM source_provenance other
                           WHERE other.source_key = provenance.source_key
                             AND other.source_item_key = provenance.source_item_key
                             AND other.media_id = ?2)
             FROM source_provenance provenance WHERE media_id = ?1",
        )?
        .query_map([loser.0, survivor.0], |row| {
            Ok(RelinkedProvenance {
                source_key: row.get(0)?,
                source_item_key: row.get(1)?,
                source_text: row.get(2)?,
                from: loser.0,
                to: survivor.0,
                target_had: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    state.aliases = transaction
        .prepare_cached("SELECT stable_key FROM library_item_alias WHERE local_id = ?1")?
        .query_map([loser.0], |row| row.get::<_, String>(0))?
        .map(|key| key.map(|key| (key, Some(loser.0), survivor.0)))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let own_key = transaction.query_row(
        "SELECT stable_key FROM library_item WHERE local_id = ?1",
        [loser.0],
        |row| row.get::<_, String>(0),
    )?;
    state.aliases.push((own_key, None, survivor.0));
    Ok(state)
}

/// Point a relink's rows at their `after` targets, or back at their `before`
/// ones. Fails when a row no longer points where the history expects.
pub(super) fn apply_relink(
    transaction: &rusqlite::Transaction<'_>,
    state: &RelinkState,
    use_after: bool,
) -> Result<()> {
    let direction = |before: u32, after: u32| if use_after { (before, after) } else { (after, before) };
    let changed = |rows: usize, what: &str| {
        if rows == 1 {
            Ok(())
        } else {
            Err(LibraryError::InvalidState(format!(
                "cannot replay history because a {what} changed"
            )))
        }
    };
    for (post, before, after) in &state.source_posts {
        let (from, to) = direction(*before, *after);
        changed(
            transaction.execute(
                "UPDATE source_post SET root_item_id = ?2 WHERE source_post_id = ?1 AND root_item_id = ?3",
                rusqlite::params![post, to, from],
            )?,
            "source post",
        )?;
    }
    for (item, before, after) in &state.source_items {
        let (from, to) = direction(*before, *after);
        changed(
            transaction.execute(
                "UPDATE source_item SET media_item_id = ?2 WHERE source_item_id = ?1 AND media_item_id = ?3",
                rusqlite::params![item, to, from],
            )?,
            "source item",
        )?;
    }
    for (folder, before, after) in &state.folder_covers {
        let (from, to) = direction(*before, *after);
        changed(
            transaction.execute(
                "UPDATE folder_definition SET cover_root_id = ?2 WHERE folder_id = ?1 AND cover_root_id = ?3",
                rusqlite::params![folder, to, from],
            )?,
            "folder cover",
        )?;
    }
    for row in &state.provenance {
        let (from, to) = direction(row.from, row.to);
        // The item that already had this source keeps its own row.
        let removes = use_after || !row.target_had;
        let adds = !use_after || !row.target_had;
        if removes {
            changed(
                transaction.execute(
                    "DELETE FROM source_provenance
                     WHERE source_key = ?1 AND source_item_key = ?2 AND media_id = ?3",
                    rusqlite::params![row.source_key, row.source_item_key, from],
                )?,
                "source link",
            )?;
        }
        if adds {
            transaction.execute(
                "INSERT INTO source_provenance (source_key, source_item_key, media_id, source_text)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![row.source_key, row.source_item_key, to, row.source_text],
            )?;
        }
    }
    for (key, before, after) in &state.aliases {
        match (before, use_after) {
            (None, true) => {
                transaction.execute(
                    "INSERT INTO library_item_alias (stable_key, local_id) VALUES (?1, ?2)",
                    rusqlite::params![key, after],
                )?;
            }
            (None, false) => changed(
                transaction.execute(
                    "DELETE FROM library_item_alias WHERE stable_key = ?1 AND local_id = ?2",
                    rusqlite::params![key, after],
                )?,
                "stable key alias",
            )?,
            (Some(before), _) => {
                let (from, to) = direction(*before, *after);
                changed(
                    transaction.execute(
                        "UPDATE library_item_alias SET local_id = ?2 WHERE stable_key = ?1 AND local_id = ?3",
                        rusqlite::params![key, to, from],
                    )?,
                    "stable key alias",
                )?;
            }
        }
    }
    Ok(())
}

/// What `Library::merge_shared_files` merged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SharedFileMerge {
    /// Files that had more than one item.
    pub files: u64,
    /// Items merged into another and removed.
    pub merged_items: u64,
    /// Collections left empty by the merge and removed.
    pub removed_collections: u64,
}

impl Library {
    /// Make every file one media item, once per library: merge the items that
    /// share a file into one, then enforce it with a unique index. Libraries
    /// made before the rule could hold a file standalone and in collections,
    /// or several times over. The item that stays is the one in the oldest
    /// collection, otherwise the oldest loose one. Runs at open (after the
    /// schema migration took its backup) and clears history, as a repair.
    pub fn merge_shared_files(&self) -> Result<SharedFileMerge> {
        let enforced = self.database.read(WorkPriority::CorrectnessRecovery, |connection| {
            Ok(connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master
                     WHERE type = 'index' AND name = 'idx_media_item_one_per_file')",
                    [],
                    |row| row.get::<_, bool>(0),
                )?)
        })?;
        if enforced {
            return Ok(SharedFileMerge::default());
        }
        let report = self.publish(WorkPriority::CorrectnessRecovery, |transaction, revision, snapshot| {
            let mut groups: Vec<Vec<MediaId>> = Vec::new();
            {
                let mut statement = transaction.prepare(
                    "SELECT file_id, media_id FROM media_item
                     WHERE file_id IN (
                         SELECT file_id FROM media_item GROUP BY file_id HAVING COUNT(*) > 1
                     )
                     ORDER BY file_id, media_id",
                )?;
                let mut rows = statement.query([])?;
                let mut current = None;
                while let Some(row) = rows.next()? {
                    let file_id = row.get::<_, u32>(0)?;
                    if current != Some(file_id) {
                        current = Some(file_id);
                        groups.push(Vec::new());
                    }
                    groups.last_mut().expect("group was pushed").push(MediaId(row.get(1)?));
                }
            }
            let mut next = (*snapshot).clone();
            let mut merge = Merge::default();
            let mut report = SharedFileMerge {
                files: groups.len() as u64,
                ..SharedFileMerge::default()
            };
            let now = now_ms();
            for group in groups {
                let survivor = survivor_of(transaction, &next, &group)?;
                for loser in group.into_iter().filter(|media| *media != survivor) {
                    merge_media(transaction, revision, &mut next, survivor, loser, now, &mut merge)?;
                    report.merged_items += 1;
                }
            }
            report.removed_collections = merge.removed_collections;
            transaction.execute_batch(crate::schema::ONE_ITEM_PER_FILE)?;
            crate::smart::settle_affected(transaction, &mut next, &merge.affected)?;
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                merge.resources,
                merge.affected.iter().map(RootId),
            );
            Ok((
                report,
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        if report.files != 0 {
            // A repair at open is not a user action to undo.
            self.history.clear();
        }
        Ok(report)
    }
}

/// The item of `group` (one file's items) that stays: the one in the oldest
/// collection, otherwise the oldest loose one.
fn survivor_of(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    group: &[MediaId],
) -> Result<MediaId> {
    let mut imported = transaction
        .prepare_cached("SELECT imported_at_ms FROM library_root WHERE root_id = ?1")?;
    let mut best: Option<((bool, i64, u32), MediaId)> = None;
    for media in group {
        let root = owner(snapshot, *media)?;
        let in_collection = root.0 != media.0;
        let imported_at = imported.query_row([root.0], |row| row.get::<_, i64>(0))?;
        // Collections first, then the earliest import, then the lowest id.
        let key = (!in_collection, imported_at, root.0);
        if best.as_ref().is_none_or(|(best, _)| key < *best) {
            best = Some((key, *media));
        }
    }
    best.map(|(_, media)| media)
        .ok_or_else(|| LibraryError::InvalidState("a shared file has no items".into()))
}
