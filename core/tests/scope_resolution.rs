//! Scope contract conformance tests.
//!
//! Each test targets `resolve_scope` / `scope_count` directly,
//! asserting intended business rules for system scopes, tag search,
//! folder search, and select-all parity.

mod common;

use picto_core::folders::db::NewFolder;
use picto_core::scope::resolver::{resolve_scope, scope_count, ScopeFilter};
use picto_core::types::*;

/// Business rule: `system:all` = active only (status=1).
/// Inbox and trash are excluded.
#[tokio::test]
async fn scope_contract_system_all_excludes_inbox_and_trash() {
    let harness = common::TestHarness::new().await;
    let f_active = harness.insert_test_file("sc_a", "a.png", 1).await;
    let f_inbox = harness.insert_test_file("sc_b", "b.png", 0).await;
    let f_trash = harness.insert_test_file("sc_c", "c.png", 2).await;
    harness.bitmaps_mark_active(f_active);
    harness.bitmaps_mark_inbox(f_inbox);
    harness.bitmaps_mark_trash(f_trash);

    let filter = ScopeFilter::default();
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert!(bm.contains(f_active as u32), "active file must be in system:all");
    assert!(!bm.contains(f_inbox as u32), "inbox file must NOT be in system:all");
    assert!(!bm.contains(f_trash as u32), "trash file must NOT be in system:all");
    assert_eq!(bm.len(), 1);
}

/// Business rule: `system:inbox` = inbox only (status=0).
#[tokio::test]
async fn scope_contract_inbox_only_inbox() {
    let harness = common::TestHarness::new().await;
    let f_active = harness.insert_test_file("si_a", "a.png", 1).await;
    let f_inbox = harness.insert_test_file("si_b", "b.png", 0).await;
    let f_trash = harness.insert_test_file("si_c", "c.png", 2).await;
    harness.bitmaps_mark_active(f_active);
    harness.bitmaps_mark_inbox(f_inbox);
    harness.bitmaps_mark_trash(f_trash);

    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::System,
            system_key: Some(GridSystemScopeKey::Inbox),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 1);
    assert!(bm.contains(f_inbox as u32));
}

/// Business rule: `system:trash` = trash only (status=2).
#[tokio::test]
async fn scope_contract_trash_only_trash() {
    let harness = common::TestHarness::new().await;
    let f_active = harness.insert_test_file("st_a", "a.png", 1).await;
    let f_inbox = harness.insert_test_file("st_b", "b.png", 0).await;
    let f_trash = harness.insert_test_file("st_c", "c.png", 2).await;
    harness.bitmaps_mark_active(f_active);
    harness.bitmaps_mark_inbox(f_inbox);
    harness.bitmaps_mark_trash(f_trash);

    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::System,
            system_key: Some(GridSystemScopeKey::Trash),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 1);
    assert!(bm.contains(f_trash as u32));
}

/// Business rule: `untagged` = active (status=1) items with no effective tags.
/// Inbox items without tags are NOT included (untagged is active-scoped).
#[tokio::test]
async fn scope_contract_untagged_means_active_without_tags() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("ut_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("ut_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("ut_3", "3.png", 1).await;
    let f4 = harness.insert_test_file("ut_4", "4.png", 0).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    harness.bitmaps_mark_inbox(f4);
    harness.bitmaps_mark_tagged(f1);

    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::System,
            system_key: Some(GridSystemScopeKey::Untagged),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert!(bm.contains(f2 as u32), "untagged active f2 should be included");
    assert!(bm.contains(f3 as u32), "untagged active f3 should be included");
    assert!(!bm.contains(f1 as u32), "tagged f1 must NOT be in untagged");
    assert!(!bm.contains(f4 as u32), "inbox f4 must NOT be in untagged");
    assert_eq!(bm.len(), 2);
}

