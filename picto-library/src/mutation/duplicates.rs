//! Duplicate detection results and their resolution.

use super::*;

impl Library {
    pub fn record_duplicate_pair(
        &self,
        file_id_a: FileId,
        file_id_b: FileId,
        distance: u32,
        detected_at_ms: i64,
    ) -> Result<Option<MutationReceipt>> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let published = self.database.published_write_if_changed(
            WorkPriority::Maintenance,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let Some(pair) = crate::duplicate::record_detected(
                    transaction,
                    file_id_a,
                    file_id_b,
                    distance,
                    detected_at_ms,
                )?
                else {
                    return Ok(None);
                };
                let affected = crate::duplicate::affected_roots(
                    transaction,
                    &snapshot,
                    pair.file_id_a,
                    pair.file_id_b,
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["duplicates".into()],
                    affected.iter().map(RootId),
                );
                Ok(Some((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                )))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok(published.map(|(receipt, _, ())| receipt))
    }

    pub fn replace_detected_duplicate_pairs(
        &self,
        pairs: &[(FileId, FileId, u32)],
        detected_at_ms: i64,
    ) -> Result<crate::DuplicateScanResult> {
        let pairs = pairs.to_vec();
        let result = self.publish(WorkPriority::Maintenance, |transaction, revision, snapshot| {
            let affected = crate::duplicate::replace_detected(
                transaction,
                &snapshot,
                &pairs,
                detected_at_ms,
            )?;
            let candidate_count = transaction.query_row(
                "SELECT COUNT(*) FROM duplicate_pair WHERE status = 1",
                [],
                |row| row.get::<_, i64>(0),
            )? as usize;
            let mut next = (*snapshot).clone();
            next.revision = revision;
            let receipt = PublicationCoordinator::receipt(
                revision,
                vec!["duplicates".into()],
                affected.iter().map(RootId),
            );
            let result = crate::DuplicateScanResult {
                candidate_count,
                affected_item_ids: receipt.item_ids.clone(),
                receipt: receipt.clone(),
            };
            Ok((
                result,
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                },
            ))
        })?;
        Ok(result)
    }

    pub fn duplicate_pairs(
        &self,
        status: Option<DuplicateStatus>,
        limit: usize,
    ) -> Result<Vec<DuplicatePair>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            crate::duplicate::list_pairs(connection, status, limit)
        })
    }

    pub fn duplicate_candidates(&self, limit: usize) -> Result<Vec<crate::DuplicateCandidate>> {
        self.duplicate_candidates_from(0, limit)
    }

    /// Returns the pairs that are still to be reviewed and whose distance is
    /// at least `min_distance`, the smallest distance first. With 1, it
    /// returns the similar pairs and leaves out every identical pair, so it
    /// reaches similar pairs that the first page of
    /// [`Library::duplicate_candidates`] may not include.
    pub fn duplicate_candidates_from(
        &self,
        min_distance: u32,
        limit: usize,
    ) -> Result<Vec<crate::DuplicateCandidate>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::duplicate::list_candidates(connection, &snapshot, min_distance, limit)
            },
        )
    }

    pub fn resolve_duplicate_automatically(
        &self,
        file_id_a: FileId,
        file_id_b: FileId,
        decided_at_ms: i64,
    ) -> Result<Option<DuplicateResolutionResult>> {
        let candidate = self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::duplicate::candidate_for_pair(connection, &snapshot, file_id_a, file_id_b)
            },
        )?;
        let winner_file_id = match candidate.map(|candidate| candidate.decision) {
            Some(crate::DuplicateQualityDecision::LeftBetter)
            | Some(crate::DuplicateQualityDecision::AutoTieLeft) => file_id_a,
            Some(crate::DuplicateQualityDecision::RightBetter)
            | Some(crate::DuplicateQualityDecision::AutoTieRight) => file_id_b,
            Some(crate::DuplicateQualityDecision::NeedsChoice) | None => return Ok(None),
        };
        self.resolve_duplicate(
            file_id_a,
            file_id_b,
            DuplicateResolutionChoice::KeepFile { winner_file_id },
            decided_at_ms,
        )
        .map(Some)
    }

    pub fn resolve_duplicate(
        &self,
        file_id_a: FileId,
        file_id_b: FileId,
        choice: DuplicateResolutionChoice,
        decided_at_ms: i64,
    ) -> Result<DuplicateResolutionResult> {
        let receipt = self.resolve_duplicates(
            &[(file_id_a, file_id_b, choice)],
            "Resolve duplicate",
            decided_at_ms,
        )?;
        Ok(DuplicateResolutionResult {
            choice,
            affected_root_ids: receipt.item_ids.clone(),
            receipt,
        })
    }

    /// Settles several duplicate pairs in order, in one publication and as
    /// one undoable step labelled `label`: a set of files reviewed together
    /// (one kept, the rest merged into it pair by pair; or every pair of the
    /// set kept apart). A later step sees what the earlier ones changed, so
    /// the file that one step kept can be merged into another file by a
    /// later step.
    pub fn resolve_duplicates(
        &self,
        steps: &[(FileId, FileId, DuplicateResolutionChoice)],
        label: &str,
        decided_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if steps.is_empty() {
            return Err(LibraryError::InvalidInput("no duplicate pairs to resolve".into()));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let label = label.to_owned();
        let (receipt, _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut snapshot = (*snapshot).clone();
                let mut affected_roots = RoaringBitmap::new();
                let mut resources = BTreeSet::from(["duplicates".to_owned()]);
                let mut changes = Vec::with_capacity(steps.len());
                let mut merges_files = false;
                for &(file_id_a, file_id_b, choice) in steps {
                    let mut output = crate::duplicate::resolve(
                        transaction,
                        snapshot,
                        file_id_a,
                        file_id_b,
                        choice,
                        decided_at_ms,
                    )?;
                    let mut merge = Merge {
                        record: true,
                        ..Merge::default()
                    };
                    if let Some((survivor, losers)) = output.merge.take() {
                        for loser in losers {
                            merge_media(
                                transaction,
                                revision,
                                &mut output.snapshot,
                                survivor,
                                loser,
                                decided_at_ms,
                                &mut merge,
                            )?;
                        }
                        output.affected_roots |= &merge.affected;
                    }
                    if matches!(choice, DuplicateResolutionChoice::KeepFile { .. }) {
                        merges_files = true;
                        resources.extend(
                            ["media", "roots", "smart-folders"].map(str::to_owned),
                        );
                        if output.history.changes_names() {
                            resources.insert("search".into());
                        }
                        resources.extend(merge.resources.iter().cloned());
                    }
                    affected_roots |= &output.affected_roots;
                    snapshot = output.snapshot;
                    changes.push(if merge.changes.is_empty() {
                        SemanticChange::DuplicateResolution(output.history)
                    } else {
                        SemanticChange::Compound(
                            std::iter::once(SemanticChange::DuplicateResolution(output.history))
                                .chain(merge.changes)
                                .collect(),
                        )
                    });
                }
                if merges_files {
                    crate::smart::settle_affected(transaction, &mut snapshot, &affected_roots)?;
                }
                snapshot.revision = revision;
                let receipt =
                    PublicationCoordinator::receipt(revision, resources, affected_roots.iter().map(RootId));
                let change = if changes.len() == 1 {
                    changes.pop().expect("one change")
                } else {
                    SemanticChange::Compound(changes)
                };
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot,
                        receipt,
                        history: Some(HistoryEntry::for_command("duplicates.resolve", &label, change)),
                    },
                ))
            },
            move |_, delta| {
                let history_entry = publish_delta(&projections, &publication, delta);
                push_history(&history, history_entry);
            },
        )?;
        Ok(receipt)
    }
}
