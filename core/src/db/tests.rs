//! Integration-style tests for LibraryDatabase (moved out of mod.rs).

use super::LibraryDatabase;
use crate::background_work::{DeferredWorkFilter, DeferredWorkType};
use crate::db::core::schema::{CURRENT_SCHEMA_VERSION, LIBRARY_DDL};
use crate::db::types::{
    BaseScope, DuplicateResolveStatus, EntityViewQuery, MediaEntityPatch, QueryFilters, QueryPage,
    QuerySort, ScopeKind, TAG_PROVENANCE_MANUAL,
};
use crate::media_analysis::ensure_missing_color_analysis_jobs;
use crate::media_analysis::TARGET_COLOR_ANALYSIS_VERSION;
use crate::media_processing::colors::{serialize_dominant_palette_blob, DominantColor};
use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::subscriptions::runtime_db::upsert_subscription_issue;
use img_hash::ImageHash;
use rusqlite::{params, Connection};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn open_test_db() -> LibraryDatabase {
    let tmp = TempDir::new().expect("tempdir");
    let db = LibraryDatabase::open(tmp.path()).expect("open library db");
    std::mem::forget(tmp);
    db
}

fn supported_phash(bytes: [u8; 32]) -> String {
    ImageHash::<Vec<u8>>::from_bytes(&bytes)
        .expect("supported pHash")
        .to_base64()
}

#[test]
fn indexed_phash_candidates_apply_live_status_and_follow_replacement() {
    let db = open_test_db();
    let phash = supported_phash([42_u8; 32]);
    let file_id = db
        .insert_file(
            "indexed-status-file",
            "image/png",
            1,
            Some(1),
            Some(1),
            None,
            Some(1),
            false,
            "2026-08-14T00:00:00Z",
        )
        .expect("insert file");
    let entity_id = db
        .insert_entity(
            "indexed-status-entity",
            file_id,
            Some("indexed"),
            1,
            "2026-08-14T00:00:00Z",
            "2026-08-14T00:00:00Z",
        )
        .expect("insert entity");
    db.replace_file_phash(file_id, Some(&phash))
        .expect("set pHash");

    let candidate_count = |db: &LibraryDatabase| {
        db.with_read(|conn| {
            super::find_perceptual_hash_candidates_on_conn(conn, &phash, 7)
                .map(|rows| rows.len() as i64)
        })
        .expect("query candidates")
    };
    assert_eq!(candidate_count(&db), 1);

    db.with_write(|conn| {
        conn.execute(
            "UPDATE media_entity SET status = 2 WHERE entity_id = ?1",
            [entity_id],
        )?;
        Ok(())
    })
    .expect("trash entity");
    assert_eq!(candidate_count(&db), 0);

    db.replace_file_phash(file_id, None).expect("clear pHash");
    db.with_read(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM media_file_phash_index WHERE file_id = ?1",
            [file_id],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    })
    .expect("inspect cleared pHash index");
}