/// Business rule: `uncategorized` = active singles not in any folder.
/// Inbox items without folders are NOT included.
#[tokio::test]
async fn scope_contract_uncategorized_means_active_without_folder() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("uc_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("uc_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("uc_3", "3.png", 1).await;
    let f4 = harness.insert_test_file("uc_4", "4.png", 0).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    harness.bitmaps_mark_inbox(f4);

    let folder = harness
        .db
        .create_folder(NewFolder {
            name: "Bucket".to_string(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .expect("create folder");
    harness
        .db
        .add_entities_to_folder_batch(folder.folder_id, &["uc_1".to_string()])
        .await
        .expect("add to folder");

    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::System,
            system_key: Some(GridSystemScopeKey::Uncategorized),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert!(!bm.contains(f1 as u32), "f1 is in a folder — not uncategorized");
    assert!(bm.contains(f2 as u32), "f2 is active and uncategorized");
    assert!(bm.contains(f3 as u32), "f3 is active and uncategorized");
    assert!(!bm.contains(f4 as u32), "f4 is inbox — not uncategorized");
    assert_eq!(bm.len(), 2);
}

/// Business rule: tag search default match mode = intersection ("all").
#[tokio::test]
async fn scope_contract_tag_search_default_intersection() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("ti_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("ti_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("ti_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let red = harness.insert_test_tag("", "red").await;
    let blue = harness.insert_test_tag("", "blue").await;
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f3, red).await;
    harness.tag_entity(f2, blue).await;
    harness.tag_entity(f3, blue).await;
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(red, f3);
    harness.bitmaps_insert_effective_tag(blue, f2);
    harness.bitmaps_insert_effective_tag(blue, f3);

    let filter = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string(), "blue".to_string()]),
            tag_match_mode: None,
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 1, "default tag match = intersection");
    assert!(bm.contains(f3 as u32), "only f3 has both red and blue");
}

/// Business rule: tag search "any" = union.
#[tokio::test]
async fn scope_contract_tag_search_union() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("tu_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("tu_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("tu_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let red = harness.insert_test_tag("", "red").await;
    let blue = harness.insert_test_tag("", "blue").await;
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f3, red).await;
    harness.tag_entity(f2, blue).await;
    harness.tag_entity(f3, blue).await;
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(red, f3);
    harness.bitmaps_insert_effective_tag(blue, f2);
    harness.bitmaps_insert_effective_tag(blue, f3);

    let filter = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string(), "blue".to_string()]),
            tag_match_mode: Some("any".to_string()),
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 3, "any = union: all three files match");
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f2 as u32));
    assert!(bm.contains(f3 as u32));
}

/// Business rule: excluded tags are subtracted from results.
#[tokio::test]
async fn scope_contract_tag_search_exclusion() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("te_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("te_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("te_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let red = harness.insert_test_tag("", "red").await;
    let blue = harness.insert_test_tag("", "blue").await;
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f3, red).await;
    harness.tag_entity(f2, blue).await;
    harness.tag_entity(f3, blue).await;
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(red, f3);
    harness.bitmaps_insert_effective_tag(blue, f2);
    harness.bitmaps_insert_effective_tag(blue, f3);

    let filter = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string()]),
            search_excluded_tags: Some(vec!["blue".to_string()]),
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 1, "f3 has blue so excluded, only f1 remains");
    assert!(bm.contains(f1 as u32));
}

