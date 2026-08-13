//! Integration-style tests for LibraryDatabase (moved out of mod.rs).

use super::LibraryDatabase;
use crate::background_work::{DeferredWorkFilter, DeferredWorkType};
use crate::db::core::schema::{CURRENT_SCHEMA_VERSION, LIBRARY_DDL};
use crate::db::types::{
    BaseScope, DuplicateResolveStatus, EntityViewQuery, IngestPreparedSingle, MediaEntityPatch,
    QueryFilters, QueryPage, QuerySort, ScopeKind,
};
use crate::media_analysis::ensure_missing_color_analysis_jobs;
use crate::media_analysis::TARGET_COLOR_ANALYSIS_VERSION;
use crate::media_processing::colors::{serialize_dominant_palette_blob, DominantColor};
use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::subscriptions::runtime_db::upsert_subscription_issue;
use img_hash::ImageHash;
use rusqlite::params;
use std::sync::{mpsc, Arc};
use std::time::Duration;
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
fn collection_materialization_indexes_phashes_and_records_pairs_atomically() {
    let db = open_test_db();
    let first_phash = supported_phash([0_u8; 32]);
    let mut near_bytes = [0_u8; 32];
    near_bytes[0] = 1;
    let second_phash = supported_phash(near_bytes);
    let member = |entity_hash: &str, perceptual_hash: &str| IngestPreparedSingle {
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
        date_created: "2026-08-14T00:00:00Z".to_string(),
        date_added: "2026-08-14T00:00:00Z".to_string(),
        has_thumbnail: false,
        skip_thumbnail: false,
        notes: None,
        source_urls: Vec::new(),
        tag_strings: Vec::new(),
        tag_provenance_mask: 0,
        perceptual_hash: Some(perceptual_hash.to_string()),
    };

    db.materialize_ingested_collection(
        "Near pair",
        &[
            member("indexed-first", &first_phash),
            member("indexed-second", &second_phash),
        ],
        &[],
        None,
    )
    .expect("materialize indexed collection");

    db.with_read(|conn| {
        let index_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM media_file_phash_index", [], |row| {
                row.get(0)
            })?;
        let pair_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM duplicate", [], |row| row.get(0))?;
        assert_eq!(index_count, 2);
        assert_eq!(pair_count, 1);
        Ok(())
    })
    .expect("inspect indexed collection");
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
        .insert_single(
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
fn collection_content_is_child_owned_and_final_member_removal_syncs_split() {
    let db = open_test_db();
    let first_file = db
        .insert_file(
            "aggregate-first-file",
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
    let first = db
        .insert_single(
            "aggregate-first",
            first_file,
            Some("first"),
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    let second_file = db
        .insert_file(
            "aggregate-second-file",
            "image/png",
            20,
            None,
            None,
            None,
            None,
            false,
            "2026-08-04",
        )
        .unwrap();
    let second = db
        .insert_single(
            "aggregate-second",
            second_file,
            Some("second"),
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    let collection = db
        .create_collection_with_members_by_hashes(
            "Aggregate",
            &[
                "aggregate-first".to_string(),
                "aggregate-second".to_string(),
            ],
        )
        .unwrap();
    let collection_hash = db.get_collection_hash(collection).unwrap().unwrap();

    db.add_tags(
        &[first],
        &["existing:child".to_string()],
        1,
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    db.add_tags(
        &[collection],
        &["bulk:collection".to_string()],
        1,
        crate::db::types::ExpansionMode::SinglesAndCollectionMembers,
    )
    .unwrap();
    db.with_read(|conn| {
        let tagged_children: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM entity_tag et
             JOIN tag t ON t.tag_id = et.tag_id
             WHERE et.entity_id IN (?1, ?2)
               AND t.namespace = 'bulk' AND t.subtag = 'collection'",
            params![first, second],
            |row| row.get(0),
        )?;
        let collection_tags: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entity_tag WHERE entity_id = ?1",
            [collection],
            |row| row.get(0),
        )?;
        assert_eq!(tagged_children, 2);
        assert_eq!(collection_tags, 0);
        Ok(())
    })
    .unwrap();
    db.remove_tags(
        &[collection],
        &["bulk:collection".to_string()],
        crate::db::types::ExpansionMode::SinglesAndCollectionMembers,
    )
    .unwrap();

    db.with_read(|conn| {
        let collection_tags: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entity_tag WHERE entity_id = ?1",
            [collection],
            |row| row.get(0),
        )?;
        let first_tags: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entity_tag WHERE entity_id = ?1",
            [first],
            |row| row.get(0),
        )?;
        let second_tags: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entity_tag WHERE entity_id = ?1",
            [second],
            |row| row.get(0),
        )?;
        assert_eq!(collection_tags, 0);
        assert_eq!(
            first_tags, 1,
            "unrelated child tag must survive bulk removal"
        );
        assert_eq!(second_tags, 0);
        Ok(())
    })
    .unwrap();

    db.patch_entity_metadata(
        &[first],
        &MediaEntityPatch {
            rating: Some(4),
            notes: Some(Some("child note".to_string())),
            source_urls: Some(vec!["https://example.test/child".to_string()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(db
        .patch_entity_metadata(
            &[collection],
            &MediaEntityPatch {
                rating: Some(5),
                notes: Some(Some("collection-owned note".to_string())),
                source_urls: Some(vec!["https://invalid.test/collection".to_string()]),
                ..Default::default()
            },
        )
        .is_err());
    db.patch_entity_metadata(
        &[collection],
        &MediaEntityPatch {
            name: Some("Renamed aggregate".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let details = db.get_entity_details(&collection_hash).unwrap().unwrap();
    assert_eq!(details.name.as_deref(), Some("Renamed aggregate"));
    assert_eq!(details.rating, Some(4));
    assert_eq!(details.notes.as_deref(), Some("child note"));
    assert_eq!(
        details.source_urls.as_deref(),
        Some(["https://example.test/child".to_string()].as_slice())
    );
    assert!(details
        .tags
        .iter()
        .any(|tag| tag.namespace == "existing" && tag.subtag == "child"));
    let child_name: Option<String> = db
        .with_read(|conn| {
            conn.query_row(
                "SELECT name FROM media_entity WHERE entity_id = ?1",
                [first],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(child_name.as_deref(), Some("first"));
    assert_eq!(
        db.get_parent_collection_ids_for_entities(&[first, second])
            .unwrap(),
        vec![collection]
    );
    let grid = db
        .get_entity_grid_items(&[collection_hash.clone()])
        .unwrap();
    assert_eq!(grid[0].rating, Some(4), "grid rating derives from children");

    let removal = db
        .remove_collection_members(collection, &[first, second])
        .unwrap();
    assert!(removal.deleted_collection);
    assert_eq!(
        removal.collection_hash.as_deref(),
        Some(collection_hash.as_str())
    );
    let ops: Vec<String> = db
        .with_read(|conn| {
            let mut stmt =
                conn.prepare("SELECT op_type FROM op_outbox WHERE entity_key = ?1 ORDER BY op_id")?;
            let ops = stmt
                .query_map([&collection_hash], |row| row.get(0))?
                .collect();
            ops
        })
        .unwrap();
    assert!(ops.iter().any(|op| op == "collection_split"));
    assert!(
        !ops.iter().any(|op| op == "collection_members_removed"),
        "final-member removal must sync the collection deletion, not a stale membership update"
    );
}

#[test]
fn collection_membership_rejects_mixed_image_and_video_without_writes() {
    let db = open_test_db();
    let image_file = db
        .insert_file(
            "collection-image-file",
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
    let image_id = db
        .insert_single(
            "collection-image-entity",
            image_file,
            None,
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    let video_file = db
        .insert_file(
            "collection-video-file",
            "video/mp4",
            20,
            None,
            None,
            None,
            None,
            false,
            "2026-08-04",
        )
        .unwrap();
    let video_id = db
        .insert_single(
            "collection-video-entity",
            video_file,
            None,
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();

    let source_collection = db.create_collection("Source").unwrap();
    let target_collection = db.create_collection("Target").unwrap();
    db.add_collection_members(source_collection, &[image_id])
        .unwrap();
    let before = db
        .with_read(|conn| {
            Ok((
                conn.query_row(
                    "SELECT parent_collection_entity_id FROM media_entity WHERE entity_id = ?1",
                    [image_id],
                    |row| row.get::<_, Option<i64>>(0),
                )?,
                conn.query_row(
                    "SELECT member_count FROM media_entity WHERE entity_id = ?1",
                    [target_collection],
                    |row| row.get::<_, Option<i64>>(0),
                )?,
                conn.query_row("SELECT COUNT(*) FROM op_outbox", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            ))
        })
        .unwrap();

    assert!(db
        .add_collection_members(target_collection, &[image_id, video_id])
        .is_err());

    let after = db
        .with_read(|conn| {
            Ok((
                conn.query_row(
                    "SELECT parent_collection_entity_id FROM media_entity WHERE entity_id = ?1",
                    [image_id],
                    |row| row.get::<_, Option<i64>>(0),
                )?,
                conn.query_row(
                    "SELECT member_count FROM media_entity WHERE entity_id = ?1",
                    [target_collection],
                    |row| row.get::<_, Option<i64>>(0),
                )?,
                conn.query_row("SELECT COUNT(*) FROM op_outbox", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            ))
        })
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(after.0, Some(source_collection));
}

#[test]
fn collection_membership_by_hashes_rejects_unknown_hash_without_writes() {
    let db = open_test_db();
    let file_id = db
        .insert_file(
            "known-collection-file",
            "image/jpeg",
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
        .insert_single(
            "known-collection-entity",
            file_id,
            None,
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    let collection_id = db.create_collection("Target").unwrap();
    let before_ops: i64 = db
        .with_read(|conn| conn.query_row("SELECT COUNT(*) FROM op_outbox", [], |row| row.get(0)))
        .unwrap();

    assert!(db
        .add_collection_members_by_hashes(
            collection_id,
            &[
                "known-collection-entity".to_string(),
                "missing-collection-entity".to_string(),
            ],
        )
        .is_err());

    let (parent, ordinal, member_count, after_ops): (Option<i64>, Option<i64>, Option<i64>, i64) =
        db.with_read(|conn| {
            Ok((
                conn.query_row(
                    "SELECT parent_collection_entity_id FROM media_entity WHERE entity_id = ?1",
                    [entity_id],
                    |row| row.get(0),
                )?,
                conn.query_row(
                    "SELECT collection_ordinal FROM media_entity WHERE entity_id = ?1",
                    [entity_id],
                    |row| row.get(0),
                )?,
                conn.query_row(
                    "SELECT member_count FROM media_entity WHERE entity_id = ?1",
                    [collection_id],
                    |row| row.get(0),
                )?,
                conn.query_row("SELECT COUNT(*) FROM op_outbox", [], |row| row.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!((parent, ordinal, member_count), (None, None, Some(0)));
    assert_eq!(after_ops, before_ops);
}

#[test]
fn create_collection_with_members_rolls_back_invalid_members() {
    let db = open_test_db();
    let image_file = db
        .insert_file(
            "create-collection-image-file",
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
    db.insert_single(
        "create-collection-image",
        image_file,
        None,
        1,
        "2026-08-04",
        "2026-08-04",
    )
    .unwrap();
    let video_file = db
        .insert_file(
            "create-collection-video-file",
            "video/mp4",
            10,
            None,
            None,
            None,
            None,
            false,
            "2026-08-04",
        )
        .unwrap();
    db.insert_single(
        "create-collection-video",
        video_file,
        None,
        1,
        "2026-08-04",
        "2026-08-04",
    )
    .unwrap();

    let before: (i64, i64) = db
        .with_read(|conn| {
            Ok((
                conn.query_row(
                    "SELECT COUNT(*) FROM media_entity WHERE entity_kind = 'collection'",
                    [],
                    |row| row.get(0),
                )?,
                conn.query_row("SELECT COUNT(*) FROM op_outbox", [], |row| row.get(0))?,
            ))
        })
        .unwrap();

    assert!(db
        .create_collection_with_members_by_hashes(
            "Invalid collection",
            &[
                "create-collection-image".to_string(),
                "create-collection-video".to_string(),
            ],
        )
        .is_err());
    assert!(db
        .create_collection_with_members_by_hashes(
            "Unknown collection",
            &["missing-collection-member".to_string()],
        )
        .is_err());

    let after: (i64, i64) = db
        .with_read(|conn| {
            Ok((
                conn.query_row(
                    "SELECT COUNT(*) FROM media_entity WHERE entity_kind = 'collection'",
                    [],
                    |row| row.get(0),
                )?,
                conn.query_row("SELECT COUNT(*) FROM op_outbox", [], |row| row.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn collection_split_preserves_media_while_entity_delete_removes_it() {
    let split_db = open_test_db();
    let split_file = split_db
        .insert_file(
            "split-file",
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
    let split_member = split_db
        .insert_single(
            "split-member",
            split_file,
            None,
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    let split_collection = split_db.create_collection("Split").unwrap();
    split_db
        .add_collection_members(split_collection, &[split_member])
        .unwrap();

    assert_eq!(
        split_db.split_collection(split_collection).unwrap(),
        vec![split_member]
    );
    let split_counts: (i64, i64, i64, Option<i64>) = split_db
        .with_read(|conn| {
            Ok((
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))?,
                conn.query_row("SELECT COUNT(*) FROM single_media_entity", [], |row| {
                    row.get(0)
                })?,
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?,
                conn.query_row(
                    "SELECT parent_collection_entity_id FROM media_entity WHERE entity_id = ?1",
                    [split_member],
                    |row| row.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!(split_counts, (1, 1, 1, None));

    let delete_db = open_test_db();
    let delete_file = delete_db
        .insert_file(
            "delete-file",
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
    let delete_member = delete_db
        .insert_single(
            "delete-member",
            delete_file,
            None,
            1,
            "2026-08-04",
            "2026-08-04",
        )
        .unwrap();
    let child_tag = delete_db.ensure_tag("general:delete-child").unwrap();
    let parent_tag = delete_db.ensure_tag("general:delete-parent").unwrap();
    delete_db
        .add_tags(
            &[delete_member],
            &["general:delete-child".to_string()],
            1,
            crate::db::types::ExpansionMode::EntityOnly,
        )
        .unwrap();
    delete_db
        .manage_tag_implication(child_tag, parent_tag, true)
        .unwrap();
    delete_db.full_rebuild();
    let delete_collection = delete_db.create_collection("Delete").unwrap();
    delete_db
        .add_collection_members(delete_collection, &[delete_member])
        .unwrap();

    let deleted = delete_db.delete_entities(&[delete_collection]).unwrap();
    assert_eq!(deleted.freed_file_hashes, vec!["delete-file"]);
    let delete_counts: (i64, i64, i64, i64) = delete_db
        .with_read(|conn| {
            Ok((
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))?,
                conn.query_row("SELECT COUNT(*) FROM single_media_entity", [], |row| {
                    row.get(0)
                })?,
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?,
                conn.query_row("SELECT COUNT(*) FROM entity_tag_implied", [], |row| {
                    row.get(0)
                })?,
            ))
        })
        .unwrap();
    assert_eq!(delete_counts, (0, 0, 0, 0));
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
            db.insert_single(
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
        db.add_folder_members(
            folder_id,
            &[entity_id],
            crate::db::types::ExpansionMode::EntityOnly,
        )
        .unwrap();
    }

    let engine = crate::engine::ApplicationEngine::new(db.clone());
    let deleted = engine.delete_folder(root_id).unwrap();
    assert_eq!(deleted.folder_ids(), vec![grandchild_id, child_id, root_id]);
    assert_eq!(deleted.deleted_folders.len(), 3);
    engine.rebuild_sidebar();

    let (folders, memberships, entities, singles, files, folder_nodes, uncategorized) = db
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
                conn.query_row("SELECT COUNT(*) FROM single_media_entity", [], |row| {
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
    assert_eq!((entities, singles, files), (3, 3, 3));
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
fn folder_count_matches_grid_collection_collapse_and_active_visibility() {
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
        db.insert_single(
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
    db.add_folder_members(
        folder_id,
        &[first, second, standalone, inbox, trash],
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    let collection_id = db
        .create_collection_with_members_by_hashes(
            "Collapsed",
            &[
                "folder-count-first".to_string(),
                "folder-count-second".to_string(),
            ],
        )
        .unwrap();
    // Direct and child membership still represent one visible collection tile.
    db.add_folder_members(
        folder_id,
        &[collection_id],
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    db.add_tags(
        &[first],
        &["general:categorized".to_string()],
        1,
        crate::db::types::ExpansionMode::EntityOnly,
    )
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
    assert_eq!(page.total_count, Some(2));
    assert_eq!(page.items.len(), 2);
    assert_eq!(db.get_folder_visible_count(folder_id).unwrap(), 2);

    db.full_rebuild();
    db.with_read(|conn| {
        let sidebar_count: i64 = conn.query_row(
            "SELECT count FROM sidebar_node WHERE node_id = ?1",
            [format!("folder:{folder_id}")],
            |row| row.get(0),
        )?;
        assert_eq!(sidebar_count, 2);
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
        db.insert_single(
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
        db.add_tags(
            &[entity_id],
            &[tag.to_string()],
            1,
            crate::db::types::ExpansionMode::EntityOnly,
        )
        .unwrap();
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
    let collection_id = db
        .create_collection_with_members_by_hashes(
            "Tagged collection",
            &[
                "tag-count-child-one".to_string(),
                "tag-count-child-two".to_string(),
            ],
        )
        .unwrap();

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

    let records = db.get_tags_paginated(None, None, None, 100).unwrap();
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

    assert_scope_count("test:visible", 2);
    assert_scope_count("test:canonical", 1);
    assert_scope_count("test:alias", 1);
    assert_scope_count("test:child", 1);
    assert_scope_count("test:parent", 2);
    assert_scope_count("test:zero", 0);

    let visible_tag_id = db.find_tag_id("test:visible").unwrap().unwrap();
    assert!(db
        .bitmaps
        .get(&crate::db::projection::bitmaps::BitmapKey::EffectiveTag(
            visible_tag_id,
        ))
        .contains(collection_id as u32));
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
    assert!(!db
        .search_tags("", 100, 0)
        .unwrap()
        .iter()
        .any(|tag| tag.tag_id == zero_tag_id));

    for query in ["1girl", "general:1girl"] {
        let matches = db
            .get_tags_paginated(None, Some(query.to_string()), None, 100)
            .unwrap();
        assert_eq!(matches.len(), 2, "picker search for {query}");
        assert!(matches.iter().any(|tag| tag.tag_id == general_one_girl));
        assert!(matches.iter().any(|tag| tag.tag_id == character_one_girl));
    }
    let autocomplete = db.search_tags("1girl", 100, 0).unwrap();
    assert_eq!(autocomplete.len(), 2);
    assert!(db
        .get_tags_paginated(
            Some("general".to_string()),
            Some("1girl".to_string()),
            None,
            100,
        )
        .unwrap()
        .iter()
        .all(|tag| tag.namespace == "general"));
    assert!(db
        .get_tags_paginated(None, Some("general".to_string()), None, 100)
        .unwrap()
        .is_empty());

    db.with_read(|conn| {
        let has_file_count = conn
            .prepare("PRAGMA table_info(tag)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|column| column == "file_count");
        assert!(!has_file_count, "tag counts must not be stored separately");
        Ok(())
    })
    .unwrap();
}

#[test]
fn recently_viewed_is_unique_top_level_active_and_latest_first() {
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
        db.insert_single(
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
    let child = insert("recent-child", 1);
    insert("recent-inbox", 0);
    insert("recent-trash", 2);
    let collection = db
        .create_collection_with_members_by_hashes(
            "Recent collection",
            &["recent-child".to_string()],
        )
        .unwrap();
    let collection_hash = db.get_collection_hash(collection).unwrap().unwrap();

    assert_eq!(
        db.record_media_view("recent-first").unwrap(),
        ("recent-first".to_string(), 1)
    );
    assert_eq!(
        db.record_media_view("recent-child").unwrap(),
        (collection_hash.clone(), 2)
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
            [collection],
        )?;
        let child_view_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM media_view WHERE entity_id = ?1",
            [child],
            |row| row.get(0),
        )?;
        assert_eq!(child_view_count, 0);
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
        // The recent scope must override user-selected grid ordering.
        sort: QuerySort {
            field: "name".to_string(),
            direction: "asc".to_string(),
        },
        page: QueryPage { limit: 1, cursor },
    };
    let first_page = db.query_entity_view(&query(None)).unwrap();
    assert_eq!(first_page.total_count, Some(2));
    assert_eq!(first_page.items[0].entity_hash, collection_hash);
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
    db.split_collection(collection_id).unwrap();
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
            "collection_split",
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
            "INSERT INTO subscription (subscription_id, name, site_id, date_added)
             VALUES (1, 'Test subscription', 'test', '2026-08-05')",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, site_id, query_text)
             VALUES (10, 1, 'test', 'query')",
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
            "INSERT INTO subscription (subscription_id, name, site_id, date_added)
             VALUES (1, 'Test subscription', 'test', '2026-08-05')",
            [],
        )?;
        conn.execute(
            "INSERT INTO subscription_query (query_id, subscription_id, site_id, query_text)
             VALUES (10, 1, 'test', 'query')",
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
            "INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'landscape')",
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

    let child_file = db
        .insert_file(
            "smart-collection-child-file",
            "image/png",
            100,
            Some(100),
            Some(100),
            None,
            Some(1),
            false,
            "2026-04-03",
        )
        .unwrap();
    let child_id = db
        .insert_single(
            "smart-collection-child",
            child_file,
            Some("Smart child"),
            1,
            "2026-04-03",
            "2026-04-03",
        )
        .unwrap();
    let collection_id = db
        .create_collection_with_members_by_hashes(
            "Smart collection",
            &["smart-collection-child".to_string()],
        )
        .unwrap();
    db.add_tags(
        &[child_id],
        &["landscape".to_string()],
        1,
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    db.with_write(|conn| {
        conn.execute(
            "INSERT INTO smart_folder (
                smart_folder_id, name, predicate_json, date_added, date_modified
             ) VALUES (8, 'Tagged collections', ?1, '2026-04-03', '2026-04-03')",
            [serde_json::json!({
                "groups": [{
                    "match_mode": "all",
                    "negate": false,
                    "rules": [{ "field": "tags", "op": "include_all", "values": ["landscape"] }]
                }]
            })
            .to_string()],
        )?;
        Ok(())
    })
    .unwrap();

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
    let collection_query = EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::SmartFolder,
            key: None,
            id: Some(8),
        },
        filters: QueryFilters::default(),
        sort: QuerySort::default(),
        page: QueryPage::default(),
    };
    let collection_page = db.query_entity_view(&collection_query).unwrap();
    assert_eq!(collection_page.total_count, Some(2));
    assert!(collection_page
        .items
        .iter()
        .any(|item| item.entity_id == collection_id));
    db.with_read(|conn| {
        let direct_count: i64 = conn.query_row(
            "SELECT count FROM sidebar_node WHERE node_id = 'smart:7'",
            [],
            |row| row.get(0),
        )?;
        let collection_count: i64 = conn.query_row(
            "SELECT count FROM sidebar_node WHERE node_id = 'smart:8'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(direct_count, 1);
        assert_eq!(collection_count, 2);
        Ok(())
    })
    .expect("read sidebar count");

    db.remove_tags(
        &[child_id],
        &["landscape".to_string()],
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    db.run_compiler(crate::db::projection::compiler::CompilerPlan {
        dirty_tag_ids: vec![1],
        rebuild_all_smart_folders: true,
        rebuild_sidebar: true,
        ..Default::default()
    });
    assert_eq!(
        db.query_entity_view(&collection_query).unwrap().total_count,
        Some(1)
    );

    db.add_tags(
        &[child_id],
        &["landscape".to_string()],
        1,
        crate::db::types::ExpansionMode::EntityOnly,
    )
    .unwrap();
    crate::ingest::apply_compiler_plan(
        &db,
        &crate::ingest::IngestFlags {
            tags_changed: true,
            ..Default::default()
        },
        &[],
    );
    assert_eq!(
        db.query_entity_view(&collection_query).unwrap().total_count,
        Some(2)
    );

    for (status, expected) in [(2, 1), (1, 2)] {
        db.set_entity_status(
            &[collection_id],
            status,
            crate::db::types::ExpansionMode::EntityOnly,
        )
        .unwrap();
        db.run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_status: true,
            rebuild_sidebar: true,
            ..Default::default()
        });
        assert_eq!(
            db.query_entity_view(&collection_query).unwrap().total_count,
            Some(expected)
        );
    }
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
        conn.execute(
            "INSERT INTO folder (folder_id, name, date_added, date_modified)
             VALUES (10, 'Folder', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO folder_member (folder_id, entity_id, position_rank)
             VALUES (10, 4, 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0)",
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

    let result = db
        .resolve_duplicate_pair("keep_left", "left_single", "right_single", Some(1))
        .expect("resolve with selected collection");
    assert!(matches!(result.status, DuplicateResolveStatus::Resolved));
    assert_eq!(result.loser_file_hash.as_deref(), Some("file_right"));
    assert_eq!(result.affected_collection_ids, vec![1, 2]);
    assert_eq!(result.affected_folder_ids, vec![10]);
    db.with_read(|conn| {
        let (parent_collection, folder_membership): (Option<i64>, i64) = conn.query_row(
            "SELECT
                (SELECT parent_collection_entity_id FROM media_entity WHERE entity_hash = 'left_single'),
                (SELECT COUNT(*) FROM folder_member fm
                 JOIN media_entity me ON me.entity_id = fm.entity_id
                 WHERE fm.folder_id = 10 AND me.entity_hash = 'left_single')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(parent_collection, Some(1));
        assert_eq!(folder_membership, 1);
        Ok(())
    })
    .expect("verify ownership and folder reference repointing");
}

#[test]
fn resolve_duplicate_pair_rejects_entities_without_a_detected_pair() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'left_single', 'single', 1, 'Left', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'right_single', 'single', 1, 'Right', '2026-04-01', '2026-04-01', '2026-04-01')",
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
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
            [],
        )?;
        Ok(())
    })
    .expect("seed unrelated entities");

    let error = db
        .resolve_duplicate_pair("keep_left", "left_single", "right_single", None)
        .expect_err("unreviewed entities must not resolve");
    assert!(error.contains("not awaiting review"));

    let (entities, files) = db
        .with_read(|conn| {
            Ok((
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| {
                    row.get::<_, i64>(0)
                })?,
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            ))
        })
        .expect("count surviving rows");
    assert_eq!((entities, files), (2, 2));
}

#[test]
fn resolve_duplicate_pair_rejects_stale_trash_candidates_without_mutation() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'left_single', 'single', 1, 'Left', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'right_single', 'single', 1, 'Right', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, has_audio,
                perceptual_hash, date_added
             ) VALUES
                (1, 'file_left', 'image/png', 100, 100, 100, 0, 'hash_left', '2026-04-01'),
                (2, 'file_right', 'image/jpeg', 200, 200, 200, 0, 'hash_right', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0)",
            [],
        )?;
        conn.execute("UPDATE media_entity SET status = 2 WHERE entity_id = 2", [])?;
        Ok(())
    })
    .expect("seed stale trash duplicate fixture");

    let error = db
        .resolve_duplicate_pair("smart_merge", "left_single", "right_single", None)
        .expect_err("stale trash candidates must not resolve");
    assert!(error.contains("active or inbox"));

    db.with_read(|conn| {
        let (entities, files, duplicate_status): (i64, i64, String) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM media_entity),
                (SELECT COUNT(*) FROM media_file),
                (SELECT status FROM duplicate WHERE file_id_a = 1 AND file_id_b = 2)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(
            (entities, files, duplicate_status.as_str()),
            (2, 2, "detected")
        );
        Ok(())
    })
    .expect("verify stale trash resolution made no mutation");
}

#[test]
fn duplicate_similarity_uses_the_full_256_bit_hash() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'left_single', 'single', 1, 'Left', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'right_single', 'single', 1, 'Right', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                has_audio, perceptual_hash, date_added
             ) VALUES
                (1, 'file_left', 'image/png', 1, 100, 100, 1, 0, 'hash_left', '2026-04-01'),
                (2, 'file_right', 'image/png', 1, 100, 100, 1, 0, 'hash_right', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 64)",
            [],
        )?;
        Ok(())
    })
    .expect("seed duplicate percentage data");

    let page = db
        .get_duplicate_pairs(None, 10, Some("detected".to_string()), None)
        .expect("read duplicate page");
    assert_eq!(page.items[0].similarity_pct, 75.0);
}

#[test]
fn duplicate_visibility_includes_inbox_and_excludes_trash_immediately() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'inbox_single', 'single', 0, 'Inbox', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'active_single', 'single', 1, 'Active', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                has_audio, perceptual_hash, date_added
             ) VALUES
                (1, 'inbox_file', 'image/png', 1, 100, 100, 1, 0, 'hash_inbox', '2026-04-01'),
                (2, 'active_file', 'image/png', 1, 100, 100, 1, 0, 'hash_active', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 4)",
            [],
        )?;
        Ok(())
    })
    .expect("seed inbox duplicate fixture");

    assert_eq!(db.get_duplicate_count().unwrap(), 1);
    assert_eq!(
        db.get_duplicate_pairs(None, 10, Some("detected".to_string()), None)
            .unwrap()
            .items
            .len(),
        1
    );

    db.with_write(|conn| {
        conn.execute("UPDATE media_entity SET status = 2 WHERE entity_id = 1", [])?;
        Ok(())
    })
    .expect("move inbox candidate to trash");
    assert_eq!(db.get_duplicate_count().unwrap(), 0);
    assert!(db
        .get_duplicate_pairs(None, 10, Some("detected".to_string()), None)
        .unwrap()
        .items
        .is_empty());

    db.with_write(|conn| {
        conn.execute("UPDATE media_entity SET status = 0 WHERE entity_id = 1", [])?;
        Ok(())
    })
    .expect("restore inbox candidate");
    assert_eq!(db.get_duplicate_count().unwrap(), 1);
    assert_eq!(
        db.get_duplicate_pairs(None, 10, Some("detected".to_string()), None)
            .unwrap()
            .items
            .len(),
        1
    );
}

#[test]
fn duplicate_rescan_reconciles_detected_truth_and_preserves_decisions() {
    let db = open_test_db();
    let mut one_bit = [0u8; 32];
    one_bit[0] = 0x80;
    let mut two_bits = [0u8; 32];
    two_bits[0] = 0x03;
    let far = [0xffu8; 32];
    let hashes = [
        ImageHash::<Vec<u8>>::from_bytes(&[0u8; 32])
            .unwrap()
            .to_base64(),
        ImageHash::<Vec<u8>>::from_bytes(&one_bit)
            .unwrap()
            .to_base64(),
        ImageHash::<Vec<u8>>::from_bytes(&two_bits)
            .unwrap()
            .to_base64(),
    ];

    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'scan_a', 'single', 1, 'A', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'scan_b', 'single', 1, 'B', '2026-04-01', '2026-04-01', '2026-04-01'),
                (3, 'scan_c', 'single', 1, 'C', '2026-04-01', '2026-04-01', '2026-04-01'),
                (4, 'scan_short', 'single', 1, 'Short', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        for (file_id, hash) in hashes.iter().enumerate() {
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height,
                    frame_count, has_audio, perceptual_hash, date_added
                 ) VALUES (?1, ?2, 'image/png', 1, 1, 1, 1, 0, ?3, '2026-04-01')",
                params![file_id as i64 + 1, format!("scan_file_{file_id}"), hash],
            )?;
            conn.execute(
                "INSERT INTO single_media_entity (entity_id, file_id) VALUES (?1, ?1)",
                [file_id as i64 + 1],
            )?;
        }
        let short_hash = ImageHash::<Vec<u8>>::from_bytes(&[0u8; 8])
            .unwrap()
            .to_base64();
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height,
                frame_count, has_audio, perceptual_hash, date_added
             ) VALUES (4, 'scan_short_file', 'image/png', 1, 1, 1, 1, 0, ?1, '2026-04-01')",
            [&short_hash],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (4, 4)",
            [],
        )?;
        Ok(())
    })
    .expect("seed rescan fixture");

    let first = db.scan_duplicates(Some(2), Some(2)).expect("initial scan");
    assert_eq!(first.candidates_found, 2);
    assert_eq!(first.pairs_inserted, 2);
    assert_eq!(first.reviewable_detected_new, 2);
    assert_eq!(first.files_scanned, 4);
    assert_eq!(first.files_with_phash, 3);

    db.with_write(|conn| {
        conn.execute(
            "UPDATE duplicate SET status = 'ignored_false_positive'
             WHERE file_id_a = 1 AND file_id_b = 3",
            [],
        )?;
        Ok(())
    })
    .expect("preserve explicit decision");

    let narrowed = db.scan_duplicates(Some(1), Some(1)).expect("narrow scan");
    assert_eq!(narrowed.candidates_found, 1);
    assert_eq!(narrowed.pairs_inserted, 0);
    assert_eq!(narrowed.reviewable_detected_new, 0);
    assert_eq!(narrowed.reviewable_detected_total, 1);

    let mut changed_b = [0u8; 32];
    changed_b[0] = 0xc0;
    let changed_b = ImageHash::<Vec<u8>>::from_bytes(&changed_b)
        .unwrap()
        .to_base64();
    let far = ImageHash::<Vec<u8>>::from_bytes(&far).unwrap().to_base64();
    db.with_write(|conn| {
        conn.execute(
            "UPDATE media_file SET perceptual_hash = ?1 WHERE file_id = 2",
            [&changed_b],
        )?;
        conn.execute(
            "UPDATE media_file SET perceptual_hash = ?1 WHERE file_id = 3",
            [&far],
        )?;
        Ok(())
    })
    .expect("change hashes");

    let changed = db
        .scan_duplicates(Some(2), Some(2))
        .expect("rescan changed hashes");
    assert_eq!(changed.candidates_found, 1);
    assert_eq!(changed.pairs_inserted, 0);
    assert_eq!(changed.reviewable_detected_new, 0);
    let pairs = db
        .get_duplicate_pairs(None, 10, Some("detected".to_string()), None)
        .expect("read reconciled pairs");
    assert_eq!(pairs.items.len(), 1);
    assert_eq!(pairs.items[0].distance, 2.0);

    db.with_read(|conn| {
        let ignored: String = conn.query_row(
            "SELECT status FROM duplicate WHERE file_id_a = 1 AND file_id_b = 3",
            [],
            |row| row.get(0),
        )?;
        let stale_detected: i64 = conn.query_row(
            "SELECT COUNT(*) FROM duplicate
             WHERE status = 'detected' AND file_id_a = 2 AND file_id_b = 3",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(ignored, "ignored_false_positive");
        assert_eq!(stale_detected, 0);
        Ok(())
    })
    .expect("verify reconciled statuses");
}

#[test]
fn duplicate_resolution_decisions_preserve_truth_and_report_physical_hash() {
    for action in [
        "not_duplicate",
        "keep_both",
        "keep_left",
        "keep_right",
        "smart_merge",
    ] {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES
                    (1, 'left_single', 'single', 1, 'Left', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (2, 'right_single', 'single', 1, 'Right', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                    has_audio, perceptual_hash, date_added
                 ) VALUES
                    (1, 'physical_left', 'image/png', 200, 200, 200, 1, 0, 'hash_left', '2026-04-01'),
                    (2, 'physical_right', 'image/jpeg', 100, 100, 100, 1, 0, 'hash_right', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO deferred_work_item
                    (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                    ('physical_right', 'thumbnail', 'pending', 0, '2026-04-01', '2026-04-01'),
                    ('physical_right', 'dominant_colors', 'pending', 0, '2026-04-01', '2026-04-01'),
                    ('physical_right', 'perceptual_hash', 'pending', 0, '2026-04-01', '2026-04-01')",
                [],
            )?;
            Ok(())
        })
        .expect("seed decision data");

        let result = db
            .resolve_duplicate_pair(action, "left_single", "right_single", None)
            .expect("resolve duplicate decision");

        assert!(matches!(result.status, DuplicateResolveStatus::Resolved));
        match action {
            "not_duplicate" => {
                assert!(result.winner_hash.is_none());
                assert!(result.loser_file_hash.is_none());
                assert_eq!(db.get_duplicate_count().unwrap(), 0);
                db.with_read(|conn| {
                    let status: String = conn.query_row(
                        "SELECT status FROM duplicate WHERE file_id_a = 1 AND file_id_b = 2",
                        [],
                        |row| row.get(0),
                    )?;
                    assert_eq!(status, "ignored_false_positive");
                    Ok(())
                })
                .expect("read not-duplicate decision");
            }
            "keep_both" => {
                assert!(result.winner_hash.is_none());
                assert!(result.loser_file_hash.is_none());
                db.with_read(|conn| {
                    let (entities, status): (i64, String) = conn.query_row(
                        "SELECT (SELECT COUNT(*) FROM media_entity),
                                (SELECT status FROM duplicate WHERE file_id_a = 1 AND file_id_b = 2)",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    assert_eq!(entities, 2);
                    assert_eq!(status, "dismissed_keep_both");
                    Ok(())
                })
                .expect("read keep-both decision");
            }
            "keep_left" | "smart_merge" => {
                assert_eq!(result.winner_hash.as_deref(), Some("left_single"));
                assert_eq!(result.loser_hash.as_deref(), Some("right_single"));
                assert_eq!(result.loser_file_hash.as_deref(), Some("physical_right"));
                assert_eq!(db.get_duplicate_count().unwrap(), 0);
            }
            "keep_right" => {
                assert_eq!(result.winner_hash.as_deref(), Some("right_single"));
                assert_eq!(result.loser_hash.as_deref(), Some("left_single"));
                assert_eq!(result.loser_file_hash.as_deref(), Some("physical_left"));
                assert_eq!(db.get_duplicate_count().unwrap(), 0);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn duplicate_resolution_persists_after_database_reopen() {
    let temp = TempDir::new().expect("create restart fixture");
    {
        let db = LibraryDatabase::open(temp.path()).expect("open initial database");
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES
                    (1, 'left_single', 'single', 1, 'Left', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (2, 'right_single', 'single', 1, 'Right', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                    has_audio, perceptual_hash, date_added
                 ) VALUES
                    (1, 'physical_left', 'image/png', 200, 200, 200, 1, 0, 'hash_left', '2026-04-01'),
                    (2, 'physical_right', 'image/jpeg', 100, 100, 100, 1, 0, 'hash_right', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO deferred_work_item
                    (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                    ('physical_right', 'thumbnail', 'pending', 0, '2026-04-01', '2026-04-01'),
                    ('physical_right', 'dominant_colors', 'pending', 0, '2026-04-01', '2026-04-01'),
                    ('physical_right', 'perceptual_hash', 'pending', 0, '2026-04-01', '2026-04-01')",
                [],
            )?;
            Ok(())
        })
        .expect("seed restart fixture");

        let result = db
            .resolve_duplicate_pair("keep_left", "left_single", "right_single", None)
            .expect("resolve before restart");
        assert_eq!(result.loser_file_hash.as_deref(), Some("physical_right"));
        assert!(result.blob_cleanup_pending);

        let remaining_jobs = db
            .list_deferred_work_items(DeferredWorkFilter {
                entity_hash: Some("physical_right".to_string()),
                ..Default::default()
            })
            .expect("list loser deferred work");
        assert_eq!(remaining_jobs.len(), 1);
        assert_eq!(remaining_jobs[0].work_type, DeferredWorkType::BlobDelete);

        let jobs = db
            .list_deferred_work_items(DeferredWorkFilter {
                work_type: Some(DeferredWorkType::BlobDelete),
                ..Default::default()
            })
            .expect("list pending blob cleanup");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].entity_hash, "physical_right");
        assert_eq!(
            jobs[0].status,
            crate::background_work::DeferredWorkStatus::Pending
        );

        let claimed = db
            .claim_next_deferred_work_items()
            .expect("claim blob cleanup");
        assert_eq!(claimed.len(), 1);
    }

    let reopened = LibraryDatabase::open(temp.path()).expect("reopen database");
    assert_eq!(reopened.reset_running_deferred_work_items().unwrap(), 1);
    let recovered = reopened
        .list_deferred_work_items(DeferredWorkFilter {
            work_type: Some(DeferredWorkType::BlobDelete),
            ..Default::default()
        })
        .expect("read recovered cleanup");
    assert_eq!(
        recovered[0].status,
        crate::background_work::DeferredWorkStatus::Pending
    );
    reopened
        .retry_blob_delete_for_hash("physical_right", "simulated cleanup failure")
        .expect("persist cleanup retry");
    let retried = reopened
        .list_deferred_work_items(DeferredWorkFilter {
            work_type: Some(DeferredWorkType::BlobDelete),
            ..Default::default()
        })
        .expect("read cleanup retry");
    assert_eq!(retried[0].attempt_count, 1);
    assert_eq!(
        retried[0].last_error.as_deref(),
        Some("simulated cleanup failure")
    );
    reopened
        .complete_blob_delete_for_hash("physical_right")
        .expect("complete cleanup by hash");
    assert!(reopened
        .list_deferred_work_items(DeferredWorkFilter {
            entity_hash: Some("physical_right".to_string()),
            ..Default::default()
        })
        .unwrap()
        .is_empty());
    reopened
        .with_read(|conn| {
            let (entities, files, detected): (i64, i64, i64) = conn.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM media_entity),
                    (SELECT COUNT(*) FROM media_file),
                    (SELECT COUNT(*) FROM duplicate WHERE status = 'detected')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!((entities, files, detected), (1, 1, 0));
            Ok(())
        })
        .expect("verify duplicate resolution after restart");
}

#[test]
fn smart_merge_preserves_both_files_when_quality_is_ambiguous() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'left_single', 'single', 1, 'Left', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'right_single', 'single', 1, 'Right', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                has_audio, perceptual_hash, date_added
             ) VALUES
                (1, 'file_left', 'image/png', 1000, 100, 100, 1, 0, 'hash_left', '2026-04-01'),
                (2, 'file_right', 'image/png', 1000, 100, 100, 1, 0, 'hash_right', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0)",
            [],
        )?;
        Ok(())
    })
    .expect("seed ambiguous duplicate pair");

    let result = db
        .resolve_duplicate_pair("smart_merge", "left_single", "right_single", None)
        .expect("evaluate smart merge");
    assert!(matches!(
        result.status,
        DuplicateResolveStatus::QualityAmbiguous
    ));

    let (entities, review_pairs) = db
        .with_read(|conn| {
            Ok((
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| {
                    row.get::<_, i64>(0)
                })?,
                conn.query_row(
                    "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?,
            ))
        })
        .expect("count preserved duplicate rows");
    assert_eq!((entities, review_pairs), (2, 1));
}

#[test]
fn smart_merge_keeps_the_quality_winner_and_preserves_its_other_matches() {
    let db = open_test_db();
    db.with_write(|conn| {
        conn.execute_batch(LIBRARY_DDL)?;
        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES
                (1, 'larger_png', 'single', 1, 'Larger PNG', '2026-04-01', '2026-04-01', '2026-04-01'),
                (2, 'smaller_jpeg', 'single', 1, 'Smaller JPEG', '2026-04-01', '2026-04-01', '2026-04-01'),
                (3, 'other_match', 'single', 1, 'Other match', '2026-04-01', '2026-04-01', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                has_audio, perceptual_hash, date_added
             ) VALUES
                (1, 'file_png', 'image/png', 1200000, 4570, 1191, 1, 0, 'hash_a', '2026-04-01'),
                (2, 'file_jpeg', 'image/jpeg', 225600, 4096, 1067, 1, 0, 'hash_b', '2026-04-01'),
                (3, 'file_other', 'image/png', 1000, 100, 100, 1, 0, 'hash_c', '2026-04-01')",
            [],
        )?;
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2), (3, 3)",
            [],
        )?;
        conn.execute(
            "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 0), (1, 3, 1), (2, 3, 1)",
            [],
        )?;
        Ok(())
    })
    .expect("seed connected duplicate pairs");

    let result = db
        .resolve_duplicate_pair("smart_merge", "larger_png", "smaller_jpeg", None)
        .expect("smart merge quality winner");
    assert!(matches!(result.status, DuplicateResolveStatus::Resolved));
    assert_eq!(result.winner_hash.as_deref(), Some("larger_png"));
    assert_eq!(result.loser_hash.as_deref(), Some("smaller_jpeg"));

    db.with_read(|conn| {
        let surviving_entities: Vec<String> = conn
            .prepare("SELECT entity_hash FROM media_entity ORDER BY entity_id")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let surviving_pairs: Vec<(i64, i64)> = conn
            .prepare("SELECT file_id_a, file_id_b FROM duplicate WHERE status = 'detected'")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(surviving_entities, vec!["larger_png", "other_match"]);
        assert_eq!(surviving_pairs, vec![(1, 3)]);
        Ok(())
    })
    .expect("inspect surviving duplicate graph");
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
