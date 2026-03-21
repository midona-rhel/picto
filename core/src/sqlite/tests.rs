use super::publish::Manifest;
use super::DerivedArtifact;
use rusqlite::Connection;

fn init_manifest_tables(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE manifest (
            key TEXT PRIMARY KEY,
            epoch INTEGER NOT NULL
        );
        CREATE TABLE artifact_manifest_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            manifest_epoch INTEGER NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE artifact_manifest_entry (
            manifest_epoch INTEGER NOT NULL,
            artifact_name TEXT NOT NULL,
            artifact_version INTEGER NOT NULL,
            built_from_truth_seq INTEGER NOT NULL DEFAULT 0,
            payload_json TEXT NOT NULL DEFAULT '{}',
            PRIMARY KEY (manifest_epoch, artifact_name)
        );
        ",
    )
    .unwrap();
}

#[test]
fn manifest_readers_do_not_see_unflushed_bumps() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_manifest_tables(&conn);

    let manifest = Manifest::new();
    let initial_bitmap_payload = manifest.published_artifact_payload_json("bitmaps").unwrap();
    assert_eq!(manifest.published_artifact_version("files"), 0);
    assert_eq!(manifest.published_epoch(), 0);

    let new_files_version = manifest.mark_artifact_dirty(DerivedArtifact::Files);
    assert_eq!(new_files_version, 1);
    manifest.set_working_artifact_payload_json(
        "bitmaps",
        "{\"active_file\":\"bitmaps.v1.bin\"}".to_string(),
    );

    assert_eq!(manifest.published_artifact_version("files"), 0);
    assert_eq!(
        manifest.published_artifact_payload_json("bitmaps").unwrap(),
        initial_bitmap_payload
    );
    assert_eq!(manifest.published_epoch(), 0);

    manifest.flush_to_db(&mut conn).unwrap();

    assert_eq!(manifest.published_epoch(), 1);
    assert_eq!(manifest.published_artifact_version("files"), 1);
    assert_eq!(
        manifest.published_artifact_payload_json("bitmaps").unwrap(),
        "{\"active_file\":\"bitmaps.v1.bin\"}"
    );

    let loaded = Manifest::load_from_db(&conn).unwrap();
    assert_eq!(loaded.published_epoch(), 1);
    assert_eq!(loaded.published_artifact_version("files"), 1);
    assert_eq!(
        loaded.published_artifact_payload_json("bitmaps").unwrap(),
        "{\"active_file\":\"bitmaps.v1.bin\"}"
    );
}
