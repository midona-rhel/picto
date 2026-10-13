//! One file is one media item: imports never copy a file the library holds,
//! and libraries from before the rule merge their copies once.

use picto_library::predicate::{FilterClause, FilterExpr, ItemSort, ViewQuerySpec};
use picto_library::selection::SelectionTarget;
use picto_library::{
    GroupRequest, ImmutableMediaFacts, LabColor, Library, Lifecycle, MediaId,
    PreparedCollectionImport, PreparedImport, Rating, RootId, RootKind, SharedFileMerge,
    SmartFolderInput,
};
use tempfile::TempDir;

fn imported(key: &str, hash: &str) -> PreparedImport {
    PreparedImport {
        stable_key: key.into(),
        media_name: format!("{key}.png"),
        file_path: format!("/tmp/{key}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 1_024,
            width: Some(100),
            height: Some(100),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: hash.into(),
            perceptual_hash: None,
            palette: vec![LabColor {
                l: 50.0,
                a: 0.0,
                b: 0.0,
                weight: 1.0,
            }],
        },
        lifecycle: Lifecycle::Inbox,
        rating: Rating::Unrated,
        notes: None,
        tags: Vec::new(),
        folders: Vec::new(),
        source_urls: Vec::new(),
        source_identity: Some(picto_library::SourceIdentity {
            source_key: "fixture".into(),
            source_item_key: key.into(),
            source_text: None,
            source_attempt_id: None,
        }),
        imported_at_ms: 1_700_000_000_000,
        captured_at_ms: None,
    }
}

fn collection(members: Vec<PreparedImport>) -> PreparedCollectionImport {
    PreparedCollectionImport {
        members,
        cover_index: 0,
        existing_root_id: None,
        name: Some("Post".into()),
        modified_at_ms: 1_700_000_000_000,
    }
}

fn scalar(library: &Library, sql: &str) -> i64 {
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| Ok(connection.query_row(sql, [], |row| row.get(0))?),
        )
        .unwrap()
}

/// Files held by more than one item; the rule keeps this at zero.
fn shared_files(library: &Library) -> i64 {
    scalar(
        library,
        "SELECT COUNT(*) FROM (SELECT file_id FROM media_item GROUP BY file_id HAVING COUNT(*) > 1)",
    )
}

fn members(library: &Library, root: RootId) -> Vec<MediaId> {
    library.projections().snapshot().collection_orders[&root]
        .as_ref()
        .clone()
}

#[test]
fn a_new_library_refuses_a_second_item_for_a_file() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library.ingest(&imported("first", "first-file")).unwrap();
    let (second, _) = library.ingest(&imported("second", "second-file")).unwrap();
    let error = library
        .database()
        .maintenance_write(picto_library::database::WorkPriority::Cloud, |transaction| {
            transaction.execute(
                "UPDATE media_item SET file_id = (SELECT file_id FROM media_item WHERE media_id = ?1)
                 WHERE media_id = ?2",
                [first.0, second.0],
            )?;
            Ok(())
        });
    assert!(error.is_err());
    assert_eq!(
        library.merge_shared_files().unwrap(),
        SharedFileMerge::default()
    );
}

#[test]
fn a_collection_import_takes_in_a_loose_copy_of_its_file() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let mut loose = imported("loose", "shared-file");
    loose.lifecycle = Lifecycle::Active;
    loose.tags = vec!["loose:tag".into()];
    let (loose_root, _) = library.ingest(&loose).unwrap();

    let (post, _) = library
        .ingest_collection(&collection(vec![
            imported("post-shared", "shared-file"),
            imported("post-new", "new-file"),
        ]))
        .unwrap();

    assert_eq!(shared_files(&library), 0);
    let snapshot = library.projections().snapshot();
    assert!(snapshot.root_kinds[&RootKind::Collection].contains(post.0));
    assert_eq!(members(&library, post)[0], MediaId(loose_root.0));
    assert_eq!(members(&library, post).len(), 2);
    assert_eq!(snapshot.media_owner.get(loose_root.0), Some(&post));
    // The set keeps the lifecycle that keeps it most, and the loose tags.
    let details = library.details(post).unwrap();
    assert_eq!(details.lifecycle, Lifecycle::Active);
    assert!(details
        .tag_ids
        .contains(&snapshot.tag_ids_by_name["loose:tag"]));
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), 1);
}

