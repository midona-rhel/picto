//! Grid paging, collection scoping, and filter combination tests.

mod common;

use picto_core::folders::db::NewFolder;
use picto_core::types::*;

#[tokio::test]
async fn grid_page_slim_returns_inserted_files() {
    let harness = common::TestHarness::new().await;

    harness.insert_test_file("aaa111", "file1.png", 1).await;
    harness.insert_test_file("bbb222", "file2.png", 1).await;
    harness.insert_test_file("ccc333", "file3.png", 1).await;

    let query = common::system_query(GridSystemScopeKey::All, 10);
    let result = picto_core::grid::query::get_grid_page_slim(&harness.db, query)
        .await
        .expect("grid page");
    assert_eq!(result.items.len(), 3);
    assert!(!result.has_more);
}

#[tokio::test]
async fn grid_page_slim_pagination_has_more() {
    let harness = common::TestHarness::new().await;

    for i in 0..5 {
        let hash = format!("hash_{:03}", i);
        let name = format!("file_{}.png", i);
        harness.insert_test_file(&hash, &name, 1).await;
    }

    let query = common::system_query(GridSystemScopeKey::All, 2);
    let result = picto_core::grid::query::get_grid_page_slim(&harness.db, query)
        .await
        .expect("grid page");
    assert_eq!(result.items.len(), 2);
    assert!(result.has_more);
    assert!(result.next_cursor.is_some());
}

#[tokio::test]
async fn grid_page_slim_collection_scope_returns_only_collection_members() {
    let harness = common::TestHarness::new().await;

    harness.insert_test_file("c111", "c1.png", 1).await;
    harness.insert_test_file("c222", "c2.png", 1).await;
    harness.insert_test_file("c333", "c3.png", 1).await;

    let collection_id = harness.create_collection("Collection A").await;
    let added = harness
        .add_collection_members_by_hashes(collection_id, &["c111", "c333"])
        .await;
    assert_eq!(added, 2);

    let query = GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        scope: GridScopeSpec {
            kind: GridScopeKind::Collection,
            collection_entity_id: Some(collection_id),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
        sort: GridSortSpec::default(),
    };
    let result = picto_core::grid::query::get_grid_page_slim(&harness.db, query)
        .await
        .expect("grid page");

    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].hash, "c111");
    assert_eq!(result.items[1].hash, "c333");
    assert!(!result.has_more);
    assert_eq!(result.total_count, Some(2));

    harness
        .db
        .reorder_collection_members_by_hashes(
            collection_id,
            &vec!["c333".to_string(), "c111".to_string()],
        )
        .await
        .expect("reorder collection members");

    let reordered_query = GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        scope: GridScopeSpec {
            kind: GridScopeKind::Collection,
            collection_entity_id: Some(collection_id),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
        sort: GridSortSpec {
            field: Some("imported_at".to_string()),
            order: Some("desc".to_string()),
            ..Default::default()
        },
    };
    let reordered = picto_core::grid::query::get_grid_page_slim(&harness.db, reordered_query)
        .await
        .expect("grid page after reorder");

    assert_eq!(reordered.items.len(), 2);
    assert_eq!(reordered.items[0].hash, "c333");
    assert_eq!(reordered.items[1].hash, "c111");
}

#[tokio::test]
async fn folder_scope_total_count_excludes_hidden_collection_members() {
    let harness = common::TestHarness::new().await;

    let member_entity_id = harness
        .insert_test_file("folder_member", "member.png", 1)
        .await;

    let folder = harness
        .db
        .create_folder(NewFolder {
            name: "Scoped".to_string(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: Vec::new(),
        })
        .await
        .unwrap();

    harness
        .db
        .add_entities_to_folder_batch(folder.folder_id, &["folder_member".to_string()])
        .await
        .unwrap();

    let collection_id = harness.create_collection("Folder Collection").await;
    let added = harness
        .add_collection_members_by_hashes(collection_id, &["folder_member"])
        .await;
    assert_eq!(added, 1);
    harness.bitmaps_mark_active(member_entity_id);
    harness.bitmaps_mark_active(collection_id);

    let query = GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        scope: GridScopeSpec {
            kind: GridScopeKind::Folder,
            folder_id: Some(folder.folder_id),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
        sort: GridSortSpec {
            field: Some("imported_at".to_string()),
            order: Some("desc".to_string()),
            ..Default::default()
        },
    };
    let result = picto_core::grid::query::get_grid_page_slim(&harness.db, query)
        .await
        .expect("folder grid page");

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.total_count, Some(1));
    assert!(result.items[0].is_collection);
    assert_eq!(result.items[0].hash, "folder_member");
}

