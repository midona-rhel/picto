//! Event contract tests — verify state-changed event shapes, sequence numbering,
//! domain/scope propagation, and change-impact presets.

use picto_core::events;
use picto_core::runtime_contract::change_builder::ChangeImpact;
use picto_core::runtime_contract::state_change::{Domain, SidebarCounts};
use std::sync::{Arc, Mutex, OnceLock};

struct EventHarness {
    events: Arc<Mutex<Vec<(String, String)>>>,
    _event_callback_guard: std::sync::MutexGuard<'static, ()>,
}

static EVENT_CALLBACK_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn event_callback_test_lock() -> &'static Mutex<()> {
    EVENT_CALLBACK_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

impl EventHarness {
    fn new() -> Self {
        let event_callback_guard = event_callback_test_lock()
            .lock()
            .expect("lock event callback test mutex");
        let collected = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let collected_clone = collected.clone();
        events::set_event_callback(move |name: &str, payload: &str| {
            collected_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload.to_string()));
        });
        Self {
            events: collected,
            _event_callback_guard: event_callback_guard,
        }
    }

    fn drain_events(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    fn find_events(&self, name: &str) -> Vec<(String, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .cloned()
            .collect()
    }
}

fn test_sidebar_counts() -> SidebarCounts {
    SidebarCounts {
        active: 3,
        inbox: 1,
        trash: 0,
        uncategorized: 1,
        untagged: 2,
        duplicates: -1,
    }
}