#[test]
fn a_post_whose_files_another_collection_holds_creates_nothing() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest_collection(&collection(vec![
            imported("first-a", "file-a"),
            imported("first-b", "file-b"),
        ]))
        .unwrap();
    let roots_before = scalar(&library, "SELECT COUNT(*) FROM library_root");

    let (second, _) = library
        .ingest_collection(&collection(vec![
            imported("second-a", "file-a"),
            imported("second-b", "file-b"),
        ]))
        .unwrap();

    assert_eq!(second, first);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), roots_before);
    assert_eq!(members(&library, first).len(), 2);
    assert_eq!(shared_files(&library), 0);
    // The second post's sources point at the items that hold its files.
    assert_eq!(
        scalar(
            &library,
            "SELECT COUNT(*) FROM source_provenance WHERE source_item_key LIKE 'second-%'"
        ),
        2
    );
}

#[test]
fn a_post_sharing_one_file_combines_both_complete_sets() {
    let directory = TempDir::new().unwrap();
    let library = Library::create(directory.path().join("library.sqlite")).unwrap();
    let (first, _) = library
        .ingest_collection(&collection(vec![
            imported("first-a", "file-a"),
            imported("first-b", "file-b"),
        ]))
        .unwrap();
    let (second, _) = library
        .ingest_collection(&collection(vec![
            imported("second-a", "file-a"),
            imported("second-c", "file-c"),
            imported("second-d", "file-d"),
        ]))
        .unwrap();
    assert_eq!(second, first);
    assert_eq!(members(&library, first).len(), 4);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), 1);
    assert_eq!(shared_files(&library), 0);
}

fn smart_tag(name: &str, tag_id: picto_library::TagId) -> SmartFolderInput {
    SmartFolderInput {
        name: name.into(),
        parent_id: None,
        icon: None,
        color: None,
        notes: None,
        view: ViewQuerySpec {
            filter: FilterExpr::Clause(FilterClause::Tags {
                tag_ids: vec![tag_id],
                mode: picto_library::predicate::SetMatchMode::Any,
            }),
            sort: ItemSort::default(),
        },
    }
}

/// Point `copies` at `original`'s file, as libraries from before the rule
/// could. Runs on the closed library's file.
fn share_file(connection: &rusqlite::Connection, original: MediaId, copies: &[MediaId]) {
    for copy in copies {
        connection
            .execute(
                "UPDATE media_item SET file_id = (SELECT file_id FROM media_item WHERE media_id = ?1)
                 WHERE media_id = ?2",
                [original.0, copy.0],
            )
            .unwrap();
    }
}

