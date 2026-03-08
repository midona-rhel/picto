//! EXPLAIN QUERY PLAN tests — verify composite indexes avoid temp B-tree sorts.

mod common;

#[tokio::test]
async fn grid_query_plans_use_composite_indexes() {
    let harness = common::TestHarness::new().await;

    let cases = [
        (
            "SELECT file_id FROM file WHERE status = 1 ORDER BY imported_at DESC, file_id DESC LIMIT 50",
            "idx_file_status_imported",
        ),
        (
            "SELECT file_id FROM file WHERE status = 1 ORDER BY rating DESC, file_id DESC LIMIT 50",
            "idx_file_status_rating",
        ),
        (
            "SELECT file_id FROM file WHERE status = 1 ORDER BY size DESC, file_id DESC LIMIT 50",
            "idx_file_status_size",
        ),
        (
            "SELECT file_id FROM file WHERE status = 1 ORDER BY view_count DESC, file_id DESC LIMIT 50",
            "idx_file_status_viewcount",
        ),
        (
            "SELECT file_id FROM file WHERE status = 1 ORDER BY name COLLATE NOCASE, file_id LIMIT 50",
            "idx_file_status_name",
        ),
    ];

    for (query, expected_index) in cases {
        let query_owned = query.to_string();
        let plan = harness
            .db
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {query_owned}"))?;
                let rows: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(3))?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(rows.join("\n"))
            })
            .await
            .expect("explain query plan");

        assert!(
            !plan.contains("TEMP B-TREE"),
            "{expected_index}: should not use temp B-tree. Plan:\n{plan}"
        );
        assert!(
            plan.contains(expected_index),
            "Should use {expected_index}. Plan:\n{plan}"
        );
    }
}
