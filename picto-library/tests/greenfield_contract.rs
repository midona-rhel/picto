use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, SortDirection, SortField, ViewQuerySpec,
};
use picto_library::query::{ItemScope, PageRequest, RootQuery};
use picto_library::selection::SelectionTarget;
use picto_library::{
    ContentSortField, GroupRequest, ImmutableMediaFacts, LabColor, Library, Lifecycle,
    PreparedImport, Rating, RootKind, RootTagAssignment, SmartFolderInput,
};
use tempfile::TempDir;

fn imported(key: &str, lifecycle: Lifecycle, tags: &[&str]) -> PreparedImport {
    PreparedImport {
        stable_key: key.into(),
        media_name: format!("{key}.png"),
        file_path: format!("/tmp/{key}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 1024,
            width: Some(800),
            height: Some(600),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: format!("hash-{key}"),
            perceptual_hash: None,
            palette: vec![LabColor {
                l: 50.0,
                a: 10.0,
                b: -5.0,
                weight: 1.0,
            }],
        },
        lifecycle,
        rating: Rating::Unrated,
        notes: None,
        tags: tags.iter().map(|value| (*value).to_owned()).collect(),
        folders: Vec::new(),
        source_urls: vec![format!("https://example.test/{key}")],
        source_identity: Some(picto_library::SourceIdentity {
            source_key: "fixture".into(),
            source_item_key: key.into(),
            source_text: Some(format!("source metadata for {key}")),
        }),
        imported_at_ms: 1_700_000_000_000,
        captured_at_ms: Some(1_600_000_000_000),
    }
}

fn imported_as(key: &str, mime: &str, color: LabColor) -> PreparedImport {
    let mut value = imported(key, Lifecycle::Active, &[]);
    value.facts.mime = mime.into();
    value.facts.palette = vec![color];
    value
}

fn query(scope: ItemScope) -> RootQuery {
    RootQuery {
        scope,
        view: ViewQuerySpec {
            filter: FilterExpr::default(),
            sort: ItemSort {
                field: SortField::ImportedAt,
                direction: SortDirection::Descending,
                random_seed: None,
            },
        },
    }
}

fn smart_input(
    name: &str,
    parent_id: Option<picto_library::SmartFolderId>,
    view: ViewQuerySpec,
) -> SmartFolderInput {
    SmartFolderInput {
        name: name.into(),
        parent_id,
        icon: None,
        color: None,
        notes: None,
        view,
    }
}

#[test]
fn fresh_schema_reopens_and_rejects_unrelated_sqlite_files() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    drop(Library::create(&path).unwrap());
    Library::open(&path).unwrap();

    let unrelated = directory.path().join("unrelated.sqlite");
    rusqlite::Connection::open(&unrelated).unwrap();
    assert!(Library::open(&unrelated).is_err());

    let missing = directory.path().join("missing.sqlite");
    assert!(Library::open(&missing).is_err());
    assert!(!missing.exists());
}

#[test]
fn startup_removes_only_view_preferences_for_missing_navigation_targets() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    library
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::CorrectnessRecovery,
            |transaction| {
                for scope in ["grid:defaults", "folder:999", "smart:999"] {
                    transaction.execute(
                        "INSERT INTO view_pref(scope, value_json) VALUES (?1, '{}')",
                        [scope],
                    )?;
                }
                Ok(())
            },
        )
        .unwrap();
    drop(library);

    let reopened = Library::open(&path).unwrap();
    let scopes = reopened
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(connection
                    .prepare("SELECT scope FROM view_pref ORDER BY scope")?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?)
            },
        )
        .unwrap();
    assert_eq!(scopes, ["grid:defaults"]);
}

#[test]
fn projection_checkpoint_is_revision_exact_and_never_advances_mutation_revision() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (root, _) = library
        .ingest(&imported("checkpoint", Lifecycle::Active, &[]))
        .unwrap();
    let checkpoint_revision = library.database().revision().unwrap();
    assert!(library.write_projection_checkpoint().unwrap() > 0);
    assert_eq!(library.database().revision().unwrap(), checkpoint_revision);
    drop(library);

    let library = Library::open(&path).unwrap();
    assert_eq!(
        library.projections().snapshot().revision,
        checkpoint_revision
    );
    library
        .add_tag(
            &SelectionTarget::Explicit {
                root_ids: vec![root],
            },
            "checkpoint:stale",
        )
        .unwrap();
    let newer_revision = library.database().revision().unwrap();
    drop(library);

    let rebuilt = Library::open(&path).unwrap();
    assert_eq!(rebuilt.projections().snapshot().revision, newer_revision);
    assert_eq!(root_tag_count(&rebuilt, root), 1);
}

#[test]
fn canonical_ingest_is_idempotent_by_stable_or_source_identity() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut input = imported("retry", Lifecycle::Active, &["creator:alice"]);
    input.notes = Some("preserved import note".into());

    let (root_id, _) = library.ingest(&input).unwrap();
    let (stable_retry, _) = library.ingest(&input).unwrap();
    assert_eq!(stable_retry, root_id);

    let mut source_retry = input.clone();
    source_retry.stable_key = "different-local-key".into();
    let (source_retry, _) = library.ingest(&source_retry).unwrap();
    assert_eq!(source_retry, root_id);
    assert_eq!(library.counts().unwrap().all, 1);
    assert_eq!(
        library.details(root_id).unwrap().root.notes.as_deref(),
        Some("preserved import note")
    );

    let target = SelectionTarget::Explicit {
        root_ids: vec![root_id],
    };
    library.set_lifecycle(&target, Lifecycle::Trash).unwrap();
    library
        .permanently_delete(&target, 1_700_000_000_100)
        .unwrap();
    assert!(library.ingest(&input).is_err());
}

#[test]
fn durable_ingest_queue_persists_only_canonical_payloads() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let payload = picto_library::PreparedIngestPayload::Item(imported(
        "queued",
        Lifecycle::Inbox,
        &["queue:canonical"],
    ));
    let job = picto_library::PreparedIngestJob {
        job_key: "manual:queued".into(),
        source_kind: "manual".into(),
        source_path: "/tmp/queued.png".into(),
        source_item_id: None,
        delete_after_ingest: false,
        payload: payload.clone(),
    };

    let (job_id, _) = library
        .enqueue_ingest_job(&job, "2026-08-28T10:00:00Z")
        .unwrap();
    let (same_job_id, _) = library
        .enqueue_ingest_job(&job, "2026-08-28T10:00:01Z")
        .unwrap();
    assert_eq!(same_job_id, job_id);

    let claimed = library
        .claim_ingest_jobs(64, "2026-08-28T10:00:02Z")
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].ingest_job_id, job_id);
    assert_eq!(claimed[0].payload, payload);
    assert_eq!(claimed[0].attempt_count, 1);

    let revision = library.database().revision().unwrap();
    assert!(library
        .claim_ingest_jobs(64, "2026-08-28T10:00:03Z")
        .unwrap()
        .is_empty());
    assert_eq!(library.database().revision().unwrap(), revision);
    assert!(library
        .complete_ingest_jobs(&[job_id], "2026-08-28T10:00:04Z")
        .unwrap()
        .is_some());

    let (reopened_job_id, _) = library
        .enqueue_ingest_job(&job, "2026-08-28T10:00:05Z")
        .unwrap();
    assert_eq!(reopened_job_id, job_id);
    let reopened = library
        .claim_ingest_jobs(64, "2026-08-28T10:00:06Z")
        .unwrap();
    assert_eq!(reopened.len(), 1);
    assert_eq!(reopened[0].ingest_job_id, job_id);
    assert_eq!(reopened[0].attempt_count, 1);
}

#[test]
fn derivative_queue_is_prioritized_bounded_and_retryable() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut input = imported("derivative-work", Lifecycle::Active, &[]);
    input.facts.palette.clear();
    library.ingest(&input).unwrap();

    let claimed = library.claim_media_work(2, "2026-08-28T10:00:00Z").unwrap();
    assert_eq!(claimed.len(), 2);
    assert_eq!(claimed[0].kind, picto_library::MediaWorkKind::Thumbnail);
    assert_eq!(
        claimed[1].kind,
        picto_library::MediaWorkKind::DominantColors
    );
    let (terminal, _) = library
        .retry_media_work(
            claimed[0].work_id,
            claimed[0].attempt_count,
            "retry",
            "2026-08-28T10:00:00Z",
        )
        .unwrap();
    assert!(!terminal);
    library.complete_media_work(&[claimed[1].work_id]).unwrap();

    let phash = library.claim_media_work(8, "2026-08-28T10:00:00Z").unwrap();
    assert_eq!(phash.len(), 1);
    assert_eq!(phash[0].kind, picto_library::MediaWorkKind::PerceptualHash);
    library.complete_media_work(&[phash[0].work_id]).unwrap();

    let thumbnail = library.claim_media_work(8, "2026-08-28T10:00:02Z").unwrap();
    assert_eq!(thumbnail.len(), 1);
    assert_eq!(thumbnail[0].attempt_count, 2);
    library
        .complete_media_work(&[thumbnail[0].work_id])
        .unwrap();
    let revision = library.database().revision().unwrap();
    assert!(library
        .claim_media_work(8, "2026-08-28T10:00:03Z")
        .unwrap()
        .is_empty());
    assert_eq!(library.database().revision().unwrap(), revision);
}

#[test]
fn lifecycle_boundaries_and_bitmap_tag_mutations_are_exact() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (active, _) = library
        .ingest(&imported("active", Lifecycle::Active, &["creator:alice"]))
        .unwrap();
    let (inbox, _) = library
        .ingest(&imported("inbox", Lifecycle::Inbox, &["creator:alice"]))
        .unwrap();

    let all = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(
        all.items
            .iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![active]
    );
    assert_eq!(all.total, 1);

    let inbox_page = library
        .query(&query(ItemScope::Inbox), &PageRequest::default())
        .unwrap();
    assert_eq!(inbox_page.items[0].root_id, inbox);
    let creator_tag = library.projections().snapshot().tag_ids_by_name["creator:alice"];
    let inbox_tagged = RootQuery {
        scope: ItemScope::Inbox,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Tags {
                tag_ids: vec![creator_tag],
                mode: SetMatchMode::All,
            }),
            sort: ItemSort::default(),
        },
    };
    assert_eq!(
        library
            .query(&inbox_tagged, &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    let counts = library.counts().unwrap();
    assert_eq!(counts.all, 1);
    assert_eq!(counts.inbox, 1);
    assert_eq!(counts.tags.values().copied().sum::<u64>(), 1);
    let statistics = library.library_statistics().unwrap();
    assert_eq!(statistics.active_items, 1);
    assert_eq!(statistics.inbox_items, 1);
    assert_eq!(statistics.media_assets, 2);
    assert_eq!(statistics.image_assets, 2);
    assert_eq!(statistics.physical_files, 2);
    assert_eq!(statistics.revision, counts.revision);

    library
        .add_tag(
            &SelectionTarget::Explicit {
                root_ids: vec![active],
            },
            "series:test",
        )
        .unwrap();
    let tag_id = library
        .projections()
        .snapshot()
        .tags
        .iter()
        .find_map(|(id, roots)| roots.contains(active.0).then_some(*id))
        .unwrap();
    let tagged = RootQuery {
        scope: ItemScope::All,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Tags {
                tag_ids: vec![tag_id],
                mode: SetMatchMode::All,
            }),
            sort: ItemSort::default(),
        },
    };
    assert_eq!(
        library
            .query(&tagged, &PageRequest::default())
            .unwrap()
            .total,
        1
    );

    library
        .set_lifecycle(
            &SelectionTarget::Explicit {
                root_ids: vec![active],
            },
            Lifecycle::Trash,
        )
        .unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        library
            .query(&query(ItemScope::Trash), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
}

