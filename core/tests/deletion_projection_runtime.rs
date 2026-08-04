mod common_canonical;

use picto_core::db::projection::bitmaps::BitmapKey;
use picto_core::db::types::{
    BaseScope, EntityTarget, EntityTargetKind, EntityViewQuery, QueryFilters, QueryPage, QuerySort,
    ScopeKind, TagFilter, TagMatchMode,
};
use picto_core::engine::ApplicationEngine;

fn entity_target(hash: &str) -> EntityTarget {
    EntityTarget {
        kind: EntityTargetKind::EntityHashes,
        entity_hashes: Some(vec![hash.to_string()]),
        query: None,
        excluded_entity_hashes: None,
    }
}

#[tokio::test]
async fn engine_delete_clears_tag_projections_and_tagged_smart_scope() {
    let harness = common_canonical::TestHarness::new().await;
    let entity_id = harness
        .insert_test_file("delete_tagged", "delete_tagged.png", 1)
        .await;
    let tag_id = harness.insert_test_tag("general", "delete-me").await;
    let implied_tag_id = harness.insert_test_tag("general", "deleted-parent").await;
    harness.tag_entity(entity_id, tag_id).await;

    let engine = ApplicationEngine::new(harness.db.clone());
    engine
        .manage_tag_implication(tag_id, implied_tag_id, true)
        .expect("create tag implication");
    let smart_folder_id = engine
        .create_smart_folder(
            "Implied tag",
            None,
            &serde_json::json!({
                "groups": [{
                    "match_mode": "all",
                    "negate": false,
                    "rules": [{
                        "field": "tags",
                        "op": "include_all",
                        "values": ["deleted-parent"]
                    }]
                }]
            })
            .to_string(),
            None,
            None,
            None,
        )
        .expect("create smart folder");
    harness.db.full_rebuild();

    assert!(harness
        .db
        .bitmaps
        .get(&BitmapKey::Tag(tag_id))
        .contains(entity_id as u32));
    assert!(harness
        .db
        .bitmaps
        .get(&BitmapKey::ImpliedTag(implied_tag_id))
        .contains(entity_id as u32));
    assert_eq!(engine.smart_folder_bitmap_len(smart_folder_id), 1);

    engine
        .delete_entities(entity_target("delete_tagged"))
        .expect("delete tagged entity");

    for key in [
        BitmapKey::Tag(tag_id),
        BitmapKey::ImpliedTag(implied_tag_id),
        BitmapKey::EffectiveTag(implied_tag_id),
        BitmapKey::Tagged,
        BitmapKey::SmartFolder(smart_folder_id),
    ] {
        assert!(
            !harness.db.bitmaps.get(&key).contains(entity_id as u32),
            "deleted entity remains in {key:?}"
        );
    }

    let tagged_scope = harness
        .db
        .query_entity_view(&EntityViewQuery {
            base_scope: BaseScope {
                kind: ScopeKind::System,
                key: Some("all".to_string()),
                id: None,
            },
            filters: QueryFilters {
                tags: Some(vec![TagFilter {
                    tag: "deleted-parent".to_string(),
                    match_mode: TagMatchMode::Include,
                }]),
                ..Default::default()
            },
            sort: QuerySort::default(),
            page: QueryPage {
                cursor: None,
                limit: 100,
            },
        })
        .expect("query deleted tag scope");
    assert_eq!(tagged_scope.total_count, Some(0));
}
