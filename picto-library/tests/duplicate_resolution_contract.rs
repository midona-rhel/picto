use picto_library::predicate::{FilterClause, FilterExpr, ItemSort, ViewQuerySpec};
use picto_library::selection::SelectionTarget;
use picto_library::{
    DuplicateResolutionChoice, DuplicateStatus, FileId, GroupRequest, ImmutableMediaFacts,
    LabColor, Library, Lifecycle, PreparedImport, Rating, RootDetails, RootId, SmartFolderInput,
};
use tempfile::TempDir;

#[allow(clippy::too_many_arguments)]
fn imported(
    key: &str,
    content_hash: &str,
    mime: &str,
    size_bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    palette: LabColor,
) -> PreparedImport {
    PreparedImport {
        stable_key: key.into(),
        media_name: format!("{key}.bin"),
        file_path: format!("/tmp/{key}.bin"),
        facts: ImmutableMediaFacts {
            mime: mime.into(),
            size_bytes,
            width,
            height,
            duration_ms,
            frame_count: Some(1),
            content_hash: content_hash.into(),
            perceptual_hash: Some(format!("phash-{content_hash}")),
            palette: vec![palette],
        },
        lifecycle: Lifecycle::Active,
        rating: Rating::Unrated,
        tags: Vec::new(),
        folders: Vec::new(),
        source_urls: vec![format!("https://example.test/{key}")],
        source_identity: Some(picto_library::SourceIdentity {
            source_key: "fixture".into(),
            source_item_key: key.into(),
            source_text: Some(format!("source {key}")),
        }),
        imported_at_ms: 1_700_000_000_000,
        captured_at_ms: Some(1_600_000_000_000),
    }
}

fn file_id(library: &Library, root_id: RootId) -> FileId {
    library.details(root_id).unwrap().media[0].file_id
}

fn smart_input(name: &str, filter: FilterClause) -> SmartFolderInput {
    SmartFolderInput {
        name: name.into(),
        parent_id: None,
        icon: None,
        color: None,
        notes: None,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(filter),
            sort: ItemSort::default(),
        },
    }
}

