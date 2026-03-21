//! Workflow test: subscription group CRUD + schedule validation + cascade delete.

mod common;

#[tokio::test]
async fn group_create_add_subscription_list_delete_cascade() {
    let harness = common::TestHarness::new().await;

    // 1. Create group with default schedule
    let group = harness.db.create_group("Test Group").await.unwrap();
    assert_eq!(group.name, "Test Group");
    assert_eq!(group.schedule, "manual"); // default schedule

    // 2. Set schedule
    harness
        .db
        .set_group_schedule(group.group_id, "daily")
        .await
        .unwrap();
    let updated = harness.db.get_group(group.group_id).await.unwrap().unwrap();
    assert_eq!(updated.schedule, "daily");

    // 3. Create subscription under group
    let sub = harness
        .db
        .create_subscription("Test Sub", "danbooru", Some(group.group_id))
        .await
        .unwrap();
    assert_eq!(sub.group_id, Some(group.group_id));

    // 4. List groups should include our group
    let groups = harness.db.list_groups().await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_id, group.group_id);

    // 5. List subscriptions for group
    let subs = harness
        .db
        .list_subscriptions_for_group(group.group_id)
        .await
        .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].name, "Test Sub");

    // 6. Delete group — should cascade delete subscriptions
    harness.db.delete_group(group.group_id).await.unwrap();
    let groups = harness.db.list_groups().await.unwrap();
    assert_eq!(groups.len(), 0, "group should be deleted");

    // Subscription should also be deleted via CASCADE
    let subs = harness
        .db
        .list_subscriptions_for_group(group.group_id)
        .await
        .unwrap();
    assert_eq!(subs.len(), 0, "subscription should be cascade-deleted");
}