#[tokio::test]
async fn state_changed_event_emits_sequence_numbers() {
    let harness = EventHarness::new();
    harness.drain_events();

    events::emit_state_changed(
        "test_origin",
        ChangeImpact {
            domains: vec![Domain::Files],
            ..Default::default()
        },
    );
    events::emit_state_changed(
        "test_origin_2",
        ChangeImpact {
            domains: vec![Domain::Tags],
            ..Default::default()
        },
    );

    let evts = harness.find_events("runtime/state_changed");
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
async fn state_changed_event_includes_sidebar_tree_changes() {
    let harness = EventHarness::new();
    harness.drain_events();

    events::emit_state_changed(
        "test_sidebar",
        ChangeImpact {
            domains: vec![Domain::Sidebar],
            ..Default::default()
        },
    );

    let evts = harness.find_events("runtime/state_changed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();
    assert_eq!(payload["origin"], "test_sidebar");
    let domains = payload["changes"]["domains"]
        .as_array()
        .expect("domains array");
    assert!(domains.iter().any(|d| d.as_str() == Some("sidebar")));
}

#[tokio::test]
async fn state_changed_event_includes_grid_scopes() {
    let harness = EventHarness::new();
    harness.drain_events();

    events::emit_state_changed(
        "test_grid",
        ChangeImpact {
            domains: vec![Domain::Files],
            extra_grid_scopes: Some(vec!["scope:a".to_string(), "scope:b".to_string()]),
            ..Default::default()
        },
    );

    let evts = harness.find_events("runtime/state_changed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();
    let scopes = payload["changes"]["extra_grid_scopes"]
        .as_array()
        .expect("extra_grid_scopes should be an array");
    let scope_strs: Vec<&str> = scopes.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(scope_strs.contains(&"scope:a"));
    assert!(scope_strs.contains(&"scope:b"));
}

#[test]
fn state_changed_event_contract() {
    use picto_core::runtime_contract::state_change::{StateChangedEvent, StateChanges};
    let event = StateChangedEvent {
        seq: 1,
        ts: "2024-01-01T00:00:00Z".into(),
        origin: "add_tags".into(),
        changes: StateChanges {
            domains: vec![Domain::Tags, Domain::Files],
            entity_hashes: Some(vec!["abc123".into()]),
            member_hashes: None,
            folder_ids: None,
            smart_folder_ids: None,
            compiler_batch_done: None,
            status_changed: None,
            tags_changed: None,
            tag_changes: None,
            tag_structure_changed: None,
            folder_membership_changed: None,
            view_prefs_changed: None,
            media_metadata_changed: None,
            media_fields_changed: None,
            media_derivatives_changed: None,
            derivative_fields_changed: None,
            extra_grid_scopes: None,
            subscription_ids: None,
            query_ids: None,
            credential_categories: None,
            folder_parent_changes: None,
            folder_order_changes: None,
            smart_folder_parent_changes: None,
            smart_folder_order_changes: None,
            sidebar_node_patches: None,
            smart_folder_counts: None,
            grid_reorder: None,
        },
        sidebar_counts: None,
    };
    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["seq"], 1);
    assert_eq!(json["origin"], "add_tags");
    assert!(json["changes"]["domains"].is_array());
    assert_eq!(json["changes"]["domains"].as_array().unwrap().len(), 2);
    assert_eq!(json["changes"]["entity_hashes"][0], "abc123");
    assert!(json.get("changes").unwrap().get("folder_ids").is_none());
    assert!(json
        .get("changes")
        .unwrap()
        .get("compiler_batch_done")
        .is_none());
}

#[tokio::test]
async fn file_lifecycle_preset_emits_state_changed_event() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::file_lifecycle().sidebar_counts(test_sidebar_counts());
    events::emit_state_changed("test_file_lifecycle", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert!(!evts.is_empty(), "should emit runtime/state_changed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let domains = payload["changes"]["domains"].as_array().unwrap();
    assert!(domains.iter().any(|d| d.as_str() == Some("sidebar")));
    assert!(domains.iter().any(|d| d.as_str() == Some("smart_folders")));
    assert!(payload["changes"]["status_changed"].as_bool() == Some(true));
    assert!(payload["changes"]["extra_grid_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("smart:all")));
    assert!(payload.get("sidebar_counts").is_some());
}

#[tokio::test]
async fn folder_sidebar_preset_emits_sidebar_state_change() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::new().add_domains(&[Domain::Folders, Domain::Sidebar]);
    events::emit_state_changed("test_folder_sidebar", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let domains: Vec<String> = payload["changes"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"folders".to_string()));
    assert!(domains.iter().any(|d| d == "sidebar"));
    assert!(
        payload["changes"]["extra_grid_scopes"].is_null(),
        "sidebar preset should NOT set extra_grid_scopes"
    );
}

#[tokio::test]
async fn batch_tags_preset_emits_single_state_change() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::batch_tags();
    events::emit_state_changed("test_batch_tags", impact);

    let evts = harness.find_events("runtime/state_changed");
    let own_evts: Vec<_> = evts
        .iter()
        .filter(|(_, p)| p.contains("test_batch_tags"))
        .collect();
    assert_eq!(
        own_evts.len(),
        1,
        "should emit exactly 1 state_changed event for batch_tags"
    );
    let payload: serde_json::Value = serde_json::from_str(&own_evts[0].1).unwrap();

    let domains: Vec<String> = payload["changes"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        domains.contains(&"sidebar".to_string()),
        "batch_tags should include sidebar because smart-folder counts can change"
    );
    assert!(
        payload["changes"]["extra_grid_scopes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("smart:all")),
        "batch_tags should declare smart:all when tag changes can affect smart folders"
    );
}

#[tokio::test]
async fn metadata_fields_that_drive_smart_folders_emit_smart_scope_changes() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::file_metadata("abc123".into())
        .media_fields_changed(&[
            picto_core::runtime_contract::state_change::MediaMetadataField::Rating,
        ])
        .smart_folder_scopes_changed_for_media_fields(&[
            picto_core::runtime_contract::state_change::MediaMetadataField::Rating,
        ]);
    events::emit_state_changed("test_metadata_smart_scope", impact);

    let evts = harness.find_events("runtime/state_changed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let domains: Vec<String> = payload["changes"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"smart_folders".to_string()));
    assert!(domains.contains(&"sidebar".to_string()));
    assert!(payload["changes"]["extra_grid_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("smart:all")));
}

#[tokio::test]
async fn merged_change_impact_emits_one_combined_delta() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::file_lifecycle()
        .sidebar_counts(test_sidebar_counts())
        .entity_hashes(vec!["hash_a".into()])
        .merge(ChangeImpact::folder_file_change(9).entity_hashes(vec!["hash_b".into()]))
        .merge(
            ChangeImpact::new()
                .tags_added(vec!["artist:test".into()])
                .tags_removed(vec!["old:tag".into()]),
        );
    events::emit_state_changed("test_merged_change_impact", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert!(!evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let hashes: Vec<String> = payload["changes"]["entity_hashes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(hashes.contains(&"hash_a".to_string()));
    assert!(hashes.contains(&"hash_b".to_string()));

    let folder_ids: Vec<i64> = payload["changes"]["folder_membership_changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert!(folder_ids.contains(&9));

    let scopes: Vec<String> = payload["changes"]["extra_grid_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(scopes.contains(&"smart:all".to_string()));

    let added_tags: Vec<String> = payload["changes"]["tag_changes"]["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let removed_tags: Vec<String> = payload["changes"]["tag_changes"]["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(added_tags.contains(&"artist:test".to_string()));
    assert!(removed_tags.contains(&"old:tag".to_string()));
}

#[tokio::test]
async fn folder_watch_style_delta_includes_both_file_and_membership_changes() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::file_lifecycle()
        .sidebar_counts(test_sidebar_counts())
        .entity_hashes(vec!["imported_hash".into()])
        .merge(ChangeImpact::folder_file_change(17).entity_hashes(vec!["skipped_hash".into()]));
    events::emit_state_changed("watch_folder_import", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert_eq!(evts.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&evts[0].1).unwrap();

    let domains: Vec<String> = payload["changes"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"folders".to_string()));
    assert!(domains.contains(&"sidebar".to_string()));
    assert!(domains.contains(&"smart_folders".to_string()));

    let hashes: Vec<String> = payload["changes"]["entity_hashes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(hashes.contains(&"imported_hash".to_string()));
    assert!(hashes.contains(&"skipped_hash".to_string()));

    let folder_ids: Vec<i64> = payload["changes"]["folder_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert!(folder_ids.contains(&17));

    let membership_changed: Vec<i64> = payload["changes"]["folder_membership_changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert!(membership_changed.contains(&17));
}

#[tokio::test]
async fn subscription_batch_delta_refreshes_inbox_and_counts() {
    let harness = EventHarness::new();
    harness.drain_events();

    let impact = ChangeImpact::file_lifecycle()
        .sidebar_counts(test_sidebar_counts())
        .extra_grid_scopes(vec!["system:inbox".into()]);
    events::emit_state_changed("subscription_import", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert_eq!(evts.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&evts[0].1).unwrap();

    let domains: Vec<String> = payload["changes"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"sidebar".to_string()));
    assert!(domains.contains(&"smart_folders".to_string()));

    let scopes: Vec<String> = payload["changes"]["extra_grid_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(scopes.contains(&"system:inbox".to_string()));
}

#[tokio::test]
async fn folder_move_emits_parent_and_order_changes() {
    let harness = EventHarness::new();

    let impact = ChangeImpact::new()
        .add_domains(&[Domain::Folders, Domain::Sidebar])
        .folder_ids(vec![10])
        .folder_parent_changes(vec![(10, Some(5))])
        .folder_order_changes(vec![(10, 0), (11, 1), (12, 2)]);
    events::emit_state_changed("move_folder", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert_eq!(evts.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&evts[0].1).unwrap();

    let parent_changes = payload["changes"]["folder_parent_changes"]
        .as_array()
        .unwrap();
    assert_eq!(parent_changes.len(), 1);
    assert_eq!(parent_changes[0][0], 10);
    assert_eq!(parent_changes[0][1], 5);

    let order_changes = payload["changes"]["folder_order_changes"]
        .as_array()
        .unwrap();
    assert_eq!(order_changes.len(), 3);
    assert_eq!(order_changes[0][0], 10);
    assert_eq!(order_changes[0][1], 0);
}

#[tokio::test]
async fn smart_folder_move_emits_parent_and_order_changes() {
    let harness = EventHarness::new();

    let impact = ChangeImpact::new()
        .add_domains(&[Domain::SmartFolders, Domain::Sidebar])
        .smart_folder_ids(vec![20])
        .smart_folder_parent_changes(vec![(20, None)])
        .smart_folder_order_changes(vec![(20, 0), (21, 1)]);
    events::emit_state_changed("move_smart_folder", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert_eq!(evts.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&evts[0].1).unwrap();

    let parent_changes = payload["changes"]["smart_folder_parent_changes"]
        .as_array()
        .unwrap();
    assert_eq!(parent_changes.len(), 1);
    assert_eq!(parent_changes[0][0], 20);
    assert!(parent_changes[0][1].is_null());

    let order_changes = payload["changes"]["smart_folder_order_changes"]
        .as_array()
        .unwrap();
    assert_eq!(order_changes.len(), 2);
    assert_eq!(order_changes[1][0], 21);
    assert_eq!(order_changes[1][1], 1);
}

#[tokio::test]
async fn folder_reorder_emits_order_changes_without_parent() {
    let harness = EventHarness::new();

    let impact = ChangeImpact::new()
        .add_domains(&[Domain::Folders, Domain::Sidebar])
        .folder_ids(vec![10, 11, 12])
        .folder_order_changes(vec![(10, 2), (11, 0), (12, 1)]);
    events::emit_state_changed("reorder_folders", impact);

    let evts = harness.find_events("runtime/state_changed");
    assert_eq!(evts.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&evts[0].1).unwrap();

    assert!(payload["changes"]["folder_parent_changes"].is_null());
    let order_changes = payload["changes"]["folder_order_changes"]
        .as_array()
        .unwrap();
    assert_eq!(order_changes.len(), 3);
}

#[tokio::test]
async fn merge_tags_emission_includes_entity_hashes_and_tag_details() {
    let harness = EventHarness::new();
    harness.drain_events();

    // Simulate what merge_tags now emits: tag_structure_change + entity_hashes + tags_added/removed
    let impact = ChangeImpact::tag_structure_change()
        .entity_hashes(vec!["hash_a".into(), "hash_b".into()])
        .tags_removed(vec!["artist:old_name".into()])
        .tags_added(vec!["artist:new_name".into()]);
    events::emit_state_changed("merge_tags", impact);

    let evts = harness.find_events("runtime/state_changed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    // Must include entity_hashes
    let hashes: Vec<String> = payload["changes"]["entity_hashes"]
        .as_array()
        .expect("merge_tags must emit entity_hashes")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(hashes.contains(&"hash_a".to_string()));
    assert!(hashes.contains(&"hash_b".to_string()));

    // Must include tag_changes with added/removed
    let added: Vec<String> = payload["changes"]["tag_changes"]["added"]
        .as_array()
        .expect("merge_tags must emit tags_added")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let removed: Vec<String> = payload["changes"]["tag_changes"]["removed"]
        .as_array()
        .expect("merge_tags must emit tags_removed")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(added.contains(&"artist:new_name".to_string()));
    assert!(removed.contains(&"artist:old_name".to_string()));

    // Must still include tag_structure_changed
    assert_eq!(
        payload["changes"]["tag_structure_changed"].as_bool(),
        Some(true),
        "merge_tags must still emit tag_structure_changed"
    );
}

#[tokio::test]
async fn backfill_deferred_emits_only_actually_changed_derivative_fields() {
    use picto_core::runtime_contract::state_change::MediaDerivativeField;

    let harness = EventHarness::new();
    harness.drain_events();

    // Simulate backfill that only changed thumbnails and phash (no color extraction)
    let mut fields = Vec::new();
    fields.push(MediaDerivativeField::Thumbnail);
    fields.push(MediaDerivativeField::Phash);
    let impact = ChangeImpact::new()
        .entity_hashes(vec!["hash_x".into()])
        .derivative_fields_changed(&fields)
        .smart_folder_scopes_changed_for_derivative_fields(&fields);
    events::emit_state_changed("backfill_missing_deferred", impact);

    let evts = harness.find_events("runtime/state_changed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let derivative_fields: Vec<String> = payload["changes"]["derivative_fields_changed"]
        .as_array()
        .expect("backfill must emit derivative_fields_changed")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        derivative_fields.contains(&"thumbnail".to_string()),
        "should include thumbnail"
    );
    assert!(
        derivative_fields.contains(&"phash".to_string()),
        "should include phash"
    );
    assert!(
        !derivative_fields.contains(&"dominant_color_hex".to_string()),
        "should NOT include dominant_color_hex when colors were not extracted"
    );
}

#[tokio::test]
async fn delete_tag_emission_includes_entity_hashes_and_tags_removed() {
    let harness = EventHarness::new();
    harness.drain_events();

    // Simulate what delete_tag now emits: tag_structure_change + entity_hashes + tags_removed
    let impact = ChangeImpact::tag_structure_change()
        .entity_hashes(vec!["hash_1".into(), "hash_2".into()])
        .tags_removed(vec!["artist:deleted_tag".into()]);
    events::emit_state_changed("delete_tag", impact);

    let evts = harness.find_events("runtime/state_changed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let hashes: Vec<String> = payload["changes"]["entity_hashes"]
        .as_array()
        .expect("delete_tag must emit entity_hashes")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(hashes.contains(&"hash_1".to_string()));
    assert!(hashes.contains(&"hash_2".to_string()));

    let removed: Vec<String> = payload["changes"]["tag_changes"]["removed"]
        .as_array()
        .expect("delete_tag must emit tags_removed")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(removed.contains(&"artist:deleted_tag".to_string()));

    assert_eq!(
        payload["changes"]["tag_structure_changed"].as_bool(),
        Some(true),
    );
}

#[tokio::test]
async fn rename_tag_emission_includes_entity_hashes_and_tag_names() {
    let harness = EventHarness::new();
    harness.drain_events();

    // Simulate what rename_tag now emits: tag_structure_change + entity_hashes + old/new tag names
    let impact = ChangeImpact::tag_structure_change()
        .entity_hashes(vec!["hash_x".into()])
        .tags_removed(vec!["character:old_name".into()])
        .tags_added(vec!["character:new_name".into()]);
    events::emit_state_changed("rename_tag", impact);

    let evts = harness.find_events("runtime/state_changed");
    let payload: serde_json::Value = serde_json::from_str(&evts.last().unwrap().1).unwrap();

    let hashes: Vec<String> = payload["changes"]["entity_hashes"]
        .as_array()
        .expect("rename_tag must emit entity_hashes")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(hashes.contains(&"hash_x".to_string()));

    let removed: Vec<String> = payload["changes"]["tag_changes"]["removed"]
        .as_array()
        .expect("rename_tag must emit tags_removed with old name")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(removed.contains(&"character:old_name".to_string()));

    let added: Vec<String> = payload["changes"]["tag_changes"]["added"]
        .as_array()
        .expect("rename_tag must emit tags_added with new name")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(added.contains(&"character:new_name".to_string()));
}
