use picto_library::selection::SelectionTarget;
use picto_library::{
    DuplicateResolutionChoice, DuplicateStatus, FileId, GroupRequest, ImmutableMediaFacts,
    LabColor, Library, Lifecycle, PreparedCollectionImport, PreparedImport, Rating, RootDetails,
    RootId,
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
        notes: None,
        tags: Vec::new(),
        folders: Vec::new(),
        source_urls: vec![format!("https://example.test/{key}")],
        source_identity: Some(picto_library::SourceIdentity {
            source_key: "fixture".into(),
            source_item_key: key.into(),
            source_text: Some(format!("source {key}")),
            source_attempt_id: None,
        }),
        imported_at_ms: 1_700_000_000_000,
        captured_at_ms: Some(1_600_000_000_000),
    }
}

fn named_import(key: &str, hash: &str, name: &str, color: LabColor) -> PreparedImport {
    let mut input = imported(
        key,
        hash,
        "image/png",
        1_024,
        Some(100),
        Some(100),
        None,
        color,
    );
    input.media_name = name.into();
    input
}

fn file_id(library: &Library, root_id: RootId) -> FileId {
    library.details(root_id).unwrap().media[0].file_id
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
fn duplicate_candidates_include_inbox_and_trash_occurrences() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let color = LabColor {
        l: 50.0,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    };
    let mut inbox = imported(
        "inbox-duplicate",
        "inbox-duplicate",
        "image/png",
        100,
        Some(100),
        Some(100),
        None,
        color.clone(),
    );
    inbox.lifecycle = Lifecycle::Inbox;
    let (inbox_root, _) = library.ingest(&inbox).unwrap();
    let (active_root, _) = library
        .ingest(&imported(
            "active-duplicate",
            "active-duplicate",
            "image/jpeg",
            90,
            Some(100),
            Some(100),
            None,
            color,
        ))
        .unwrap();
    let mut trash = imported(
        "trash-duplicate",
        "trash-duplicate",
        "image/webp",
        80,
        Some(100),
        Some(100),
        None,
        LabColor {
            l: 50.0,
            a: 0.0,
            b: 0.0,
            weight: 1.0,
        },
    );
    trash.lifecycle = Lifecycle::Trash;
    let (trash_root, _) = library.ingest(&trash).unwrap();

    library
        .record_duplicate_pair(
            file_id(&library, inbox_root),
            file_id(&library, active_root),
            1,
            1_700_000_000_100,
        )
        .unwrap();
    library
        .record_duplicate_pair(
            file_id(&library, trash_root),
            file_id(&library, active_root),
            1,
            1_700_000_000_101,
        )
        .unwrap();

    let candidates = library.duplicate_candidates(10).unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|candidate| {
        candidate.left.occurrences.len() == 1 && candidate.right.occurrences.len() == 1
    }));
}

#[test]
fn rescanning_replaces_detected_pairs_without_reopening_keep_both_decisions() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let color = LabColor {
        l: 50.0,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    };
    let mut roots = Vec::new();
    for key in ["scan-left", "scan-right", "scan-third"] {
        roots.push(
            library
                .ingest(&imported(
                    key,
                    key,
                    "image/png",
                    100,
                    Some(100),
                    Some(100),
                    None,
                    color.clone(),
                ))
                .unwrap()
                .0,
        );
    }
    let files = roots
        .into_iter()
        .map(|root_id| file_id(&library, root_id))
        .collect::<Vec<_>>();
    library
        .record_duplicate_pair(files[0], files[1], 1, 1_700_000_000_100)
        .unwrap();
    library
        .resolve_duplicate(
            files[0],
            files[1],
            DuplicateResolutionChoice::KeepBoth,
            1_700_000_000_200,
        )
        .unwrap();

    let scan = library
        .replace_detected_duplicate_pairs(
            &[(files[0], files[1], 2), (files[0], files[2], 3)],
            1_700_000_000_300,
        )
        .unwrap();
    assert_eq!(scan.candidate_count, 1);
    let pairs = library.duplicate_pairs(None, 10).unwrap();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.iter().any(|pair| {
        pair.file_id_a == files[0]
            && pair.file_id_b == files[1]
            && pair.status == DuplicateStatus::NotDuplicate
    }));
    assert!(pairs.iter().any(|pair| {
        pair.file_id_a == files[0]
            && pair.file_id_b == files[2]
            && pair.status == DuplicateStatus::Detected
    }));
}

fn color(l: f32) -> LabColor {
    LabColor {
        l,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    }
}

fn root_count(library: &Library) -> i64 {
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))?)
            },
        )
        .unwrap()
}

