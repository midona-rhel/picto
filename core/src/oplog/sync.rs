//! One sync cycle: push pending ops, pull and apply new peer segments.
//!
//! Peer segments are consumed strictly contiguously per device — a gap in
//! sequence numbers (transport still uploading an earlier segment) stops
//! ingestion for that device at the gap, never skips past it. Application
//! and cursor advancement commit in one transaction.

use std::collections::BTreeMap;

use crate::db::LibraryDatabase;

use super::backend::SyncBackend;
use super::drain::{drain_outbox, DEFAULT_OPS_PER_SEGMENT};
use super::segment::decode_segment;
use super::{OpRecord, OP_VERSION};

#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct SyncReport {
    pub segments_uploaded: usize,
    pub segments_consumed: usize,
    pub ops_applied: usize,
    pub blobs_uploaded: usize,
    pub blobs_downloaded: usize,
}

/// Mirror originals between the local blob store and `blobs/f/...` on the
/// remote. Content-addressed and write-once on both sides, so both
/// directions are pure fill-in-the-gaps; nothing is ever replaced.
pub fn sync_blobs(
    blob_store: &crate::blob_store::BlobStore,
    backend: &dyn SyncBackend,
) -> Result<(usize, usize), String> {
    let remote: std::collections::HashSet<String> = backend
        .list("blobs/f/")
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let mut uploaded = 0usize;
    let mut local_keys = std::collections::HashSet::new();
    for (hash, ext) in blob_store.list_originals() {
        let key = format!("blobs/f/{}/{}/{}.{}", &hash[0..2], &hash[2..4], hash, ext);
        local_keys.insert(key.clone());
        if remote.contains(&key) {
            continue;
        }
        let bytes = blob_store
            .read_original(&hash, Some(&ext))
            .map_err(|e| e.to_string())?;
        match backend.put(&key, &bytes) {
            Ok(()) => uploaded += 1,
            Err(super::backend::BackendError::AlreadyExists(_)) => {}
            Err(e) => return Err(format!("blob upload {key}: {e}")),
        }
    }
    let mut downloaded = 0usize;
    for key in &remote {
        if local_keys.contains(key) {
            continue;
        }
        let Some(name) = key.rsplit('/').next() else {
            continue;
        };
        let Some((hash, ext)) = name.split_once('.') else {
            continue;
        };
        if hash.len() != 64 {
            continue;
        }
        if let Some(bytes) = backend.get(key).map_err(|e| e.to_string())? {
            blob_store
                .write_original(hash, &bytes, Some(ext))
                .map_err(|e| e.to_string())?;
            downloaded += 1;
        }
    }
    Ok((uploaded, downloaded))
}

/// Parse `oplog/<device>/<seq>.seg` → `(device, seq)`.
fn parse_segment_key(key: &str) -> Option<(&str, i64)> {
    let rest = key.strip_prefix("oplog/")?;
    let (device, file) = rest.split_once('/')?;
    let seq = file.strip_suffix(".seg")?.parse().ok()?;
    Some((device, seq))
}