#[test]
fn folder_mutations_emit_outbox_ops_with_stable_uuid() {
    let db = open_test_db();

    let folder_id = db.create_folder("Art", None, None, None).unwrap();
    db.update_folder(
        folder_id,
        &crate::db::types::FolderPatch {
            name: Some("Artwork".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    db.delete_folder(folder_id).unwrap();

    let ops: Vec<(String, String, String, String)> = db
        .with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT op_type, entity_key, hlc, device_id FROM op_outbox ORDER BY op_id",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();

    assert_eq!(
        ops.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(),
        vec!["folder_created", "folder_updated", "folder_deleted"]
    );
    let uuid = &ops[0].1;
    assert_eq!(uuid.len(), 32, "entity key must be the folder uuid");
    assert!(ops.iter().all(|o| &o.1 == uuid));
    assert!(
        ops[0].2 < ops[1].2 && ops[1].2 < ops[2].2,
        "hlc must increase"
    );
    assert!(ops.iter().all(|o| !o.3.is_empty()));
}

#[test]
fn deleting_folder_tree_removes_memberships_not_media_and_refreshes_sidebar() {
    let db = Arc::new(open_test_db());
    let root_id = db.create_folder("Root", None, None, None).unwrap();
    let child_id = db
        .create_folder("Child", Some(root_id), None, None)
        .unwrap();
    let grandchild_id = db
        .create_folder("Grandchild", Some(child_id), None, None)
        .unwrap();

    let entity_ids: Vec<i64> = (0..3)
        .map(|index| {
            let file_id = db
                .insert_file(
                    &format!("folder-delete-file-{index}"),
                    "image/png",
                    10,
                    None,
                    None,
                    None,
                    None,
                    false,
                    "2026-08-04",
                )
                .unwrap();
            db.insert_entity(
                &format!("folder-delete-entity-{index}"),
                file_id,
                None,
                1,
                "2026-08-04",
                "2026-08-04",
            )
            .unwrap()
        })
        .collect();
    for (folder_id, entity_id) in [root_id, child_id, grandchild_id]
        .into_iter()
        .zip(entity_ids.iter().copied())
    {
        db.add_folder_members(folder_id, &[entity_id]).unwrap();
    }

    let engine = crate::engine::ApplicationEngine::new(db.clone());
    let deleted = engine.delete_folder(root_id).unwrap();
    assert_eq!(deleted.folder_ids(), vec![grandchild_id, child_id, root_id]);
    assert_eq!(deleted.deleted_folders.len(), 3);
    engine.rebuild_sidebar();

    let (folders, memberships, entities, files, folder_nodes, uncategorized) = db
        .with_read(|conn| {
            Ok((
                conn.query_row("SELECT COUNT(*) FROM folder", [], |row| {
                    row.get::<_, i64>(0)
                })?,
                conn.query_row("SELECT COUNT(*) FROM folder_member", [], |row| {
                    row.get::<_, i64>(0)
                })?,
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| {
                    row.get::<_, i64>(0)
                })?,
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| {
                    row.get::<_, i64>(0)
                })?,
                conn.query_row(
                    "SELECT COUNT(*) FROM sidebar_node WHERE node_id LIKE 'folder:%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?,
                conn.query_row(
                    "SELECT count FROM sidebar_node WHERE node_id = 'system:uncategorized'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!((folders, memberships), (0, 0));
    assert_eq!((entities, files), (3, 3));
    assert_eq!(folder_nodes, 0);
    assert_eq!(uncategorized, 3);
    assert_eq!(db.get_scope_counts().unwrap().uncategorized, 3);

    let tombstone_uuids: Vec<String> = db
        .with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT entity_key FROM op_outbox
                 WHERE op_type = 'folder_deleted'
                 ORDER BY op_id",
            )?;
            let uuids = stmt
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(uuids)
        })
        .unwrap();
    let expected_uuids: Vec<String> = deleted
        .deleted_folders
        .iter()
        .map(|folder| folder.uuid.clone().expect("folder UUID"))
        .collect();
    assert_eq!(tombstone_uuids, expected_uuids);
}

#[test]
fn folder_count_matches_grid_active_visibility() {
    let db = open_test_db();
    let folder_id = db.create_folder("Visible", None, None, None).unwrap();

    let insert = |hash: &str, status: i64| {
        let file_id = db
            .insert_file(
                &format!("{hash}-file"),
                "image/png",
                10,
                None,
                None,
                None,
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        db.insert_entity(
            hash,
            file_id,
            Some(hash),
            status,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap()
    };

    let first = insert("folder-count-first", 1);
    let second = insert("folder-count-second", 1);
    let standalone = insert("folder-count-standalone", 1);
    let inbox = insert("folder-count-inbox", 0);
    let trash = insert("folder-count-trash", 2);
    db.add_folder_members(folder_id, &[first, second, standalone, inbox, trash])
        .unwrap();
    db.add_tags(&[first], &["general:categorized".to_string()], 1)
        .unwrap();

    let page = db
        .query_entity_view(&EntityViewQuery {
            base_scope: BaseScope {
                kind: ScopeKind::Folder,
                key: None,
                id: Some(folder_id),
            },
            filters: QueryFilters::default(),
            sort: QuerySort::default(),
            page: QueryPage::default(),
        })
        .unwrap();
    assert_eq!(page.total_count, Some(3));
    assert_eq!(page.items.len(), 3);
    assert_eq!(db.get_folder_visible_count(folder_id).unwrap(), 3);

    db.full_rebuild();
    db.with_read(|conn| {
        let sidebar_count: i64 = conn.query_row(
            "SELECT count FROM sidebar_node WHERE node_id = ?1",
            [format!("folder:{folder_id}")],
            |row| row.get(0),
        )?;
        assert_eq!(sidebar_count, 3);
        Ok(())
    })
    .unwrap();

    let scope_counts = db.get_scope_counts().unwrap();
    for (scope_key, sidebar_key, expected) in [
        ("all", "active", scope_counts.active),
        ("inbox", "inbox", scope_counts.inbox),
        ("trash", "trash", scope_counts.trash),
        ("uncategorized", "uncategorized", scope_counts.uncategorized),
        ("untagged", "untagged", scope_counts.untagged),
    ] {
        let page = db
            .query_entity_view(&EntityViewQuery {
                base_scope: BaseScope {
                    kind: ScopeKind::System,
                    key: Some(scope_key.to_string()),
                    id: None,
                },
                filters: QueryFilters::default(),
                sort: QuerySort::default(),
                page: QueryPage::default(),
            })
            .unwrap();
        assert_eq!(
            page.total_count,
            Some(expected),
            "grid count for {scope_key}"
        );
        db.with_read(|conn| {
            let sidebar_count: i64 = conn.query_row(
                "SELECT count FROM sidebar_node WHERE node_id = ?1",
                [format!("system:{sidebar_key}")],
                |row| row.get(0),
            )?;
            assert_eq!(sidebar_count, expected, "sidebar count for {scope_key}");
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn folder_grid_pages_by_position_rank_without_capping_the_scope() {
    let db = open_test_db();
    let folder_id = db.create_folder("Paged", None, None, None).unwrap();
    let mut entity_ids = Vec::new();
    for index in 1..=5 {
        let hash = format!("folder-page-{index}");
        let file_id = db
            .insert_file(
                &format!("{hash}-file"),
                "image/png",
                10,
                None,
                None,
                None,
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        entity_ids.push(
            db.insert_entity(&hash, file_id, Some(&hash), 1, "2026-08-04", "2026-08-04")
                .unwrap(),
        );
    }
    db.add_folder_members(folder_id, &entity_ids).unwrap();

    let query = |cursor: Option<String>| EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::Folder,
            key: None,
            id: Some(folder_id),
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage { limit: 2, cursor },
    };

    let first = db.query_entity_view(&query(None)).unwrap();
    let second = db
        .query_entity_view(&query(first.next_cursor.clone()))
        .unwrap();
    let third = db
        .query_entity_view(&query(second.next_cursor.clone()))
        .unwrap();

    assert_eq!(first.total_count, Some(5));
    assert_eq!(second.total_count, None);
    assert_eq!(second.total_size_bytes, None);
    assert_eq!(third.total_count, None);
    assert_eq!(
        first
            .items
            .iter()
            .chain(&second.items)
            .chain(&third.items)
            .map(|item| item.entity_hash.as_str())
            .collect::<Vec<_>>(),
        vec![
            "folder-page-1",
            "folder-page-2",
            "folder-page-3",
            "folder-page-4",
            "folder-page-5",
        ]
    );
    assert!(first.next_cursor.is_some());
    assert!(second.next_cursor.is_some());
    assert!(third.next_cursor.is_none());
}

#[test]
fn query_target_aggregate_excludes_without_materializing_grid_rows() {
    let db = open_test_db();
    for (index, mime, size, rating) in [
        (1, "image/png", 10, 1),
        (2, "image/png", 20, 1),
        (3, "video/mp4", 30, 2),
    ] {
        let file_id = db
            .insert_file(
                &format!("aggregate-file-{index}"),
                mime,
                size,
                None,
                None,
                None,
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        let entity_id = db
            .insert_entity(
                &format!("aggregate-{index}"),
                file_id,
                Some(&format!("Aggregate {index}")),
                1,
                "2026-08-04",
                "2026-08-04",
            )
            .unwrap();
        db.patch_entity_metadata(
            &[entity_id],
            &MediaEntityPatch {
                rating: Some(rating),
                ..MediaEntityPatch::default()
            },
        )
        .unwrap();
    }
    let query = EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::System,
            key: Some("all".into()),
            id: None,
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage::default(),
    };
    let aggregate = db
        .query_target_aggregate(&query, &["aggregate-3".into()])
        .unwrap();
    assert_eq!(aggregate.total_count, 3);
    assert_eq!(aggregate.selected_count, 2);
    assert_eq!(aggregate.entity_ids.len(), 2);
    assert_eq!(aggregate.total_size_bytes, 30);
    assert_eq!(aggregate.mime_counts.get("image/png"), Some(&2));
    assert_eq!(aggregate.shared_rating, Some(1));
}

#[test]
fn query_target_mutations_execute_against_the_temp_target() {
    let db = open_test_db();
    let mut ids = Vec::new();
    for index in 1..=3 {
        let file_id = db
            .insert_file(
                &format!("target-file-{index}"),
                "image/png",
                10,
                None,
                None,
                None,
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        ids.push(
            db.insert_entity(
                &format!("target-{index}"),
                file_id,
                Some(&format!("Target {index}")),
                1,
                "2026-08-04",
                "2026-08-04",
            )
            .unwrap(),
        );
    }
    let query = EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::System,
            key: Some("all".into()),
            id: None,
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage::default(),
    };
    let excluded = vec!["target-3".to_string()];
    db.patch_entity_metadata_bulk(
        &query,
        &excluded,
        &MediaEntityPatch {
            rating: Some(4),
            ..MediaEntityPatch::default()
        },
    )
    .unwrap();
    db.add_tags_bulk(
        &query,
        &excluded,
        &["bulk:test".into()],
        TAG_PROVENANCE_MANUAL,
    )
    .unwrap();
    let folder_id = db.create_folder("Bulk", None, None, None).unwrap();
    db.add_folder_members_bulk(folder_id, &query, &excluded)
        .unwrap();

    db.with_read(|conn| {
        let rated: i64 = conn.query_row(
            "SELECT COUNT(*) FROM media_entity WHERE rating = 4",
            [],
            |row| row.get(0),
        )?;
        let tagged: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_tag", [], |row| row.get(0))?;
        let members: i64 = conn.query_row(
            "SELECT COUNT(*) FROM folder_member WHERE folder_id = ?1",
            [folder_id],
            |row| row.get(0),
        )?;
        assert_eq!((rated, tagged, members), (2, 2, 2));
        Ok(())
    })
    .unwrap();

    db.remove_tags_bulk(&query, &excluded, &["bulk:test".into()])
        .unwrap();
    db.remove_folder_members_bulk(folder_id, &query, &excluded)
        .unwrap();
    db.set_entity_status_bulk(&query, &excluded, 2).unwrap();
    db.with_read(|conn| {
        let remaining_tags: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_tag", [], |row| row.get(0))?;
        let remaining_members: i64 =
            conn.query_row("SELECT COUNT(*) FROM folder_member", [], |row| row.get(0))?;
        let trashed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM media_entity WHERE status = 2",
            [],
            |row| row.get(0),
        )?;
        assert_eq!((remaining_tags, remaining_members, trashed), (0, 0, 2));
        Ok(())
    })
    .unwrap();
}

#[test]
fn typed_cursors_page_every_supported_grid_sort_without_gaps() {
    let db = open_test_db();
    for index in 0..7_i64 {
        let file_id = db
            .insert_file(
                &format!("cursor-file-{index}"),
                "image/png",
                10 + index * 7,
                None,
                None,
                (index % 3 != 0).then_some(index * 100),
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        let entity_id = db
            .insert_entity(
                &format!("cursor-{index}"),
                file_id,
                Some(&format!("Name {}", 6 - index)),
                1,
                &format!("2026-08-{:02}", index + 1),
                &format!("2026-09-{:02}", index + 1),
            )
            .unwrap();
        db.with_write(|conn| {
            conn.execute(
                "UPDATE media_entity
                 SET rating = ?1, date_modified = ?2
                 WHERE entity_id = ?3",
                params![
                    (index % 4 != 0).then_some(index % 5),
                    format!("2026-10-{:02}", index + 1),
                    entity_id,
                ],
            )?;
            Ok(())
        })
        .unwrap();
    }

    for field in [
        "name",
        "rating",
        "size_bytes",
        "duration",
        "duration_ms",
        "date_added",
        "date_created",
        "date_modified",
    ] {
        for direction in ["asc", "desc"] {
            let query = |limit, cursor| EntityViewQuery {
                base_scope: BaseScope {
                    kind: ScopeKind::System,
                    key: Some("all".into()),
                    id: None,
                },
                filters: QueryFilters::default(),
                sort: QuerySort {
                    field: field.into(),
                    direction: direction.into(),
                },
                page: QueryPage { limit, cursor },
            };
            let expected = db
                .query_entity_view(&query(100, None))
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.entity_hash)
                .collect::<Vec<_>>();
            let mut actual = Vec::new();
            let mut cursor = None;
            loop {
                let page = db.query_entity_view(&query(2, cursor)).unwrap();
                actual.extend(page.items.into_iter().map(|item| item.entity_hash));
                let Some(next) = page.next_cursor else { break };
                cursor = Some(next);
            }
            assert_eq!(actual, expected, "{field} {direction}");
        }
    }
}

#[test]
#[ignore = "explicit million-entity performance verification"]
fn million_entity_smart_scope_summary_and_bulk_membership_stay_db_backed() {
    const ENTITY_COUNT: i64 = 1_000_000;
    let db = open_test_db();
    let seed_started = Instant::now();
    db.with_write(|conn| {
        conn.execute_batch(
            "WITH RECURSIVE digit(value) AS (
                 VALUES(0) UNION ALL SELECT value + 1 FROM digit WHERE value < 999
             )
             INSERT INTO media_file(file_id, file_hash, mime_type, size_bytes, date_added)
             SELECT first.value * 1000 + second.value + 1,
                    printf('million-file-%07d', first.value * 1000 + second.value + 1),
                    'image/jpeg', 1, '2026-08-04'
             FROM digit first CROSS JOIN digit second;

             WITH RECURSIVE digit(value) AS (
                 VALUES(0) UNION ALL SELECT value + 1 FROM digit WHERE value < 999
             )
             INSERT INTO media_entity(
                 entity_id, entity_hash, file_id, status, name,
                 date_created, date_added, date_modified
             )
             SELECT first.value * 1000 + second.value + 1,
                    printf('million-entity-%07d', first.value * 1000 + second.value + 1),
                    first.value * 1000 + second.value + 1,
                    1,
                    printf('Entity %07d', first.value * 1000 + second.value + 1),
                    '2026-08-04', '2026-08-04', '2026-08-04'
             FROM digit first CROSS JOIN digit second;",
        )?;
        Ok(())
    })
    .unwrap();
    eprintln!("million seed: {:?}", seed_started.elapsed());

    db.bitmaps.set(
        crate::db::projection::bitmaps::BitmapKey::SmartFolder(1),
        roaring::RoaringBitmap::from_iter(1..=ENTITY_COUNT as u32),
    );
    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO smart_folder(
                 smart_folder_id, name, predicate_json, total_size_bytes,
                 date_added, date_modified
             ) VALUES (1, 'Million', '{}', ?1, '2026-08-04', '2026-08-04')",
            [ENTITY_COUNT],
        )?;
        Ok(())
    })
    .unwrap();
    let smart_query = EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::SmartFolder,
            key: None,
            id: Some(1),
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage {
            limit: 500,
            cursor: None,
        },
    };
    let page_started = Instant::now();
    let page = db.query_entity_view(&smart_query).unwrap();
    eprintln!("million smart page + count: {:?}", page_started.elapsed());
    assert_eq!(page.total_count, Some(ENTITY_COUNT));
    assert_eq!(page.items.len(), 500);

    let summary_started = Instant::now();
    let aggregate = db.query_target_aggregate(&smart_query, &[]).unwrap();
    eprintln!("million target summary: {:?}", summary_started.elapsed());
    assert_eq!(aggregate.total_count, ENTITY_COUNT);
    assert_eq!(aggregate.selected_count, ENTITY_COUNT);
    assert_eq!(aggregate.entity_ids.len(), ENTITY_COUNT as u64);

    let folder_id = db.create_folder("Million", None, None, None).unwrap();
    let mutation_started = Instant::now();
    db.with_write(|conn| {
        super::write::bulk::populate_bulk_target(conn, &smart_query, &[])?;
        let change = super::write::bulk::add_folder_members_to_target(conn, folder_id)?;
        assert_eq!(change.entity_ids.len(), ENTITY_COUNT as usize);
        Ok(())
    })
    .unwrap();
    eprintln!("million bulk membership: {:?}", mutation_started.elapsed());
    db.with_read(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM folder_member WHERE folder_id = ?1",
            [folder_id],
            |row| row.get(0),
        )?;
        assert_eq!(count, ENTITY_COUNT);
        Ok(())
    })
    .unwrap();
}

#[test]
fn folder_cover_hashes_batch_returns_populated_and_empty_folders() {
    let db = open_test_db();
    let populated_id = db.create_folder("Populated", None, None, None).unwrap();
    let empty_id = db.create_folder("Empty", None, None, None).unwrap();
    let file_id = db
        .insert_file(
            "folder-cover-file",
            "image/png",
            10,
            None,
            None,
            None,
            None,
            false,
            "2026-08-04",
        )
        .unwrap();
    let entity_id = db
        .insert_entity(
            "folder-cover-entity",
            file_id,
            Some("Cover"),
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    db.add_folder_members(populated_id, &[entity_id]).unwrap();

    assert_eq!(
        db.get_folder_cover_hashes(&[empty_id, populated_id])
            .unwrap(),
        vec![
            (populated_id, Some("folder-cover-entity".to_string())),
            (empty_id, None),
        ]
    );
}

#[test]
fn tag_counts_match_visible_tag_scopes() {
    let db = open_test_db();
    let insert = |hash: &str, status: i64| {
        let file_id = db
            .insert_file(
                &format!("{hash}-file"),
                "image/png",
                10,
                None,
                None,
                None,
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        db.insert_entity(
            hash,
            file_id,
            Some(hash),
            status,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap()
    };
    let add_tag = |entity_id: i64, tag: &str| {
        db.add_tags(&[entity_id], &[tag.to_string()], 1).unwrap();
    };
    let tag_scope = |tag: &str| EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::Tag,
            key: Some(tag.to_string()),
            id: None,
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage::default(),
    };

    let standalone = insert("tag-count-standalone", 1);
    let first_child = insert("tag-count-child-one", 1);
    let second_child = insert("tag-count-child-two", 1);
    let inbox = insert("tag-count-inbox", 0);
    let trash = insert("tag-count-trash", 2);
    add_tag(standalone, "test:visible");
    add_tag(first_child, "test:visible");
    add_tag(second_child, "test:visible");
    add_tag(inbox, "test:visible");
    add_tag(trash, "test:visible");
    let alias_entity = insert("tag-count-alias", 1);
    add_tag(alias_entity, "test:canonical");
    let alias_seed = insert("tag-count-alias-seed", 0);
    add_tag(alias_seed, "test:alias");
    let canonical_id = db.find_tag_id("test:canonical").unwrap().unwrap();
    let alias_id = db.find_tag_id("test:alias").unwrap().unwrap();
    db.manage_tag_alias(alias_id, Some(canonical_id)).unwrap();

    let implication_child_entity = insert("tag-count-implication-child", 1);
    let implication_parent_entity = insert("tag-count-implication-parent", 1);
    add_tag(implication_child_entity, "test:child");
    add_tag(implication_parent_entity, "test:parent");
    let child_tag_id = db.find_tag_id("test:child").unwrap().unwrap();
    let parent_tag_id = db.find_tag_id("test:parent").unwrap().unwrap();
    db.manage_tag_implication(child_tag_id, parent_tag_id, true)
        .unwrap();

    let zero_count_entity = insert("tag-count-zero", 0);
    add_tag(zero_count_entity, "test:zero");
    let general_one_girl = db.ensure_tag("1girl").unwrap();
    let character_one_girl = db.ensure_tag("character:1girl").unwrap();
    db.full_rebuild();

    let records = db.get_tags_paginated(None, None, None, 100).unwrap().items;
    let count = |tag: &str| {
        let (namespace, subtag) = tag.split_once(':').unwrap();
        records
            .iter()
            .find(|record| record.namespace == namespace && record.subtag == subtag)
            .unwrap()
            .file_count
    };
    let assert_scope_count = |tag: &str, expected: i64| {
        let page = db.query_entity_view(&tag_scope(tag)).unwrap();
        assert_eq!(page.total_count, Some(expected), "grid count for {tag}");
        assert_eq!(count(tag), expected, "tag list count for {tag}");
    };

    assert_scope_count("test:visible", 3);
    assert_scope_count("test:canonical", 1);
    assert_scope_count("test:alias", 1);
    assert_scope_count("test:child", 1);
    assert_scope_count("test:parent", 2);
    assert_scope_count("test:zero", 0);

    let visible_tag_id = db.find_tag_id("test:visible").unwrap().unwrap();
    let visible = db
        .bitmaps
        .get(&crate::db::projection::bitmaps::BitmapKey::EffectiveTag(
            visible_tag_id,
        ));
    assert!(visible.contains(standalone as u32));
    assert!(visible.contains(first_child as u32));
    assert!(visible.contains(second_child as u32));
    assert!(db
        .bitmaps
        .get(&crate::db::projection::bitmaps::BitmapKey::EffectiveTag(
            alias_id,
        ))
        .contains(alias_entity as u32));

    let zero_tag_id = db.find_tag_id("test:zero").unwrap().unwrap();
    assert!(db
        .get_all_tag_keys()
        .unwrap()
        .iter()
        .any(|tag| tag.0 == zero_tag_id));
    for query in ["1girl", "general:1girl"] {
        let matches = db
            .get_tags_paginated(None, Some(query.to_string()), None, 100)
            .unwrap()
            .items;
        assert_eq!(matches.len(), 2, "picker search for {query}");
        assert!(matches.iter().any(|tag| tag.tag_id == general_one_girl));
        assert!(matches.iter().any(|tag| tag.tag_id == character_one_girl));
    }
    assert!(db
        .get_tags_paginated(
            Some("general".to_string()),
            Some("1girl".to_string()),
            None,
            100,
        )
        .unwrap()
        .items
        .iter()
        .all(|tag| tag.namespace == "general"));
    assert!(db
        .get_tags_paginated(None, Some("general".to_string()), None, 100)
        .unwrap()
        .items
        .is_empty());

    db.with_read(|conn| {
        let columns = conn
            .prepare("PRAGMA table_info(tag)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert!(
            !columns.iter().any(|column| column == "file_count"),
            "tag counts must not be stored separately"
        );
        assert!(
            !columns.iter().any(|column| column == "site_mask"),
            "source-specific tag masks are not part of the product model"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn paginated_tags_have_stable_cursors_and_keep_zero_count_tags_visible() {
    let db = open_test_db();
    let expected = [
        db.ensure_tag("general:alpha").unwrap(),
        db.ensure_tag("general:alphabet").unwrap(),
        db.ensure_tag("general:beta").unwrap(),
        db.ensure_tag("character:alpha").unwrap(),
        db.ensure_tag("character:beta").unwrap(),
        db.ensure_tag("zero:alpha").unwrap(),
    ];

    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = db
            .get_tags_paginated(None, None, cursor.clone(), 2)
            .unwrap();
        ids.extend(page.items.iter().map(|tag| tag.tag_id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(ids.len(), expected.len());
    assert!(ids
        .iter()
        .all(|id| ids.iter().filter(|other| *other == id).count() == 1));
    assert!(
        ids.contains(&expected[5]),
        "zero-count tags must be paginated"
    );

    let first = db.get_tags_paginated(None, None, None, 2).unwrap();
    let first_json = serde_json::to_value(&first).unwrap();
    assert!(first_json.get("items").is_some());
    assert!(first_json.get("next_cursor").is_some());
    assert!(first.next_cursor.is_some());
    let second = db
        .get_tags_paginated(None, None, first.next_cursor.clone(), 2)
        .unwrap();
    assert_eq!(first.items.len(), 2);
    assert_eq!(second.items.len(), 2);
    assert!(first
        .items
        .iter()
        .all(|tag| !second.items.iter().any(|other| other.tag_id == tag.tag_id)));

    let minimum_page = db.get_tags_paginated(None, None, None, 0).unwrap();
    assert_eq!(minimum_page.items.len(), 1);
    assert!(minimum_page.next_cursor.is_some());

    let character_page = db
        .get_tags_paginated(Some("character".to_string()), None, None, 1)
        .unwrap();
    assert_eq!(character_page.items.len(), 1);
    assert_eq!(character_page.items[0].namespace, "character");
    let character_next = db
        .get_tags_paginated(
            Some("character".to_string()),
            None,
            character_page.next_cursor,
            1,
        )
        .unwrap();
    assert_eq!(character_next.items.len(), 1);
    assert_ne!(
        character_page.items[0].tag_id,
        character_next.items[0].tag_id
    );

    let search_page = db
        .get_tags_paginated(None, Some("character:alpha".to_string()), None, 1)
        .unwrap();
    assert_eq!(search_page.items.len(), 1);
    let search_next = db
        .get_tags_paginated(
            None,
            Some("character:alpha".to_string()),
            search_page.next_cursor,
            1,
        )
        .unwrap();
    assert_eq!(search_next.items.len(), 1);
    assert_ne!(search_page.items[0].tag_id, search_next.items[0].tag_id);

    let filtered_zero = db
        .get_tags_paginated(
            Some("zero".to_string()),
            Some("alpha".to_string()),
            None,
            10,
        )
        .unwrap();
    assert_eq!(filtered_zero.items.len(), 1);
    assert_eq!(filtered_zero.items[0].file_count, 0);
}

#[test]
fn recently_viewed_is_unique_active_and_latest_first() {
    let db = open_test_db();
    let insert = |hash: &str, status: i64| {
        let file_id = db
            .insert_file(
                &format!("{hash}-file"),
                "image/png",
                10,
                None,
                None,
                None,
                None,
                false,
                "2026-08-04",
            )
            .unwrap();
        db.insert_entity(
            hash,
            file_id,
            Some(hash),
            status,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap()
    };

    let first = insert("recent-first", 1);
    let second = insert("recent-second", 1);
    insert("recent-inbox", 0);
    insert("recent-trash", 2);

    assert_eq!(
        db.record_media_view("recent-first").unwrap(),
        ("recent-first".to_string(), 1)
    );
    assert_eq!(
        db.record_media_view("recent-second").unwrap(),
        ("recent-second".to_string(), 2)
    );
    assert_eq!(
        db.record_media_view("recent-first").unwrap(),
        ("recent-first".to_string(), 2)
    );
    db.record_media_view("recent-inbox").unwrap();
    db.record_media_view("recent-trash").unwrap();
    assert!(db.record_media_view("missing-recent").is_err());

    db.with_write(|conn| {
        conn.execute(
            "UPDATE media_view SET viewed_at = '2026-08-04T10:00:00Z' WHERE entity_id = ?1",
            [first],
        )?;
        conn.execute(
            "UPDATE media_view SET viewed_at = '2026-08-04T11:00:00Z' WHERE entity_id = ?1",
            [second],
        )?;
        Ok(())
    })
    .unwrap();

    let query = |cursor: Option<String>| EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::System,
            key: Some("recent_viewed".to_string()),
            id: None,
        },
        filters: QueryFilters::default(),
        sort: QuerySort {
            field: "name".to_string(),
            direction: "asc".to_string(),
        },
        page: QueryPage { limit: 1, cursor },
    };
    let first_page = db.query_entity_view(&query(None)).unwrap();
    assert_eq!(first_page.total_count, Some(2));
    assert_eq!(first_page.items[0].entity_hash, "recent-second");
    let second_page = db
        .query_entity_view(&query(first_page.next_cursor.clone()))
        .unwrap();
    assert_eq!(second_page.items[0].entity_hash, "recent-first");

    db.full_rebuild();
    db.with_read(|conn| {
        let sidebar_count: i64 = conn.query_row(
            "SELECT count FROM sidebar_node WHERE node_id = 'system:recent_viewed'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(sidebar_count, 2);
        Ok(())
    })
    .unwrap();
}

#[test]
fn with_write_rolls_back_every_statement_on_error() {
    let db = open_test_db();

    let result: Result<(), String> = db.with_write(|conn| {
        conn.execute(
            "INSERT INTO tag (namespace, subtag) VALUES ('test', 'rollback_a')",
            [],
        )?;
        conn.execute(
            "INSERT INTO tag (namespace, subtag) VALUES ('test', 'rollback_b')",
            [],
        )?;
        Err(rusqlite::Error::InvalidQuery)
    });
    assert!(result.is_err());

    let count: i64 = db
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tag WHERE namespace = 'test'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(count, 0, "failed write action must leave no partial rows");

    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO tag (namespace, subtag) VALUES ('test', 'commit_a')",
            [],
        )
        .map(|_| ())
    })
    .unwrap();
    let count: i64 = db
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tag WHERE namespace = 'test'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(count, 1, "successful write action must commit");
}

#[test]
fn get_file_colors_for_entity_hash_prefers_blob_and_falls_back_to_index() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex,
                dominant_palette_blob, color_analysis_version, date_added
             ) VALUES (1, 'file_blob', 'image/png', 1, 0, '#abcdef', ?1, ?2, '2026-04-01')",
            rusqlite::params![
                serialize_dominant_palette_blob(&[DominantColor {
                    hex: "#abcdef".into(),
                    l: 10.0,
                    a: 1.0,
                    b: 2.0,
                }])
                .expect("serialize palette"),
                TARGET_COLOR_ANALYSIS_VERSION
            ],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex,
                color_analysis_version, date_added
             ) VALUES (2, 'file_fallback', 'image/png', 1, 0, '#123456', 0, '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, file_id, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'with_blob', 1, 1, 'Blob', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'fallback', 2, 1, 'Fallback', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO file_color (file_id, hex, l, a, b) VALUES (2, '#fedcba', 20.0, 3.0, 4.0)",
            [],
        )?;
        Ok(())
    })
    .expect("seed db");

    let blob_colors = db
        .get_file_colors_for_entity_hash("with_blob")
        .expect("get blob colors");
    let fallback_colors = db
        .get_file_colors_for_entity_hash("fallback")
        .expect("get fallback colors");

    assert_eq!(blob_colors[0].0, "#abcdef");
    assert_eq!(fallback_colors[0].0, "#fedcba");
}

#[test]
fn fresh_library_starts_at_current_canonical_schema_version() {
    let tmp = TempDir::new().expect("tempdir");
    let db = LibraryDatabase::open(tmp.path()).expect("open fresh library");
    db.with_read(|conn| {
        let (row_count, version): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), MIN(version) FROM schema_version",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(row_count, 1);
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        Ok(())
    })
    .expect("inspect fresh schema");
}

#[test]
fn schema_117_migrates_subfolder_visibility_without_losing_preferences() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("library.db");
    let old_ddl = LIBRARY_DDL
        .replace(",\n    show_subfolders INTEGER DEFAULT 1", "")
        .replace("SELECT 118", "SELECT 117");
    let conn = rusqlite::Connection::open(&db_path).expect("open raw database");
    conn.execute_batch(&old_ddl).expect("create schema 117");
    conn.execute(
        "INSERT INTO view_pref (scope, layout, tile_size) VALUES ('folder:7', 'grid', 180)",
        [],
    )
    .expect("seed preference");
    drop(conn);

    let db = LibraryDatabase::open(tmp.path()).expect("migrate schema");
    db.with_read(|conn| {
        let (version, layout, tile_size, show_subfolders): (i64, String, i64, bool) = conn
            .query_row(
                "SELECT sv.version, vp.layout, vp.tile_size, vp.show_subfolders
                 FROM schema_version sv CROSS JOIN view_pref vp
                 WHERE vp.scope = 'folder:7'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert_eq!(layout, "grid");
        assert_eq!(tile_size, 180);
        assert!(show_subfolders);
        Ok(())
    })
    .expect("verify migrated preference");
}

#[test]
fn subscription_definitions_are_direct_and_have_no_group_schema() {
    let db = open_test_db();
    db.with_read(|conn| {
        let group_table_exists: i64 = conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'subscription_group'
             )",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(group_table_exists, 0);

        let subscription_columns: Vec<String> = conn
            .prepare("PRAGMA table_info(subscription)")?
            .query_map([], |row| row.get(1))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(!subscription_columns
            .iter()
            .any(|column| column == "group_id"));
        assert!(subscription_columns.iter().any(|column| column == "uuid"));
        Ok(())
    })
    .expect("verify direct subscription schema");
}