#[test]
fn multiple_manual_tags_use_one_atomic_publication_and_one_inverse() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (root, _) = library
        .ingest(&imported("batch-tags", Lifecycle::Active, &[]))
        .unwrap();
    let before_revision = library.database().revision().unwrap();

    let receipt = library
        .apply_tags(
            &SelectionTarget::Explicit {
                root_ids: vec![root],
            },
            &["creator:alice".into(), "series:example".into()],
            true,
        )
        .unwrap();

    assert_eq!(receipt.revision, before_revision + 1);
    assert_eq!(library.details(root).unwrap().tag_ids.len(), 2);
    assert_eq!(library.history().state().entries, 1);
    library.undo().unwrap();
    assert!(library.details(root).unwrap().tag_ids.is_empty());
}

#[test]
fn compound_metadata_patch_settles_once_and_undoes_as_one_action() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported("metadata-one", Lifecycle::Active, &[]))
        .unwrap();
    let (second, _) = library
        .ingest(&imported("metadata-two", Lifecycle::Active, &[]))
        .unwrap();
    let before_revision = library.database().revision().unwrap();
    let target = SelectionTarget::Explicit {
        root_ids: vec![first, second],
    };

    let receipt = library
        .patch_metadata(
            &target,
            Some(Rating::Five),
            Some(Some("shared note".into())),
            Some(vec!["https://example.test/shared".into()]),
            1_800_000_000_000,
        )
        .unwrap();

    assert_eq!(receipt.revision, before_revision + 1);
    assert_eq!(library.history().state().entries, 1);
    for root in [first, second] {
        let details = library.details(root).unwrap();
        assert_eq!(details.rating, Rating::Five);
        assert_eq!(details.root.notes.as_deref(), Some("shared note"));
        assert_eq!(details.root.source_urls, ["https://example.test/shared"]);
    }

    library.undo().unwrap();
    for root in [first, second] {
        let details = library.details(root).unwrap();
        assert_eq!(details.rating, Rating::Unrated);
        assert_eq!(details.root.notes, None);
        assert_eq!(
            details.root.source_urls,
            [format!(
                "https://example.test/{}",
                details.root.name.trim_end_matches(".png")
            )]
        );
    }
}

#[test]
fn multi_rename_uses_one_publication_and_preserves_one_inverse() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported("rename-one", Lifecycle::Active, &[]))
        .unwrap();
    let (second, _) = library
        .ingest(&imported("rename-two", Lifecycle::Active, &[]))
        .unwrap();
    let before_revision = library.database().revision().unwrap();

    let receipt = library
        .rename_roots(
            &[(first, "First".into()), (second, "Second".into())],
            1_800_000_000_000,
        )
        .unwrap();

    assert_eq!(receipt.revision, before_revision + 1);
    assert_eq!(library.history().state().entries, 1);
    assert_eq!(library.details(first).unwrap().root.name, "First");
    assert_eq!(library.details(first).unwrap().media[0].media_name, "First");
    assert_eq!(library.details(second).unwrap().root.name, "Second");
    library.undo().unwrap();
    assert_eq!(library.details(first).unwrap().root.name, "rename-one.png");
    assert_eq!(library.details(second).unwrap().root.name, "rename-two.png");
}

#[test]
fn tag_rename_changes_only_the_dictionary() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (root, _) = library
        .ingest(&imported("tagged", Lifecycle::Active, &["old:name"]))
        .unwrap();
    let before = library.projections().snapshot();
    let tag_id = before.tag_ids_by_name["old:name"];
    let members = before.tags[&tag_id].clone();

    library.rename_tag(tag_id, "new:name").unwrap();
    let after = library.projections().snapshot();
    assert!(!after.tag_ids_by_name.contains_key("old:name"));
    assert_eq!(after.tag_ids_by_name["new:name"], tag_id);
    assert_eq!(after.tags[&tag_id], members);
    assert!(after.tags[&tag_id].contains(root.0));

    library.undo().unwrap().unwrap();
    let undone = library.projections().snapshot();
    assert_eq!(undone.tag_ids_by_name["old:name"], tag_id);
    assert_eq!(undone.tags[&tag_id], members);
    library.redo().unwrap().unwrap();
    assert_eq!(
        library.projections().snapshot().tag_ids_by_name["new:name"],
        tag_id
    );

    let tag = library
        .tags()
        .unwrap()
        .into_iter()
        .find(|tag| tag.tag_id == tag_id)
        .unwrap();
    assert_eq!(tag.active_count, 1);
    assert_eq!(tag.assignment_count, 1);
    library
        .rename_tag_namespace(tag.namespace_id, "artist")
        .unwrap();
    let renamed_namespace = library.projections().snapshot();
    assert_eq!(renamed_namespace.tag_ids_by_name["artist:name"], tag_id);
    assert_eq!(renamed_namespace.tags[&tag_id], members);
    library.undo().unwrap().unwrap();
    assert_eq!(
        library.projections().snapshot().tag_ids_by_name["new:name"],
        tag_id
    );
}

#[test]
fn empty_tag_namespace_creation_is_published_and_undoable() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();

    let receipt = library.create_tag_namespace("creator").unwrap();
    assert!(receipt.resources.contains(&"tags".to_string()));
    let created = library
        .tag_namespaces()
        .unwrap()
        .into_iter()
        .find(|namespace| namespace.name == "creator")
        .unwrap();
    assert_eq!(created.tag_count, 0);

    library.undo().unwrap().unwrap();
    assert!(!library
        .tag_namespaces()
        .unwrap()
        .iter()
        .any(|namespace| namespace.name == "creator"));
    library.redo().unwrap().unwrap();
    assert!(library
        .tag_namespaces()
        .unwrap()
        .iter()
        .any(|namespace| namespace.name == "creator"));
}

#[test]
fn tag_merge_and_delete_rewrite_saved_queries_and_restore_from_memory_history() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported("first-tag", Lifecycle::Active, &["source"]))
        .unwrap();
    let (second, _) = library
        .ingest(&imported(
            "second-tag",
            Lifecycle::Active,
            &["source", "destination"],
        ))
        .unwrap();
    let snapshot = library.projections().snapshot();
    let source = snapshot.tag_ids_by_name["source"];
    let destination = snapshot.tag_ids_by_name["destination"];
    let (auto_folder, _) = library.create_folder("Auto source", None).unwrap();
    library
        .set_folder_auto_tags(auto_folder, vec![source], 1_700_000_000_650)
        .unwrap();
    let (smart_folder, _) = library
        .create_smart_folder(smart_input(
            "Source",
            None,
            ViewQuerySpec {
                filter: FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: vec![source],
                    mode: SetMatchMode::All,
                }),
                sort: ItemSort::default(),
            },
        ))
        .unwrap();
    let smart_query = RootQuery {
        scope: ItemScope::SmartFolder {
            smart_folder_id: smart_folder,
        },
        view: ViewQuerySpec::default(),
    };

    library
        .merge_tags(source, destination, 1_700_000_000_700)
        .unwrap();
    let merged = library.projections().snapshot();
    assert!(!merged.tags.contains_key(&source));
    assert_eq!(merged.tags[&destination].len(), 2);
    assert_eq!(root_tag_count(&library, first), 1);
    assert_eq!(root_tag_count(&library, second), 1);
    assert_eq!(folder_auto_tags(&library, auto_folder), vec![destination.0]);
    assert_eq!(
        library
            .query(&smart_query, &PageRequest::default())
            .unwrap()
            .total,
        2
    );

    library.undo().unwrap().unwrap();
    let restored = library.projections().snapshot();
    assert_eq!(restored.tags[&source].len(), 2);
    assert_eq!(restored.tags[&destination].len(), 1);
    assert_eq!(root_tag_count(&library, first), 1);
    assert_eq!(root_tag_count(&library, second), 2);
    assert_eq!(folder_auto_tags(&library, auto_folder), vec![source.0]);
    assert_eq!(
        library
            .query(&smart_query, &PageRequest::default())
            .unwrap()
            .total,
        2
    );
    library.redo().unwrap().unwrap();

    library.delete_tag(destination, 1_700_000_000_800).unwrap();
    assert_eq!(root_tag_count(&library, first), 0);
    assert_eq!(root_tag_count(&library, second), 0);
    assert_eq!(
        library
            .query(&smart_query, &PageRequest::default())
            .unwrap()
            .total,
        2
    );
    library.undo().unwrap().unwrap();
    assert_eq!(library.projections().snapshot().tags[&destination].len(), 2);
}

#[test]
fn deleting_a_starred_tag_prunes_settings_and_undo_restores_both() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    library
        .ingest(&imported(
            "starred-item",
            Lifecycle::Active,
            &["starred-delete"],
        ))
        .unwrap();
    let tag_id = library.projections().snapshot().tag_ids_by_name["starred-delete"];
    library
        .replace_auxiliary_json(
            "settings.replace",
            "Replace settings",
            "setting",
            "application",
            Some(r#"{"starredTags":["starred-delete"]}"#.into()),
        )
        .unwrap();

    let receipt = library.delete_tag(tag_id, 1_700_000_000_900).unwrap();
    assert!(receipt
        .resources
        .iter()
        .any(|resource| resource == "settings"));
    assert_eq!(starred_tags(&library), Vec::<String>::new());

    library.undo().unwrap().unwrap();
    assert!(library
        .projections()
        .snapshot()
        .tag_ids_by_name
        .contains_key("starred-delete"));
    assert_eq!(starred_tags(&library), vec!["starred-delete"]);

    library.redo().unwrap().unwrap();
    assert!(!library
        .projections()
        .snapshot()
        .tag_ids_by_name
        .contains_key("starred-delete"));
    assert_eq!(starred_tags(&library), Vec::<String>::new());
}

#[test]
fn undo_and_redo_are_process_memory_only_and_restore_bitmap_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (root, _) = library
        .ingest(&imported("one", Lifecycle::Active, &[]))
        .unwrap();
    let target = SelectionTarget::Explicit {
        root_ids: vec![root],
    };

    library.add_tag(&target, "character:one").unwrap();
    assert_eq!(library.history().state().entries, 1);
    assert_eq!(root_tag_count(&library, root), 1);

    library.undo().unwrap().unwrap();
    assert_eq!(root_tag_count(&library, root), 0);
    assert!(library.history().state().can_redo);

    library.redo().unwrap().unwrap();
    assert_eq!(root_tag_count(&library, root), 1);

    library
        .rename_root(root, "Renamed", 1_700_000_000_010)
        .unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .items[0]
            .name,
        "Renamed"
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .items[0]
            .name,
        "one.png"
    );

    drop(library);
    let reopened = Library::open(&path).unwrap();
    assert_eq!(root_tag_count(&reopened, root), 1);
    assert_eq!(reopened.history().state().entries, 0);
}

