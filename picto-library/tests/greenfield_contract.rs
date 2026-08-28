use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, SortDirection, SortField, ViewQuerySpec,
};
use picto_library::query::{ItemScope, PageRequest, RootQuery};
use picto_library::selection::SelectionTarget;
use picto_library::{
    GroupRequest, ImmutableMediaFacts, LabColor, Library, Lifecycle, PreparedImport, Rating,
    RootKind,
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
        tags: tags.iter().map(|value| (*value).to_owned()).collect(),
        folders: Vec::new(),
        source_urls: vec![format!("https://example.test/{key}")],
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

#[test]
fn fresh_schema_reopens_and_rejects_unrelated_sqlite_files() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    drop(Library::create(&path).unwrap());
    Library::open(&path).unwrap();

    let unrelated = directory.path().join("unrelated.sqlite");
    rusqlite::Connection::open(&unrelated).unwrap();
    assert!(Library::open(&unrelated).is_err());
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
    let counts = library.counts().unwrap();
    assert_eq!(counts.all, 1);
    assert_eq!(counts.inbox, 1);
    assert_eq!(counts.tags.values().copied().sum::<u64>(), 1);

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
    let (smart_folder, _) = library
        .create_smart_folder(
            "Source",
            None,
            ViewQuerySpec {
                filter: FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: vec![source],
                    mode: SetMatchMode::All,
                }),
                sort: ItemSort::default(),
            },
        )
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

fn root_tag_count(library: &Library, root_id: picto_library::RootId) -> usize {
    library
        .projections()
        .snapshot()
        .tags
        .values()
        .filter(|roots| roots.contains(root_id.0))
        .count()
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
        .create_smart_folder("Alice", None, view.clone())
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
fn bounded_ingest_batch_publishes_once_and_fts_respects_each_scope() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let before = library.database().revision().unwrap();
    let values = vec![
        imported("searchable-active", Lifecycle::Active, &[]),
        imported("searchable-inbox", Lifecycle::Inbox, &[]),
    ];
    let outputs = library.ingest_batch(&values).unwrap();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].1.revision, outputs[1].1.revision);
    assert_eq!(outputs[0].1.revision, before + 1);

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
fn collections_are_one_root_and_media_filters_use_only_the_cover() {
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
        0
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
        0
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
            modified_at_ms: 1_700_000_001_000,
        })
        .unwrap();
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
        0
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
    assert_eq!(library.history().state().entries, 0);
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
    let (first, _) = library.ingest(&first_import).unwrap();
    let (second, _) = library.ingest(&second_import).unwrap();

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
    assert_eq!(cleanup[0].file_path, first_import.file_path);
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
}