#[test]
fn canonical_schema_version_requires_exactly_one_row() {
    for (mutation, expected_count) in [
        ("DELETE FROM schema_version", 0),
        (
            "INSERT INTO schema_version (version) SELECT version FROM schema_version",
            2,
        ),
    ] {
        let tmp = TempDir::new().expect("tempdir");
        let conn =
            rusqlite::Connection::open(tmp.path().join("library.db")).expect("open raw database");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute_batch(mutation).expect("malform version rows");
        drop(conn);

        let error = match LibraryDatabase::open(tmp.path()) {
            Ok(_) => panic!("schema with {expected_count} version rows must fail"),
            Err(error) => error,
        };
        assert!(
            error.contains(&format!("exactly one row; found {expected_count}")),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn pre_1_0_schema_mismatches_are_rejected_without_mutation() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("library.db");
    let conn = rusqlite::Connection::open(&db_path).expect("open raw database");
    conn.execute_batch(LIBRARY_DDL).expect("create schema");
    conn.execute("UPDATE schema_version SET version = 100", [])
        .expect("set old version");
    drop(conn);

    let error = match LibraryDatabase::open(tmp.path()) {
        Ok(_) => panic!("old pre-1.0 schema must fail"),
        Err(error) => error,
    };
    assert!(
        error.contains("requires exactly"),
        "unexpected error: {error}"
    );

    let conn = rusqlite::Connection::open(db_path).expect("reopen raw database");
    let version: i64 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .expect("read unchanged version");
    assert_eq!(version, 100, "open must not migrate or reset the database");
}

#[test]
fn unknown_nonempty_schema_is_rejected_without_mutation() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("library.db");
    let conn = rusqlite::Connection::open(&db_path).expect("open raw database");
    conn.execute_batch(
        "CREATE TABLE user_data (value TEXT); INSERT INTO user_data VALUES ('keep')",
    )
    .expect("create unknown schema");
    drop(conn);

    let error = match LibraryDatabase::open(tmp.path()) {
        Ok(_) => panic!("unknown pre-1.0 schema must fail"),
        Err(error) => error,
    };
    assert!(
        error.contains("schema_version is missing"),
        "unexpected error: {error}"
    );

    let conn = rusqlite::Connection::open(db_path).expect("reopen raw database");
    let value: String = conn
        .query_row("SELECT value FROM user_data", [], |row| row.get(0))
        .expect("read preserved data");
    assert_eq!(value, "keep");
}

#[test]
fn current_schema_validation_rejects_missing_tables_columns_and_indexes() {
    for (malformation, expected_error) in [
        ("DROP TABLE media_view", "media_view"),
        (
            "DROP TABLE media_file_phash_index",
            "media_file_phash_index",
        ),
        ("DROP TABLE sync_ingest_cursor", "sync_ingest_cursor"),
        ("ALTER TABLE folder DROP COLUMN pin_order", "folder"),
        (
            "DROP INDEX idx_media_view_viewed_at",
            "idx_media_view_viewed_at",
        ),
        ("DROP INDEX idx_mf_phash_p0", "idx_mf_phash_p0"),
        (
            "DROP INDEX idx_ingest_queue_ready",
            "idx_ingest_queue_ready",
        ),
        ("DROP INDEX idx_entity_tag_tag_id", "idx_entity_tag_tag_id"),
        (
            "DROP INDEX idx_entity_tag_implied_tag_id",
            "idx_entity_tag_implied_tag_id",
        ),
        (
            "DROP INDEX idx_folder_member_entity_id",
            "idx_folder_member_entity_id",
        ),
        (
            "DROP INDEX idx_subscription_issue_key",
            "idx_subscription_issue_key",
        ),
        (
            "DROP INDEX idx_subscription_query_uuid",
            "idx_subscription_query_uuid",
        ),
    ] {
        let tmp = TempDir::new().expect("tempdir");
        let conn =
            rusqlite::Connection::open(tmp.path().join("library.db")).expect("open raw database");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute_batch(malformation)
            .expect("malform current schema");
        drop(conn);

        let error = match LibraryDatabase::open(tmp.path()) {
            Ok(_) => panic!("malformed current schema must fail"),
            Err(error) => error,
        };
        assert!(
            error.contains(expected_error),
            "unexpected error for {malformation}: {error}"
        );
    }
}

#[test]
fn current_schema_validation_rejects_wrong_required_index_definition() {
    for (replacement, index_name) in [
        (
            "DROP INDEX idx_entity_tag_tag_id;
             CREATE INDEX idx_entity_tag_tag_id ON entity_tag(entity_id, tag_id);",
            "idx_entity_tag_tag_id",
        ),
        (
            "DROP INDEX idx_folder_uuid;
             CREATE INDEX idx_folder_uuid ON folder(uuid);",
            "idx_folder_uuid",
        ),
        (
            "DROP INDEX idx_folder_uuid;
             CREATE UNIQUE INDEX idx_folder_uuid ON folder(uuid) WHERE uuid <> '';",
            "idx_folder_uuid",
        ),
    ] {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("library.db");
        let conn = rusqlite::Connection::open(&db_path).expect("open raw database");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute_batch(replacement)
            .expect("replace index with malformed definition");
        drop(conn);

        let error = match LibraryDatabase::open(tmp.path()) {
            Ok(_) => panic!("malformed index must fail"),
            Err(error) => error,
        };
        assert!(
            error.contains(index_name) && error.contains("incompatible"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn current_schema_validation_rejects_missing_sync_state_constraint() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("library.db");
    let malformed = LIBRARY_DDL.replace(
        "status        TEXT NOT NULL CHECK (status IN ('pending', 'failed'))",
        "status        TEXT NOT NULL",
    );
    assert_ne!(malformed, LIBRARY_DDL);
    let conn = rusqlite::Connection::open(&db_path).expect("open raw database");
    conn.execute_batch(&malformed)
        .expect("create malformed schema");
    drop(conn);

    let error = match LibraryDatabase::open(tmp.path()) {
        Ok(_) => panic!("schema without sync status constraint must fail"),
        Err(error) => error,
    };
    assert!(
        error.contains("sync_missing_blob status constraints"),
        "unexpected error: {error}"
    );
}

#[test]
fn fresh_schema_creates_hot_reverse_indexes() {
    let db = open_test_db();
    db.with_read(|conn| {
        for (name, table, columns) in [
            (
                "idx_folder_member_entity_id",
                "folder_member",
                "entity_id, folder_id",
            ),
            ("idx_entity_tag_tag_id", "entity_tag", "tag_id, entity_id"),
            (
                "idx_entity_tag_implied_tag_id",
                "entity_tag_implied",
                "tag_id, entity_id",
            ),
        ] {
            let sql: String = conn.query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [name],
                |row| row.get(0),
            )?;
            assert!(
                sql.contains(&format!("ON {table}({columns})")),
                "unexpected SQL: {sql}"
            );
        }
        Ok(())
    })
    .expect("read canonical reverse indexes");
}

#[test]
fn subscription_issue_keys_deduplicate_query_and_global_recurrences() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, uuid, date_added)
             VALUES (1, 'Test subscription', 'subscription-issue-test', '2026-08-05')",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, uuid, site_id, query_text)
             VALUES (10, 1, 'query-issue-test', 'test', 'query')",
            [],
        )?;
        Ok(())
    })
    .expect("seed subscription issue owners");

    let query_issue_id = db
        .with_write(|conn| {
            upsert_subscription_issue(
                conn,
                1,
                Some(10),
                FailureKind::Network,
                "first query message",
                Some("first detail"),
            )
        })
        .expect("insert query issue")
        .expect("query issue id");
    let global_issue_id = db
        .with_write(|conn| {
            upsert_subscription_issue(
                conn,
                1,
                None,
                FailureKind::Network,
                "first global message",
                None,
            )
        })
        .expect("insert global issue")
        .expect("global issue id");

    db.with_write(|conn| {
        conn.execute(
            "UPDATE subscription_issue
             SET first_seen_at = 'first-seen', last_seen_at = 'old-seen',
                 status = 'resolved', resolved_at = 'old-resolved',
                 recovery_action = 'none', next_retry_at = 'tomorrow'
             WHERE issue_key IN ('query:10:network', 'subscription:1:network')",
            [],
        )?;
        Ok(())
    })
    .expect("set recurrence sentinel values");

    let repeated_query_issue_id = db
        .with_write(|conn| {
            upsert_subscription_issue(
                conn,
                1,
                Some(10),
                FailureKind::Network,
                "changed query message",
                Some("changed detail"),
            )
        })
        .expect("recur query issue")
        .expect("repeated query issue id");
    let repeated_global_issue_id = db
        .with_write(|conn| {
            upsert_subscription_issue(
                conn,
                1,
                None,
                FailureKind::Network,
                "changed global message",
                Some("changed detail"),
            )
        })
        .expect("recur global issue")
        .expect("repeated global issue id");

    assert_eq!(query_issue_id, repeated_query_issue_id);
    assert_eq!(global_issue_id, repeated_global_issue_id);

    db.with_read(|conn| {
        let read_issue = |key: &str| {
            conn.query_row(
                "SELECT issue_id, issue_key, query_id, status, message, detail,
                        first_seen_at, last_seen_at, resolved_at, recovery_action, next_retry_at
                 FROM subscription_issue
                 WHERE issue_key = ?1",
                [key],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, Option<String>>(10)?,
                    ))
                },
            )
        };

        let query = read_issue("query:10:network")?;
        assert_eq!(query.0, query_issue_id);
        assert_eq!(query.1, "query:10:network");
        assert_eq!(query.2, Some(10));
        assert_eq!(query.3, "open");
        assert_eq!(query.4, "changed query message");
        assert_eq!(query.5.as_deref(), Some("changed detail"));
        assert_eq!(query.6, "first-seen");
        assert_ne!(query.7, "old-seen");
        assert_eq!(query.8, None);
        assert_eq!(query.9, "retry_automatically");
        assert_eq!(query.10, None);

        let global = read_issue("subscription:1:network")?;
        assert_eq!(global.0, global_issue_id);
        assert_eq!(global.1, "subscription:1:network");
        assert_eq!(global.2, None);
        assert_eq!(global.4, "changed global message");
        assert_eq!(global.6, "first-seen");
        assert_ne!(global.7, "old-seen");

        let query_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM subscription_issue WHERE query_id = 10",
            [],
            |row| row.get(0),
        )?;
        let global_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM subscription_issue WHERE query_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(query_count, 1);
        assert_eq!(global_count, 1);
        Ok(())
    })
    .expect("verify subscription issue recurrence");
}