fn provenance_media(library: &Library, source_item_key: &str) -> Vec<u32> {
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(connection
                    .prepare(
                        "SELECT media_id FROM source_provenance
                         WHERE source_item_key = ?1 ORDER BY media_id",
                    )?
                    .query_map([source_item_key], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?)
            },
        )
        .unwrap()
}

#[test]
fn keep_file_merges_the_losing_item_into_the_kept_one_and_undoes_exactly() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (folder, _) = library.create_folder("Kept place", None).unwrap();
    let (neighbour, _) = library
        .ingest(&{
            let mut input = named_import("neighbour", "neighbour-file", "Neighbour", color(10.0));
            input.folders = vec![folder];
            input
        })
        .unwrap();
    let mut loser_input = named_import("loser", "loser-file", "Loser", color(20.0));
    loser_input.tags = vec!["kept:from-loser".into()];
    loser_input.rating = Rating::Four;
    loser_input.notes = Some("Loser note".into());
    loser_input.folders = vec![folder];
    let (loser, _) = library.ingest(&loser_input).unwrap();
    let mut winner_input = named_import("winner", "winner-file", "Winner", color(30.0));
    winner_input.tags = vec!["kept:from-winner".into()];
    winner_input.rating = Rating::Two;
    winner_input.lifecycle = Lifecycle::Inbox;
    let (winner, _) = library.ingest(&winner_input).unwrap();
    let loser_file = file_id(&library, loser);
    let winner_file = file_id(&library, winner);
    let loser_before = library.details(loser).unwrap();
    let winner_before = library.details(winner).unwrap();
    let folder_before = library.projections().snapshot().folder_orders[&folder].clone();
    assert_eq!(folder_before.as_ref(), &vec![neighbour, loser]);

    library
        .record_duplicate_pair(loser_file, winner_file, 1, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    library
        .resolve_duplicate(
            loser_file,
            winner_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: winner_file,
            },
            1_700_000_000_200,
        )
        .unwrap();

    let merged = |library: &Library| {
        assert_eq!(root_count(library), 2);
        assert!(library.details(loser).is_err());
        let kept = library.details(winner).unwrap();
        let snapshot = library.projections().snapshot();
        let tag = |name: &str| snapshot.tag_ids_by_name[name];
        let mut tags = kept.tag_ids.clone();
        tags.sort_unstable();
        let mut expected = vec![tag("kept:from-loser"), tag("kept:from-winner")];
        expected.sort_unstable();
        assert_eq!(tags, expected);
        assert_eq!(kept.rating, Rating::Four);
        assert_eq!(kept.lifecycle, Lifecycle::Active);
        assert_eq!(kept.root.notes.as_deref(), Some("Loser note"));
        assert_eq!(
            kept.root.source_urls,
            vec![
                "https://example.test/winner".to_string(),
                "https://example.test/loser".to_string()
            ]
        );
        assert_eq!(kept.media[0].file_id, winner_file);
        assert_eq!(
            snapshot.folder_orders[&folder].as_ref(),
            &vec![neighbour, winner]
        );
        assert_eq!(provenance_media(library, "loser"), vec![winner.0]);
        assert_eq!(snapshot.tags[&tag("kept:from-loser")].len(), 1);
    };
    merged(&library);
    // The loser's file waits for the entry to leave history.
    assert!(library.pending_blob_cleanup(10).unwrap().is_empty());

    library.undo().unwrap().unwrap();
    assert_eq!(root_count(&library), 3);
    assert_root_metadata_unchanged(&loser_before, &library.details(loser).unwrap());
    assert_root_metadata_unchanged(&winner_before, &library.details(winner).unwrap());
    assert_eq!(file_id(&library, loser), loser_file);
    assert_eq!(
        library.projections().snapshot().folder_orders[&folder],
        folder_before
    );
    assert_eq!(provenance_media(&library, "loser"), vec![loser.0]);
    assert_eq!(
        library
            .duplicate_pairs(Some(DuplicateStatus::Detected), 10)
            .unwrap()
            .len(),
        1
    );

    library.redo().unwrap().unwrap();
    merged(&library);

    // Re-importing the merged-away item resolves to the one that stays.
    let (again, _) = library.ingest(&loser_input).unwrap();
    assert_eq!(again, winner);
    assert_eq!(root_count(&library), 2);
    drop(library);

    let reopened = Library::open(&path).unwrap();
    assert_eq!(reopened.history().state().entries, 0);
    assert_eq!(
        reopened
            .pending_blob_cleanup(10)
            .unwrap()
            .into_iter()
            .map(|pending| pending.file_id)
            .collect::<Vec<_>>(),
        vec![loser_file]
    );
}

