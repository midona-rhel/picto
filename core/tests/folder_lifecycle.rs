//! Workflow test: folder create → add files → subfolder → move → reorder.

mod common;

use picto_core::folders::db::NewFolder;
use picto_core::types::*;

#[tokio::test]
async fn folder_create_add_subfolder_move_files() {
    let harness = common::TestHarness::new().await;

    // Insert 3 active files
    let f1 = harness.insert_test_file("fl_1", "alpha.png", 1).await;
    let f2 = harness.insert_test_file("fl_2", "beta.png", 1).await;
    let f3 = harness.insert_test_file("fl_3", "gamma.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);

    // 1. Create parent folder
    let parent = harness
        .db
        .create_folder(NewFolder {
            name: "Parent".to_string(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();

    // 2. Add all 3 files to parent
    harness
        .db
        .add_entities_to_folder_batch(
            parent.folder_id,
            &["fl_1".to_string(), "fl_2".to_string(), "fl_3".to_string()],
        )
        .await
        .unwrap();

    // Verify folder scope returns 3 files
    let parent_query = GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        scope: GridScopeSpec {
            kind: GridScopeKind::Folder,
            folder_id: Some(parent.folder_id),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
        sort: GridSortSpec::default(),
    };
    let parent_res = picto_core::grid::query::get_grid_page_slim(&harness.db, parent_query)
        .await
        .unwrap();
    assert_eq!(
        parent_res.items.len(),
        3,
        "parent folder should have 3 files"
    );

    // 3. Create subfolder under parent
    let child = harness
        .db
        .create_folder(NewFolder {
            name: "Child".to_string(),
            parent_id: Some(parent.folder_id),
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        child.parent_id,
        Some(parent.folder_id),
        "child should be nested under parent"
    );

    // 4. Move f2 from parent to child
    harness
        .db
        .remove_entities_from_folder_batch(parent.folder_id, &["fl_2".to_string()])
        .await
        .unwrap();
    harness
        .db
        .add_entities_to_folder_batch(child.folder_id, &["fl_2".to_string()])
        .await
        .unwrap();

    // Parent should now have 2 files
    let parent_query = GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        scope: GridScopeSpec {
            kind: GridScopeKind::Folder,
            folder_id: Some(parent.folder_id),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
        sort: GridSortSpec::default(),
    };
    let parent_res = picto_core::grid::query::get_grid_page_slim(&harness.db, parent_query)
        .await
        .unwrap();
    assert_eq!(
        parent_res.items.len(),
        2,
        "parent should have 2 files after move"
    );

    // Child should have 1 file
    let child_query = GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        scope: GridScopeSpec {
            kind: GridScopeKind::Folder,
            folder_id: Some(child.folder_id),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
        sort: GridSortSpec::default(),
    };
    let child_res = picto_core::grid::query::get_grid_page_slim(&harness.db, child_query)
        .await
        .unwrap();
    assert_eq!(child_res.items.len(), 1, "child should have 1 file");
    assert_eq!(child_res.items[0].hash, "fl_2");
}
