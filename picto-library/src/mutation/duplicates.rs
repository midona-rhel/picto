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
                insert_cloud_journal(
                    transaction,
                    revision,
                    "duplicate.detect",
                    (!affected.is_empty()).then_some(&affected),
                    serde_json::json!({
                        "file_id_a": pair.file_id_a.0,
                        "file_id_b": pair.file_id_b.0,
                        "distance": pair.distance,
                    }),
                    detected_at_ms,
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
            insert_cloud_journal(
                transaction,
                revision,
                "duplicate.scan",
                (!affected.is_empty()).then_some(&affected),
                serde_json::json!({"candidate_count": candidate_count}),
                detected_at_ms,
            )?;
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
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::duplicate::list_candidates(connection, &snapshot, limit),
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
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (result, _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut output = crate::duplicate::resolve(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    file_id_a,
                    file_id_b,
                    choice,
                    decided_at_ms,
                )?;
                if matches!(choice, DuplicateResolutionChoice::KeepFile { .. }) {
                    crate::smart::settle_affected(
                        transaction,
                        &mut output.snapshot,
                        &output.affected_roots,
                    )?;
                }
                insert_cloud_journal(
                    transaction,
                    revision,
                    "duplicate.resolve",
                    (!output.affected_roots.is_empty()).then_some(&output.affected_roots),
                    serde_json::json!({
                        "file_id_a": output.history.file_id_a().0,
                        "file_id_b": output.history.file_id_b().0,
                        "choice": choice,
                    }),
                    decided_at_ms,
                )?;
                output.snapshot.revision = revision;
                let resources = if matches!(choice, DuplicateResolutionChoice::KeepFile { .. }) {
                    let mut resources = vec![
                        "duplicates".into(),
                        "media".into(),
                        "roots".into(),
                        "smart-folders".into(),
                    ];
                    if output.history.changes_names() {
                        resources.push("search".into());
                    }
                    resources
                } else {
                    vec!["duplicates".into()]
                };
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    output.affected_roots.iter().map(RootId),
                );
                let result = DuplicateResolutionResult {
                    choice,
                    affected_root_ids: receipt.item_ids.clone(),
                    receipt: receipt.clone(),
                };
                Ok((
                    result,
                    PublishedDelta {
                        snapshot: output.snapshot,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "duplicates.resolve",
                            "Resolve duplicate",
                            SemanticChange::DuplicateResolution(output.history),
                        )),
                    },
                ))
            },
            move |_, delta| {
                let history_entry = publish_delta(&projections, &publication, delta);
                push_history(&history, history_entry);
            },
        )?;
        Ok(result)
    }
}