/// Business rule: folder default match mode = union ("any").
#[tokio::test]
async fn scope_contract_folder_default_union() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("fu_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("fu_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("fu_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let fa = harness
        .db
        .create_folder(NewFolder {
            name: "A".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    let fb = harness
        .db
        .create_folder(NewFolder {
            name: "B".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    harness.db.add_entities_to_folder_batch(fa.folder_id, &["fu_1".to_string(), "fu_3".to_string()]).await.unwrap();
    harness.db.add_entities_to_folder_batch(fb.folder_id, &["fu_2".to_string(), "fu_3".to_string()]).await.unwrap();

    let filter = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            folder_ids: Some(vec![fa.folder_id, fb.folder_id]),
            folder_match_mode: None,
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 3, "default folder = union: all three");
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f2 as u32));
    assert!(bm.contains(f3 as u32));
}

/// Business rule: folder match mode "all" = intersection.
#[tokio::test]
async fn scope_contract_folder_intersection() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("fint_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("fint_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("fint_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let fa = harness
        .db
        .create_folder(NewFolder {
            name: "A".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    let fb = harness
        .db
        .create_folder(NewFolder {
            name: "B".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    harness.db.add_entities_to_folder_batch(fa.folder_id, &["fint_1".to_string(), "fint_3".to_string()]).await.unwrap();
    harness.db.add_entities_to_folder_batch(fb.folder_id, &["fint_2".to_string(), "fint_3".to_string()]).await.unwrap();

    let filter = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            folder_ids: Some(vec![fa.folder_id, fb.folder_id]),
            folder_match_mode: Some("all".to_string()),
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 1, "folder all = intersection: only f3");
    assert!(bm.contains(f3 as u32));
}

/// Business rule: excluded folders are subtracted from results.
#[tokio::test]
async fn scope_contract_folder_exclusion() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("fex_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("fex_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("fex_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let fa = harness
        .db
        .create_folder(NewFolder {
            name: "A".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    let fb = harness
        .db
        .create_folder(NewFolder {
            name: "B".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    harness.db.add_entities_to_folder_batch(fa.folder_id, &["fex_1".to_string(), "fex_3".to_string()]).await.unwrap();
    harness.db.add_entities_to_folder_batch(fb.folder_id, &["fex_2".to_string(), "fex_3".to_string()]).await.unwrap();

    let filter = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            folder_ids: Some(vec![fa.folder_id]),
            excluded_folder_ids: Some(vec![fb.folder_id]),
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();

    assert_eq!(bm.len(), 1, "f3 is in B (excluded), only f1 remains");
    assert!(bm.contains(f1 as u32));
}

/// Business rule: select-all resolves to the same scope as the grid.
#[tokio::test]
async fn scope_contract_grid_and_selection_same_scope() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("gs_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("gs_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("gs_3", "3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let red = harness.insert_test_tag("", "red").await;
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f3, red).await;
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(red, f3);

    let grid_query = GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string()]),
            ..Default::default()
        },
        sort: GridSortSpec::default(),
    };
    let selection_query = SelectionQuerySpec {
        mode: SelectionMode::AllResults,
        hashes: None,
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string()]),
            ..Default::default()
        },
        sort: GridSortSpec::default(),
        excluded_hashes: None,
        included_hashes: None,
    };

    let grid_filter = ScopeFilter::from(&grid_query);
    let sel_filter = ScopeFilter::from(&selection_query);
    let grid_bm = resolve_scope(&harness.db, &grid_filter).await.unwrap();
    let sel_bm = resolve_scope(&harness.db, &sel_filter).await.unwrap();

    assert_eq!(grid_bm, sel_bm, "grid and selection must resolve identical bitmaps");
    assert_eq!(grid_bm.len(), 2);
    assert!(grid_bm.contains(f1 as u32));
    assert!(grid_bm.contains(f3 as u32));
}

/// Business rule: scope_count agrees with resolve_scope.len() for all system scopes.
#[tokio::test]
async fn scope_contract_scope_count_agrees_with_resolve_scope() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("cnt_1", "1.png", 1).await;
    let f2 = harness.insert_test_file("cnt_2", "2.png", 1).await;
    let f3 = harness.insert_test_file("cnt_3", "3.png", 0).await;
    let f4 = harness.insert_test_file("cnt_4", "4.png", 2).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_inbox(f3);
    harness.bitmaps_mark_trash(f4);
    harness.bitmaps_mark_tagged(f2);

    let folder = harness
        .db
        .create_folder(NewFolder {
            name: "F".into(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    harness.db.add_entities_to_folder_batch(folder.folder_id, &["cnt_2".to_string()]).await.unwrap();

    let cases: Vec<(&str, ScopeFilter)> = vec![
        (
            "system:all",
            ScopeFilter::default(),
        ),
        (
            "system:all_files",
            ScopeFilter::default(),
        ),
        (
            "system:inbox",
            ScopeFilter {
                scope: GridScopeSpec {
                    kind: GridScopeKind::System,
                    system_key: Some(GridSystemScopeKey::Inbox),
                    ..Default::default()
                },
                filters: GridFilterSpec::default(),
            },
        ),
        (
            "system:trash",
            ScopeFilter {
                scope: GridScopeSpec {
                    kind: GridScopeKind::System,
                    system_key: Some(GridSystemScopeKey::Trash),
                    ..Default::default()
                },
                filters: GridFilterSpec::default(),
            },
        ),
        (
            "system:untagged",
            ScopeFilter {
                scope: GridScopeSpec {
                    kind: GridScopeKind::System,
                    system_key: Some(GridSystemScopeKey::Untagged),
                    ..Default::default()
                },
                filters: GridFilterSpec::default(),
            },
        ),
        (
            "system:uncategorized",
            ScopeFilter {
                scope: GridScopeSpec {
                    kind: GridScopeKind::System,
                    system_key: Some(GridSystemScopeKey::Uncategorized),
                    ..Default::default()
                },
                filters: GridFilterSpec::default(),
            },
        ),
    ];

    for (scope_key, filter) in cases {
        let bm = resolve_scope(&harness.db, &filter).await.unwrap();
        let bitmap_count = bm.len() as i64;
        let sidebar_count = harness
            .db
            .with_read_conn({
                let bitmaps = harness.db.bitmaps.clone();
                let key = scope_key.to_string();
                move |conn| Ok(scope_count(conn, &bitmaps, &key)?)
            })
            .await
            .unwrap();
        assert_eq!(
            sidebar_count, bitmap_count,
            "scope_count({}) = {} but resolve_scope.len() = {}",
            scope_key, sidebar_count, bitmap_count
        );
    }
}