#[test]
fn non_issue_failure_kinds_do_not_create_subscription_issues() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, uuid, date_added)
             VALUES (1, 'Test subscription', 'subscription-failure-test', '2026-08-05')",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, uuid, site_id, query_text)
             VALUES (10, 1, 'query-failure-test', 'test', 'query')",
            [],
        )?;

        assert_eq!(
            upsert_subscription_issue(
                conn,
                1,
                Some(10),
                FailureKind::InboxFull,
                "Inbox is full",
                None,
            )?,
            None
        );
        assert_eq!(
            upsert_subscription_issue(
                conn,
                1,
                Some(10),
                FailureKind::Stale,
                "Run was interrupted",
                None,
            )?,
            None
        );

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM subscription_issue WHERE subscription_id = 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    })
    .expect("no issue rows for non-issue failure kinds");
}

#[test]
fn smart_folder_scope_query_matches_runtime_compiled_bitmap_and_sidebar_count() {
    let db = open_test_db();
    let first_file = db
        .insert_file(
            "smart-first-file",
            "image/png",
            100,
            Some(1000),
            Some(500),
            None,
            Some(1),
            false,
            "2026-04-01",
        )
        .unwrap();
    let first = db
        .insert_entity(
            "smart-first",
            first_file,
            Some("Landscape"),
            1,
            "2026-04-01",
            "2026-04-01",
        )
        .unwrap();
    let second_file = db
        .insert_file(
            "smart-second-file",
            "image/jpeg",
            100,
            Some(500),
            Some(1000),
            None,
            Some(1),
            false,
            "2026-04-02",
        )
        .unwrap();
    db.insert_entity(
        "smart-second",
        second_file,
        Some("Portrait"),
        1,
        "2026-04-02",
        "2026-04-02",
    )
    .unwrap();
    db.with_write(|conn| {
        conn.execute(
            "UPDATE media_entity SET rating = 5 WHERE entity_id = ?1",
            [first],
        )?;
        conn.execute(
            "INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'landscape')",
            [],
        )?;
        conn.execute(
            "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
             VALUES (?1, 1, 1, 'local')",
            [first],
        )?;
        conn.execute(
            "INSERT INTO file_color (file_id, hex, l, a, b)
             VALUES (?1, '#ff0000', 50.0, 60.0, 70.0)",
            [first_file],
        )?;
        conn.execute(
            "INSERT INTO smart_folder (
                smart_folder_id, name, predicate_json, date_added, date_modified
             ) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                7_i64,
                "Smart Landscape",
                serde_json::json!({
                    "groups": [{
                        "match_mode": "all",
                        "negate": false,
                        "rules": [
                            { "field": "tags", "op": "include_all", "values": ["landscape"] },
                            { "field": "color", "op": "contains", "values": ["#ff0000"] },
                            { "field": "rating", "op": "gte", "value": 4 }
                        ]
                    }]
                })
                .to_string(),
                "2026-04-01T00:00:00Z",
            ],
        )?;
        Ok(())
    })
    .expect("seed smart folder data");

    db.full_rebuild();
    let query = EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::SmartFolder,
            key: None,
            id: Some(7),
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage::default(),
    };
    let page = db
        .query_entity_view(&query)
        .expect("query smart folder scope");
    assert_eq!(page.total_count, Some(1));
    assert_eq!(page.items[0].entity_hash, "smart-first");
    assert_eq!(
        db.bitmap_len(&crate::db::projection::bitmaps::BitmapKey::SmartFolder(7)),
        1
    );
    db.with_read(|conn| {
        let count: i64 = conn.query_row(
            "SELECT count FROM sidebar_node WHERE node_id = 'smart:7'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    })
    .expect("read smart-folder sidebar count");

    db.remove_tags(&[first], &["landscape".to_string()])
        .unwrap();
    db.run_compiler(crate::db::projection::compiler::CompilerPlan {
        dirty_tag_ids: vec![1],
        rebuild_all_smart_folders: true,
        rebuild_sidebar: true,
        ..Default::default()
    });
    assert_eq!(db.query_entity_view(&query).unwrap().total_count, Some(0));

    db.add_tags(&[first], &["landscape".to_string()], 1)
        .unwrap();
    db.run_compiler(crate::db::projection::compiler::CompilerPlan {
        dirty_tag_ids: vec![1],
        rebuild_all_smart_folders: true,
        rebuild_sidebar: true,
        ..Default::default()
    });
    assert_eq!(db.query_entity_view(&query).unwrap().total_count, Some(1));

    db.set_entity_status(&[first], 2).unwrap();
    db.run_compiler(crate::db::projection::compiler::CompilerPlan {
        rebuild_status: true,
        rebuild_sidebar: true,
        ..Default::default()
    });
    assert_eq!(db.query_entity_view(&query).unwrap().total_count, Some(0));
}

#[test]
fn run_compiler_waits_for_the_serialized_write_connection() {
    let db = Arc::new(open_test_db());
    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let write_guard = db.write_conn.lock().unwrap();
    let compiler_db = Arc::clone(&db);
    let handle = std::thread::spawn(move || {
        started_tx.send(()).expect("signal compiler start");
        compiler_db.run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
        finished_tx.send(()).expect("signal compiler completion");
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("compiler thread started");
    let completed_while_writer_locked =
        finished_rx.recv_timeout(Duration::from_millis(100)).is_ok();
    drop(write_guard);

    let completed_after_release = finished_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    handle.join().expect("compiler thread completed");

    assert!(
        !completed_while_writer_locked,
        "compiler must wait for the serialized write connection"
    );
    assert!(completed_after_release);
}

#[test]
fn full_rebuild_waits_for_the_serialized_write_connection() {
    let db = Arc::new(open_test_db());
    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let write_guard = db.write_conn.lock().unwrap();
    let rebuild_db = Arc::clone(&db);
    let handle = std::thread::spawn(move || {
        started_tx.send(()).expect("signal rebuild start");
        rebuild_db.full_rebuild();
        finished_tx.send(()).expect("signal rebuild completion");
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("rebuild thread started");
    let completed_while_writer_locked =
        finished_rx.recv_timeout(Duration::from_millis(100)).is_ok();
    drop(write_guard);

    let completed_after_release = finished_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    handle.join().expect("rebuild thread completed");

    assert!(
        !completed_while_writer_locked,
        "full rebuild must wait for the serialized write connection"
    );
    assert!(completed_after_release);
}

fn seed_duplicate_entity(
    conn: &Connection,
    entity_id: i64,
    entity_hash: &str,
    file_id: i64,
    file_hash: &str,
    status: i64,
    name: Option<&str>,
    notes: Option<&str>,
    source_urls_json: Option<&str>,
    rating: Option<i64>,
    date_created: &str,
    date_added: &str,
    mime_type: &str,
    size_bytes: i64,
    pixel_width: i64,
    pixel_height: i64,
    frame_count: i64,
    perceptual_hash: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO media_file (
             file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height,
             frame_count, has_audio, perceptual_hash, date_added
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
        params![
            file_id,
            file_hash,
            mime_type,
            size_bytes,
            pixel_width,
            pixel_height,
            frame_count,
            perceptual_hash,
            date_added
        ],
    )?;
    conn.execute(
        "INSERT INTO media_entity (
             entity_id, entity_hash, file_id, status, name, notes, rating, source_urls_json,
             date_created, date_added, date_modified
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            entity_id,
            entity_hash,
            file_id,
            status,
            name,
            notes,
            rating,
            source_urls_json,
            date_created,
            date_added
        ],
    )?;
    Ok(())
}

fn seed_duplicate_pair(
    conn: &Connection,
    left_file_id: i64,
    right_file_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (?1, ?2, 0)",
        params![
            left_file_id.min(right_file_id),
            left_file_id.max(right_file_id)
        ],
    )?;
    Ok(())
}

#[test]
fn duplicate_resolution_merges_metadata_and_repoints_all_entity_references() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "winner",
            1,
            "winner_file",
            1,
            None,
            Some("winner note"),
            Some(r#"["https://one.example"]"#),
            Some(2),
            "2026-04-01",
            "2026-04-02",
            "image/png",
            200,
            200,
            200,
            1,
            Some("winner-phash"),
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "loser",
            2,
            "loser_file",
            0,
            Some("recovered name"),
            Some("loser note"),
            Some(r#"["https://two.example","https://one.example"]"#),
            Some(4),
            "2026-04-03",
            "2026-04-04",
            "image/jpeg",
            100,
            100,
            100,
            1,
            Some("loser-phash"),
        )?;
        conn.execute(
            "INSERT INTO folder (folder_id, name, date_added, date_modified)
             VALUES (10, 'Shared', '2026-04-01', '2026-04-01'),
                    (11, 'Loser only', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO folder_member (folder_id, entity_id, position_rank)
             VALUES (10, 1, 1), (10, 2, 2), (11, 2, 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription (subscription_id, name, uuid, date_added)
             VALUES (1, 'Shared subscription', 'sub-1', '2026-04-01'),
                    (2, 'Loser subscription', 'sub-2', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription_entity (subscription_id, entity_id)
             VALUES (1, 1), (1, 2), (2, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO tag (tag_id, namespace, subtag)
             VALUES (1, 'general', 'shared'), (2, 'general', 'loser'), (3, 'general', 'implied')",
            [],
        )?;
        conn.execute(
            "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
             VALUES (1, 1, 1, 'local'), (2, 1, 2, 'local'), (2, 2, 4, 'remote')",
            [],
        )?;
        conn.execute(
            "INSERT INTO entity_tag_implied (entity_id, tag_id) VALUES (2, 3)",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_view (entity_id, viewed_at)
             VALUES (1, '2026-04-05T10:00:00Z'), (2, '2026-04-06T10:00:00Z')",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription_post_member (
                 subscription_id, site_id, post_id, item_key, canonical_post_url,
                 media_url, entity_id, status, created_at, updated_at
             ) VALUES (2, 'site', 'post', 'item', NULL, NULL, 2, 'available',
                       '2026-04-01', '2026-04-01')",
            [],
        )?;
        seed_duplicate_pair(conn, 1, 2)
    })
    .expect("seed duplicate provenance fixture");

    let result = db
        .resolve_duplicate_pair("keep_left", "winner", "loser")
        .expect("resolve duplicate pair");
    assert!(matches!(result.status, DuplicateResolveStatus::Resolved));
    assert_eq!(result.winner_hash.as_deref(), Some("winner"));
    assert_eq!(result.loser_hash.as_deref(), Some("loser"));
    assert_eq!(result.loser_file_hash.as_deref(), Some("loser_file"));
    assert_eq!(result.affected_folder_ids, vec![10, 11]);
    assert_eq!(result.tags_merged, 1);

    db.with_read(|conn| {
        let (name, notes, urls, rating, status, date_created, date_added): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            i64,
            String,
            String,
        ) = conn.query_row(
            "SELECT name, notes, source_urls_json, rating, status, date_created, date_added
             FROM media_entity WHERE entity_id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )?;
        assert_eq!(name.as_deref(), Some("recovered name"));
        assert!(notes.as_deref().unwrap().contains("winner note"));
        assert!(notes.as_deref().unwrap().contains("loser note"));
        assert!(urls.as_deref().unwrap().contains("https://two.example"));
        assert_eq!((rating, status, date_created, date_added), (Some(4), 1, "2026-04-01".into(), "2026-04-02".into()));

        let tag_rows: Vec<(i64, i64, String)> = conn
            .prepare("SELECT tag_id, provenance_mask, source FROM entity_tag WHERE entity_id = 1 ORDER BY tag_id, source")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(tag_rows, vec![(1, 3, "local".into()), (2, 4, "remote".into())]);
        let implied: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entity_tag_implied WHERE entity_id = 1 AND tag_id = 3",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(implied, 1);

        let folder_rows: Vec<(i64, i64)> = conn
            .prepare("SELECT folder_id, position_rank FROM folder_member WHERE entity_id = 1 ORDER BY folder_id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(folder_rows, vec![(10, 1), (11, 1)]);
        let subscriptions: Vec<i64> = conn
            .prepare("SELECT subscription_id FROM subscription_entity WHERE entity_id = 1 ORDER BY subscription_id")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(subscriptions, vec![1, 2]);
        let viewed_at: String = conn.query_row(
            "SELECT viewed_at FROM media_view WHERE entity_id = 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(viewed_at, "2026-04-06T10:00:00Z");
        let post_entity: i64 = conn.query_row(
            "SELECT entity_id FROM subscription_post_member WHERE subscription_id = 2",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(post_entity, 1);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get::<_, i64>(0))?, 1);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get::<_, i64>(0))?, 1);
        Ok(())
    })
    .expect("verify duplicate merge");
}

#[test]
fn resolve_duplicate_pair_rejects_entities_without_a_detected_pair() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "left",
            1,
            "left-file",
            1,
            Some("Left"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            1,
            1,
            1,
            None,
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "right",
            2,
            "right-file",
            1,
            Some("Right"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            1,
            1,
            1,
            None,
        )
    })
    .expect("seed unrelated entities");
    let error = db
        .resolve_duplicate_pair("keep_left", "left", "right")
        .expect_err("unreviewed entities must not resolve");
    assert!(error.contains("not awaiting review"));
}

#[test]
fn resolve_duplicate_pair_rejects_stale_trash_candidates_without_mutation() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "left",
            1,
            "left-file",
            1,
            Some("Left"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            100,
            100,
            100,
            1,
            None,
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "right",
            2,
            "right-file",
            2,
            Some("Right"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/jpeg",
            200,
            200,
            200,
            1,
            None,
        )?;
        seed_duplicate_pair(conn, 1, 2)
    })
    .expect("seed trash duplicate fixture");
    let error = db
        .resolve_duplicate_pair("smart_merge", "left", "right")
        .expect_err("trash candidates must not resolve");
    assert!(error.contains("active or inbox"));
    db.with_read(|conn| {
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
                .get::<_, i64>(0))?,
            2
        );
        assert_eq!(
            conn.query_row("SELECT status FROM duplicate", [], |row| row
                .get::<_, String>(0))?,
            "detected"
        );
        Ok(())
    })
    .expect("verify stale duplicate is unchanged");
}

