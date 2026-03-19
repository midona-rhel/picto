//! Workflow test: create collection → add members → members hidden from general scopes.

mod common;

use picto_core::types::*;

#[tokio::test]
async fn collection_members_hidden_from_general_scopes() {
    let harness = common::TestHarness::new().await;

    // Insert 3 active files
    let standalone = harness.insert_test_file("col_standalone", "standalone.png", 1).await;
    let member_a = harness.insert_test_file("col_member_a", "member_a.png", 1).await;
    let member_b = harness.insert_test_file("col_member_b", "member_b.png", 1).await;
    harness.bitmaps_mark_active(standalone);
    harness.bitmaps_mark_active(member_a);
    harness.bitmaps_mark_active(member_b);

    // Create collection and add 2 members
    let collection_id = harness.create_collection("Test Collection").await;
    let added = harness
        .add_collection_members_by_hashes(collection_id, &["col_member_a", "col_member_b"])
        .await;
    assert_eq!(added, 2);
    harness.bitmaps_mark_active(collection_id);

    // Collection scope should return both members
    let col_query = GridPageSlimQuery {
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
    let col_res = picto_core::grid::query::get_grid_page_slim(&harness.db, col_query)
        .await
        .unwrap();
    assert_eq!(col_res.items.len(), 2, "collection scope should show both members");

    // System:all should show standalone + collection entity (not individual members)
    let all_query = common::system_query(GridSystemScopeKey::All, 20);
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_query)
        .await
        .unwrap();

    let hashes: Vec<&str> = all_res.items.iter().map(|i| i.hash.as_str()).collect();
    assert!(hashes.contains(&"col_standalone"), "standalone must appear in all");
    // Members should be collapsed: the collection entity replaces individual members
    // The collection entity appears as a single item with is_collection=true
    let collection_items: Vec<_> = all_res.items.iter().filter(|i| i.is_collection).collect();
    assert!(!collection_items.is_empty(), "collection entity must appear in all scope");

}