#[test]
fn ai_tag_assignments_union_member_predictions_onto_one_collection_root() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (image, _) = library
        .ingest(&imported_as(
            "ai-image",
            "image/png",
            LabColor {
                l: 50.0,
                a: 10.0,
                b: -5.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (video, _) = library
        .ingest(&imported_as(
            "ai-video",
            "video/mp4",
            LabColor {
                l: 60.0,
                a: -10.0,
                b: 5.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![image, video],
            },
            cover_root_id: video,
            winning_collection_id: None,
            name: Some("AI collection".into()),
            notes: None,
            modified_at_ms: 1_700_000_000_100,
        })
        .unwrap();

    let receipt = library
        .add_tag_assignments(&[
            RootTagAssignment {
                root_id: collection,
                tags: vec!["character:cat".into(), "series:test".into()],
            },
            RootTagAssignment {
                root_id: collection,
                tags: vec!["character:cat".into(), "character:dog".into()],
            },
        ])
        .unwrap();

    assert_eq!(receipt.item_ids, vec![collection]);
    let details = library.details(collection).unwrap();
    assert_eq!(details.tag_ids.len(), 3);
    assert_eq!(root_tag_count(&library, collection), 3);
    assert_eq!(root_tag_count(&library, image), 0);
    assert_eq!(root_tag_count(&library, video), 0);

    library.undo().unwrap().unwrap();
    assert_eq!(root_tag_count(&library, collection), 0);
    library.redo().unwrap().unwrap();
    assert_eq!(root_tag_count(&library, collection), 3);
}

#[test]
fn cloud_reads_compact_semantic_journal_without_advancing_library_revision() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (root, _) = library
        .ingest(&imported("cloud-journal", Lifecycle::Active, &[]))
        .unwrap();
    library
        .add_tag(
            &SelectionTarget::Explicit {
                root_ids: vec![root],
            },
            "cloud:tag",
        )
        .unwrap();
    let revision = library.database().revision().unwrap();
    let pending = library.pending_cloud_journal(10).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|record| record.operation_kind.as_str())
            .collect::<Vec<_>>(),
        vec!["root.ingest", "tag.add"]
    );
    assert!(pending[1]
        .target_root_ids
        .as_ref()
        .is_some_and(|roots| roots.contains(root.0)));
    library
        .mark_cloud_journal_expanded(&[pending[0].journal_id], 1_700_000_005_000)
        .unwrap();
    assert_eq!(library.database().revision().unwrap(), revision);
    assert_eq!(library.pending_cloud_journal(10).unwrap().len(), 1);
}

fn root_tag_count(library: &Library, root_id: picto_library::RootId) -> usize {
    library
        .projections()
        .snapshot()
        .tags
        .values()
        .filter(|roots| roots.contains(root_id.0))
        .count()
}

fn folder_auto_tags(library: &Library, folder_id: picto_library::FolderId) -> Vec<u32> {
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let payload = connection.query_row(
                    "SELECT auto_tag_ids FROM folder_definition WHERE folder_id = ?1",
                    [folder_id.0],
                    |row| row.get::<_, Vec<u8>>(0),
                )?;
                if payload.is_empty() {
                    return Ok(Vec::new());
                }
                let tags =
                    roaring::RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(payload))?;
                Ok(tags.iter().collect())
            },
        )
        .unwrap()
}

fn starred_tags(library: &Library) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(
        library
            .read_auxiliary_json("setting", "application")
            .unwrap()
            .as_deref()
            .unwrap(),
    )
    .unwrap()["starredTags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

fn folder_name(library: &Library, folder_id: picto_library::FolderId) -> String {
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                connection
                    .query_row(
                        "SELECT name FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                        |row| row.get(0),
                    )
                    .map_err(Into::into)
            },
        )
        .unwrap()
}

#[test]
fn folder_vector_is_the_only_folder_membership_authority() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported("one", Lifecycle::Active, &[]))
        .unwrap();
    let (second, _) = library
        .ingest(&imported("two", Lifecycle::Active, &[]))
        .unwrap();
    let (folder, _) = library.create_folder("Reference", None).unwrap();
    library.rename_folder(folder, "Renamed reference").unwrap();
    assert_eq!(folder_name(&library, folder), "Renamed reference");
    library.undo().unwrap().unwrap();
    assert_eq!(folder_name(&library, folder), "Reference");
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![second, first],
            },
            folder,
        )
        .unwrap();
    let folder_query = RootQuery {
        scope: ItemScope::Folder { folder_id: folder },
        view: ViewQuerySpec {
            filter: FilterExpr::default(),
            sort: ItemSort {
                field: SortField::FolderOrder,
                direction: SortDirection::Ascending,
                random_seed: None,
            },
        },
    };
    let page = library
        .query(&folder_query, &PageRequest::default())
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(library.projections().snapshot().folders[&folder].len(), 2);
}

#[test]
fn folder_tree_scope_includes_descendant_content_without_duplicates() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (parent_only, _) = library
        .ingest(&imported("parent-only", Lifecycle::Active, &[]))
        .unwrap();
    let (child_only, _) = library
        .ingest(&imported("child-only", Lifecycle::Active, &[]))
        .unwrap();
    let (shared, _) = library
        .ingest(&imported("shared", Lifecycle::Active, &[]))
        .unwrap();
    let (parent, _) = library.create_folder("Parent", None).unwrap();
    let (child, _) = library.create_folder("Child", Some(parent)).unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![parent_only, shared],
            },
            parent,
        )
        .unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![child_only, shared],
            },
            child,
        )
        .unwrap();

    let direct = library
        .query(
            &query(ItemScope::Folder { folder_id: parent }),
            &PageRequest::default(),
        )
        .unwrap();
    assert_eq!(direct.total, 2);

    let recursive = library
        .query(
            &query(ItemScope::FolderTree { folder_id: parent }),
            &PageRequest::default(),
        )
        .unwrap();
    assert_eq!(recursive.total, 3);
    assert_eq!(
        recursive
            .items
            .iter()
            .map(|item| item.root_id)
            .collect::<std::collections::HashSet<_>>(),
        [parent_only, child_only, shared].into_iter().collect()
    );
}

#[test]
fn folder_hierarchy_is_capped_at_eight_levels() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();

    let mut parent = None;
    for depth in 1..=picto_library::MAX_FOLDER_DEPTH {
        let (folder_id, _) = library
            .create_folder(&format!("Level {depth}"), parent)
            .unwrap();
        parent = Some(folder_id);
    }
    let error = library.create_folder("Too deep", parent).unwrap_err();
    assert!(error
        .to_string()
        .contains("folders may be nested at most 8 levels deep"));

    let (subtree, _) = library.create_folder("Subtree", None).unwrap();
    let (subtree_child, _) = library
        .create_folder("Subtree child", Some(subtree))
        .unwrap();
    let destination = library
        .folders()
        .unwrap()
        .into_iter()
        .find(|folder| folder.name == "Level 7")
        .unwrap()
        .folder_id;
    let error = library.move_folder(subtree, Some(destination)).unwrap_err();
    assert!(error
        .to_string()
        .contains("folders may be nested at most 8 levels deep"));
    let folders = library.folders().unwrap();
    assert_eq!(
        folders
            .iter()
            .find(|folder| folder.folder_id == subtree)
            .unwrap()
            .parent_id,
        None
    );
    assert_eq!(
        folders
            .iter()
            .find(|folder| folder.folder_id == subtree_child)
            .unwrap()
            .parent_id,
        Some(subtree)
    );
}

#[test]
fn folder_hierarchy_metadata_and_item_order_use_one_reversible_path() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (temporary, _) = library.create_folder("Temporary", None).unwrap();
    assert_eq!(library.folders().unwrap().len(), 1);
    library.undo().unwrap().unwrap();
    assert!(library.folders().unwrap().is_empty());
    library.redo().unwrap().unwrap();
    assert_eq!(library.folders().unwrap()[0].folder_id, temporary);

    let (parent, _) = library.create_folder("Parent", None).unwrap();
    let (first_child, _) = library.create_folder("First", Some(parent)).unwrap();
    let (second_child, _) = library.create_folder("Second", Some(parent)).unwrap();
    library
        .set_folder_metadata(
            first_child,
            Some("folder-heart"),
            Some("#336699"),
            Some("  references  "),
        )
        .unwrap();
    let first_record = library
        .folders()
        .unwrap()
        .into_iter()
        .find(|folder| folder.folder_id == first_child)
        .unwrap();
    assert_eq!(first_record.icon.as_deref(), Some("folder-heart"));
    assert_eq!(first_record.color.as_deref(), Some("#336699"));
    assert_eq!(first_record.notes.as_deref(), Some("references"));

    library
        .reorder_folder_children(Some(parent), &[second_child, first_child])
        .unwrap();
    let children = library
        .folders()
        .unwrap()
        .into_iter()
        .filter(|folder| folder.parent_id == Some(parent))
        .map(|folder| folder.folder_id)
        .collect::<Vec<_>>();
    assert_eq!(children, vec![second_child, first_child]);
    library.undo().unwrap().unwrap();
    let children = library
        .folders()
        .unwrap()
        .into_iter()
        .filter(|folder| folder.parent_id == Some(parent))
        .map(|folder| folder.folder_id)
        .collect::<Vec<_>>();
    assert_eq!(children, vec![first_child, second_child]);

    library.move_folder(second_child, None).unwrap();
    assert_eq!(
        library
            .folders()
            .unwrap()
            .into_iter()
            .find(|folder| folder.folder_id == second_child)
            .unwrap()
            .parent_id,
        None
    );
    assert!(library.move_folder(parent, Some(first_child)).is_err());

    let mut alpha = imported("alpha", Lifecycle::Active, &[]);
    alpha.imported_at_ms = 100;
    alpha.captured_at_ms = Some(100);
    alpha.facts.size_bytes = 100;
    alpha.notes = Some("zulu".into());
    let mut beta = imported("beta", Lifecycle::Active, &[]);
    beta.imported_at_ms = 200;
    beta.captured_at_ms = Some(200);
    beta.facts.size_bytes = 200;
    beta.notes = Some("alpha".into());
    let (first, _) = library.ingest(&alpha).unwrap();
    let (second, _) = library.ingest(&beta).unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![first, second],
            },
            first_child,
        )
        .unwrap();
    library
        .reorder_folder_items(first_child, &[second, first])
        .unwrap();
    let folder_query = RootQuery {
        scope: ItemScope::Folder {
            folder_id: first_child,
        },
        view: ViewQuerySpec {
            filter: FilterExpr::default(),
            sort: ItemSort {
                field: SortField::FolderOrder,
                direction: SortDirection::Ascending,
                random_seed: None,
            },
        },
    };
    assert_eq!(
        library
            .query(&folder_query, &PageRequest::default())
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![second, first]
    );
    library
        .sort_folder_items(first_child, ContentSortField::Name)
        .unwrap();
    assert_eq!(
        library
            .query(&folder_query, &PageRequest::default())
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    for field in [
        ContentSortField::ImportedAt,
        ContentSortField::CreatedAt,
        ContentSortField::ModifiedAt,
        ContentSortField::Size,
        ContentSortField::Notes,
    ] {
        library.sort_folder_items(first_child, field).unwrap();
        assert_eq!(
            library
                .query(&folder_query, &PageRequest::default())
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.root_id)
                .collect::<Vec<_>>(),
            vec![second, first]
        );
    }
    assert_eq!(
        library
            .folders()
            .unwrap()
            .into_iter()
            .find(|folder| folder.folder_id == first_child)
            .unwrap()
            .count,
        2
    );
}

