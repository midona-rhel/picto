//! Workflow test: import inbox → accept active → trash → restore.
//!
//! Verifies the full file lifecycle through status transitions,
//! ensuring grid queries return files in the correct scope at each step.

mod common;

use picto_core::types::*;

#[tokio::test]
async fn lifecycle_inbox_to_active_to_trash_to_restore() {
    let harness = common::TestHarness::new().await;

    // 1. Import into inbox (status=0)
    let fid = harness.insert_test_file("lc_hash", "photo.png", 0).await;
    harness.bitmaps_mark_inbox(fid);

    // Inbox scope should contain the file
    let inbox_q = common::system_query(GridSystemScopeKey::Inbox, 10);
    let inbox_res = picto_core::grid::query::get_grid_page_slim(&harness.db, inbox_q)
        .await
        .unwrap();
    assert_eq!(inbox_res.items.len(), 1, "file should be in inbox");

    // All scope should NOT contain the file
    let all_q = common::system_query(GridSystemScopeKey::All, 10);
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_q)
        .await
        .unwrap();
    assert_eq!(all_res.items.len(), 0, "file must not be in all while inbox");

    // 2. Accept to active (status=1) — auto-syncs bitmaps
    harness.db.update_file_status("lc_hash", 1).await.unwrap();

    let all_q = common::system_query(GridSystemScopeKey::All, 10);
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_q)
        .await
        .unwrap();
    assert_eq!(all_res.items.len(), 1, "file should now be in all");

    let inbox_q = common::system_query(GridSystemScopeKey::Inbox, 10);
    let inbox_res = picto_core::grid::query::get_grid_page_slim(&harness.db, inbox_q)
        .await
        .unwrap();
    assert_eq!(inbox_res.items.len(), 0, "file must not be in inbox after accept");

    // 3. Trash (status=2)
    harness.db.update_file_status("lc_hash", 2).await.unwrap();

    let trash_q = common::system_query(GridSystemScopeKey::Trash, 10);
    let trash_res = picto_core::grid::query::get_grid_page_slim(&harness.db, trash_q)
        .await
        .unwrap();
    assert_eq!(trash_res.items.len(), 1, "file should be in trash");

    let all_q = common::system_query(GridSystemScopeKey::All, 10);
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_q)
        .await
        .unwrap();
    assert_eq!(all_res.items.len(), 0, "file must not be in all while trashed");

    // 4. Restore to active (status=1)
    harness.db.update_file_status("lc_hash", 1).await.unwrap();

    let all_q = common::system_query(GridSystemScopeKey::All, 10);
    let all_res = picto_core::grid::query::get_grid_page_slim(&harness.db, all_q)
        .await
        .unwrap();
    assert_eq!(all_res.items.len(), 1, "file should be back in all after restore");

    let trash_q = common::system_query(GridSystemScopeKey::Trash, 10);
    let trash_res = picto_core::grid::query::get_grid_page_slim(&harness.db, trash_q)
        .await
        .unwrap();
    assert_eq!(trash_res.items.len(), 0, "file must not be in trash after restore");
}
