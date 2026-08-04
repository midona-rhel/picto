mod common_canonical;

use std::sync::{Arc, Mutex};

use picto_core::db::types::{EntityTarget, EntityTargetKind, ExpansionMode};
use picto_core::engine::ApplicationEngine;
use picto_core::events;

fn entity_target(hash: &str) -> EntityTarget {
    EntityTarget {
        kind: EntityTargetKind::EntityHashes,
        entity_hashes: Some(vec![hash.to_string()]),
        query: None,
        excluded_entity_hashes: None,
    }
}

#[tokio::test]
async fn lifecycle_event_contains_post_compile_smart_folder_count() {
    let harness = common_canonical::TestHarness::new().await;
    let entity_id = harness
        .insert_test_file("one_girl", "one_girl.png", 1)
        .await;
    let tag_id = harness.insert_test_tag("general", "1girl").await;
    harness.tag_entity(entity_id, tag_id).await;

    let engine = ApplicationEngine::new(harness.db.clone());
    let folder_id = engine
        .create_folder("People", None, None, None)
        .expect("create folder");
    harness
        .db
        .add_folder_members(folder_id, &[entity_id], ExpansionMode::EntityOnly)
        .expect("add folder member");
    let smart_folder_id = engine
        .create_smart_folder(
            "One girl",
            None,
            &serde_json::json!({
                "groups": [{
                    "match_mode": "all",
                    "negate": false,
                    "rules": [{
                        "field": "tags",
                        "op": "include_all",
                        "values": ["1girl"]
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
    assert_eq!(engine.smart_folder_bitmap_len(smart_folder_id), 1);

    let captured = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let callback_events = captured.clone();
    events::set_event_callback(move |name, payload| {
        if name == "runtime/state_changed" {
            callback_events
                .lock()
                .expect("lock captured events")
                .push(serde_json::from_str(payload).expect("parse state event"));
        }
    });

    for (status, expected_count) in [(2, 0), (1, 1)] {
        captured.lock().expect("clear events").clear();
        engine
            .set_entity_status(entity_target("one_girl"), status)
            .expect("set entity status");

        assert_eq!(
            engine.smart_folder_bitmap_len(smart_folder_id),
            expected_count
        );
        let events = captured.lock().expect("read captured events");
        assert_eq!(events.len(), 1, "one write should emit one settled event");
        assert_eq!(
            events[0]["changes"]["smart_folder_ids"],
            serde_json::json!([smart_folder_id])
        );
        assert_eq!(
            events[0]["changes"]["smart_folder_counts"],
            serde_json::json!([[smart_folder_id, expected_count]])
        );
        assert!(events[0]["changes"]["sidebar_node_patches"]
            .as_array()
            .expect("folder count patches")
            .iter()
            .any(|patch| {
                patch["node_id"] == format!("folder:{folder_id}")
                    && patch["count"] == expected_count
            }));
    }
}