#[test]
fn folder_subtree_deletion_preserves_media_and_restores_exact_membership_on_undo() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (active, _) = library
        .ingest(&imported("folder-delete-active", Lifecycle::Active, &[]))
        .unwrap();
    let (inbox, _) = library
        .ingest(&imported("folder-delete-inbox", Lifecycle::Inbox, &[]))
        .unwrap();
    let (parent, _) = library.create_folder("Parent", None).unwrap();
    let (child, _) = library.create_folder("Child", Some(parent)).unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![active, inbox],
            },
            child,
        )
        .unwrap();
    let (smart, _) = library
        .create_smart_folder(smart_input(
            "In child",
            None,
            ViewQuerySpec {
                filter: FilterExpr::Clause(FilterClause::Folders {
                    folder_ids: vec![child],
                    mode: SetMatchMode::Any,
                }),
                sort: ItemSort::default(),
            },
        ))
        .unwrap();
    assert_eq!(library.counts().unwrap().folders[&child], 1);
    assert_eq!(library.counts().unwrap().smart_folders[&smart], 1);

    let child_delete = library.delete_folders(&[child]).unwrap();
    assert_eq!(child_delete.deleted_folder_ids, vec![child]);
    assert_eq!(child_delete.fallback_folder_id, Some(parent));
    assert_eq!(library.counts().unwrap().all, 1);
    assert_eq!(library.counts().unwrap().inbox, 1);
    assert_eq!(library.counts().unwrap().smart_folders[&smart], 0);
    library.undo().unwrap().unwrap();
    assert_eq!(library.counts().unwrap().folders[&child], 1);
    assert_eq!(library.counts().unwrap().smart_folders[&smart], 1);

    let tree_delete = library.delete_folders(&[parent]).unwrap();
    assert_eq!(
        tree_delete
            .deleted_folder_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        [parent, child].into_iter().collect()
    );
    assert_eq!(tree_delete.fallback_folder_id, None);
    assert!(library.folders().unwrap().is_empty());
    assert_eq!(library.counts().unwrap().all, 1);
    assert_eq!(library.counts().unwrap().inbox, 1);
    library.undo().unwrap().unwrap();
    assert_eq!(library.folders().unwrap().len(), 2);
    assert_eq!(library.counts().unwrap().folders[&child], 1);
    assert!(library.projections().snapshot().folders[&child].contains(inbox.0));
    library.redo().unwrap().unwrap();
    assert!(library.folders().unwrap().is_empty());
}

#[test]
fn folder_auto_tags_apply_once_on_assignment_and_participate_in_memory_history() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (seed, _) = library
        .ingest(&imported("auto-seed", Lifecycle::Active, &["auto:folder"]))
        .unwrap();
    let tag_id = library.projections().snapshot().tag_ids_by_name["auto:folder"];
    let (folder, _) = library.create_folder("Automatic", None).unwrap();
    library
        .set_folder_auto_tags(folder, vec![tag_id], 1_700_000_003_000)
        .unwrap();

    let mut imported_into_folder = imported("auto-ingest", Lifecycle::Active, &[]);
    imported_into_folder.folders = vec![folder];
    let (ingested, _) = library.ingest(&imported_into_folder).unwrap();
    assert_eq!(root_tag_count(&library, ingested), 1);

    let (manual, _) = library
        .ingest(&imported("auto-manual", Lifecycle::Active, &[]))
        .unwrap();
    let target = SelectionTarget::Explicit {
        root_ids: vec![manual],
    };
    library.add_to_folder(&target, folder).unwrap();
    assert_eq!(root_tag_count(&library, manual), 1);
    library.undo().unwrap().unwrap();
    assert_eq!(root_tag_count(&library, manual), 0);
    assert!(!library.projections().snapshot().folders[&folder].contains(manual.0));
    library.redo().unwrap().unwrap();
    assert_eq!(root_tag_count(&library, manual), 1);

    library.remove_from_folder(&target, folder).unwrap();
    assert_eq!(root_tag_count(&library, manual), 1);
    assert_eq!(root_tag_count(&library, seed), 1);
}

#[test]
fn explicit_and_query_selections_have_identical_summaries() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported("one", Lifecycle::Active, &[]))
        .unwrap();
    let (second, _) = library
        .ingest(&imported("two", Lifecycle::Active, &[]))
        .unwrap();
    let explicit = library
        .selection_summary(&SelectionTarget::Explicit {
            root_ids: vec![first, second],
        })
        .unwrap();
    let selected_query = library
        .selection_summary(&SelectionTarget::Query {
            query: query(ItemScope::All),
            excluded_root_ids: Vec::new(),
        })
        .unwrap();
    assert_eq!(explicit.selected_count, selected_query.selected_count);
    assert_eq!(explicit.total_size_bytes, selected_query.total_size_bytes);
    assert_eq!(explicit.media_count, selected_query.media_count);
    assert_eq!(explicit.shared_source_urls, None);
}

#[test]
fn selection_summary_returns_six_ordered_previews_and_structural_collection_candidates() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut roots = Vec::new();
    for index in 0..8 {
        let mut input = imported(&format!("preview-{index}"), Lifecycle::Active, &[]);
        input.imported_at_ms += index;
        roots.push(library.ingest(&input).unwrap().0);
    }
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![roots[0], roots[1]],
            },
            cover_root_id: roots[0],
            winning_collection_id: None,
            name: Some("Preview collection".into()),
            notes: None,
            modified_at_ms: 1_700_000_000_100,
        })
        .unwrap();
    let explicit_roots = [collection]
        .into_iter()
        .chain(roots[2..].iter().copied())
        .collect::<Vec<_>>();
    let summary = library
        .selection_summary(&SelectionTarget::Explicit {
            root_ids: explicit_roots,
        })
        .unwrap();

    assert_eq!(
        summary.sample_hashes,
        (2..8)
            .map(|index| format!("hash-preview-{index}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(summary.collection_candidates.len(), 1);
    assert_eq!(summary.collection_candidates[0].collection_id, collection);
    assert_eq!(summary.collection_candidates[0].label, "Preview collection");
    assert_eq!(summary.collection_candidates[0].member_count, 2);
}

#[test]
fn selection_image_compatibility_treats_a_collection_as_one_root_with_images() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (image, _) = library
        .ingest(&imported_as(
            "image-member",
            "image/png",
            LabColor {
                l: 50.0,
                a: 0.0,
                b: 0.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (video, _) = library
        .ingest(&imported_as(
            "video-member",
            "video/mp4",
            LabColor {
                l: 40.0,
                a: 0.0,
                b: 0.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![image, video],
            },
            cover_root_id: image,
            winning_collection_id: None,
            name: Some("Mixed media".into()),
            notes: None,
            modified_at_ms: 1_700_000_000_100,
        })
        .unwrap();

    let summary = library
        .selection_summary(&SelectionTarget::Explicit {
            root_ids: vec![collection],
        })
        .unwrap();
    assert!(summary.all_selected_roots_have_images);
    assert_eq!(summary.sample_hashes, vec!["hash-image-member"]);
}

#[test]
fn smart_folders_use_the_grid_predicate_and_settle_with_mutations() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported("first", Lifecycle::Active, &["character:alice"]))
        .unwrap();
    let tag_id = *library.projections().snapshot().tags.keys().next().unwrap();
    let view = ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::Tags {
            tag_ids: vec![tag_id],
            mode: SetMatchMode::All,
        }),
        sort: ItemSort::default(),
    };
    let (smart_folder_id, _) = library
        .create_smart_folder(smart_input("Alice", None, view.clone()))
        .unwrap();
    let smart_query = RootQuery {
        scope: ItemScope::SmartFolder { smart_folder_id },
        view: ViewQuerySpec::default(),
    };
    assert_eq!(
        library
            .query(&smart_query, &PageRequest::default())
            .unwrap()
            .total,
        1
    );

    let (second, _) = library
        .ingest(&imported("second", Lifecycle::Active, &[]))
        .unwrap();
    library
        .add_tag(
            &SelectionTarget::Explicit {
                root_ids: vec![second],
            },
            "character:alice",
        )
        .unwrap();
    assert_eq!(
        library
            .query(&smart_query, &PageRequest::default())
            .unwrap()
            .total,
        2
    );

    let direct_query = RootQuery {
        scope: ItemScope::All,
        view,
    };
    assert_eq!(
        library
            .query(&direct_query, &PageRequest::default())
            .unwrap()
            .total,
        2
    );

    library
        .set_lifecycle(
            &SelectionTarget::Explicit {
                root_ids: vec![first],
            },
            Lifecycle::Inbox,
        )
        .unwrap();
    assert_eq!(
        library
            .query(&smart_query, &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(library.smart_folders().unwrap()[0].count, 1);
}

#[test]
fn nested_smart_folders_inherit_rules_with_bounded_depth_and_complexity() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    library
        .ingest(&imported(
            "alice-image",
            Lifecycle::Active,
            &["character:alice"],
        ))
        .unwrap();
    let mut alice_video = imported("alice-video", Lifecycle::Active, &["character:alice"]);
    alice_video.facts.mime = "video/mp4".into();
    library.ingest(&alice_video).unwrap();
    let mut bob_image = imported("bob-image", Lifecycle::Active, &["character:bob"]);
    bob_image.facts.size_bytes = 2048;
    let (bob_root, _) = library.ingest(&bob_image).unwrap();

    let snapshot = library.projections().snapshot();
    let alice = snapshot.tag_ids_by_name["character:alice"];
    let bob = snapshot.tag_ids_by_name["character:bob"];
    drop(snapshot);
    let tag_view = |tag_id| ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::Tags {
            tag_ids: vec![tag_id],
            mode: SetMatchMode::All,
        }),
        sort: ItemSort::default(),
    };
    let (parent, _) = library
        .create_smart_folder(smart_input("Alice", None, tag_view(alice)))
        .unwrap();
    let child_view = ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::Mime {
            values: vec!["image/png".into()],
            families: Vec::new(),
        }),
        sort: ItemSort::default(),
    };
    let (child, _) = library
        .create_smart_folder(smart_input(
            "Alice images",
            Some(parent),
            child_view.clone(),
        ))
        .unwrap();
    let grandchild_view = ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::TotalSize {
            minimum_bytes: Some(1024),
            maximum_bytes: Some(1024),
        }),
        sort: ItemSort::default(),
    };
    let (grandchild, _) = library
        .create_smart_folder(smart_input(
            "Small Alice images",
            Some(child),
            grandchild_view,
        ))
        .unwrap();

    let total = |smart_folder_id| {
        library
            .query(
                &query(ItemScope::SmartFolder { smart_folder_id }),
                &PageRequest::default(),
            )
            .unwrap()
            .total
    };
    assert_eq!(total(parent), 2);
    assert_eq!(total(child), 1);
    assert_eq!(total(grandchild), 1);
    assert_eq!(
        picto_library::predicate::clause_count(
            &library.projections().snapshot().smart_effective_queries[&grandchild.0].filter,
        ),
        3
    );
    let saved_child = library
        .smart_folders()
        .unwrap()
        .into_iter()
        .find(|folder| folder.smart_folder_id == child)
        .unwrap();
    assert_eq!(saved_child.view, child_view);

    library
        .add_tag(
            &SelectionTarget::Explicit {
                root_ids: vec![bob_root],
            },
            "character:alice",
        )
        .unwrap();
    assert_eq!(total(parent), 3);
    assert_eq!(total(child), 2);
    assert_eq!(total(grandchild), 1);

    library
        .update_smart_folder(parent, smart_input("Bob", None, tag_view(bob)))
        .unwrap();
    assert_eq!(total(parent), 1);
    assert_eq!(total(child), 1);
    assert_eq!(total(grandchild), 0);

    let great_grandchild_view = ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::NotesPresent { present: false }),
        sort: ItemSort::default(),
    };
    let (great_grandchild, _) = library
        .create_smart_folder(smart_input(
            "Fourth level",
            Some(grandchild),
            great_grandchild_view,
        ))
        .unwrap();
    assert_eq!(
        picto_library::predicate::clause_count(
            &library.projections().snapshot().smart_effective_queries[&great_grandchild.0].filter,
        ),
        4
    );
    let mut deepest = great_grandchild;
    for level in 5..=8 {
        deepest = library
            .create_smart_folder(smart_input(
                &format!("Level {level}"),
                Some(deepest),
                ViewQuerySpec {
                    filter: FilterExpr::Clause(FilterClause::SourceUrlsPresent { present: true }),
                    sort: ItemSort::default(),
                },
            ))
            .unwrap()
            .0;
    }
    assert_eq!(
        picto_library::predicate::clause_count(
            &library.projections().snapshot().smart_effective_queries[&deepest.0].filter,
        ),
        8
    );
    assert!(library
        .create_smart_folder(smart_input(
            "Too deep",
            Some(deepest),
            ViewQuerySpec::default(),
        ))
        .is_err());
    let eleven_rules = ViewQuerySpec {
        filter: FilterExpr::All(
            (0..11)
                .map(|_| FilterExpr::Clause(FilterClause::NotesPresent { present: true }))
                .collect(),
        ),
        sort: ItemSort::default(),
    };
    assert!(library
        .create_smart_folder(smart_input("Too many rules", None, eleven_rules))
        .is_err());
    library.write_projection_checkpoint().unwrap();
    drop(library);
    let reopened = Library::open(path).unwrap();
    assert_eq!(
        reopened
            .query(
                &query(ItemScope::SmartFolder {
                    smart_folder_id: grandchild
                }),
                &PageRequest::default(),
            )
            .unwrap()
            .total,
        0
    );
}