#[test]
fn an_older_library_merges_every_shared_file_once_and_keeps_its_counts() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (folder, _) = library.create_folder("Folder", None).unwrap();

    // File A: three loose copies with different metadata.
    let mut a1 = imported("a1", "a1");
    a1.lifecycle = Lifecycle::Trash;
    a1.tags = vec!["a:one".into()];
    a1.rating = Rating::Two;
    a1.notes = Some("First note".into());
    a1.imported_at_ms = 1_000;
    let mut a2 = imported("a2", "a2");
    a2.lifecycle = Lifecycle::Active;
    a2.tags = vec!["a:two".into()];
    a2.rating = Rating::Five;
    a2.notes = Some("Second note".into());
    a2.folders = vec![folder];
    a2.source_urls = vec!["https://example.test/a2".into()];
    a2.imported_at_ms = 2_000;
    let mut a3 = imported("a3", "a3");
    a3.lifecycle = Lifecycle::Inbox;
    a3.imported_at_ms = 3_000;
    let roots = library.ingest_batch(&[a1, a2.clone(), a3]).unwrap();
    let (a1, a2_root, a3) = (roots[0].0, roots[1].0, roots[2].0);

    // File B: loose, and a member of a collection.
    let (b_loose, _) = library
        .ingest(&{
            let mut input = imported("b-loose", "b-loose");
            input.tags = vec!["b:loose".into()];
            input
        })
        .unwrap();
    let (b_collection, _) = library
        .ingest_collection(&collection(vec![
            imported("b-member", "b-member"),
            imported("b-other", "b-other"),
        ]))
        .unwrap();
    let b_member = members(&library, b_collection)[0];

    // File C: in an older collection, and the only item of a newer one.
    let (c_old, _) = library
        .ingest_collection(&PreparedCollectionImport {
            modified_at_ms: 1_000,
            ..collection(vec![
                {
                    let mut input = imported("c-old", "c-old");
                    input.imported_at_ms = 1_000;
                    input
                },
                {
                    let mut input = imported("c-old-other", "c-old-other");
                    input.imported_at_ms = 1_000;
                    input
                },
            ])
        })
        .unwrap();
    let c_kept = members(&library, c_old)[0];
    let (c_lone, _) = library.ingest(&imported("c-lone", "c-lone")).unwrap();
    let (c_spare, _) = library.ingest(&imported("c-spare", "c-spare")).unwrap();
    let (c_new, _) = library
        .organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit {
                root_ids: vec![c_lone, c_spare],
            },
            cover_root_id: c_lone,
            winning_collection_id: None,
            name: Some("Newer".into()),
            notes: None,
            modified_at_ms: 1_700_000_000_000,
        })
        .unwrap();
    library
        .detach_collection_member(c_new, MediaId(c_spare.0), 1_700_000_000_001)
        .unwrap();
    library
        .database()
        .maintenance_write(picto_library::database::WorkPriority::Cloud, |transaction| {
            transaction.execute(
                "INSERT INTO source_post (site_id, post_key, root_item_id, created_at, updated_at)
                 VALUES ('fixture', 'newer', ?1, 'now', 'now')",
                [c_new.0],
            )?;
            Ok(())
        })
        .unwrap();

    let a_two = library.projections().snapshot().tag_ids_by_name["a:two"];
    let (smart, _) = library.create_smart_folder(smart_tag("A two", a_two)).unwrap();

    drop(library);
    // Reopen as the schema before aliases, with copies an older library
    // could hold.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP INDEX idx_media_item_one_per_file;
             DROP TABLE library_item_alias;
             UPDATE library_meta
             SET schema_fingerprint = 'picto-library-schema-1-2026-10-10-no-cloud-clocks';",
        )
        .unwrap();
    share_file(&connection, MediaId(a1.0), &[MediaId(a2_root.0), MediaId(a3.0)]);
    share_file(&connection, b_member, &[MediaId(b_loose.0)]);
    share_file(&connection, c_kept, &[MediaId(c_lone.0)]);
    drop(connection);
    let library = Library::open(&path).unwrap();
    assert_eq!(shared_files(&library), 3);

    let report = library.merge_shared_files().unwrap();
    assert_eq!(
        report,
        SharedFileMerge {
            files: 3,
            merged_items: 4,
            removed_collections: 1,
        }
    );
    assert_eq!(shared_files(&library), 0);
    assert_eq!(library.history().state().entries, 0);

    // A: the oldest loose copy stays and gathers the rest.
    let kept = library.details(a1).unwrap();
    assert!(library.details(a2_root).is_err());
    assert!(library.details(a3).is_err());
    let snapshot = library.projections().snapshot();
    let tag = |name: &str| snapshot.tag_ids_by_name[name];
    assert!(kept.tag_ids.contains(&tag("a:one")));
    assert!(kept.tag_ids.contains(&tag("a:two")));
    assert_eq!(kept.rating, Rating::Five);
    assert_eq!(kept.lifecycle, Lifecycle::Active);
    assert_eq!(kept.root.notes.as_deref(), Some("First note\n\nSecond note"));
    assert_eq!(kept.root.source_urls, vec!["https://example.test/a2".to_string()]);
    assert_eq!(kept.folder_ids, vec![folder]);
    assert_eq!(snapshot.folder_orders[&folder].as_ref(), &vec![a1]);
    assert_eq!(
        snapshot.smart_results[&smart.0].iter().collect::<Vec<_>>(),
        vec![a1.0]
    );
    assert_eq!(snapshot.tags[&tag("a:two")].len(), 1);

    // B: the collection keeps its member and takes the loose copy's tags.
    assert!(library.details(b_loose).is_err());
    assert_eq!(members(&library, b_collection).len(), 2);
    assert!(library
        .details(b_collection)
        .unwrap()
        .tag_ids
        .contains(&tag("b:loose")));

    // C: the older collection keeps the file; the emptied one is gone and
    // its post points at the one that stays.
    assert!(library.details(c_new).is_err());
    assert_eq!(members(&library, c_old)[0], c_kept);
    assert_eq!(
        scalar(&library, "SELECT root_item_id FROM source_post WHERE post_key = 'newer'"),
        i64::from(c_old.0)
    );

    // Re-importing a merged-away item resolves to the one that stays.
    let (again, _) = library.ingest(&a2).unwrap();
    assert_eq!(again, a1);
    assert_eq!(shared_files(&library), 0);

    // Once is enough: the next open finds the rule enforced.
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(library.merge_shared_files().unwrap(), SharedFileMerge::default());
    assert_eq!(
        scalar(
            &library,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'idx_media_item_one_per_file'"
        ),
        1
    );
}

