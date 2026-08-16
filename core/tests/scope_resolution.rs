//! Canonical grid scope and count conformance tests.

mod common_canonical;

use std::collections::BTreeSet;

use picto_core::db::types::{
    BaseScope, EntityViewQuery, QueryFilters, QueryPage, QuerySort, ScopeKind,
    TagFilter, TagMatchMode,
};

fn query_ids(
    harness: &common_canonical::TestHarness,
    kind: ScopeKind,
    key: Option<&str>,
    id: Option<i64>,
    filters: QueryFilters,
) -> BTreeSet<i64> {
    harness
        .db
        .query_entity_view(&EntityViewQuery {
            base_scope: BaseScope {
                kind,
                key: key.map(str::to_owned),
                id,
            },
            filters,
            sort: QuerySort::default(),
            page: QueryPage {
                cursor: None,
                limit: 1_000,
            },
        })
        .expect("query entity view")
        .items
        .into_iter()
        .map(|item| item.entity_id)
        .collect()
}

fn system_ids(harness: &common_canonical::TestHarness, key: &str) -> BTreeSet<i64> {
    query_ids(
        harness,
        ScopeKind::System,
        Some(key),
        None,
        QueryFilters::default(),
    )
}

#[tokio::test]
async fn system_scopes_partition_lifecycle_states() {
    let harness = common_canonical::TestHarness::new().await;
    let active = harness
        .insert_test_file("scope_active", "active.png", 1)
        .await;
    let inbox = harness
        .insert_test_file("scope_inbox", "inbox.png", 0)
        .await;
    let trash = harness
        .insert_test_file("scope_trash", "trash.png", 2)
        .await;

    assert_eq!(system_ids(&harness, "all"), BTreeSet::from([active]));
    assert_eq!(system_ids(&harness, "inbox"), BTreeSet::from([inbox]));
    assert_eq!(system_ids(&harness, "trash"), BTreeSet::from([trash]));
}

#[tokio::test]
async fn untagged_and_uncategorized_are_active_only() {
    let harness = common_canonical::TestHarness::new().await;
    let tagged = harness
        .insert_test_file("scope_tagged", "tagged.png", 1)
        .await;
    let categorized = harness
        .insert_test_file("scope_categorized", "categorized.png", 1)
        .await;
    let plain = harness
        .insert_test_file("scope_plain", "plain.png", 1)
        .await;
    let inbox = harness
        .insert_test_file("scope_plain_inbox", "inbox.png", 0)
        .await;

    let red = harness.insert_test_tag("", "red").await;
    harness.tag_entity(tagged, red).await;
    let folder = harness
        .db
        .create_folder("Folder", None, None, None)
        .unwrap();
    harness
        .db
        .add_folder_members(folder, &[categorized])
        .unwrap();

    let untagged = system_ids(&harness, "untagged");
    assert_eq!(untagged, BTreeSet::from([categorized, plain]));
    assert!(!untagged.contains(&inbox));

    let uncategorized = system_ids(&harness, "uncategorized");
    assert_eq!(uncategorized, BTreeSet::from([tagged, plain]));
    assert!(!uncategorized.contains(&inbox));
}

#[tokio::test]
async fn folder_scope_returns_active_members() {
    let harness = common_canonical::TestHarness::new().await;
    let member = harness
        .insert_test_file("scope_member", "member.png", 1)
        .await;
    let other = harness
        .insert_test_file("scope_other", "other.png", 1)
        .await;
    let folder = harness
        .db
        .create_folder("Folder", None, None, None)
        .unwrap();
    harness
        .db
        .add_folder_members(folder, &[member])
        .unwrap();

    let ids = query_ids(
        &harness,
        ScopeKind::Folder,
        None,
        Some(folder),
        QueryFilters::default(),
    );
    assert_eq!(ids, BTreeSet::from([member]));
    assert!(!ids.contains(&other));
}

#[tokio::test]
async fn tag_filters_intersect_and_exclude() {
    let harness = common_canonical::TestHarness::new().await;
    let red_only = harness.insert_test_file("scope_red", "red.png", 1).await;
    let blue_only = harness.insert_test_file("scope_blue", "blue.png", 1).await;
    let both = harness.insert_test_file("scope_both", "both.png", 1).await;
    let red = harness.insert_test_tag("", "red").await;
    let blue = harness.insert_test_tag("", "blue").await;
    harness.tag_entity(red_only, red).await;
    harness.tag_entity(blue_only, blue).await;
    harness.tag_entity(both, red).await;
    harness.tag_entity(both, blue).await;

    let both_tags = query_ids(
        &harness,
        ScopeKind::System,
        Some("all"),
        None,
        QueryFilters {
            tags: Some(vec![
                TagFilter {
                    tag: "red".into(),
                    match_mode: TagMatchMode::Include,
                },
                TagFilter {
                    tag: "blue".into(),
                    match_mode: TagMatchMode::Include,
                },
            ]),
            ..Default::default()
        },
    );
    assert_eq!(both_tags, BTreeSet::from([both]));

    let red_without_blue = query_ids(
        &harness,
        ScopeKind::System,
        Some("all"),
        None,
        QueryFilters {
            tags: Some(vec![
                TagFilter {
                    tag: "red".into(),
                    match_mode: TagMatchMode::Include,
                },
                TagFilter {
                    tag: "blue".into(),
                    match_mode: TagMatchMode::Exclude,
                },
            ]),
            ..Default::default()
        },
    );
    assert_eq!(red_without_blue, BTreeSet::from([red_only]));
}

#[tokio::test]
async fn system_counts_match_canonical_queries_after_mutations() {
    let harness = common_canonical::TestHarness::new().await;
    let active = harness
        .insert_test_file("count_active", "active.png", 1)
        .await;
    let inbox = harness
        .insert_test_file("count_inbox", "inbox.png", 0)
        .await;
    let trash = harness
        .insert_test_file("count_trash", "trash.png", 2)
        .await;

    harness
        .db
        .set_entity_status(&[inbox], 1)
        .unwrap();
    harness
        .db
        .set_entity_status(&[active], 2)
        .unwrap();

    let counts = harness.db.get_scope_counts().unwrap();
    assert_eq!(counts.active, system_ids(&harness, "all").len() as i64);
    assert_eq!(counts.inbox, system_ids(&harness, "inbox").len() as i64);
    assert_eq!(counts.trash, system_ids(&harness, "trash").len() as i64);
    assert_eq!(
        counts.untagged,
        system_ids(&harness, "untagged").len() as i64
    );
    assert_eq!(
        counts.uncategorized,
        system_ids(&harness, "uncategorized").len() as i64
    );
    assert!(system_ids(&harness, "trash").contains(&trash));
}