#[test]
fn keep_file_takes_a_losing_member_out_of_its_collection_without_its_tags() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut member = named_import("member", "member-file", "Member", color(20.0));
    member.tags = vec!["post:only".into()];
    let (collection, _) = library
        .ingest_collection(&PreparedCollectionImport {
            members: vec![
                member,
                named_import("support-a", "support-a-file", "Support A", color(30.0)),
                named_import("support-b", "support-b-file", "Support B", color(40.0)),
            ],
            cover_index: 0,
            existing_root_id: None,
            name: Some("Post".into()),
            modified_at_ms: 1_700_000_000_000,
        })
        .unwrap();
    let before = library.details(collection).unwrap();
    let loser_media = before.media[0].clone();
    let (winner, _) = library
        .ingest(&named_import("winner", "winner-file", "Winner", color(50.0)))
        .unwrap();
    let winner_file = file_id(&library, winner);
    library
        .record_duplicate_pair(loser_media.file_id, winner_file, 0, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    library
        .resolve_duplicate(
            loser_media.file_id,
            winner_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: winner_file,
            },
            1_700_000_000_200,
        )
        .unwrap();

    let after = library.details(collection).unwrap();
    assert_eq!(
        after.media.iter().map(|media| media.media_name.as_str()).collect::<Vec<_>>(),
        vec!["Support A", "Support B"]
    );
    assert_eq!(after.root.cover_media_id, after.media[0].media_id);
    assert_eq!(after.root.name, "Post");
    let post_tag = library.projections().snapshot().tag_ids_by_name["post:only"];
    assert!(!library.details(winner).unwrap().tag_ids.contains(&post_tag));
    assert_eq!(provenance_media(&library, "member"), vec![winner.0]);

    library.undo().unwrap().unwrap();
    let restored = library.details(collection).unwrap();
    assert_eq!(restored.media, before.media);
    assert_eq!(restored.root.cover_media_id, before.root.cover_media_id);
    assert_eq!(restored.tag_ids, before.tag_ids);
    assert_eq!(provenance_media(&library, "member"), vec![loser_media.media_id.0]);
}

#[test]
fn keep_file_removes_a_collection_it_empties_and_points_its_post_at_the_kept_item() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (only, _) = library
        .ingest(&named_import("only", "only-file", "Only", color(20.0)))
        .unwrap();
    // A collection down to one item: group two, then take one out.
    let (other, _) = library
        .ingest(&named_import("other", "other-file", "Other", color(25.0)))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![only, other],
            },
            cover_root_id: only,
            winning_collection_id: None,
            name: Some("Single".into()),
            notes: None,
            modified_at_ms: 1_700_000_000_050,
        })
        .unwrap();
    library
        .detach_collection_member(collection, picto_library::MediaId(other.0), 1_700_000_000_060)
        .unwrap();
    library
        .apply_tags(
            &SelectionTarget::Explicit {
                root_ids: vec![collection],
            },
            &["set:tag".to_string()],
            true,
        )
        .unwrap();
    let (winner, _) = library
        .ingest(&named_import("winner", "winner-file", "Winner", color(50.0)))
        .unwrap();
    let post_root = |library: &Library| -> Option<u32> {
        library
            .database()
            .read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok(connection.query_row(
                        "SELECT root_item_id FROM source_post WHERE post_key = 'post'",
                        [],
                        |row| row.get(0),
                    )?)
                },
            )
            .unwrap()
    };
    library
        .database()
        .maintenance_write(picto_library::database::WorkPriority::Cloud, |transaction| {
            transaction.execute(
                "INSERT INTO source_post (site_id, post_key, root_item_id, created_at, updated_at)
                 VALUES ('fixture', 'post', ?1, 'now', 'now')",
                [collection.0],
            )?;
            Ok(())
        })
        .unwrap();
    let only_file = library.details(collection).unwrap().media[0].file_id;
    let winner_file = file_id(&library, winner);
    library
        .record_duplicate_pair(only_file, winner_file, 0, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    library
        .resolve_duplicate(
            only_file,
            winner_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: winner_file,
            },
            1_700_000_000_200,
        )
        .unwrap();
    assert!(library.details(collection).is_err());
    let set_tag = library.projections().snapshot().tag_ids_by_name["set:tag"];
    assert!(library.details(winner).unwrap().tag_ids.contains(&set_tag));
    assert_eq!(post_root(&library), Some(winner.0));

    library.undo().unwrap().unwrap();
    assert_eq!(post_root(&library), Some(collection.0));
    let restored = library.details(collection).unwrap();
    assert_eq!(restored.media.len(), 1);
    assert!(restored.tag_ids.contains(&set_tag));
    assert!(!library.details(winner).unwrap().tag_ids.contains(&set_tag));
}