#[test]
fn a_post_bridging_two_collections_preserves_their_metadata_and_reopens() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (folder, _) = library.create_folder("Second set", None).unwrap();
    let mut a = imported("a", "a");
    a.tags = vec!["first".into()];
    a.notes = Some("First note".into());
    let (first, _) = library.ingest_collection(&collection(vec![a, imported("b", "b")])).unwrap();
    let mut c = imported("c", "c");
    c.tags = vec!["second".into()];
    c.rating = Rating::Five;
    c.lifecycle = Lifecycle::Active;
    c.notes = Some("Second note".into());
    c.folders = vec![folder];
    c.source_urls = vec!["https://example.test/second".into()];
    let mut d = imported("d", "d");
    d.lifecycle = Lifecycle::Active;
    let (second, _) = library.ingest_collection(&collection(vec![c, d])).unwrap();
    let second_key = library.database().read(picto_library::database::WorkPriority::VisibleRead,
        |db| Ok(db.query_row("SELECT stable_key FROM library_item WHERE local_id = ?1", [second.0], |row| row.get::<_, String>(0))?)).unwrap();
    library.database().maintenance_write(picto_library::database::WorkPriority::Cloud, |tx| {
        tx.execute("INSERT INTO source_post(site_id,post_key,root_item_id,created_at,updated_at) VALUES('fixture','bridge-source',?1,'now','now')", [second.0])?;
        Ok(())
    }).unwrap();
    let mut bridge = imported("bridge-a", "a");
    bridge.tags = vec!["bridge".into()];
    let (combined, receipt) = library.ingest_collection(&collection(vec![bridge, imported("bridge-c", "c")])).unwrap();
    assert_eq!(combined, first);
    assert!(receipt.item_ids.contains(&second));
    assert_eq!(members(&library, combined).len(), 4);
    assert_eq!(shared_files(&library), 0);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), 1);
    assert_eq!(scalar(&library, "SELECT root_item_id FROM source_post WHERE post_key='bridge-source'"), i64::from(first.0));
    drop(library);
    let library = Library::open(&path).unwrap();
    let details = library.details(combined).unwrap();
    let snapshot = library.projections().snapshot();
    for tag in ["first", "second", "bridge"] {
        assert!(details.tag_ids.contains(&snapshot.tag_ids_by_name[tag]));
    }
    assert_eq!(details.rating, Rating::Five);
    assert_eq!(details.lifecycle, Lifecycle::Active);
    assert_eq!(details.folder_ids, vec![folder]);
    assert_eq!(details.root.notes.as_deref(), Some("First note\n\nSecond note"));
    assert_eq!(details.root.source_urls, vec!["https://example.test/second"]);
    assert_eq!(members(&library, combined).len(), 4);
    let alias = library.database().read(picto_library::database::WorkPriority::VisibleRead,
        |db| Ok(db.query_row("SELECT local_id FROM library_item_alias WHERE stable_key = ?1", [&second_key], |row| row.get::<_, u32>(0))?)).unwrap();
    assert_eq!(alias, first.0);

    let target = SelectionTarget::Explicit { root_ids: vec![combined] };
    library.set_lifecycle(&target, Lifecycle::Trash).unwrap();
    library.history().clear();
    for _ in 0..2 {
        library.permanently_delete(&target, 2000, 500).unwrap();
        assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_item_alias"), 0);
        library.undo().unwrap().unwrap();
        let restored = library.database().read(picto_library::database::WorkPriority::VisibleRead,
            |db| Ok(db.query_row("SELECT local_id FROM library_item_alias WHERE stable_key = ?1", [&second_key], |row| row.get::<_, u32>(0))?)).unwrap();
        assert_eq!(restored, first.0);
        assert_eq!(members(&library, combined).len(), 4);
    }
}