pub fn sync_once(db: &LibraryDatabase, backend: &dyn SyncBackend) -> Result<SyncReport, String> {
    let mut report = SyncReport {
        segments_uploaded: drain_outbox(db, backend, DEFAULT_OPS_PER_SEGMENT)?,
        ..Default::default()
    };

    // Group remote segments per peer device.
    let mut per_device: BTreeMap<String, BTreeMap<i64, String>> = BTreeMap::new();
    for key in backend.list("oplog/").map_err(|e| e.to_string())? {
        if let Some((device, seq)) = parse_segment_key(&key) {
            if device != db.device_id() {
                per_device
                    .entry(device.to_string())
                    .or_default()
                    .insert(seq, key);
            }
        }
    }

    let mut new_ops: Vec<OpRecord> = Vec::new();
    let mut cursor_updates: Vec<(String, i64)> = Vec::new();
    for (device, segments) in &per_device {
        let mut cursor = db.ingest_cursor(device)?;
        // Contiguous consumption only: stop at the first missing seq.
        while let Some(key) = segments.get(&(cursor + 1)) {
            let Some(bytes) = backend.get(key).map_err(|e| e.to_string())? else {
                break;
            };
            let ops =
                decode_segment(&bytes).map_err(|e| format!("quarantined segment {key}: {e}"))?;
            new_ops.extend(ops);
            cursor += 1;
            report.segments_consumed += 1;
        }
        if cursor > db.ingest_cursor(device)? {
            cursor_updates.push((device.clone(), cursor));
        }
    }

    if new_ops.is_empty() {
        return Ok(report);
    }
    if let Some(op) = new_ops.iter().find(|op| op.op_version > OP_VERSION) {
        return Err(format!(
            "peer op version {} is newer than this build supports — update required",
            op.op_version
        ));
    }
    new_ops.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    report.ops_applied = db.apply_remote_ops(&new_ops, &cursor_updates)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oplog::backend::MemoryBackend;
    use tempfile::TempDir;

    fn open_device(device_id: &str) -> LibraryDatabase {
        let tmp = TempDir::new().unwrap();
        let db = LibraryDatabase::open_with_device_id(tmp.path(), device_id.to_string()).unwrap();
        std::mem::forget(tmp);
        db
    }

    fn folder_names(db: &LibraryDatabase) -> Vec<(String, Option<String>)> {
        db.with_read(|conn| {
            let mut stmt = conn.prepare("SELECT name, uuid FROM folder ORDER BY name")?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
    }

    #[test]
    fn two_devices_converge_through_a_shared_backend() {
        let backend = MemoryBackend::new();
        let dev_a = open_device("dev-a");
        let dev_b = open_device("dev-b");

        // Device A: an entity with tags, in a folder.
        let folder_a = dev_a.create_folder("Art", None, None, None).unwrap();
        let file_id = dev_a
            .insert_file(
                "hash_s",
                "image/png",
                9,
                Some(64),
                Some(64),
                None,
                None,
                false,
                "2026-01-01",
            )
            .unwrap();
        let entity_a = dev_a
            .insert_single(
                "hash_s",
                file_id,
                Some("pic"),
                1,
                "2026-01-01",
                "2026-01-02",
            )
            .unwrap();
        // insert_single is a non-emitting low-level path, so device B never
        // materializes hash_s: the tag/membership ops referencing it are
        // skipped on B. Entities reach peers via the emitting ingest paths.
        dev_a
            .add_tags(
                &[entity_a],
                &["artist:someone".to_string()],
                crate::db::types::TAG_PROVENANCE_MANUAL,
                crate::db::types::ExpansionMode::EntityOnly,
            )
            .unwrap();
        dev_a
            .add_folder_members(
                folder_a,
                &[entity_a],
                crate::db::types::ExpansionMode::EntityOnly,
            )
            .unwrap();

        let report = sync_once(&dev_a, &backend).unwrap();
        assert!(report.segments_uploaded >= 1);
        assert_eq!(report.ops_applied, 0);

        // Device B pulls A's history.
        let report = sync_once(&dev_b, &backend).unwrap();
        assert!(report.ops_applied > 0);
        let folders_b = folder_names(&dev_b);
        assert_eq!(folders_b.len(), 1);
        assert_eq!(folders_b[0].0, "Art");
        let folders_a = folder_names(&dev_a);
        assert_eq!(folders_a[0].1, folders_b[0].1, "uuid must be identical");

        // Device B renames the folder; A picks it up on its next cycle.
        let folder_b = dev_b
            .with_read(|conn| {
                conn.query_row("SELECT folder_id FROM folder LIMIT 1", [], |r| r.get(0))
            })
            .unwrap();
        dev_b
            .update_folder(
                folder_b,
                &crate::db::types::FolderPatch {
                    name: Some("Artwork".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        sync_once(&dev_b, &backend).unwrap();
        let report = sync_once(&dev_a, &backend).unwrap();
        assert!(report.ops_applied >= 1);
        assert_eq!(folder_names(&dev_a)[0].0, "Artwork");

        // Steady state: nothing new for either.
        assert_eq!(sync_once(&dev_a, &backend).unwrap(), SyncReport::default());
        assert_eq!(sync_once(&dev_b, &backend).unwrap(), SyncReport::default());
    }

    #[test]
    fn sequence_gap_stops_ingestion_without_skipping() {
        let backend = MemoryBackend::new();
        let dev_a = open_device("gap-a");
        let dev_b = open_device("gap-b");

        dev_a.create_folder("First", None, None, None).unwrap();
        sync_once(&dev_a, &backend).unwrap();
        dev_a.create_folder("Second", None, None, None).unwrap();
        sync_once(&dev_a, &backend).unwrap();

        // Simulate transport lag: segment 1 not yet visible to B.
        let seg1 = crate::oplog::drain::segment_key("gap-a", 1);
        let seg1_bytes = backend.get(&seg1).unwrap().unwrap();
        backend.delete(&seg1).unwrap();

        let report = sync_once(&dev_b, &backend).unwrap();
        assert_eq!(report.segments_consumed, 0, "must stop at the gap");
        assert!(folder_names(&dev_b).is_empty());

        // Segment 1 arrives; both segments now apply, in order.
        backend.put(&seg1, &seg1_bytes).unwrap();
        let report = sync_once(&dev_b, &backend).unwrap();
        assert_eq!(report.segments_consumed, 2);
        assert_eq!(
            folder_names(&dev_b)
                .iter()
                .map(|f| f.0.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second"]
        );
    }
}