#[test]
fn duplicate_similarity_uses_the_full_256_bit_hash() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "left",
            1,
            "left-file",
            1,
            Some("Left"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            100,
            100,
            1,
            None,
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "right",
            2,
            "right-file",
            1,
            Some("Right"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            100,
            100,
            1,
            None,
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 64)",
            [],
        )
    })
    .expect("seed similarity fixture");
    let page = db
        .get_duplicate_pairs(None, 10, Some("detected".to_string()), None)
        .unwrap();
    assert_eq!(page.items[0].similarity_pct, 75.0);
}

#[test]
fn duplicate_visibility_includes_inbox_and_excludes_trash_immediately() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "inbox",
            1,
            "inbox-file",
            0,
            Some("Inbox"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            100,
            100,
            1,
            None,
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "active",
            2,
            "active-file",
            1,
            Some("Active"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            100,
            100,
            1,
            None,
        )?;
        seed_duplicate_pair(conn, 1, 2)
    })
    .expect("seed inbox duplicate fixture");
    assert_eq!(db.get_duplicate_count().unwrap(), 1);
    db.with_write(|conn| {
        conn.execute("UPDATE media_entity SET status = 2 WHERE entity_id = 1", [])
    })
    .expect("move inbox entity to trash");
    assert_eq!(db.get_duplicate_count().unwrap(), 0);
}