#[test]
fn migration_combines_chained_overlaps_without_splitting_any_original_set() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let (a, _) = library.ingest_collection(&collection(vec![imported("a1", "a1"), imported("a2", "a2")])).unwrap();
    let (b, _) = library.ingest_collection(&collection(vec![imported("b1", "b1"), imported("b2", "b2")])).unwrap();
    let (c, _) = library.ingest_collection(&collection(vec![imported("c1", "c1"), imported("c2", "c2")])).unwrap();
    let (am, bm, cm) = (members(&library, a), members(&library, b), members(&library, c));
    drop(library);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP INDEX idx_media_item_one_per_file").unwrap();
    share_file(&db, am[0], &[bm[0]]);
    share_file(&db, bm[1], &[cm[0]]);
    drop(db);
    let library = Library::open(&path).unwrap();
    let report = library.merge_shared_files().unwrap();
    assert_eq!(report.files, 2);
    assert_eq!(report.merged_items, 2);
    assert_eq!(report.removed_collections, 2);
    assert_eq!(members(&library, a), vec![am[0], am[1], bm[1], cm[1]]);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), 1);
    assert_eq!(shared_files(&library), 0);
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(library.merge_shared_files().unwrap(), SharedFileMerge::default());
    assert_eq!(members(&library, a).len(), 4);
}

#[test]
fn task_rollback_restores_a_partially_reused_collection_without_copying_shared_files() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let first = imported("original-a", "hash-a");
    let second = imported("original-b", "hash-b");
    let root = library
        .ingest_collection(&collection(vec![first, second]))
        .unwrap()
        .0;
    let before = library.details(root).unwrap();
    let before_members = members(&library, root);
    library.auxiliary_write(
        picto_library::database::WorkPriority::ForegroundMutation,
        ["roots".to_owned()], [], |transaction, _| {
            transaction.execute(
                "INSERT INTO library_item_alias(stable_key, local_id) VALUES ('older-collection', ?1)",
                [root.0],
            )?;
            Ok(())
        },
    ).unwrap();
    let task = library
        .create_task("import", "Overlapping import", "{}", true, false, 1)
        .unwrap();
    let mut shared = imported("different-source-a", "hash-a");
    shared.tags = vec!["task:added".into()];
    let added = imported("new-c", "hash-c");
    assert_eq!(
        library
            .ingest_collection_for_task(&collection(vec![shared, added]), task, false)
            .unwrap()
            .0,
        root
    );
    assert_eq!(shared_files(&library), 0);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM media_item"), 3);
    assert_eq!(members(&library, root).len(), 3);
    let progress = library.task(task).unwrap().unwrap();
    assert_eq!((progress.added_units, progress.existing_units), (1, 1));
    drop(library);

    let library = Library::open(&path).unwrap();
    library.stop_task(task, true, 2).unwrap();
    library.revert_task(task, 3).unwrap();
    let after = library.details(root).unwrap();
    assert_eq!(after.root, before.root);
    assert_eq!(after.tag_ids, before.tag_ids);
    assert_eq!(members(&library, root), before_members);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM media_item"), 2);
    assert_eq!(shared_files(&library), 0);
    assert_eq!(
        scalar(
            &library,
            "SELECT local_id FROM library_item_alias WHERE stable_key='older-collection'"
        ),
        root.0 as i64
    );
    drop(library);
    let library = Library::open(&path).unwrap();
    assert_eq!(members(&library, root), before_members);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM media_item"), 2);
    assert_eq!(
        library
            .ingest(&imported("third-source-a", "hash-a"))
            .unwrap()
            .0,
        root
    );
    assert_eq!(shared_files(&library), 0);
}

