//! Regression tests for PBI-543: subscription stop, reset, post limit, and name import.

mod common;

use picto_core::subscriptions::gallery_dl_runner::{self, ParsedMetadata};
use picto_core::subscriptions::import_policy::{collection_group_parts, preferred_import_name};
use picto_core::subscriptions::policy::effective_query_post_limit;

// ---------------------------------------------------------------------------
// 1. Reset clears both files_found AND posts_found
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reset_clears_file_and_post_counts() {
    let h = common::TestHarness::new().await;

    // Create a subscription with a query
    let sub = h
        .db
        .create_subscription("test-sub", None)
        .await
        .expect("create subscription");
    let query = h
        .db
        .add_subscription_query(sub.subscription_id, "danbooru", "1girl solo", None)
        .await
        .expect("add query");

    // Simulate progress: set files_found=42, posts_found=10
    let now = chrono::Utc::now().to_rfc3339();
    h.db.update_query_progress(query.query_id, &now, 42, 10)
        .await
        .expect("update progress");

    // Also mark it as having started initial run with a resume cursor
    h.db.set_query_completed_initial_run(query.query_id, false)
        .await
        .ok();
    h.db.set_query_resume_state(
        query.query_id,
        Some("12345".to_string()),
        Some("tag_id_lt".to_string()),
    )
    .await
    .ok();

    // Verify progress was written
    let before = h
        .db
        .get_subscription_query(query.query_id)
        .await
        .expect("get query")
        .expect("query exists");
    assert_eq!(before.files_found, 42);
    assert_eq!(before.posts_found, 10);
    assert!(before.resume_cursor.is_some());

    // Reset subscription state
    h.db.reset_subscription_state(sub.subscription_id)
        .await
        .expect("reset subscription state");

    // Verify BOTH counters are zero
    let after = h
        .db
        .get_subscription_query(query.query_id)
        .await
        .expect("get query")
        .expect("query exists");
    assert_eq!(after.files_found, 0, "files_found should be reset to 0");
    assert_eq!(after.posts_found, 0, "posts_found should be reset to 0");
    assert!(
        after.resume_cursor.is_none(),
        "resume_cursor should be cleared"
    );
    assert!(!after.completed_initial_run, "completed_initial_run should be false");
}

#[tokio::test]
async fn reset_single_query_clears_post_count() {
    let h = common::TestHarness::new().await;

    let sub = h
        .db
        .create_subscription("test-sub2", None)
        .await
        .expect("create subscription");
    let query = h
        .db
        .add_subscription_query(sub.subscription_id, "gelbooru", "landscape", None)
        .await
        .expect("add query");

    let now = chrono::Utc::now().to_rfc3339();
    h.db.update_query_progress(query.query_id, &now, 100, 50)
        .await
        .expect("update progress");

    h.db.reset_query_progress(query.query_id)
        .await
        .expect("reset query progress");

    let after = h
        .db
        .get_subscription_query(query.query_id)
        .await
        .expect("get query")
        .expect("query exists");
    assert_eq!(after.files_found, 0);
    assert_eq!(after.posts_found, 0);
}

// ---------------------------------------------------------------------------
// 2. Post limit policy: no artificial 100-post cap
// ---------------------------------------------------------------------------

#[test]
fn post_limit_no_global_cap_passes_subscription_limit() {
    // global=0, subscription=500 → should be Some(500), not capped at 100
    assert_eq!(effective_query_post_limit(0, 500), Some(500));
}

#[test]
fn post_limit_both_zero_means_unlimited() {
    // global=0, subscription=0 → None (no --post-range flag to gallery-dl)
    assert_eq!(effective_query_post_limit(0, 0), None);
}

#[test]
fn post_limit_global_set_caps_subscription() {
    // global=200, subscription=500 → min(500,200) = 200
    assert_eq!(effective_query_post_limit(200, 500), Some(200));
    // global=200, subscription=50 → min(50,200) = 50
    assert_eq!(effective_query_post_limit(200, 50), Some(50));
}

#[test]
fn post_limit_global_set_subscription_zero_uses_global() {
    assert_eq!(effective_query_post_limit(300, 0), Some(300));
}

