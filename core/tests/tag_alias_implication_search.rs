//! Tag aliases and implications must affect the canonical grid query.

mod common_canonical;

use std::collections::BTreeSet;

use picto_core::db::types::{
    BaseScope, EntityViewQuery, QueryFilters, QueryPage, QuerySort, ScopeKind, TagFilter,
    TagMatchMode,
};
use picto_core::engine::ApplicationEngine;

fn query_tag(harness: &common_canonical::TestHarness, tag: &str) -> BTreeSet<i64> {
    harness
        .db
        .query_entity_view(&EntityViewQuery {
            base_scope: BaseScope {
                kind: ScopeKind::System,
                key: Some("all".into()),
                id: None,
            },
            filters: QueryFilters {
                tags: Some(vec![TagFilter {
                    tag: tag.to_string(),
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
        .expect("query tag")
        .items
        .into_iter()
        .map(|item| item.entity_id)
        .collect()
}

#[tokio::test]
async fn aliases_and_implications_are_visible_to_grid_search() {
    let harness = common_canonical::TestHarness::new().await;
    let red_entity = harness.insert_test_file("tag_red", "red.png", 1).await;
    let crimson_entity = harness
        .insert_test_file("tag_crimson", "crimson.png", 1)
        .await;
    let red = harness.insert_test_tag("", "red").await;
    let crimson = harness.insert_test_tag("", "crimson").await;
    let color = harness.insert_test_tag("", "color").await;
    harness.tag_entity(red_entity, red).await;
    harness.tag_entity(crimson_entity, crimson).await;

    let engine = ApplicationEngine::new(harness.db.clone());
    engine.manage_tag_alias(crimson, Some(red)).unwrap();
    engine.manage_tag_implication(red, color, true).unwrap();

    let expected = BTreeSet::from([red_entity, crimson_entity]);
    assert_eq!(query_tag(&harness, "red"), expected);
    assert_eq!(query_tag(&harness, "crimson"), expected);
    assert_eq!(query_tag(&harness, "color"), expected);
}