#[test]
fn duplicate_merge_upgrades_weak_standalone_names_and_replays_them_exactly() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let color = LabColor {
        l: 50.0,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    };
    let (weak_root, _) = library
        .ingest(&named_import(
            "weak-name",
            "weak-file",
            "938475938475.png",
            color.clone(),
        ))
        .unwrap();
    let (meaningful_root, _) = library
        .ingest(&named_import(
            "meaningful-name",
            "meaningful-file",
            "A quiet landscape",
            color.clone(),
        ))
        .unwrap();
    let (between, _) = library
        .ingest(&named_import(
            "between",
            "between-file",
            "99 bottles",
            color,
        ))
        .unwrap();
    let order = || {
        let mut query = picto_library::query::RootQuery {
            scope: picto_library::query::ItemScope::All,
            view: Default::default(),
        };
        query.view.sort.field = picto_library::predicate::SortField::Name;
        query.view.sort.direction = picto_library::predicate::SortDirection::Ascending;
        library
            .query_window(&query, &picto_library::query::WindowRequest::at(0, 10))
            .unwrap()
            .page
            .items
            .into_iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(order(), vec![weak_root, between, meaningful_root]);
    let weak_file = file_id(&library, weak_root);
    let meaningful_file = file_id(&library, meaningful_root);
    library
        .record_duplicate_pair(weak_file, meaningful_file, 0, 1_700_000_000_100)
        .unwrap()
        .unwrap();

    let result = library
        .resolve_duplicate(
            weak_file,
            meaningful_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: weak_file,
            },
            1_700_000_000_200,
        )
        .unwrap();
    assert!(result.receipt.resources.contains(&"library".to_string()));
    // The kept item takes the better name; the other is merged into it.
    assert_eq!(order(), vec![between, weak_root]);
    let details = library.details(weak_root).unwrap();
    assert_eq!(details.root.name, "A quiet landscape");
    assert_eq!(details.media[0].media_name, "A quiet landscape");

    library.undo().unwrap().unwrap();
    assert_eq!(order(), vec![weak_root, between, meaningful_root]);
    assert_eq!(
        library.details(weak_root).unwrap().root.name,
        "938475938475.png"
    );
    assert_eq!(
        library.details(meaningful_root).unwrap().root.name,
        "A quiet landscape"
    );

    library.redo().unwrap().unwrap();
    assert_eq!(order(), vec![between, weak_root]);
    assert_eq!(
        library.details(weak_root).unwrap().root.name,
        "A quiet landscape"
    );
}

#[test]
fn duplicate_merge_upgrades_collection_members_without_renaming_the_collection() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let color = LabColor {
        l: 50.0,
        a: 0.0,
        b: 0.0,
        weight: 1.0,
    };
    let weak_member = named_import(
        "collection-weak",
        "collection-weak-file",
        "gelbooru_14583420",
        color.clone(),
    );
    let other_member = named_import(
        "collection-other",
        "collection-other-file",
        "Other member",
        color.clone(),
    );
    let (collection, _) = library
        .ingest_collection(&PreparedCollectionImport {
            members: vec![weak_member, other_member],
            cover_index: 0,
            existing_root_id: None,
            name: Some("Collection-owned title".into()),
            modified_at_ms: 1_700_000_000_000,
        })
        .unwrap();
    let weak_media = library
        .details(collection)
        .unwrap()
        .media
        .into_iter()
        .find(|media| media.media_name == "gelbooru_14583420")
        .unwrap();
    let (standalone, _) = library
        .ingest(&named_import(
            "standalone-human",
            "standalone-human-file",
            "Meaningful member title",
            color,
        ))
        .unwrap();
    let standalone_file = file_id(&library, standalone);
    library
        .record_duplicate_pair(weak_media.file_id, standalone_file, 0, 1_700_000_000_100)
        .unwrap()
        .unwrap();
    // Keep the member's file: the loose item merges into the collection.
    library
        .resolve_duplicate(
            weak_media.file_id,
            standalone_file,
            DuplicateResolutionChoice::KeepFile {
                winner_file_id: weak_media.file_id,
            },
            1_700_000_000_200,
        )
        .unwrap();
    assert!(library.details(standalone).is_err());

    let details = library.details(collection).unwrap();
    assert_eq!(details.root.name, "Collection-owned title");
    assert_eq!(
        details
            .media
            .iter()
            .find(|media| media.media_id == weak_media.media_id)
            .unwrap()
            .media_name,
        "Meaningful member title"
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library.details(standalone).unwrap().root.name,
        "Meaningful member title"
    );
    let details = library.details(collection).unwrap();
    assert_eq!(details.root.name, "Collection-owned title");
    assert_eq!(
        details
            .media
            .iter()
            .find(|media| media.media_id == weak_media.media_id)
            .unwrap()
            .media_name,
        "gelbooru_14583420"
    );
}
