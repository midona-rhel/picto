//! DB operation tests — hash resolution, tag/folder batch operations,
//! sidebar projection sync, and projection corruption detection.

mod common;

#[tokio::test]
async fn projection_corruption_is_tracked() {
    let harness = common::TestHarness::new().await;

    let file_id = harness
        .insert_test_file("corrupt_hash", "corrupt.png", 1)
        .await;
    let epoch = harness
        .db
        .manifest
        .published_artifact_version("metadata_projection") as i64;

    let fid = file_id;
    harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO entity_metadata_projection (entity_id, epoch, resolved_json, parents_json)
                 VALUES (?1, ?2, 'THIS_IS_NOT_JSON{{{', '[]')",
                rusqlite::params![fid, epoch],
            )?;
            Ok(())
        })
        .await
        .expect("insert corrupt projection");

    let result = harness
        .db
        .get_files_metadata_batch(vec!["corrupt_hash".to_string()])
        .await
        .expect("batch should succeed even with corrupt projection");

    assert!(!result.is_empty(), "should still return data via fallback");

    let count = picto_core::perf::get_projection_corruption_count();
    assert!(count > 0, "corruption count should be incremented");
}

#[tokio::test]
async fn seed_sidebar_normalizes_legacy_all_files_node() {
    let harness = common::TestHarness::new().await;

    harness
        .db
        .with_conn(move |conn| {
            conn.execute("DELETE FROM sidebar_node", [])?;
            conn.execute(
                "INSERT INTO sidebar_node
                 (node_id, kind, parent_id, name, icon, color, sort_order, count,
                  freshness, epoch, selectable, expanded_by_default, meta_json, updated_at)
                 VALUES
                 ('system:active_files', 'system', 'system:library', 'All Files', 'IconPhoto', NULL,
                  1, 7, 'stale', 0, 1, 0, NULL, datetime('now'))",
                [],
            )?;
            picto_core::sidebar::db::seed_sidebar_if_empty(conn)?;
            Ok(())
        })
        .await
        .expect("normalize legacy sidebar node");

    let nodes = harness
        .db
        .get_sidebar_tree()
        .await
        .expect("get sidebar tree");
    assert!(
        nodes
            .iter()
            .any(|node| node.node_id == "system:active" && node.name == "All Active"),
        "canonical all-active node should exist after normalization",
    );
    assert!(
        !nodes.iter().any(|node| node.node_id == "system:active_files"),
        "legacy all-files alias should be removed after normalization",
    );
}

#[tokio::test]
async fn resolve_entity_hashes_batch_returns_entity_ids() {
    let harness = common::TestHarness::new().await;

    let fid1 = harness.insert_test_file("hash_a", "a.png", 1).await;
    let fid2 = harness.insert_test_file("hash_b", "b.png", 1).await;
    harness.insert_test_file("hash_c", "c.png", 1).await;

    let resolved = harness
        .db
        .resolve_entity_hashes_batch(&[
            "hash_a".to_string(),
            "hash_b".to_string(),
            "nonexistent".to_string(),
        ])
        .await
        .expect("resolve batch");

    assert_eq!(resolved.len(), 2);
    let ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    assert!(ids.contains(&fid1));
    assert!(ids.contains(&fid2));
}

#[tokio::test]
async fn resolve_ids_batch_returns_hashes() {
    let harness = common::TestHarness::new().await;

    let fid1 = harness.insert_test_file("hash_x", "x.png", 1).await;
    let fid2 = harness.insert_test_file("hash_y", "y.png", 1).await;

    let resolved = harness
        .db
        .resolve_ids_batch(&[fid1, fid2, 99999])
        .await
        .expect("resolve ids batch");

    assert_eq!(resolved.len(), 2);
    let hashes: Vec<&str> = resolved.iter().map(|(_, h)| h.as_str()).collect();
    assert!(hashes.contains(&"hash_x"));
    assert!(hashes.contains(&"hash_y"));
}

#[tokio::test]
async fn tag_table_query_returns_seeded_tags() {
    let harness = common::TestHarness::new().await;

    let fid = harness
        .insert_test_file("tag_hash_1", "tagged.png", 1)
        .await;
    let tid = harness.insert_test_tag("character", "alice").await;
    harness.tag_entity(fid, tid).await;

    harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "UPDATE tag SET file_count = 1 WHERE tag_id = ?1",
                rusqlite::params![tid],
            )?;
            Ok(())
        })
        .await
        .expect("update file_count");

    let tags = harness
        .db
        .get_all_tags_with_counts()
        .await
        .expect("get_all_tags_with_counts");
    assert!(!tags.is_empty(), "should return seeded tag");
    assert!(tags
        .iter()
        .any(|t| t.subtag == "alice" && t.namespace == "character"));
}

