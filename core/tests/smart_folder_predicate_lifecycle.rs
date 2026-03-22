//! Workflow test: smart folder create/edit → count/membership update.

mod common;

use picto_core::scope::resolver::{resolve_scope, ScopeFilter};
use picto_core::smart_folders::db::{
    MatchMode, PredicateRule, SmartFolderPredicate, SmartRuleGroup,
};
use picto_core::types::*;

#[tokio::test]
async fn smart_folder_predicate_changes_update_membership() {
    let harness = common::TestHarness::new().await;

    // Insert 5 files with different tag combos
    let f1 = harness.insert_test_file("sf_1", "landscape1.png", 1).await;
    let f2 = harness.insert_test_file("sf_2", "landscape2.png", 1).await;
    let f3 = harness.insert_test_file("sf_3", "portrait1.png", 1).await;
    let f4 = harness.insert_test_file("sf_4", "nature1.png", 1).await;
    let f5 = harness.insert_test_file("sf_5", "plain.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    harness.bitmaps_mark_active(f4);
    harness.bitmaps_mark_active(f5);

    // Create tags and apply them
    let landscape = harness.insert_test_tag("", "landscape").await;
    let portrait = harness.insert_test_tag("", "portrait").await;
    let nature = harness.insert_test_tag("", "nature").await;

    // f1: landscape
    // f2: landscape + nature
    // f3: portrait
    // f4: nature
    // f5: (no tags)
    harness.tag_entity(f1, landscape).await;
    harness.tag_entity(f2, landscape).await;
    harness.tag_entity(f2, nature).await;
    harness.tag_entity(f3, portrait).await;
    harness.tag_entity(f4, nature).await;

    harness.bitmaps_insert_effective_tag(landscape, f1);
    harness.bitmaps_insert_effective_tag(landscape, f2);
    harness.bitmaps_insert_effective_tag(nature, f2);
    harness.bitmaps_insert_effective_tag(portrait, f3);
    harness.bitmaps_insert_effective_tag(nature, f4);

    // 1. Smart folder with include_all: ["landscape"]
    // Should match f1, f2 (both have landscape)
    let pred_landscape = SmartFolderPredicate {
        groups: vec![SmartRuleGroup {
            match_mode: MatchMode::All,
            negate: false,
            rules: vec![PredicateRule {
                field: "tags".to_string(),
                op: "include_all".to_string(),
                value: None,
                value2: None,
                values: Some(vec!["landscape".to_string()]),
            }],
        }],
    };
    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::Smart,
            smart_folder_predicate: Some(pred_landscape),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();
    assert_eq!(bm.len(), 2, "landscape predicate should match f1 and f2");
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f2 as u32));

    // 2. Edit to include_any: ["landscape", "portrait"]
    // Should match f1, f2, f3
    let pred_any = SmartFolderPredicate {
        groups: vec![SmartRuleGroup {
            match_mode: MatchMode::All,
            negate: false,
            rules: vec![PredicateRule {
                field: "tags".to_string(),
                op: "include_any".to_string(),
                value: None,
                value2: None,
                values: Some(vec!["landscape".to_string(), "portrait".to_string()]),
            }],
        }],
    };
    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::Smart,
            smart_folder_predicate: Some(pred_any),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();
    assert_eq!(
        bm.len(),
        3,
        "any(landscape, portrait) should match f1, f2, f3"
    );
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f2 as u32));
    assert!(bm.contains(f3 as u32));

    // 3. Add exclusion: do_not_include "nature"
    // From the any(landscape, portrait) set, exclude files with "nature"
    // f2 has nature, so only f1 and f3 remain
    let pred_exclude = SmartFolderPredicate {
        groups: vec![SmartRuleGroup {
            match_mode: MatchMode::All,
            negate: false,
            rules: vec![
                PredicateRule {
                    field: "tags".to_string(),
                    op: "include_any".to_string(),
                    value: None,
                    value2: None,
                    values: Some(vec!["landscape".to_string(), "portrait".to_string()]),
                },
                PredicateRule {
                    field: "tags".to_string(),
                    op: "do_not_include".to_string(),
                    value: None,
                    value2: None,
                    values: Some(vec!["nature".to_string()]),
                },
            ],
        }],
    };
    let filter = ScopeFilter {
        scope: GridScopeSpec {
            kind: GridScopeKind::Smart,
            smart_folder_predicate: Some(pred_exclude),
            ..Default::default()
        },
        filters: GridFilterSpec::default(),
    };
    let bm = resolve_scope(&harness.db, &filter).await.unwrap();
    assert_eq!(
        bm.len(),
        2,
        "after excluding nature, f2 drops out: f1 + f3 remain"
    );
    assert!(bm.contains(f1 as u32));
    assert!(bm.contains(f3 as u32));
    assert!(
        !bm.contains(f2 as u32),
        "f2 has nature tag, must be excluded"
    );
}