// ---------------------------------------------------------------------------
// 3. Coomer/Kemono canonical site ID mapping
// ---------------------------------------------------------------------------

#[test]
fn canonical_site_id_maps_gallery_dl_categories() {
    use gallery_dl_runner::canonical_site_id;

    // Gallery-dl internal category names → our site IDs
    assert_eq!(canonical_site_id("kemonoparty"), "kemono");
    assert_eq!(canonical_site_id("comerparty"), "coomer");
    assert_eq!(canonical_site_id("coomerparty"), "coomer");

    // Legacy domain aliases still work
    assert_eq!(canonical_site_id("kemono.party"), "kemono");
    assert_eq!(canonical_site_id("kemono.su"), "kemono");
    assert_eq!(canonical_site_id("coomer.party"), "coomer");
    assert_eq!(canonical_site_id("coomer.su"), "coomer");
}

// ---------------------------------------------------------------------------
// 4. Kemono/Coomer title import: subject fallback and preferred name
// ---------------------------------------------------------------------------

#[test]
fn parse_metadata_uses_title_from_kemono_sidecar() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{
            "id": "12345678",
            "title": "My Kemono Post Title",
            "user": "creatorname",
            "service": "patreon",
            "category": "kemonoparty",
            "content": "post body here"
        }"#,
    )
    .unwrap();

    let meta = gallery_dl_runner::parse_metadata(&json);
    assert_eq!(meta.title.as_deref(), Some("My Kemono Post Title"));
    assert_eq!(meta.post_id.as_deref(), Some("12345678"));
    assert_eq!(
        preferred_import_name(&meta).as_deref(),
        Some("My Kemono Post Title")
    );
}

#[test]
fn parse_metadata_falls_back_to_subject_for_kemono() {
    // Some kemono posts use "subject" instead of "title"
    let json: serde_json::Value = serde_json::from_str(
        r#"{
            "id": "99999",
            "subject": "Subject Line Post",
            "user": "creator",
            "service": "fanbox",
            "category": "kemonoparty"
        }"#,
    )
    .unwrap();

    let meta = gallery_dl_runner::parse_metadata(&json);
    assert_eq!(
        meta.title.as_deref(),
        Some("Subject Line Post"),
        "should fall back to 'subject' when 'title' is missing"
    );
    assert_eq!(
        preferred_import_name(&meta).as_deref(),
        Some("Subject Line Post")
    );
}

#[test]
fn parse_metadata_prefers_title_over_subject() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{
            "id": "111",
            "title": "Real Title",
            "subject": "Subject Line",
            "category": "kemonoparty"
        }"#,
    )
    .unwrap();

    let meta = gallery_dl_runner::parse_metadata(&json);
    assert_eq!(meta.title.as_deref(), Some("Real Title"));
}

#[test]
fn parse_metadata_empty_title_falls_back_to_subject() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{
            "id": "222",
            "title": "",
            "subject": "Fallback Subject",
            "category": "kemonoparty"
        }"#,
    )
    .unwrap();

    let meta = gallery_dl_runner::parse_metadata(&json);
    assert_eq!(meta.title.as_deref(), Some("Fallback Subject"));
}

#[test]
fn collection_group_uses_canonical_kemono_category() {
    let meta = ParsedMetadata {
        post_id: Some("555".to_string()),
        category: Some("kemonoparty".to_string()),
        title: Some("Post Title".to_string()),
        page_count: Some(2),
        ..Default::default()
    };

    // collection_group_parts uses site_id, not category, for the group key
    let parts = collection_group_parts("kemono", &meta).expect("group parts");
    assert_eq!(parts.0, "kemonoparty"); // category from metadata
    assert_eq!(parts.1, "555");
    assert_eq!(parts.2, "Post Title"); // preferred name from title
}

// ---------------------------------------------------------------------------
// 5. Abort threshold default
// ---------------------------------------------------------------------------

#[test]
fn default_abort_threshold_is_50() {
    let settings = picto_core::settings::store::AppSettings::default();
    assert_eq!(
        settings.sub_abort_threshold, 50,
        "abort threshold should default to 50 (not 10)"
    );
}