#[test]
fn duplicate_scan_reconciles_detected_pairs_against_current_phash_truth() {
    let db = open_test_db();
    let first = supported_phash([0; 32]);
    let mut near_bytes = [0_u8; 32];
    near_bytes[0] = 1;
    let second = supported_phash(near_bytes);
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "scan-left",
            1,
            "scan-left-file",
            1,
            Some("Left"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            1,
            1,
            1,
            Some(&first),
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "scan-right",
            2,
            "scan-right-file",
            1,
            Some("Right"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1,
            1,
            1,
            1,
            Some(&second),
        )
    })
    .expect("seed scan fixture");
    let result = db
        .scan_duplicates(Some(2), Some(2))
        .expect("scan duplicates");
    assert_eq!(result.candidates_found, 1);
    assert_eq!(result.reviewable_detected_total, 1);
    assert_eq!(db.get_duplicate_count().unwrap(), 1);
}

#[test]
fn duplicate_resolution_decisions_keep_both_and_mark_false_positives() {
    for action in ["not_duplicate", "keep_both"] {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            seed_duplicate_entity(
                conn,
                1,
                "left",
                1,
                "left-file",
                1,
                Some("Left"),
                None,
                None,
                None,
                "2026-04-01",
                "2026-04-01",
                "image/png",
                1,
                100,
                100,
                1,
                None,
            )?;
            seed_duplicate_entity(
                conn,
                2,
                "right",
                2,
                "right-file",
                1,
                Some("Right"),
                None,
                None,
                None,
                "2026-04-01",
                "2026-04-01",
                "image/png",
                1,
                100,
                100,
                1,
                None,
            )?;
            seed_duplicate_pair(conn, 1, 2)
        })
        .expect("seed decision fixture");
        db.resolve_duplicate_pair(action, "left", "right")
            .expect("resolve decision");
        db.with_read(|conn| {
            let status: String =
                conn.query_row("SELECT status FROM duplicate", [], |row| row.get(0))?;
            assert_eq!(
                status,
                if action == "not_duplicate" {
                    "ignored_false_positive"
                } else {
                    "dismissed_keep_both"
                }
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
                    .get::<_, i64>(0))?,
                2
            );
            Ok(())
        })
        .expect("verify decision");
    }
}

