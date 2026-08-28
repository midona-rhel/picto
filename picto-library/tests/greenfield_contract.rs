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
