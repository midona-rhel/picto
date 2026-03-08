//! Event contract tests — verify mutation receipt shapes, sequence numbering,
//! domain/scope propagation, and mutation impact presets.

mod common;

use picto_core::events;

#[tokio::test]
async fn mutation_receipt_emits_sequence_numbers() {
    let harness = common::TestHarness::new().await;
    harness.drain_events();

    events::emit_mutation(
        "test_origin",
        events::MutationImpact {
            domains: vec![events::Domain::Files],
            ..Default::default()
        },
    );
    events::emit_mutation(
        "test_origin_2",
        events::MutationImpact {
            domains: vec![events::Domain::Tags],
            ..Default::default()
        },
    );

    let evts = harness.find_events("runtime/mutation_committed");
    assert!(evts.len() >= 2);

    let first: serde_json::Value = serde_json::from_str(&evts[0].1).unwrap();
    let second: serde_json::Value = serde_json::from_str(&evts[1].1).unwrap();
    let seq1 = first["seq"].as_u64().unwrap();
    let seq2 = second["seq"].as_u64().unwrap();
    assert!(
        seq2 > seq1,
        "seq numbers should be monotonically increasing"
    );
}

#[tokio::test]
async fn mutation_receipt_includes_sidebar_tree_invalidation() {
    let harness = common::TestHarness::new().await;
    harness.drain_events();

    events::emit_mutation(
        "test_sidebar",
        events::MutationImpact {
            domains: vec![events::Domain::Sidebar],
            ..Default::default()
        },
    );

    let evts = harness.find_events("runtime/mutation_committed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();
    assert_eq!(payload["origin_command"], "test_sidebar");
    let domains = payload["facts"]["domains"].as_array().expect("domains array");
    assert!(domains.iter().any(|d| d.as_str() == Some("sidebar")));
}

#[tokio::test]
async fn mutation_receipt_includes_grid_scopes() {
    let harness = common::TestHarness::new().await;
    harness.drain_events();

    events::emit_mutation(
        "test_grid",
        events::MutationImpact {
            domains: vec![events::Domain::Files],
            extra_grid_scopes: Some(vec!["scope:a".to_string(), "scope:b".to_string()]),
            ..Default::default()
        },
    );

    let evts = harness.find_events("runtime/mutation_committed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();
    let scopes = payload["facts"]["extra_grid_scopes"]
        .as_array()
        .expect("extra_grid_scopes should be an array");
    let scope_strs: Vec<&str> = scopes.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(scope_strs.contains(&"scope:a"));
    assert!(scope_strs.contains(&"scope:b"));
}

#[test]
fn mutation_receipt_event_contract() {
    use picto_core::events::Domain;
    use picto_core::runtime_contract::mutation::{MutationFacts, MutationReceipt};
    let receipt = MutationReceipt {
        seq: 1,
        ts: "2024-01-01T00:00:00Z".into(),
        origin_command: "add_tags".into(),
        facts: MutationFacts {
            domains: vec![Domain::Tags, Domain::Files],
            file_hashes: Some(vec!["abc123".into()]),
            folder_ids: None,
            smart_folder_ids: None,
            compiler_batch_done: None,
            status_changed: None,
            tags_changed: None,
            tag_structure_changed: None,
            folder_membership_changed: None,
            view_prefs_changed: None,
            extra_grid_scopes: None,
        },
        sidebar_counts: None,
    };
    let json: serde_json::Value = serde_json::to_value(&receipt).unwrap();
    assert_eq!(json["seq"], 1);
    assert_eq!(json["origin_command"], "add_tags");
    assert!(json["facts"]["domains"].is_array());
    assert_eq!(json["facts"]["domains"].as_array().unwrap().len(), 2);
    assert_eq!(json["facts"]["file_hashes"][0], "abc123");
    assert!(json.get("facts").unwrap().get("folder_ids").is_none());
    assert!(
        json.get("facts")
            .unwrap()
            .get("compiler_batch_done")
            .is_none()
    );
}

#[tokio::test]
async fn file_lifecycle_preset_emits_mutation_receipt() {
    let harness = common::TestHarness::new().await;
    harness.drain_events();

    let impact = events::MutationImpact::file_lifecycle(&harness.db);
    events::emit_mutation("test_file_lifecycle", impact);

    let evts = harness.find_events("runtime/mutation_committed");
    assert!(!evts.is_empty(), "should emit runtime/mutation_committed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let domains: Vec<String> = payload["facts"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"files".to_string()));
    assert!(domains.iter().any(|d| d == "sidebar"));
    assert!(payload["facts"]["status_changed"].as_bool() == Some(true));
    assert!(payload.get("sidebar_counts").is_some());
}

#[tokio::test]
async fn folder_sidebar_preset_emits_sidebar_receipt() {
    let harness = common::TestHarness::new().await;
    harness.drain_events();

    let impact = events::MutationImpact::sidebar(events::Domain::Folders);
    events::emit_mutation("test_folder_sidebar", impact);

    let evts = harness.find_events("runtime/mutation_committed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let domains: Vec<String> = payload["facts"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"folders".to_string()));
    assert!(domains.iter().any(|d| d == "sidebar"));
    assert!(
        payload["facts"]["extra_grid_scopes"].is_null(),
        "sidebar preset should NOT set extra_grid_scopes"
    );
}

#[tokio::test]
async fn batch_tags_preset_emits_single_receipt() {
    let harness = common::TestHarness::new().await;
    harness.drain_events();

    let impact = events::MutationImpact::batch_tags();
    events::emit_mutation("test_batch_tags", impact);

    let evts = harness.find_events("runtime/mutation_committed");
    let own_evts: Vec<_> = evts
        .iter()
        .filter(|(_, p)| p.contains("test_batch_tags"))
        .collect();
    assert_eq!(
        own_evts.len(),
        1,
        "should emit exactly 1 mutation receipt for batch_tags"
    );
    let payload: serde_json::Value = serde_json::from_str(&own_evts[0].1).unwrap();

    let domains: Vec<String> = payload["facts"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        !domains.contains(&"sidebar".to_string()),
        "batch_tags should NOT include sidebar domain"
    );
    assert!(
        payload["facts"]["extra_grid_scopes"].is_null(),
        "batch_tags should NOT set extra_grid_scopes"
    );
}