#[test]
fn smart_merge_keeps_the_earlier_file_when_quality_is_tied() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(
            conn,
            1,
            "left",
            1,
            "left-file",
            1,
            Some("Left"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1000,
            100,
            100,
            1,
            None,
        )?;
        seed_duplicate_entity(
            conn,
            2,
            "right",
            2,
            "right-file",
            1,
            Some("Right"),
            None,
            None,
            None,
            "2026-04-01",
            "2026-04-01",
            "image/png",
            1000,
            100,
            100,
            1,
            None,
        )?;
        seed_duplicate_pair(conn, 1, 2)
    })
    .expect("seed ambiguous duplicate fixture");
    let result = db
        .resolve_duplicate_pair("smart_merge", "left", "right")
        .unwrap();
    assert!(matches!(result.status, DuplicateResolveStatus::Resolved));
    assert_eq!(result.winner_hash.as_deref(), Some("left"));
    assert_eq!(result.loser_hash.as_deref(), Some("right"));
    db.with_read(|conn| {
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
                .get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM duplicate", [], |row| {
                row.get::<_, i64>(0)
            })?,
            0
        );
        Ok(())
    })
    .expect("verify tied pair resolved");
}

#[test]
fn smart_merge_keeps_the_quality_winner_and_preserves_its_other_matches() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        seed_duplicate_entity(conn, 1, "larger", 1, "larger-file", 1, Some("Larger"), None, None, None, "2026-04-01", "2026-04-01", "image/png", 1_200_000, 4570, 1191, 1, None)?;
        seed_duplicate_entity(conn, 2, "smaller", 2, "smaller-file", 1, Some("Smaller"), None, None, None, "2026-04-01", "2026-04-01", "image/jpeg", 225_600, 4096, 1067, 1, None)?;
        seed_duplicate_entity(conn, 3, "other", 3, "other-file", 1, Some("Other"), None, None, None, "2026-04-01", "2026-04-01", "image/png", 1000, 100, 100, 1, None)?;
        conn.execute("INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0), (1, 3, 1), (2, 3, 1)", [])
    })
    .expect("seed connected duplicate fixture");
    let result = db
        .resolve_duplicate_pair("smart_merge", "larger", "smaller")
        .unwrap();
    assert!(matches!(result.status, DuplicateResolveStatus::Resolved));
    assert_eq!(result.winner_hash.as_deref(), Some("larger"));
    assert_eq!(result.loser_hash.as_deref(), Some("smaller"));
    db.with_read(|conn| {
        let entities: Vec<String> = conn
            .prepare("SELECT entity_hash FROM media_entity ORDER BY entity_id")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let pairs: Vec<(i64, i64)> = conn
            .prepare("SELECT file_id_a, file_id_b FROM duplicate WHERE status = 'detected'")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(entities, vec!["larger", "other"]);
        assert_eq!(pairs, vec![(1, 3)]);
        Ok(())
    })
    .expect("verify quality winner");
}