#[tokio::test]
async fn remove_tags_batch_by_entity_ids_correctness() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("hash_a", "a.png", 1).await;
    let f2 = harness.insert_test_file("hash_b", "b.png", 1).await;
    let f3 = harness.insert_test_file("hash_c", "c.png", 1).await;
    let t1 = harness.insert_test_tag("", "red").await;
    let t2 = harness.insert_test_tag("", "blue").await;
    for &fid in &[f1, f2, f3] {
        harness.tag_entity(fid, t1).await;
        harness.tag_entity(fid, t2).await;
    }
    let t1_copy = t1;
    let t2_copy = t2;
    harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "UPDATE tag SET file_count = 3 WHERE tag_id IN (?1, ?2)",
                rusqlite::params![t1_copy, t2_copy],
            )?;
            Ok(())
        })
        .await
        .expect("set file_counts");

    harness
        .db
        .remove_tags_batch_by_entity_ids(vec![f1, f2], vec!["red".into()])
        .await
        .expect("remove tags batch");

    let remaining_t1: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM entity_tag_raw WHERE tag_id = ?1",
                [t1],
                |row| row.get(0),
            )
        })
        .await
        .expect("count t1 tags");
    assert_eq!(remaining_t1, 1, "Only f3 should still have tag 'red'");

    let count_t1: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT file_count FROM tag WHERE tag_id = ?1",
                [t1],
                |row| row.get(0),
            )
        })
        .await
        .expect("get t1 file_count");
    assert_eq!(count_t1, 1, "file_count for 'red' should be 1 (3-2)");

    let remaining_t2: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM entity_tag_raw WHERE tag_id = ?1",
                [t2],
                |row| row.get(0),
            )
        })
        .await
        .expect("count t2 tags");
    assert_eq!(remaining_t2, 3, "All 3 files should still have tag 'blue'");
}

#[tokio::test]
async fn remove_files_from_folder_batch_correctness() {
    let harness = common::TestHarness::new().await;
    let f1 = harness.insert_test_file("hash_fa", "a.png", 1).await;
    let f2 = harness.insert_test_file("hash_fb", "b.png", 1).await;
    let f3 = harness.insert_test_file("hash_fc", "c.png", 1).await;
    let folder_id: i64 = harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO folder (name, created_at) VALUES ('test_folder', datetime('now'))",
                [],
            )?;
            let fid = conn.last_insert_rowid();
            let mut stmt = conn.prepare_cached(
                "INSERT INTO folder_entity (folder_id, entity_id, position_rank) VALUES (?1, ?2, ?3)",
            )?;
            stmt.execute(rusqlite::params![fid, f1, 1])?;
            stmt.execute(rusqlite::params![fid, f2, 2])?;
            stmt.execute(rusqlite::params![fid, f3, 3])?;
            Ok(fid)
        })
        .await
        .expect("create folder with files");

    let removed = harness
        .db
        .remove_entities_from_folder_batch(folder_id, &[f1, f2])
        .await
        .expect("remove files batch");
    assert_eq!(removed, 2, "Should have removed 2 files");

    let remaining: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM folder_entity WHERE folder_id = ?1",
                [folder_id],
                |row| row.get(0),
            )
        })
        .await
        .expect("count remaining folder files");
    assert_eq!(remaining, 1, "Only f3 should remain in the folder");
}

#[tokio::test]
async fn folder_controller_updates_sidebar_projection_immediately() {
    let harness = common::TestHarness::new().await;

    let parent = picto_core::folders::service::create_folder(
        &harness.db,
        "Parent".to_string(),
        None,
        Some("IconFolder".to_string()),
        Some("#aaaaaa".to_string()),
    )
    .await
    .expect("create parent folder");

    let child = picto_core::folders::service::create_folder(
        &harness.db,
        "Child".to_string(),
        Some(parent.folder_id),
        Some("IconPhoto".to_string()),
        Some("#ff0000".to_string()),
    )
    .await
    .expect("create child folder");

    let child_node_id = format!("folder:{}", child.folder_id);

    let child_node: (String, Option<String>, Option<String>, Option<String>) = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT name, parent_id, icon, color FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            }
        })
        .await
        .expect("read created child sidebar node");
    assert_eq!(child_node.0, "Child");
    assert_eq!(child_node.1, Some(format!("folder:{}", parent.folder_id)));
    assert_eq!(child_node.2, Some("IconPhoto".to_string()));
    assert_eq!(child_node.3, Some("#ff0000".to_string()));

    picto_core::folders::service::update_folder(
        &harness.db,
        child.folder_id,
        Some("Child Renamed".to_string()),
        Some("IconStar".to_string()),
        Some("#00ff00".to_string()),
        None,
    )
    .await
    .expect("update folder visuals");

    let updated_node: (String, Option<String>, Option<String>, Option<String>) = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT name, parent_id, icon, color FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            }
        })
        .await
        .expect("read updated child sidebar node");
    assert_eq!(updated_node.0, "Child Renamed");
    assert_eq!(updated_node.1, Some(format!("folder:{}", parent.folder_id)));
    assert_eq!(updated_node.2, Some("IconStar".to_string()));
    assert_eq!(updated_node.3, Some("#00ff00".to_string()));

    picto_core::folders::service::update_folder(
        &harness.db,
        child.folder_id,
        None,
        Some(String::new()),
        Some(String::new()),
        None,
    )
    .await
    .expect("clear folder visuals");

    let cleared_node: (String, Option<String>, Option<String>, Option<String>) = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT name, parent_id, icon, color FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            }
        })
        .await
        .expect("read cleared child sidebar node");
    assert_eq!(cleared_node.0, "Child Renamed");
    assert_eq!(cleared_node.1, Some(format!("folder:{}", parent.folder_id)));
    assert_eq!(cleared_node.2, None);
    assert_eq!(cleared_node.3, None);

    picto_core::folders::service::update_folder_parent(&harness.db, child.folder_id, None)
        .await
        .expect("move child to root");

    let reparents_node_parent: Option<String> = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT parent_id FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| row.get(0),
                )
            }
        })
        .await
        .expect("read reparented child sidebar node");
    assert_eq!(reparents_node_parent, Some("section:folders".to_string()));
}