fn assert_root_metadata_unchanged(before: &RootDetails, after: &RootDetails) {
    assert_eq!(after.root.root_id, before.root.root_id);
    assert_eq!(after.root.stable_key, before.root.stable_key);
    assert_eq!(after.root.kind, before.root.kind);
    assert_eq!(after.root.name, before.root.name);
    assert_eq!(after.root.notes, before.root.notes);
    assert_eq!(after.root.source_urls, before.root.source_urls);
    assert_eq!(after.root.cover_media_id, before.root.cover_media_id);
    assert_eq!(after.root.imported_at_ms, before.root.imported_at_ms);
    assert_eq!(after.root.captured_at_ms, before.root.captured_at_ms);
    assert_eq!(after.root.modified_at_ms, before.root.modified_at_ms);
    assert_eq!(after.root.media_count, before.root.media_count);
    assert_eq!(after.lifecycle, before.lifecycle);
    assert_eq!(after.rating, before.rating);
    assert_eq!(after.folder_ids, before.folder_ids);
    assert_eq!(after.tag_ids, before.tag_ids);
    assert_eq!(
        after
            .media
            .iter()
            .map(|media| (media.media_id, media.media_name.clone()))
            .collect::<Vec<_>>(),
        before
            .media
            .iter()
            .map(|media| (media.media_id, media.media_name.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn fresh_generation_one_schema_contains_only_the_duplicate_pair_domain_table() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let tables = connection
                    .prepare(
                        "SELECT name FROM sqlite_master
                         WHERE type = 'table' AND name LIKE 'duplicate%'
                         ORDER BY name",
                    )?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                assert_eq!(tables, vec!["duplicate_pair"]);
                assert_eq!(picto_library::schema::SCHEMA_GENERATION, 1);
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn keep_both_changes_only_pair_state_and_is_session_undoable() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let red = LabColor {
        l: 50.0,
        a: 40.0,
        b: 20.0,
        weight: 1.0,
    };
    let (left, _) = library
        .ingest(&imported(
            "keep-both-left",
            "keep-both-left",
            "image/png",
            100,
            Some(100),
            Some(100),
            None,
            red.clone(),
        ))
        .unwrap();
    let (right, _) = library
        .ingest(&imported(
            "keep-both-right",
            "keep-both-right",
            "image/jpeg",
            90,
            Some(100),
            Some(100),
            None,
            red,
        ))
        .unwrap();
    let left_file = file_id(&library, left);
    let right_file = file_id(&library, right);
    let left_before = library.details(left).unwrap();
    let right_before = library.details(right).unwrap();

    let detected = library
        .record_duplicate_pair(right_file, left_file, 2, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    assert_eq!(detected.resources, vec!["duplicates"]);
    let pair = library
        .duplicate_pairs(Some(DuplicateStatus::Detected), 10)
        .unwrap()
        .remove(0);
    assert_eq!((pair.file_id_a, pair.file_id_b), (left_file, right_file));
    library.publication().flush();

    let result = library
        .resolve_duplicate(
            right_file,
            left_file,
            DuplicateResolutionChoice::KeepBoth,
            1_700_000_000_200,
        )
        .unwrap();
    assert_eq!(result.receipt.resources, vec!["duplicates"]);
    assert_eq!(
        library.duplicate_pairs(None, 10).unwrap()[0].status,
        DuplicateStatus::NotDuplicate
    );
    let left_after = library.details(left).unwrap();
    let right_after = library.details(right).unwrap();
    assert_root_metadata_unchanged(&left_before, &left_after);
    assert_root_metadata_unchanged(&right_before, &right_after);
    assert_eq!(left_after.media, left_before.media);
    assert_eq!(right_after.media, right_before.media);
    let event = library.publication().flush().unwrap();
    assert_eq!(event.revision, result.receipt.revision);
    assert_eq!(event.resources, vec!["duplicates"]);

    library.undo().unwrap().unwrap();
    assert_eq!(
        library.duplicate_pairs(None, 10).unwrap()[0].status,
        DuplicateStatus::Detected
    );
    library.redo().unwrap().unwrap();
    assert_eq!(
        library.duplicate_pairs(None, 10).unwrap()[0].status,
        DuplicateStatus::NotDuplicate
    );
}

#[test]
fn keep_file_rewires_every_occurrence_and_settles_exactly_without_changing_roots() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let loser_color = LabColor {
        l: 40.0,
        a: 60.0,
        b: 30.0,
        weight: 1.0,
    };
    let winner_color = LabColor {
        l: 25.0,
        a: -20.0,
        b: -50.0,
        weight: 1.0,
    };
    let support_color = LabColor {
        l: 80.0,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    };
    let (loser_member, _) = library
        .ingest(&imported(
            "loser-member",
            "shared-loser",
            "image/png",
            1_024,
            Some(800),
            Some(600),
            None,
            loser_color,
        ))
        .unwrap();
    let (loser_standalone, _) = library
        .ingest(&imported(
            "loser-standalone",
            "shared-loser",
            "image/png",
            1_024,
            Some(800),
            Some(600),
            None,
            LabColor {
                l: 1.0,
                a: 1.0,
                b: 1.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (support, _) = library
        .ingest(&imported(
            "support",
            "support",
            "application/pdf",
            256,
            None,
            None,
            None,
            support_color,
        ))
        .unwrap();
    let (winner, _) = library
        .ingest(&imported(
            "winner",
            "winner",
            "video/mp4",
            4_096,
            Some(1_920),
            Some(1_080),
            Some(4_000),
            winner_color.clone(),
        ))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![loser_member, support],
            },
            cover_root_id: loser_member,
            winning_collection_id: None,
            name: Some("Preserved collection".into()),
            modified_at_ms: 1_700_000_000_050,
        })
        .unwrap();
    let (image_smart, _) = library
        .create_smart_folder(smart_input(
            "Images",
            FilterClause::Mime {
                values: vec!["image/png".into()],
                families: Vec::new(),
            },
        ))
        .unwrap();
    let (video_smart, _) = library
        .create_smart_folder(smart_input(
            "Videos",
            FilterClause::Mime {
                values: vec!["video/mp4".into()],
                families: Vec::new(),
            },
        ))
        .unwrap();

    let loser_file = file_id(&library, loser_standalone);
    let winner_file = file_id(&library, winner);
    let collection_before = library.details(collection).unwrap();
    let standalone_before = library.details(loser_standalone).unwrap();
    let order_before = library.projections().snapshot().collection_orders[&collection].clone();
    assert_eq!(
        library.projections().snapshot().smart_results[&image_smart.0].len(),
        2
    );
    assert_eq!(
        library.projections().snapshot().smart_results[&video_smart.0].len(),
        1
    );

    library
        .record_duplicate_pair(loser_file, winner_file, 1, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    library.publication().flush();
    let result = library
        .resolve_duplicate(
            loser_file,
            winner_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: winner_file,
            },
            1_700_000_000_200,
        )
        .unwrap();

    assert_eq!(
        result.receipt.resources,
        vec!["duplicates", "library", "smart_folders"]
    );
    assert_eq!(
        library
            .duplicate_pairs(Some(DuplicateStatus::Resolved), 10)
            .unwrap()[0]
            .winner_file_id,
        Some(winner_file)
    );
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let occurrences = connection
                    .prepare(
                        "SELECT media_id, file_id FROM media_item
                         WHERE media_id IN (?1, ?2) ORDER BY media_id",
                    )?
                    .query_map([loser_member.0, loser_standalone.0], |row| {
                        Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                assert_eq!(
                    occurrences,
                    vec![
                        (loser_member.0, winner_file.0),
                        (loser_standalone.0, winner_file.0)
                    ]
                );
                assert_eq!(
                    connection.query_row(
                        "SELECT COUNT(*) FROM source_provenance
                         WHERE media_id IN (?1, ?2)",
                        [loser_member.0, loser_standalone.0],
                        |row| row.get::<_, u32>(0)
                    )?,
                    2
                );
                Ok(())
            },
        )
        .unwrap();

    let collection_after = library.details(collection).unwrap();
    let standalone_after = library.details(loser_standalone).unwrap();
    assert_root_metadata_unchanged(&collection_before, &collection_after);
    assert_root_metadata_unchanged(&standalone_before, &standalone_after);
    assert_eq!(collection_after.root.total_size_bytes, 4_096 + 256);
    assert_eq!(standalone_after.root.total_size_bytes, 4_096);
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection],
        order_before
    );
    let snapshot = library.projections().snapshot();
    assert!(!snapshot.mime["image/png"].contains(collection.0));
    assert!(!snapshot.mime["image/png"].contains(loser_standalone.0));
    assert!(snapshot.mime["video/mp4"].contains(collection.0));
    assert!(snapshot.mime["video/mp4"].contains(loser_standalone.0));
    assert!(snapshot.mime["application/pdf"].contains(collection.0));
    assert_eq!(snapshot.width.value(collection.0), Some(1_920));
    assert_eq!(snapshot.height.value(collection.0), Some(1_080));
    assert_eq!(snapshot.duration.value(collection.0), Some(4_000));
    assert_eq!(
        snapshot.cover_palettes.get(collection.0).unwrap().as_ref(),
        &[winner_color]
    );
    assert!(!snapshot.roots_with_images.contains(collection.0));
    assert!(!snapshot.roots_with_images.contains(loser_standalone.0));
    assert_eq!(snapshot.smart_results[&image_smart.0].len(), 0);
    assert_eq!(snapshot.smart_results[&video_smart.0].len(), 3);
    assert!(library.pending_blob_cleanup(10).unwrap().is_empty());

    let journal = library.pending_cloud_journal(100).unwrap();
    assert_eq!(journal.last().unwrap().operation_kind, "duplicate.resolve");
    let event = library.publication().flush().unwrap();
    assert_eq!(event.revision, result.receipt.revision);
    assert_eq!(event.resources, result.receipt.resources);

    library.undo().unwrap().unwrap();
    assert_eq!(file_id(&library, loser_standalone), loser_file);
    assert_eq!(
        library.details(collection).unwrap().root.total_size_bytes,
        1_024 + 256
    );
    assert!(library.projections().snapshot().mime["image/png"].contains(collection.0));
    assert_eq!(
        library.projections().snapshot().smart_results[&image_smart.0].len(),
        2
    );
    assert!(library.pending_blob_cleanup(10).unwrap().is_empty());

    library.redo().unwrap().unwrap();
    assert_eq!(file_id(&library, loser_standalone), winner_file);
    assert!(library.pending_blob_cleanup(10).unwrap().is_empty());
    drop(library);

    let reopened = Library::open(&path).unwrap();
    assert_eq!(reopened.history().state().entries, 0);
    assert_eq!(
        reopened.pending_blob_cleanup(10).unwrap(),
        vec![picto_library::PendingBlobCleanup {
            file_id: loser_file,
            file_path: "/tmp/loser-member.bin".into(),
        }]
    );
}

#[test]
fn duplicate_publication_uses_the_shared_broad_receipt_cap() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let color = LabColor {
        l: 50.0,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    };
    let mut loser_file = None;
    for index in 0..=picto_library::publication::MAX_RECEIPT_IDS {
        let (root_id, _) = library
            .ingest(&imported(
                &format!("cap-loser-{index}"),
                "cap-shared-loser",
                "image/png",
                100,
                Some(10),
                Some(10),
                None,
                color.clone(),
            ))
            .unwrap();
        loser_file.get_or_insert_with(|| file_id(&library, root_id));
    }
    let (winner, _) = library
        .ingest(&imported(
            "cap-winner",
            "cap-winner",
            "image/jpeg",
            200,
            Some(20),
            Some(20),
            None,
            color,
        ))
        .unwrap();
    let loser_file = loser_file.unwrap();
    let winner_file = file_id(&library, winner);
    library.publication().flush();

    let detection = library
        .record_duplicate_pair(loser_file, winner_file, 0, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    assert!(detection.item_ids.is_empty());
    library.publication().flush();
    let resolution = library
        .resolve_duplicate(
            loser_file,
            winner_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: winner_file,
            },
            1_700_000_000_200,
        )
        .unwrap();
    assert!(resolution.receipt.item_ids.is_empty());
    assert!(resolution.affected_root_ids.is_empty());
    assert!(library.publication().flush().unwrap().item_ids.is_empty());
}