#[test]
fn captured_date_filter_uses_the_exact_numeric_projection() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (captured, _) = library
        .ingest(&imported("captured-date", Lifecycle::Active, &[]))
        .unwrap();
    let mut missing_date = imported("missing-date", Lifecycle::Active, &[]);
    missing_date.captured_at_ms = None;
    library.ingest(&missing_date).unwrap();

    let result = library
        .query(
            &RootQuery {
                scope: ItemScope::All,
                view: ViewQuerySpec {
                    filter: FilterExpr::Clause(FilterClause::CapturedAt {
                        minimum_ms: Some(1_500_000_000_000),
                        maximum_ms: Some(1_650_000_000_000),
                    }),
                    sort: ItemSort::default(),
                },
            },
            &PageRequest::default(),
        )
        .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].root_id, captured);
}

#[test]
fn smart_folder_definitions_are_typed_cycle_safe_and_session_undoable() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (parent, _) = library
        .create_smart_folder(smart_input("Parent", None, ViewQuerySpec::default()))
        .unwrap();
    let (child, _) = library
        .create_smart_folder(SmartFolderInput {
            name: "Child".into(),
            parent_id: Some(parent),
            icon: Some("search-heart".into()),
            color: Some("#336699".into()),
            notes: Some("saved query".into()),
            view: ViewQuerySpec::default(),
        })
        .unwrap();
    let child_record = library
        .smart_folders()
        .unwrap()
        .into_iter()
        .find(|folder| folder.smart_folder_id == child)
        .unwrap();
    assert_eq!(child_record.icon.as_deref(), Some("search-heart"));
    assert_eq!(child_record.color.as_deref(), Some("#336699"));
    assert_eq!(child_record.notes.as_deref(), Some("saved query"));

    assert!(library
        .update_smart_folder(
            parent,
            SmartFolderInput {
                name: "Parent".into(),
                parent_id: Some(child),
                icon: None,
                color: None,
                notes: None,
                view: ViewQuerySpec::default(),
            },
        )
        .is_err());
    library
        .update_smart_folder(
            child,
            SmartFolderInput {
                name: "Renamed child".into(),
                parent_id: Some(parent),
                icon: None,
                color: Some("#ffffff".into()),
                notes: None,
                view: ViewQuerySpec::default(),
            },
        )
        .unwrap();
    assert_eq!(
        library
            .smart_folders()
            .unwrap()
            .into_iter()
            .find(|folder| folder.smart_folder_id == child)
            .unwrap()
            .name,
        "Renamed child"
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library
            .smart_folders()
            .unwrap()
            .into_iter()
            .find(|folder| folder.smart_folder_id == child)
            .unwrap()
            .name,
        "Child"
    );

    let (sibling, _) = library
        .create_smart_folder(smart_input(
            "Sibling",
            Some(parent),
            ViewQuerySpec::default(),
        ))
        .unwrap();
    library
        .reorder_smart_folder_children(Some(parent), &[sibling, child])
        .unwrap();
    assert_eq!(
        library
            .smart_folders()
            .unwrap()
            .into_iter()
            .filter(|folder| folder.parent_id == Some(parent))
            .map(|folder| folder.smart_folder_id)
            .collect::<Vec<_>>(),
        vec![sibling, child]
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library
            .smart_folders()
            .unwrap()
            .into_iter()
            .filter(|folder| folder.parent_id == Some(parent))
            .map(|folder| folder.smart_folder_id)
            .collect::<Vec<_>>(),
        vec![child, sibling]
    );

    let deleted = library.delete_smart_folder(parent).unwrap();
    assert_eq!(deleted.deleted_smart_folder_ids.len(), 3);
    assert_eq!(deleted.fallback_smart_folder_id, None);
    assert!(library.smart_folders().unwrap().is_empty());
    library.undo().unwrap().unwrap();
    assert_eq!(library.smart_folders().unwrap().len(), 3);
    library.redo().unwrap().unwrap();
    assert!(library.smart_folders().unwrap().is_empty());
}

#[test]
fn bounded_ingest_batch_publishes_once_and_fts_respects_each_scope() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let before = library.database().revision().unwrap();
    let values = vec![
        imported("searchable-active", Lifecycle::Active, &[]),
        imported("searchable-inbox", Lifecycle::Inbox, &[]),
        imported("searchable-trash", Lifecycle::Trash, &[]),
    ];
    let outputs = library.ingest_batch(&values).unwrap();
    assert_eq!(outputs.len(), 3);
    assert!(outputs
        .iter()
        .all(|(_, receipt)| receipt.revision == outputs[0].1.revision));
    assert_eq!(outputs[0].1.revision, before + 1);
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM cloud_journal", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    1
                );
                Ok(())
            },
        )
        .unwrap();

    assert!(library.settle_fts(64).unwrap().is_some());
    let text_view = ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::Text {
            field: picto_library::predicate::TextField::Global,
            query: "searchable".into(),
        }),
        sort: ItemSort::default(),
    };
    assert_eq!(
        library
            .query(
                &RootQuery {
                    scope: ItemScope::All,
                    view: text_view.clone(),
                },
                &PageRequest::default(),
            )
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        library
            .query(
                &RootQuery {
                    scope: ItemScope::Inbox,
                    view: text_view,
                },
                &PageRequest::default(),
            )
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        library
            .query(
                &RootQuery {
                    scope: ItemScope::Trash,
                    view: ViewQuerySpec {
                        filter: FilterExpr::Clause(FilterClause::Text {
                            field: picto_library::predicate::TextField::Global,
                            query: "searchable".into(),
                        }),
                        sort: ItemSort::default(),
                    },
                },
                &PageRequest::default(),
            )
            .unwrap()
            .total,
        1
    );
    let source_text_view = ViewQuerySpec {
        filter: FilterExpr::Clause(FilterClause::Text {
            field: picto_library::predicate::TextField::Global,
            query: "metadata".into(),
        }),
        sort: ItemSort::default(),
    };
    assert_eq!(
        library
            .query(
                &RootQuery {
                    scope: ItemScope::Inbox,
                    view: source_text_view,
                },
                &PageRequest::default(),
            )
            .unwrap()
            .total,
        1
    );
}