#[tokio::test]
async fn grid_page_slim_tag_filters_support_any_all_and_reject() {
    let harness = common::TestHarness::new().await;

    let f1 = harness.insert_test_file("t_any_1", "t1.png", 1).await;
    let f2 = harness.insert_test_file("t_any_2", "t2.png", 1).await;
    let f3 = harness.insert_test_file("t_any_3", "t3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let red = harness.insert_test_tag("", "red").await;
    let blue = harness.insert_test_tag("", "blue").await;
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f2, blue).await;
    harness.tag_entity(f3, red).await;
    harness.tag_entity(f3, blue).await;
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(red, f3);
    harness.bitmaps_insert_effective_tag(blue, f2);
    harness.bitmaps_insert_effective_tag(blue, f3);

    let any_query = GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string(), "blue".to_string()]),
            tag_match_mode: Some("any".to_string()),
            ..Default::default()
        },
        sort: GridSortSpec {
            field: Some("name".to_string()),
            order: Some("asc".to_string()),
            ..Default::default()
        },
    };
    let any_res = picto_core::grid::query::get_grid_page_slim(&harness.db, any_query)
        .await
        .expect("any filter");
    assert_eq!(any_res.items.len(), 3);

    let all_query = GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string(), "blue".to_string()]),
            search_excluded_tags: Some(vec!["blue".to_string()]),
            tag_match_mode: Some("all".to_string()),
            ..Default::default()
        },
        sort: GridSortSpec {
            field: Some("name".to_string()),
            order: Some("asc".to_string()),
            ..Default::default()
        },
    };
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_query)
        .await
        .expect("all + reject filter");
    assert_eq!(all_res.items.len(), 0);
}

#[tokio::test]
async fn grid_page_slim_folder_filters_support_any_all_and_reject() {
    let harness = common::TestHarness::new().await;

    let f1 = harness.insert_test_file("f_any_1", "f1.png", 1).await;
    let f2 = harness.insert_test_file("f_any_2", "f2.png", 1).await;
    let f3 = harness.insert_test_file("f_any_3", "f3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let fa = harness
        .db
        .create_folder(NewFolder {
            name: "A".to_string(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .expect("create folder A");
    let fb = harness
        .db
        .create_folder(NewFolder {
            name: "B".to_string(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .expect("create folder B");

    harness
        .db
        .add_entities_to_folder_batch(
            fa.folder_id,
            &["f_any_1".to_string(), "f_any_3".to_string()],
        )
        .await
        .expect("add f1,f3->A");
    harness
        .db
        .add_entities_to_folder_batch(
            fb.folder_id,
            &["f_any_2".to_string(), "f_any_3".to_string()],
        )
        .await
        .expect("add f2,f3->B");

    let any_query = GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            folder_ids: Some(vec![fa.folder_id, fb.folder_id]),
            excluded_folder_ids: Some(vec![fb.folder_id]),
            folder_match_mode: Some("any".to_string()),
            ..Default::default()
        },
        sort: GridSortSpec {
            field: Some("name".to_string()),
            order: Some("asc".to_string()),
            ..Default::default()
        },
    };
    let any_res = picto_core::grid::query::get_grid_page_slim(&harness.db, any_query)
        .await
        .expect("folder any + reject");
    let any_hashes: Vec<String> = any_res.items.iter().map(|i| i.hash.clone()).collect();
    assert_eq!(any_hashes, vec!["f_any_1".to_string()]);

    let all_query = GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            folder_ids: Some(vec![fa.folder_id, fb.folder_id]),
            folder_match_mode: Some("all".to_string()),
            ..Default::default()
        },
        sort: GridSortSpec {
            field: Some("name".to_string()),
            order: Some("asc".to_string()),
            ..Default::default()
        },
    };
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_query)
        .await
        .expect("folder all");
    let all_hashes: Vec<String> = all_res.items.iter().map(|i| i.hash.clone()).collect();
    assert_eq!(all_hashes, vec!["f_any_3".to_string()]);
}
