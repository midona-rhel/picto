mod common;

use picto_core::folders::db::NewFolder;
use picto_core::selection::summary::get_selection_summary;
use picto_core::sqlite::bitmaps::BitmapKey;
use picto_core::types::{SelectionMode, SelectionQuerySpec};

#[tokio::test]
async fn folder_filtered_selection_summary_uses_entity_aggregate_stats() {
    let harness = common::TestHarness::new().await;

    let member_entity_id = harness.insert_test_file("member_hash", "member.png", 1).await;
    let solo_entity_id = harness.insert_test_file("solo_hash", "solo.png", 1).await;

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
        .add_entities_to_folder_batch(folder.folder_id, &["member_hash".to_string()])
        .await
        .unwrap();

    let collection_id = harness.create_collection("Aggregate").await;
    harness
        .add_collection_members_by_hashes(collection_id, &["member_hash"])
        .await;

    harness
        .db
        .bitmaps
        .insert(&BitmapKey::Status(1), collection_id as u32);
    harness
        .db
        .bitmaps
        .insert(&BitmapKey::Status(1), solo_entity_id as u32);
    harness
        .db
        .bitmaps
        .insert(&BitmapKey::AllActive, collection_id as u32);
    harness
        .db
        .bitmaps
        .insert(&BitmapKey::AllActive, solo_entity_id as u32);

    let summary = get_selection_summary(
        &harness.db,
        SelectionQuerySpec {
            mode: SelectionMode::AllResults,
            hashes: None,
            search_tags: None,
            search_excluded_tags: None,
            tag_match_mode: None,
            smart_folder_predicate: None,
            smart_folder_sort_field: None,
            smart_folder_sort_order: None,
            sort_field: None,
            sort_order: None,
            excluded_hashes: None,
            included_hashes: None,
            status: None,
            folder_ids: Some(vec![folder.folder_id]),
            excluded_folder_ids: None,
            folder_match_mode: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(summary.total_count, 1);
    assert_eq!(summary.selected_count, 1);
    assert_eq!(summary.stats.total_size_bytes, Some(1024));
    assert_eq!(
        summary
            .stats
            .mime_counts
            .as_ref()
            .and_then(|m| m.get("image/png"))
            .copied(),
        Some(1)
    );
    assert_eq!(summary.sample_hashes, vec!["member_hash".to_string()]);
    assert!(!summary.pending);
    let _ = member_entity_id;
}