#[test]
fn prepared_collection_import_never_publishes_standalone_members() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let before = library.database().revision().unwrap();
    let (collection, receipt) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members: vec![
                imported("atomic-one", Lifecycle::Inbox, &["post:atomic"]),
                imported("atomic-two", Lifecycle::Inbox, &["post:atomic"]),
            ],
            cover_index: 1,
            name: Some("Atomic post".into()),
            modified_at_ms: 1_700_000_004_000,
        })
        .unwrap();
    assert_eq!(receipt.revision, before + 1);
    let inbox = library
        .query(&query(ItemScope::Inbox), &PageRequest::default())
        .unwrap();
    assert_eq!(inbox.total, 1);
    assert_eq!(inbox.items[0].root_id, collection);
    assert_eq!(inbox.items[0].kind, RootKind::Collection);
    assert_eq!(inbox.items[0].media_count, 2);
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    1
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    2
                );
                assert_eq!(
                    connection.query_row(
                        "SELECT COUNT(DISTINCT revision) FROM cloud_journal",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?,
                    1
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM cloud_journal", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    1
                );
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn exact_hash_ingest_reuses_the_owner_and_transfers_only_tags() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut original = imported("exact-original", Lifecycle::Active, &["state:original"]);
    original.media_name = "938475938475.png".into();
    let (root_id, _) = library.ingest(&original).unwrap();
    library
        .database()
        .maintenance_write(
            picto_library::database::WorkPriority::Maintenance,
            |transaction| {
                transaction.execute(
                    "UPDATE work_item SET status = 'failed', attempt_count = 8,
                         last_error = 'stale path'
                     WHERE work_type = 'thumbnail'",
                    [],
                )?;
                Ok(())
            },
        )
        .unwrap();

    let mut repeated = imported("exact-repeat", Lifecycle::Inbox, &["state:incoming"]);
    repeated.facts.content_hash = original.facts.content_hash.clone();
    repeated.rating = Rating::Five;
    repeated.media_name = "A quiet landscape.png".into();
    repeated.notes = Some("A useful source note".into());
    let (reused_root_id, _) = library.ingest(&repeated).unwrap();

    assert_eq!(reused_root_id, root_id);
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        library
            .query(&query(ItemScope::Inbox), &PageRequest::default())
            .unwrap()
            .total,
        0
    );
    let snapshot = library.projections().snapshot();
    let incoming_tag = snapshot.tag_ids_by_name["state:incoming"];
    assert!(snapshot.tags[&incoming_tag].contains(root_id.0));
    assert!(snapshot.ratings[&Rating::Unrated].contains(root_id.0));
    let details = library.details(root_id).unwrap();
    assert_eq!(details.root.name, "A quiet landscape.png");
    assert_eq!(details.root.notes.as_deref(), Some("A useful source note"));
    assert_eq!(details.media[0].media_name, "A quiet landscape.png");
    assert_eq!(
        details.media[0].media_notes.as_deref(),
        Some("A useful source note")
    );
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_file", [], |row| row
                        .get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| row
                        .get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| row
                        .get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    connection
                        .query_row("SELECT file_path FROM media_file", [], |row| row
                            .get::<_, String>(0))?,
                    repeated.file_path
                );
                assert_eq!(
                    connection.query_row(
                        "SELECT status || ':' || attempt_count FROM work_item
                         WHERE work_type = 'thumbnail'",
                        [],
                        |row| row.get::<_, String>(0)
                    )?,
                    "pending:0"
                );
                assert_eq!(
                    connection
                        .query_row("SELECT COUNT(*) FROM source_provenance", [], |row| row
                            .get::<_, i64>(0))?,
                    2
                );
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn exact_hash_collection_member_reuses_file_without_absorbing_its_owner() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut original = imported("collection-existing", Lifecycle::Inbox, &["state:original"]);
    original.facts.perceptual_hash = Some("already-computed".into());
    let (existing_root, _) = library.ingest(&original).unwrap();

    let mut repeated = imported("collection-repeat", Lifecycle::Inbox, &["state:incoming"]);
    repeated.facts.content_hash = original.facts.content_hash.clone();
    let members = vec![
        repeated,
        imported("collection-new-a", Lifecycle::Inbox, &["source:post"]),
        imported("collection-new-b", Lifecycle::Inbox, &["source:post"]),
    ];
    let (collection, _) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members,
            cover_index: 1,
            name: Some("Filtered collection".into()),
            modified_at_ms: 1_700_000_003_000,
        })
        .unwrap();

    assert_ne!(collection, existing_root);
    let inbox = library
        .query(&query(ItemScope::Inbox), &PageRequest::default())
        .unwrap();
    assert_eq!(inbox.total, 2);
    assert_eq!(inbox.media_count, 4);
    let exact_matches = library
        .query(
            &query(ItemScope::MediaMatches {
                item_id: collection.0,
            }),
            &PageRequest::default(),
        )
        .unwrap();
    assert_eq!(exact_matches.total, 2);
    assert_eq!(
        exact_matches
            .items
            .iter()
            .map(|item| item.root_id)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([existing_root, collection])
    );
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection].len(),
        3
    );
    let snapshot = library.projections().snapshot();
    let incoming_tag = snapshot.tag_ids_by_name["state:incoming"];
    let original_tag = snapshot.tag_ids_by_name["state:original"];
    assert!(!snapshot.tags[&incoming_tag].contains(existing_root.0));
    assert!(snapshot.tags[&incoming_tag].contains(collection.0));
    assert!(snapshot.tags[&original_tag].contains(existing_root.0));
    assert!(snapshot.tags[&original_tag].contains(collection.0));
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_file", [], |row| row
                        .get::<_, i64>(0))?,
                    3
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| row
                        .get::<_, i64>(0))?,
                    4
                );
                assert_eq!(
                    connection.query_row(
                        "SELECT COUNT(*) FROM work_item WHERE work_type = 'perceptual_hash'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )?,
                    2
                );
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn exact_hash_collection_tags_never_flow_to_other_collections_or_standalones() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut first_member = imported(
        "collection-isolation-first",
        Lifecycle::Inbox,
        &["collection:first"],
    );
    first_member.facts.content_hash = "shared-collection-only-hash".into();
    let (first_collection, _) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members: vec![
                first_member,
                imported("collection-isolation-first-support", Lifecycle::Inbox, &[]),
            ],
            cover_index: 0,
            name: Some("First collection".into()),
            modified_at_ms: 1_700_000_004_000,
        })
        .unwrap();

    let mut second_member = imported(
        "collection-isolation-second",
        Lifecycle::Inbox,
        &["collection:second"],
    );
    second_member.facts.content_hash = "shared-collection-only-hash".into();
    let (second_collection, _) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members: vec![
                second_member,
                imported("collection-isolation-second-support", Lifecycle::Inbox, &[]),
            ],
            cover_index: 0,
            name: Some("Second collection".into()),
            modified_at_ms: 1_700_000_005_000,
        })
        .unwrap();

    let snapshot = library.projections().snapshot();
    let first_tag = snapshot.tag_ids_by_name["collection:first"];
    let second_tag = snapshot.tag_ids_by_name["collection:second"];
    assert!(snapshot.tags[&first_tag].contains(first_collection.0));
    assert!(!snapshot.tags[&first_tag].contains(second_collection.0));
    assert!(snapshot.tags[&second_tag].contains(second_collection.0));
    assert!(!snapshot.tags[&second_tag].contains(first_collection.0));
}

#[test]
fn exact_hash_standalone_import_collapses_into_collection_and_donates_its_tags() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let member = imported(
        "collection-owner-member",
        Lifecycle::Inbox,
        &["collection:own"],
    );
    let shared_hash = member.facts.content_hash.clone();
    let (collection, _) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members: vec![
                member,
                imported("collection-owner-support", Lifecycle::Inbox, &[]),
            ],
            cover_index: 0,
            name: Some("Owner collection".into()),
            modified_at_ms: 1_700_000_006_000,
        })
        .unwrap();

    let mut standalone = imported(
        "standalone-donor",
        Lifecycle::Inbox,
        &["standalone:donated"],
    );
    standalone.facts.content_hash = shared_hash;
    let (retained, _) = library.ingest(&standalone).unwrap();

    assert_eq!(retained, collection);
    assert_eq!(
        library
            .query(&query(ItemScope::Inbox), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    let snapshot = library.projections().snapshot();
    let donated = snapshot.tag_ids_by_name["standalone:donated"];
    assert!(snapshot.tags[&donated].contains(collection.0));
}

#[test]
fn exact_hash_standalone_tags_fan_out_to_every_owner_across_lifecycles() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let shared_hash = "shared-owner-fan-out-hash";
    let standalone_inputs = [
        ("fan-out-active", Lifecycle::Active),
        ("fan-out-inbox", Lifecycle::Inbox),
        ("fan-out-trash", Lifecycle::Trash),
    ]
    .map(|(key, lifecycle)| {
        let mut input = imported(key, lifecycle, &[]);
        input.facts.content_hash = shared_hash.into();
        input
    });
    let standalone_roots = library
        .ingest_conversion_batch(&standalone_inputs)
        .unwrap()
        .into_iter()
        .map(|(root_id, _)| root_id)
        .collect::<Vec<_>>();

    let mut collection_member = imported(
        "fan-out-collection-member",
        Lifecycle::Inbox,
        &["collection:local"],
    );
    collection_member.facts.content_hash = shared_hash.into();
    let (collection, _) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members: vec![
                collection_member,
                imported("fan-out-collection-support", Lifecycle::Inbox, &[]),
            ],
            cover_index: 0,
            name: Some("Fan-out collection".into()),
            modified_at_ms: 1_700_000_007_000,
        })
        .unwrap();
    let (unrelated, _) = library
        .ingest(&imported("fan-out-unrelated", Lifecycle::Active, &[]))
        .unwrap();

    let visible_roots_before = library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(
                    connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                )
            },
        )
        .unwrap();
    let mut incoming = imported(
        "fan-out-incoming-standalone",
        Lifecycle::Inbox,
        &["standalone:incoming"],
    );
    incoming.facts.content_hash = shared_hash.into();
    let (retained, _) = library.ingest(&incoming).unwrap();

    assert_eq!(retained, standalone_roots[0]);
    assert_eq!(
        library
            .database()
            .read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    Ok(
                        connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| {
                            row.get::<_, i64>(0)
                        })?,
                    )
                },
            )
            .unwrap(),
        visible_roots_before
    );
    let snapshot = library.projections().snapshot();
    let incoming_tag = snapshot.tag_ids_by_name["standalone:incoming"];
    for owner in standalone_roots.iter().copied().chain([collection]) {
        assert!(snapshot.tags[&incoming_tag].contains(owner.0));
    }
    assert!(!snapshot.tags[&incoming_tag].contains(unrelated.0));
    let collection_tag = snapshot.tag_ids_by_name["collection:local"];
    for standalone in standalone_roots {
        assert!(!snapshot.tags[&collection_tag].contains(standalone.0));
    }
}

#[test]
fn large_prepared_collection_publishes_as_one_coherent_root() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let members = (0..100)
        .map(|index| imported(&format!("atomic-member-{index}"), Lifecycle::Inbox, &[]))
        .collect::<Vec<_>>();

    let (collection, receipt) = library
        .ingest_collection(&picto_library::PreparedCollectionImport {
            members,
            cover_index: 50,
            name: Some("Large atomic source".into()),
            modified_at_ms: 1_700_000_003_000,
        })
        .unwrap();

    assert_eq!(receipt.item_ids, vec![collection]);
    let inbox = library
        .query(&query(ItemScope::Inbox), &PageRequest::default())
        .unwrap();
    assert_eq!(inbox.total, 1);
    assert_eq!(inbox.media_count, 100);
    assert_eq!(inbox.items[0].root_id, collection);
    assert_eq!(inbox.items[0].media_count, 100);
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection].len(),
        100
    );
}

#[test]
fn text_search_matches_prefixes_and_substrings() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    library
        .ingest_batch(&[
            imported("OldNewgroundsDump", Lifecycle::Active, &[]),
            imported("sunset-photo", Lifecycle::Active, &[]),
            imported("axb", Lifecycle::Active, &[]),
            imported("a_b", Lifecycle::Active, &[]),
            imported("fooXYZbar", Lifecycle::Active, &[]),
            imported("foo%bar", Lifecycle::Active, &[]),
            imported("ÄRTHUR", Lifecycle::Active, &[]),
        ])
        .unwrap();
    assert!(library.settle_fts(64).unwrap().is_some());
    let total = |field: picto_library::predicate::TextField, query: &str| {
        library
            .query(
                &RootQuery {
                    scope: ItemScope::All,
                    view: ViewQuerySpec {
                        filter: FilterExpr::Clause(FilterClause::Text {
                            field,
                            query: query.into(),
                        }),
                        sort: ItemSort::default(),
                    },
                },
                &PageRequest::default(),
            )
            .unwrap()
            .total
    };
    let global = picto_library::predicate::TextField::Global;

    assert_eq!(total(global, "oldnew"), 1, "prefix is a substring");
    assert_eq!(total(global, "ol"), 1, "one- and two-character prefixes");
    assert_eq!(total(global, "ne"), 1, "short mid-word substring");
    assert_eq!(total(global, "newground"), 1, "mid-word substring");
    assert_eq!(total(global, "NEWGROUNDS"), 1, "case-insensitive substring");
    assert_eq!(total(global, "ärth"), 1, "Unicode lowercase normalization");
    assert_eq!(total(global, "unset"), 1, "substring of a hyphenated name");
    assert_eq!(
        total(global, "sunset-photo"),
        1,
        "literal whitespace and punctuation"
    );
    assert_eq!(
        total(global, "photo sunset"),
        0,
        "contains is not token reordering"
    );
    assert_eq!(total(global, "a_b"), 1, "underscore stays literal");
    assert_eq!(total(global, "foo%bar"), 1, "percent stays literal");
    assert_eq!(total(global, "zzzz"), 0);
    assert_eq!(
        total(global, "sun\"set ((("),
        0,
        "operator characters stay literal"
    );
    assert_eq!(total(global, "..."), 0, "punctuation is searched literally");
    assert_eq!(
        total(picto_library::predicate::TextField::Name, "ewgroundsdum"),
        1,
        "column-scoped substring"
    );
    assert_eq!(
        total(picto_library::predicate::TextField::Notes, "ewgroundsdum"),
        0,
        "column scope excludes other columns"
    );
}

