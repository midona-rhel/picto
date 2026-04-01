//! Workflow test: sidebar counts update correctly after lifecycle/tag/folder changes.

mod common;

use picto_core::folders::db::NewFolder;
use picto_core::scope::resolver::{resolve_scope, scope_count, ScopeFilter};
use picto_core::types::*;

/// Helper to get scope_count for a given scope key.
async fn get_count(
    db: &std::sync::Arc<picto_core::sqlite::SqliteDatabase>,
    scope_key: &str,
) -> i64 {
    let bitmaps = db.bitmaps.clone();
    let key = scope_key.to_string();
    db.with_read_conn(move |conn| Ok(scope_count(conn, &bitmaps, &key).unwrap_or(0)))
        .await
        .unwrap()
}

#[tokio::test]
async fn sidebar_counts_stay_consistent_across_state_changes() {
    let harness = common::TestHarness::new().await;

    // Insert files: 3 active, 2 inbox, 1 trash
    let a1 = harness.insert_test_file("sc_a1", "a1.png", 1).await;
    let a2 = harness.insert_test_file("sc_a2", "a2.png", 1).await;
    let a3 = harness.insert_test_file("sc_a3", "a3.png", 1).await;
    let i1 = harness.insert_test_file("sc_i1", "i1.png", 0).await;
    let i2 = harness.insert_test_file("sc_i2", "i2.png", 0).await;
    let t1 = harness.insert_test_file("sc_t1", "t1.png", 2).await;
    harness.bitmaps_mark_active(a1);
    harness.bitmaps_mark_active(a2);
    harness.bitmaps_mark_active(a3);
    harness.bitmaps_mark_inbox(i1);
    harness.bitmaps_mark_inbox(i2);
    harness.bitmaps_mark_trash(t1);

    // 1. Initial counts
    assert_eq!(get_count(&harness.db, "system:active").await, 3);
    assert_eq!(get_count(&harness.db, "system:inbox").await, 2);
    assert_eq!(get_count(&harness.db, "system:trash").await, 1);

    // 2. Accept inbox file → all increases, inbox decreases
    harness.db.update_file_status("sc_i1", 1).await.unwrap();
    assert_eq!(get_count(&harness.db, "system:active").await, 4);
    assert_eq!(get_count(&harness.db, "system:inbox").await, 1);

    // 3. Tag a file → untagged decreases
    let untagged_before = get_count(&harness.db, "system:untagged").await;
    let tag_id = harness.insert_test_tag("", "red").await;
    harness.tag_entity(a1, tag_id).await;
    harness.bitmaps_insert_effective_tag(tag_id, a1);
    harness.bitmaps_mark_tagged(a1);
    let untagged_after = get_count(&harness.db, "system:untagged").await;
    assert_eq!(
        untagged_after,
        untagged_before - 1,
        "tagging a file should decrease untagged count"
    );

    // 4. Add file to folder → uncategorized decreases
    let uncategorized_before = get_count(&harness.db, "system:uncategorized").await;
    let folder = harness
        .db
        .create_folder(NewFolder {
            name: "Folder".to_string(),
            parent_id: None,
            icon: None,
            color: None,
            auto_tags: vec![],
        })
        .await
        .unwrap();
    harness
        .db
        .add_entities_to_folder_batch(folder.folder_id, &["sc_a2".to_string()])
        .await
        .unwrap();
    let uncategorized_after = get_count(&harness.db, "system:uncategorized").await;
    assert_eq!(
        uncategorized_after,
        uncategorized_before - 1,
        "adding file to folder should decrease uncategorized count"
    );

    // 5. Trash a file → all decreases, trash increases
    let all_before = get_count(&harness.db, "system:active").await;
    let trash_before = get_count(&harness.db, "system:trash").await;
    harness.db.update_file_status("sc_a3", 2).await.unwrap();
    assert_eq!(
        get_count(&harness.db, "system:active").await,
        all_before - 1
    );
    assert_eq!(
        get_count(&harness.db, "system:trash").await,
        trash_before + 1
    );

    // 6. Cross-check: scope_count agrees with resolve_scope for all system scopes
    let scope_checks = vec![
        ("system:active", GridSystemScopeKey::All),
        ("system:inbox", GridSystemScopeKey::Inbox),
        ("system:trash", GridSystemScopeKey::Trash),
        ("system:untagged", GridSystemScopeKey::Untagged),
        ("system:uncategorized", GridSystemScopeKey::Uncategorized),
    ];
    for (key, scope_key) in scope_checks {
        let count = get_count(&harness.db, key).await;
        let filter = ScopeFilter {
            scope: GridScopeSpec {
                kind: GridScopeKind::System,
                system_key: Some(scope_key),
                ..Default::default()
            },
            filters: GridFilterSpec::default(),
        };
        let bm = resolve_scope(&harness.db, &filter).await.unwrap();
        assert_eq!(
            count,
            bm.len() as i64,
            "scope_count({}) = {} but resolve_scope.len() = {}",
            key,
            count,
            bm.len()
        );
    }
}