#[test]
fn duplicate_resolution_persists_loser_blob_cleanup_after_restart() {
    let temp = TempDir::new().expect("create restart fixture");
    {
        let db = LibraryDatabase::open(temp.path()).expect("open initial database");
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            seed_duplicate_entity(
                conn,
                1,
                "winner",
                1,
                "winner-file",
                1,
                Some("Winner"),
                None,
                None,
                None,
                "2026-04-01",
                "2026-04-01",
                "image/png",
                200,
                200,
                200,
                1,
                None,
            )?;
            seed_duplicate_entity(
                conn,
                2,
                "loser",
                2,
                "loser-file",
                1,
                Some("Loser"),
                None,
                None,
                None,
                "2026-04-01",
                "2026-04-01",
                "image/jpeg",
                100,
                100,
                100,
                1,
                None,
            )?;
            seed_duplicate_pair(conn, 1, 2)
        })
        .expect("seed restart fixture");
        let result = db
            .resolve_duplicate_pair("keep_left", "winner", "loser")
            .unwrap();
        assert!(result.blob_cleanup_pending);
        let jobs = db
            .list_deferred_work_items(DeferredWorkFilter {
                entity_hash: Some("loser-file".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].work_type, DeferredWorkType::BlobDelete);
    }
    let reopened = LibraryDatabase::open(temp.path()).expect("reopen database");
    let recovered = reopened
        .list_deferred_work_items(DeferredWorkFilter {
            work_type: Some(DeferredWorkType::BlobDelete),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].status,
        crate::background_work::DeferredWorkStatus::Pending
    );
}

#[test]
fn enqueue_stale_color_analysis_jobs_only_queues_stale_color_capable_rows() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        let blob = serialize_dominant_palette_blob(&[DominantColor {
            hex: "#010203".into(),
            l: 1.0,
            a: 2.0,
            b: 3.0,
        }])
        .expect("serialize");

        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, color_analysis_version, date_added
             ) VALUES
                (1, 'stale_file', 'image/png', 1, 0, 0, '2026-04-01'),
                (3, 'audio_file', 'audio/mpeg', 1, 1, 0, '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_palette_blob,
                color_analysis_version, date_added
             ) VALUES (2, 'fresh_file', 'image/png', 1, 0, ?1, ?2, '2026-04-01')",
            rusqlite::params![blob, TARGET_COLOR_ANALYSIS_VERSION],
        )?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, file_id, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'stale_image', 1, 1, 'Stale', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'fresh_image', 2, 1, 'Fresh', '2026-04-01', '2026-04-01', '2026-04-01'),
                (3, 'audio_only', 3, 1, 'Audio', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        Ok(())
    })
    .expect("seed db");

    let queued = db
        .enqueue_stale_color_analysis_jobs(TARGET_COLOR_ANALYSIS_VERSION)
        .expect("enqueue stale colors");
    let jobs = db
        .list_deferred_work_items(DeferredWorkFilter::default())
        .expect("list jobs");

    assert_eq!(queued, 1);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].entity_hash, "stale_image");
    assert_eq!(jobs[0].work_type, DeferredWorkType::DominantColors);
}

#[test]
fn ensure_deferred_jobs_present_does_not_reset_existing_running_job() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO deferred_work_item (
                entity_hash, work_type, status, attempt_count, available_at, queued_at, started_at
             ) VALUES (?1, 'dominant_colors', 'running', 3, '2026-04-01', '2026-04-01', '2026-04-01T00:00:01Z')",
            ["hash_a"],
        )?;
        Ok(())
    })
    .expect("seed deferred work");

    db.ensure_deferred_jobs_present("hash_a", &[DeferredWorkType::DominantColors])
        .expect("ensure deferred work");

    let jobs = db
        .list_deferred_work_items(DeferredWorkFilter::default())
        .expect("list jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].entity_hash, "hash_a");
    assert_eq!(jobs[0].work_type, DeferredWorkType::DominantColors);
    assert_eq!(
        jobs[0].status,
        crate::background_work::DeferredWorkStatus::Running
    );
    assert_eq!(jobs[0].attempt_count, 3);
    assert_eq!(jobs[0].started_at.as_deref(), Some("2026-04-01T00:00:01Z"));
}

#[test]
fn ensure_missing_color_analysis_jobs_only_queues_missing_colors_once() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        let blob = serialize_dominant_palette_blob(&[DominantColor {
            hex: "#010203".into(),
            l: 1.0,
            a: 2.0,
            b: 3.0,
        }])
        .expect("serialize");
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, color_analysis_version, date_added
             ) VALUES (1, 'stale_file', 'image/png', 1, 0, 0, '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_palette_blob,
                color_analysis_version, date_added
             ) VALUES (2, 'fresh_file', 'image/png', 1, 0, ?1, ?2, '2026-04-01')",
            rusqlite::params![blob, TARGET_COLOR_ANALYSIS_VERSION],
        )?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, file_id, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'stale_image', 1, 1, 'Stale', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'fresh_image', 2, 1, 'Fresh', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        Ok(())
    })
    .expect("seed db");

    let hashes = vec![
        "stale_image".to_string(),
        "fresh_image".to_string(),
        "stale_image".to_string(),
    ];
    ensure_missing_color_analysis_jobs(&db, &hashes).expect("first ensure");
    ensure_missing_color_analysis_jobs(&db, &hashes).expect("second ensure");

    let jobs = db
        .list_deferred_work_items(DeferredWorkFilter::default())
        .expect("list jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].entity_hash, "stale_image");
    assert_eq!(jobs[0].work_type, DeferredWorkType::DominantColors);
}

#[test]
fn deferred_work_summary_reports_dominant_color_backlog_counts() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO deferred_work_item (
                entity_hash, work_type, status, attempt_count, available_at, queued_at
             ) VALUES
                ('pending_color', 'dominant_colors', 'pending', 0, '2026-04-01', '2026-04-01'),
                ('failed_color', 'dominant_colors', 'pending', 2, '2026-04-01', '2026-04-01'),
                ('running_color', 'dominant_colors', 'running', 0, '2026-04-01', '2026-04-01'),
                ('thumb_job', 'thumbnail', 'pending', 0, '2026-04-01', '2026-04-01')",
            [],
        )?;
        Ok(())
    })
    .expect("seed deferred summary");

    let summary = db
        .get_deferred_work_summary()
        .expect("get deferred summary");

    assert_eq!(summary.pending_count, 3);
    assert_eq!(summary.running_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.dominant_colors_pending_count, 1);
    assert_eq!(summary.dominant_colors_running_count, 1);
    assert_eq!(summary.dominant_colors_failed_count, 1);
}