#[test]
fn text_search_stays_fresh_across_repeated_queries_and_mutations() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    library
        .ingest_batch(&[
            imported("dragon-castle", Lifecycle::Active, &[]),
            imported("sunset-photo", Lifecycle::Active, &[]),
        ])
        .unwrap();
    assert!(library.settle_fts(64).unwrap().is_some());
    let query = RootQuery {
        scope: ItemScope::All,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Text {
                // Name-scoped: the fixture's source url and source text also
                // mention the key, and those survive a rename.
                field: picto_library::predicate::TextField::Name,
                query: "dragon".into(),
            }),
            sort: ItemSort::default(),
        },
    };

    let first = library.query(&query, &PageRequest::default()).unwrap();
    assert_eq!(first.total, 1);
    let repeated = library.query(&query, &PageRequest::default()).unwrap();
    assert_eq!(repeated.items, first.items, "repeated query is identical");

    let dragon = first.items[0].root_id;
    library.rename_root(dragon, "phoenix-castle", 42).unwrap();
    assert!(library.settle_fts(64).unwrap().is_some());
    assert_eq!(
        library.query(&query, &PageRequest::default()).unwrap().total,
        0,
        "a rename away from the term drops the match"
    );

    let sunset = library
        .query(
            &RootQuery {
                scope: ItemScope::All,
                view: ViewQuerySpec::default(),
            },
            &PageRequest::default(),
        )
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.root_id != dragon)
        .unwrap()
        .root_id;
    library.rename_root(sunset, "dragon-sea", 43).unwrap();
    assert!(library.settle_fts(64).unwrap().is_some());
    let after = library.query(&query, &PageRequest::default()).unwrap();
    assert_eq!(after.total, 1, "a rename into the term appears");
    assert_eq!(after.items[0].root_id, sunset);
}

#[test]
fn fts_settlement_interleaves_dirty_categories() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let inputs = (0..4)
        .map(|index| imported(&format!("fts-{index}"), Lifecycle::Active, &[]))
        .collect::<Vec<_>>();
    let roots = library
        .ingest_batch(&inputs)
        .unwrap()
        .into_iter()
        .map(|(root, _)| root)
        .collect::<Vec<_>>();
    library
        .database()
        .maintenance_write(picto_library::database::WorkPriority::Fts, |transaction| {
            transaction.execute("DELETE FROM fts_dirty", [])?;
            for root in &roots[..3] {
                transaction.execute(
                    "INSERT INTO fts_dirty(root_id, category, queued_at_ms)
                         VALUES (?1, 1, 100)",
                    [root.0],
                )?;
            }
            transaction.execute(
                "INSERT INTO fts_dirty(root_id, category, queued_at_ms)
                     VALUES (?1, 2, 100)",
                [roots[3].0],
            )?;
            Ok(())
        })
        .unwrap();

    library.settle_fts(2).unwrap().unwrap();
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row(
                        "SELECT COUNT(*) FROM fts_dirty WHERE category = 2",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?,
                    0
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM fts_dirty", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    2
                );
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn empty_fts_poll_does_not_advance_the_library_revision() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let before = library.database().revision().unwrap();

    assert!(library.settle_fts(64).unwrap().is_none());
    assert_eq!(library.database().revision().unwrap(), before);
    assert_eq!(library.projections().snapshot().revision, before);
}

#[test]
fn name_text_filter_uses_the_root_fts_column_in_every_lifecycle_scope() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut input = imported("name-search", Lifecycle::Inbox, &[]);
    input.media_name = "Original Capture Fox".into();
    let (root_id, _) = library.ingest(&input).unwrap();
    assert!(library.settle_fts(64).unwrap().is_some());

    let name_query = RootQuery {
        scope: ItemScope::Inbox,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Text {
                field: picto_library::predicate::TextField::Name,
                query: "Capture".into(),
            }),
            sort: ItemSort::default(),
        },
    };
    let page = library.query(&name_query, &PageRequest::default()).unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![root_id]
    );
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        0
    );
}

#[test]
fn collections_are_one_root_and_mime_filters_include_every_member() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (image, _) = library
        .ingest(&imported_as(
            "image",
            "image/png",
            LabColor {
                l: 50.0,
                a: 60.0,
                b: 40.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (video, _) = library
        .ingest(&imported_as(
            "video",
            "video/mp4",
            LabColor {
                l: 30.0,
                a: 10.0,
                b: -70.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![image, video],
            },
            cover_root_id: image,
            winning_collection_id: None,
            name: Some("Pair".into()),
            notes: None,
            modified_at_ms: 1_700_000_000_100,
        })
        .unwrap();
    let all = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(all.total, 1);
    assert_eq!(all.items[0].root_id, collection);
    assert_eq!(all.items[0].kind, RootKind::Collection);
    assert_eq!(all.items[0].media_count, 2);
    assert_eq!(all.items[0].mime, "image/png");

    let mime_query = |mime: &str| RootQuery {
        scope: ItemScope::All,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Mime {
                values: vec![mime.into()],
                families: Vec::new(),
            }),
            sort: ItemSort::default(),
        },
    };
    assert_eq!(
        library
            .query(&mime_query("image/png"), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        library
            .query(&mime_query("video/mp4"), &PageRequest::default())
            .unwrap()
            .total,
        1
    );

    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| row
                        .get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| row
                        .get::<_, i64>(0))?,
                    2
                );
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection].len(),
        2
    );

    library
        .reorder_collection(
            collection,
            vec![
                picto_library::MediaId(video.0),
                picto_library::MediaId(image.0),
            ],
            1_700_000_000_150,
        )
        .unwrap();
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection]
            .iter()
            .map(|media| media.0)
            .collect::<Vec<_>>(),
        vec![video.0, image.0]
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection]
            .iter()
            .map(|media| media.0)
            .collect::<Vec<_>>(),
        vec![image.0, video.0]
    );
    library.redo().unwrap().unwrap();
    library.undo().unwrap().unwrap();

    library
        .set_collection_cover(
            collection,
            picto_library::MediaId(video.0),
            1_700_000_000_175,
        )
        .unwrap();
    assert_eq!(
        library
            .query(&mime_query("video/mp4"), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        library
            .query(&mime_query("image/png"), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library
            .query(&mime_query("image/png"), &PageRequest::default())
            .unwrap()
            .total,
        1
    );

    let (roots, _) = library
        .ungroup_collection(collection, 1_700_000_000_200)
        .unwrap();
    assert_eq!(roots, vec![image, video]);
    let all = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(all.total, 2);
    assert_eq!(
        all.items
            .iter()
            .map(|item| (item.root_id, item.mime.as_str()))
            .collect::<Vec<_>>(),
        vec![(video, "video/mp4"), (image, "image/png")]
    );
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| row
                        .get::<_, i64>(0))?,
                    2
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| row
                        .get::<_, i64>(0))?,
                    2
                );
                Ok(())
            },
        )
        .unwrap();
    library.undo().unwrap().unwrap();
    let regrouped = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(regrouped.total, 1);
    assert_eq!(regrouped.items[0].root_id, collection);
    assert_eq!(regrouped.items[0].media_count, 2);
    library.redo().unwrap().unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        2
    );
}

#[test]
fn details_keep_collection_organization_on_the_root_and_media_in_vector_order() {
    let directory = tempfile::tempdir().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported_as(
            "details-first",
            "image/png",
            LabColor {
                l: 40.0,
                a: 10.0,
                b: 5.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (cover, _) = library
        .ingest(&imported_as(
            "details-cover",
            "video/mp4",
            LabColor {
                l: 60.0,
                a: -5.0,
                b: 15.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    library
        .set_rating(
            &SelectionTarget::Explicit {
                root_ids: vec![cover],
            },
            Rating::Four,
        )
        .unwrap();
    library
        .add_tag(
            &SelectionTarget::Explicit {
                root_ids: vec![first],
            },
            "character:alice",
        )
        .unwrap();
    let (folder_id, _) = library.create_folder("Details", None).unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![cover],
            },
            folder_id,
        )
        .unwrap();

    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![first, cover],
            },
            cover_root_id: cover,
            winning_collection_id: None,
            name: Some("Details collection".into()),
            notes: None,
            modified_at_ms: 500,
        })
        .unwrap();
    let details = library.details(collection).unwrap();

    assert_eq!(details.root.kind, picto_library::RootKind::Collection);
    assert_eq!(details.root.name, "Details collection");
    assert_eq!(details.root.cover_media_id, picto_library::MediaId(cover.0));
    assert_eq!(details.lifecycle, Lifecycle::Active);
    assert_eq!(details.rating, Rating::Four);
    assert_eq!(details.folder_ids, vec![folder_id]);
    assert_eq!(details.tag_ids.len(), 1);
    assert_eq!(
        details
            .media
            .iter()
            .map(|media| media.media_id.0)
            .collect::<Vec<_>>(),
        vec![first.0, cover.0]
    );
    assert_eq!(details.media[0].media_name, "details-first.png");
    assert_eq!(details.media[1].media_name, "details-cover.png");
}

#[test]
fn derivative_updates_refresh_only_roots_using_the_changed_file_as_cover() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (cover, _) = library
        .ingest(&imported_as(
            "derivative-cover",
            "image/png",
            LabColor {
                l: 50.0,
                a: 10.0,
                b: 10.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (member, _) = library
        .ingest(&imported_as(
            "derivative-member",
            "video/mp4",
            LabColor {
                l: 20.0,
                a: -10.0,
                b: -10.0,
                weight: 1.0,
            },
        ))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![cover, member],
            },
            cover_root_id: cover,
            winning_collection_id: None,
            name: None,
            notes: None,
            modified_at_ms: 1_700_000_001_000,
        })
        .unwrap();
    let history_before_derivative = library.history().state().entries;
    let mime_query = |mime: &str| RootQuery {
        scope: ItemScope::All,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Mime {
                values: vec![mime.into()],
                families: Vec::new(),
            }),
            sort: ItemSort::default(),
        },
    };

    library
        .update_media_facts(
            picto_library::MediaId(member.0),
            &picto_library::MediaFactsUpdate {
                mime: Some("audio/mpeg".into()),
                ..Default::default()
            },
            1_700_000_001_100,
        )
        .unwrap();
    assert_eq!(
        library
            .query(&mime_query("image/png"), &PageRequest::default())
            .unwrap()
            .items[0]
            .root_id,
        collection
    );
    assert_eq!(
        library
            .query(&mime_query("audio/mpeg"), &PageRequest::default())
            .unwrap()
            .total,
        1
    );

    library
        .update_media_facts(
            picto_library::MediaId(cover.0),
            &picto_library::MediaFactsUpdate {
                mime: Some("application/pdf".into()),
                width: Some(None),
                height: Some(None),
                ..Default::default()
            },
            1_700_000_001_200,
        )
        .unwrap();
    assert_eq!(
        library
            .query(&mime_query("application/pdf"), &PageRequest::default())
            .unwrap()
            .items[0]
            .root_id,
        collection
    );
    assert_eq!(library.history().state().entries, history_before_derivative);
}

