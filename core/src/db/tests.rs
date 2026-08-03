//! Integration-style tests for LibraryDatabase (moved out of mod.rs).

use super::LibraryDatabase;
use crate::background_work::{DeferredWorkFilter, DeferredWorkType};
use crate::db::core::schema::{CURRENT_SCHEMA_VERSION, LIBRARY_DDL};
use crate::db::types::{
    BaseScope, DuplicateResolveStatus, EntityViewQuery, IngestPreparedSingle, QueryFilters,
    QueryPage, QuerySort, ScopeKind,
};
use crate::media_analysis::ensure_missing_color_analysis_jobs;
use crate::media_analysis::TARGET_COLOR_ANALYSIS_VERSION;
use crate::media_processing::colors::{serialize_dominant_palette_blob, DominantColor};
use rusqlite::params;
use tempfile::TempDir;

fn open_test_db() -> LibraryDatabase {
    let tmp = TempDir::new().expect("tempdir");
    let db = LibraryDatabase::open(tmp.path()).expect("open library db");
    std::mem::forget(tmp);
    db
}

#[test]
fn collection_materialization_persists_prepared_perceptual_hashes() {
    let db = open_test_db();
    let member = |entity_hash: &str, perceptual_hash: Option<&str>| IngestPreparedSingle {
        entity_hash: entity_hash.to_string(),
        name: Some(entity_hash.to_string()),
        size_bytes: 1,
        mime_type: "image/png".to_string(),
        pixel_width: Some(1),
        pixel_height: Some(1),
        duration_ms: None,
        frame_count: Some(1),
        has_audio: false,
        status: 1,
        date_created: "2026-08-03T00:00:00Z".to_string(),
        date_added: "2026-08-03T00:00:00Z".to_string(),
        has_thumbnail: false,
        skip_thumbnail: false,
        notes: None,
        source_urls: Vec::new(),
        tag_strings: Vec::new(),
        tag_provenance_mask: 0,
        perceptual_hash: perceptual_hash.map(str::to_string),
    };

    db.materialize_ingested_collection(
        "Set",
        &[
            member("collection_member_one", Some("phash-one")),
            member("collection_member_two", None),
        ],
        &[],
        None,
    )
    .expect("materialize collection");

    let stored_phash: Option<String> = db
        .with_read(|conn| {
            conn.query_row(
                "SELECT mf.perceptual_hash
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE me.entity_hash = ?1",
                ["collection_member_one"],
                |row| row.get(0),
            )
        })
        .expect("read stored perceptual hash");

    assert_eq!(stored_phash.as_deref(), Some("phash-one"));
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
fn entity_tag_and_collection_mutations_emit_ops() {
    let db = open_test_db();
    let file_id = db
        .insert_file(
            "hash_x",
            "image/png",
            10,
            None,
            None,
            None,
            None,
            false,
            "2026-01-01",
        )
        .unwrap();
    let entity_id = db
        .insert_single(
            "hash_x",
            file_id,
            Some("img"),
            1,
            "2026-01-01",
            "2026-01-01",
        )
        .unwrap();

    db.add_tags(
        &[entity_id],
        &["general:cat".to_string()],
        1,
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    db.set_entity_status(&[entity_id], 2, crate::db::types::ExpansionMode::EntityOnly)
        .unwrap();
    let collection_id = db.create_collection("My set").unwrap();
    db.update_collection_name(collection_id, "Renamed set")
        .unwrap();
    db.delete_collection(collection_id).unwrap();
    db.delete_entities(&[entity_id]).unwrap();

    let ops: Vec<(String, String)> = db
        .with_read(|conn| {
            let mut stmt =
                conn.prepare("SELECT op_type, entity_key FROM op_outbox ORDER BY op_id")?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();

    let types: Vec<&str> = ops.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "entity_tags_added",
            "entity_status_changed",
            "collection_created",
            "collection_renamed",
            "collection_deleted",
            "entity_deleted",
        ]
    );
    assert!(ops
        .iter()
        .filter(|o| o.0.starts_with("entity_"))
        .all(|o| o.1 == "hash_x"));
    // Collection ops key on the collection's entity_hash, not the member's.
    assert!(ops
        .iter()
        .filter(|o| o.0.starts_with("collection_"))
        .all(|o| o.1 != "hash_x" && !o.1.is_empty()));
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
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES (1, 'with_blob', 'single', 1, 'Blob', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES (2, 'fallback', 'single', 1, 'Fallback', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;

        let blob = serialize_dominant_palette_blob(&[DominantColor {
            hex: "#abcdef".into(),
            l: 10.0,
            a: 1.0,
            b: 2.0,
        }])
        .expect("serialize");

        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex,
                dominant_palette_blob, color_analysis_version, date_added
             ) VALUES (1, 'file_blob', 'image/png', 1, 0, '#abcdef', ?1, ?2, '2026-04-01')",
            rusqlite::params![blob, TARGET_COLOR_ANALYSIS_VERSION],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex,
                color_analysis_version, date_added
             ) VALUES (2, 'file_fallback', 'image/png', 1, 0, '#123456', 0, '2026-04-01')",
            [],
        )?;
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)", [])?;
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (2, 2)", [])?;
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
        ("DROP TABLE sync_ingest_cursor", "sync_ingest_cursor"),
        ("ALTER TABLE folder DROP COLUMN pin_order", "folder"),
        (
            "DROP INDEX idx_ingest_queue_ready",
            "idx_ingest_queue_ready",
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
fn smart_folder_scope_query_matches_runtime_compiled_bitmap_and_sidebar_count() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, rating, date_created, date_added, date_modified
             ) VALUES
                (1, 'entity_1', 'single', 1, 'Landscape', 5, '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'entity_2', 'single', 1, 'Portrait', 2, '2026-04-02', '2026-04-02', '2026-04-02')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, has_audio, date_added
             ) VALUES
                (1, 'file_1', 'image/png', 100, 1000, 500, 0, '2026-04-01'),
                (2, 'file_2', 'image/jpeg', 100, 500, 1000, 0, '2026-04-02')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO tag (tag_id, namespace, subtag, file_count) VALUES (1, 'general', 'landscape', 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source) VALUES (1, 1, 1, 'local')",
            [],
        )?;
        conn.execute(
            "INSERT INTO file_color (file_id, hex, l, a, b) VALUES (1, '#ff0000', 50.0, 60.0, 70.0)",
            [],
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
                }).to_string(),
                "2026-04-01T00:00:00Z",
            ],
        )?;
        Ok(())
    })
    .expect("seed smart folder data");

    db.full_rebuild();

    let page = db
        .query_entity_view(&EntityViewQuery {
            base_scope: BaseScope {
                kind: ScopeKind::SmartFolder,
                key: None,
                id: Some(7),
            },
            filters: QueryFilters::default(),
            sort: QuerySort::default(),
            page: QueryPage::default(),
        })
        .expect("query smart folder scope");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].entity_hash, "entity_1");
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
    .expect("read sidebar count");
}