#[test]
fn task_rollback_splits_overlapping_collections_and_reverses_their_aliases() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let first = library
        .ingest_collection(&collection(vec![imported("a", "a"), imported("b", "b")]))
        .unwrap()
        .0;
    let mut c = imported("c", "c");
    c.tags = vec!["keep:second".into()];
    let second = library
        .ingest_collection(&collection(vec![c, imported("d", "d")]))
        .unwrap()
        .0;
    let tag = library.projections().snapshot().tag_ids_by_name["keep:second"];
    let smart = library
        .create_smart_folder(smart_tag("Second collection", tag))
        .unwrap()
        .0;
    assert!(library.projections().snapshot().smart_results[&smart.0].contains(second.0));

    let originals =
        [first, second].map(|root| (library.details(root).unwrap(), members(&library, root)));
    let folder = library.create_folder("Cover", None).unwrap().0;
    library
        .add_to_folder(
            &SelectionTarget::Explicit {
                root_ids: vec![second],
            },
            folder,
        )
        .unwrap();
    library.set_folder_cover(folder, second).unwrap();
    let task = library
        .create_task("import", "Combine collections", "{}", true, false, 1)
        .unwrap();
    let joined = library
        .ingest_collection_for_task(
            &collection(vec![
                imported("new-source-a", "a"),
                imported("new-source-c", "c"),
                imported("new-e", "e"),
            ]),
            task,
            false,
        )
        .unwrap()
        .0;
    assert_eq!(members(&library, joined).len(), 5);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), 1);
    assert_eq!(
        scalar(&library, "SELECT COUNT(*) FROM library_item_alias"),
        1
    );
    assert_eq!(shared_files(&library), 0);
    assert!(library.projections().snapshot().smart_results[&smart.0].contains(joined.0));
    drop(library);

    let library = Library::open(&path).unwrap();
    library.stop_task(task, true, 2).unwrap();
    library.revert_task(task, 3).unwrap();
    for (before, order) in originals {
        let after = library.details(before.root.root_id).unwrap();
        assert_eq!(after.root, before.root);
        assert_eq!(after.lifecycle, before.lifecycle);
        assert_eq!(after.tag_ids, before.tag_ids);
        assert_eq!(members(&library, before.root.root_id), order);
    }
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM library_root"), 2);
    assert_eq!(scalar(&library, "SELECT COUNT(*) FROM media_item"), 4);
    assert_eq!(
        scalar(&library, "SELECT COUNT(*) FROM library_item_alias"),
        0
    );
    assert_eq!(
        library
            .folders()
            .unwrap()
            .iter()
            .find(|row| row.folder_id == folder)
            .unwrap()
            .cover_root_id,
        Some(second)
    );
    let snapshot = library.projections().snapshot();
    assert!(snapshot.smart_results[&smart.0].contains(second.0));
    assert!(!snapshot.smart_results[&smart.0].contains(first.0));
    assert_eq!(shared_files(&library), 0);
}
