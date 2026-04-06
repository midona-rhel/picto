//! Workflow test: add tag → alias → implication → search agrees.

mod common_canonical;

use picto_core::scope::resolver::{resolve_scope, ScopeFilter};
use picto_core::types::*;

#[tokio::test]
async fn tag_search_direct_then_alias_then_implication() {
    let harness = common_canonical::TestHarness::new().await;

    // Insert 3 active files
    let f1 = harness.insert_test_file("ta_1", "f1.png", 1).await;
    let f2 = harness.insert_test_file("ta_2", "f2.png", 1).await;
    let f3 = harness.insert_test_file("ta_3", "f3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);

    // Create tags: "red", "crimson", "color"
    let red = harness.insert_test_tag("", "red").await;
    let crimson = harness.insert_test_tag("", "crimson").await;
    let color = harness.insert_test_tag("", "color").await;

    // Tag f1 with "red", f2 with "crimson", f3 untagged
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f2, crimson).await;
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(crimson, f2);

    // 1. Direct search: "red" returns only f1
    let filter_red = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["red".to_string()]),
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter_red).await.unwrap();
    assert_eq!(bm.len(), 1, "only f1 has tag red");
    assert!(bm.contains(f1 as u32));

    // 2. Simulate alias: "crimson" resolves to "red"
    // In the real system, the alias compiler would update effective tag bitmaps.
    // For deterministic testing, seed effective tags manually:
    // f2 (tagged "crimson") now also appears under "red" effective tag
    harness.bitmaps_insert_effective_tag(red, f2);

    let bm = resolve_scope(&harness.db, &filter_red).await.unwrap();
    assert_eq!(bm.len(), 2, "after alias, both f1 and f2 match red");
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f2 as u32));

    // 3. Simulate implication: "red" implies "color"
    // f1 and f2 (which have effective tag "red") should also have effective tag "color"
    harness.bitmaps_insert_effective_tag(color, f1);
    harness.bitmaps_insert_effective_tag(color, f2);

    let filter_color = ScopeFilter {
        scope: GridScopeSpec::default(),
        filters: GridFilterSpec {
            search_tags: Some(vec!["color".to_string()]),
            ..Default::default()
        },
    };
    let bm = resolve_scope(&harness.db, &filter_color).await.unwrap();
    assert_eq!(
        bm.len(),
        2,
        "implication: red implies color, so f1+f2 match color"
    );
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f2 as u32));

    // f3 should never appear in any tag search
    assert!(!bm.contains(f3 as u32), "f3 has no tags, must not match");
}