#[test]
fn detaching_restores_one_root_and_keeps_collection_organization_on_the_root() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut image_import = imported_as(
        "detach-image",
        "image/png",
        LabColor {
            l: 50.0,
            a: 10.0,
            b: 10.0,
            weight: 1.0,
        },
    );
    image_import.tags = vec!["creator:image".into()];
    let mut video_import = imported_as(
        "detach-video",
        "video/mp4",
        LabColor {
            l: 20.0,
            a: -10.0,
            b: -10.0,
            weight: 1.0,
        },
    );
    video_import.tags = vec!["creator:video".into()];
    let (image, _) = library.ingest(&image_import).unwrap();
    let (video, _) = library.ingest(&video_import).unwrap();
    let (folder, _) = library.create_folder("Detach", None).unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![image, video],
            },
            folder,
        )
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![image, video],
            },
            cover_root_id: image,
            winning_collection_id: None,
            name: Some("Detach pair".into()),
            notes: None,
            modified_at_ms: 1_700_000_002_000,
        })
        .unwrap();

    let (detached, _) = library
        .detach_collection_member(
            collection,
            picto_library::MediaId(image.0),
            1_700_000_002_100,
        )
        .unwrap();
    assert_eq!(detached, image);
    let all = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(all.total, 2);
    assert_eq!(root_tag_count(&library, collection), 2);
    assert_eq!(root_tag_count(&library, detached), 2);
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection]
            .iter()
            .map(|media| media.0)
            .collect::<Vec<_>>(),
        vec![video.0]
    );
    let folder_page = library
        .query(
            &RootQuery {
                scope: ItemScope::Folder { folder_id: folder },
                view: ViewQuerySpec {
                    filter: FilterExpr::default(),
                    sort: ItemSort {
                        field: SortField::FolderOrder,
                        direction: SortDirection::Ascending,
                        random_seed: None,
                    },
                },
            },
            &PageRequest::default(),
        )
        .unwrap();
    assert_eq!(
        folder_page
            .items
            .iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![collection, detached]
    );
    assert_eq!(
        folder_page
            .items
            .iter()
            .map(|item| item.mime.as_str())
            .collect::<Vec<_>>(),
        vec!["video/mp4", "image/png"]
    );
    library.undo().unwrap().unwrap();
    let regrouped = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(regrouped.total, 1);
    assert_eq!(regrouped.items[0].root_id, collection);
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection]
            .iter()
            .map(|media| media.0)
            .collect::<Vec<_>>(),
        vec![image.0, video.0]
    );
    library.redo().unwrap().unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        2
    );
}

#[test]
fn detaching_many_members_is_one_exact_reversible_publication() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported(
            "detach-many-first",
            Lifecycle::Active,
            &["set:one"],
        ))
        .unwrap();
    let (second, _) = library
        .ingest(&imported(
            "detach-many-second",
            Lifecycle::Active,
            &["set:two"],
        ))
        .unwrap();
    let (third, _) = library
        .ingest(&imported(
            "detach-many-third",
            Lifecycle::Active,
            &["set:three"],
        ))
        .unwrap();
    let (folder, _) = library.create_folder("Detach many", None).unwrap();
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![first, second, third],
            },
            folder,
        )
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![first, second, third],
            },
            cover_root_id: first,
            winning_collection_id: None,
            name: Some("Detach many".into()),
            notes: None,
            modified_at_ms: 1_700_000_002_200,
        })
        .unwrap();
    let revision_before = library.projections().snapshot().revision;
    let history_before = library.history().state().entries;

    let (detached, receipt) = library
        .detach_collection_members(
            collection,
            vec![
                picto_library::MediaId(first.0),
                picto_library::MediaId(third.0),
            ],
            Some(Lifecycle::Trash),
            1_700_000_002_300,
        )
        .unwrap();

    assert_eq!(detached, vec![first, third]);
    assert_eq!(receipt.revision, revision_before + 1);
    assert_eq!(library.history().state().entries, history_before + 1);
    assert_eq!(
        library.history().state().undo.unwrap().command,
        "collections.detach"
    );
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection]
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![picto_library::MediaId(second.0)]
    );
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        library
            .query(&query(ItemScope::Trash), &PageRequest::default())
            .unwrap()
            .items
            .iter()
            .map(|item| item.root_id)
            .collect::<std::collections::BTreeSet<_>>(),
        [first, third].into_iter().collect()
    );
    assert_eq!(
        library.projections().snapshot().folder_orders[&folder]
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![collection, first, third]
    );

    library.undo().unwrap().unwrap();
    assert_eq!(
        library.projections().snapshot().collection_orders[&collection]
            .iter()
            .map(|media| media.0)
            .collect::<Vec<_>>(),
        vec![first.0, second.0, third.0]
    );
    assert_eq!(
        library
            .query(&query(ItemScope::Trash), &PageRequest::default())
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        library.projections().snapshot().folder_orders[&folder]
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![collection]
    );
}

#[test]
fn organizing_a_collection_is_reversible_without_persisted_history() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest(&imported(
            "undo-group-one",
            Lifecycle::Active,
            &["creator:one"],
        ))
        .unwrap();
    let (second, _) = library
        .ingest(&imported(
            "undo-group-two",
            Lifecycle::Active,
            &["creator:two"],
        ))
        .unwrap();
    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![first, second],
            },
            cover_root_id: first,
            winning_collection_id: None,
            name: Some("Undo group".into()),
            notes: None,
            modified_at_ms: 1_700_000_003_000,
        })
        .unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::All), &PageRequest::default())
            .unwrap()
            .total,
        1
    );

    library.undo().unwrap().unwrap();
    let roots = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(roots.total, 2);
    assert!(roots.items.iter().all(|item| item.kind == RootKind::Media));

    library.redo().unwrap().unwrap();
    let regrouped = library
        .query(&query(ItemScope::All), &PageRequest::default())
        .unwrap();
    assert_eq!(regrouped.total, 1);
    assert_eq!(regrouped.items[0].root_id, collection);
    assert_eq!(regrouped.items[0].kind, RootKind::Collection);
}

#[test]
fn collection_members_retain_notes_through_group_cover_undo_and_ungroup() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut first_import = imported("noted-one", Lifecycle::Active, &[]);
    first_import.notes = Some("First member note".into());
    let mut second_import = imported("noted-two", Lifecycle::Active, &[]);
    second_import.notes = Some("Second member note".into());
    let converted = library
        .ingest_conversion_batch(&[first_import.clone(), second_import.clone()])
        .unwrap();
    let first = converted[0].0;
    let second = converted[1].0;
    let target = SelectionTarget::Explicit {
        root_ids: vec![first, second],
    };

    let draft = library.collection_note_draft(&target).unwrap();
    assert_eq!(draft.notes, "First member note\n\nSecond member note");
    assert_eq!(draft.source_count, 2);

    let (collection, _) = library
        .organize_into_collection(&GroupRequest {
            target,
            cover_root_id: first,
            winning_collection_id: None,
            name: Some("Noted collection".into()),
            notes: Some(draft.notes.clone()),
            modified_at_ms: 1_700_000_004_000,
        })
        .unwrap();
    let grouped = library.details(collection).unwrap();
    assert_eq!(grouped.root.notes.as_deref(), Some(draft.notes.as_str()));
    assert_eq!(
        grouped.media[0].media_notes.as_deref(),
        Some("First member note")
    );
    assert_eq!(
        grouped.media[1].media_notes.as_deref(),
        Some("Second member note")
    );

    library
        .set_collection_cover(
            collection,
            picto_library::MediaId(second.0),
            1_700_000_004_100,
        )
        .unwrap();
    assert_eq!(
        library.details(collection).unwrap().root.notes.as_deref(),
        Some("Second member note")
    );
    library.undo().unwrap().unwrap();
    let restored_cover = library.details(collection).unwrap();
    assert_eq!(restored_cover.root.cover_media_id.0, first.0);
    assert_eq!(
        restored_cover.root.notes.as_deref(),
        Some(draft.notes.as_str())
    );

    library
        .ungroup_collection(collection, 1_700_000_004_200)
        .unwrap();
    assert_eq!(
        library.details(first).unwrap().root.notes.as_deref(),
        Some("First member note")
    );
    assert_eq!(
        library.details(second).unwrap().root.notes.as_deref(),
        Some("Second member note")
    );
}

#[test]
fn recent_views_are_active_only_timestamp_ordered_and_clear_is_session_undoable() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (older, _) = library
        .ingest(&imported("recent-older", Lifecycle::Active, &[]))
        .unwrap();
    let (newer, _) = library
        .ingest(&imported("recent-newer", Lifecycle::Active, &[]))
        .unwrap();
    let (inbox, _) = library
        .ingest(&imported("recent-inbox", Lifecycle::Inbox, &[]))
        .unwrap();
    library.record_recent_view(older, 100).unwrap();
    library.record_recent_view(inbox, 300).unwrap();
    library.record_recent_view(newer, 200).unwrap();

    let recent = library
        .query(
            &RootQuery {
                scope: ItemScope::RecentlyViewed,
                view: ViewQuerySpec {
                    filter: FilterExpr::default(),
                    sort: ItemSort {
                        field: SortField::FolderOrder,
                        direction: SortDirection::Ascending,
                        random_seed: None,
                    },
                },
            },
            &PageRequest::default(),
        )
        .unwrap();
    assert_eq!(
        recent
            .items
            .iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>(),
        vec![newer, older]
    );

    library.clear_recent_views().unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::RecentlyViewed), &PageRequest::default())
            .unwrap()
            .total,
        0
    );
    library.undo().unwrap().unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::RecentlyViewed), &PageRequest::default())
            .unwrap()
            .total,
        2
    );
    library.redo().unwrap().unwrap();
    assert_eq!(
        library
            .query(&query(ItemScope::RecentlyViewed), &PageRequest::default())
            .unwrap()
            .total,
        0
    );

    drop(library);
    let reopened = Library::open(&path).unwrap();
    assert_eq!(reopened.history().state().entries, 0);
}

#[test]
fn permanent_delete_is_non_undoable_and_only_queues_unreferenced_blobs() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let shared_hash = "shared-content";
    let mut first_import = imported("delete-first", Lifecycle::Active, &["creator:one"]);
    first_import.facts.content_hash = shared_hash.into();
    let mut second_import = imported("delete-second", Lifecycle::Active, &["creator:two"]);
    second_import.facts.content_hash = shared_hash.into();
    let converted = library
        .ingest_conversion_batch(&[first_import.clone(), second_import.clone()])
        .unwrap();
    let first = converted[0].0;
    let second = converted[1].0;

    let first_target = SelectionTarget::Explicit {
        root_ids: vec![first],
    };
    library
        .set_lifecycle(&first_target, Lifecycle::Trash)
        .unwrap();
    let history_before_delete = library.history().state().entries;
    let (_, cleanup) = library
        .permanently_delete(&first_target, 1_700_000_000_500)
        .unwrap();
    assert!(cleanup.is_empty());
    assert_eq!(library.history().state().entries, history_before_delete);

    let second_target = SelectionTarget::Explicit {
        root_ids: vec![second],
    };
    library
        .set_lifecycle(&second_target, Lifecycle::Trash)
        .unwrap();
    let history_before_delete = library.history().state().entries;
    let (_, cleanup) = library
        .permanently_delete(&second_target, 1_700_000_000_600)
        .unwrap();
    assert_eq!(cleanup.len(), 1);
    assert_eq!(cleanup[0].file_path, second_import.file_path);
    assert_eq!(library.history().state().entries, history_before_delete);
    assert_eq!(library.counts().unwrap().trash, 0);

    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    0
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    0
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM deletion_tombstone", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    2
                );
                assert_eq!(
                    connection.query_row("SELECT COUNT(*) FROM blob_cleanup_queue", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    1
                );
                Ok(())
            },
        )
        .unwrap();

    drop(library);
    let reopened = Library::open(&path).unwrap();
    assert_eq!(reopened.counts().unwrap().trash, 0);
    assert_eq!(reopened.history().state().entries, 0);
    let mut removed = Vec::new();
    assert_eq!(
        reopened
            .clean_pending_blobs(10, |pending| {
                removed.push(pending.content_hash.clone());
                Ok(())
            })
            .unwrap(),
        1
    );
    assert_eq!(removed, vec![shared_hash]);
    assert!(reopened.pending_blob_cleanup(10).unwrap().is_empty());
}