#[test]
fn resolve_duplicate_pair_requires_explicit_collection_choice_for_cross_collection_members() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'collection_left', 'collection', 1, 'Left Collection', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'collection_right', 'collection', 1, 'Right Collection', '2026-04-01', '2026-04-01', '2026-04-01'),
                (3, 'left_single', 'single', 1, 'Left Single', '2026-04-01', '2026-04-01', '2026-04-01'),
                (4, 'right_single', 'single', 1, 'Right Single', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "UPDATE media_entity
             SET parent_collection_entity_id = 1, collection_ordinal = 1
             WHERE entity_id = 3",
            [],
        )?;
        conn.execute(
            "UPDATE media_entity
             SET parent_collection_entity_id = 2, collection_ordinal = 1
             WHERE entity_id = 4",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, has_audio,
                perceptual_hash, date_added
             ) VALUES
                (1, 'file_left', 'image/png', 1, 100, 100, 0, 'hash_left', '2026-04-01'),
                (2, 'file_right', 'image/png', 1, 100, 100, 0, 'hash_right', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (3, 1), (4, 2)",
            [],
        )?;
        Ok(())
    })
    .expect("seed duplicate conflict data");

    let result = db
        .resolve_duplicate_pair("keep_left", "left_single", "right_single", None)
        .expect("resolve duplicate conflict");

    assert!(matches!(result.status, DuplicateResolveStatus::Conflict));
    let conflict = result.conflict.expect("conflict payload");
    assert_eq!(conflict.winner_collection_id, Some(1));
    assert_eq!(conflict.loser_collection_id, Some(2));
}

#[test]
fn enqueue_stale_color_analysis_jobs_only_queues_stale_color_capable_rows() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'stale_image', 'single', 1, 'Stale', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'fresh_image', 'single', 1, 'Fresh', '2026-04-01', '2026-04-01', '2026-04-01'),
                (3, 'audio_only', 'single', 1, 'Audio', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;

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
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)", [])?;
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (2, 2)", [])?;
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (3, 3)", [])?;
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
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'stale_image', 'single', 1, 'Stale', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'fresh_image', 'single', 1, 'Fresh', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
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
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)", [])?;
        conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (2, 2)", [])?;
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
