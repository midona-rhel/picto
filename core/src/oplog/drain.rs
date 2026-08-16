//! Drain the durable outbox into immutable segments on a backend.
//!
//! Each device writes only under `oplog/<device-id>/`; segments are numbered
//! and write-once (`put` is create-only), so a name collision means the
//! device's sequence regressed — e.g. restored from backup — and is surfaced
//! as an error instead of ever overwriting.

use crate::db::LibraryDatabase;

use super::backend::SyncBackend;
use super::segment::{encode_segment, MAX_SEGMENT_BYTES};

pub const DEFAULT_OPS_PER_SEGMENT: usize = 512;

pub fn segment_key(device_id: &str, seq: i64) -> String {
    format!("oplog/{device_id}/{seq:016}.seg")
}

pub fn head_key(device_id: &str) -> String {
    format!("oplog/{device_id}/head")
}

/// Upload all pending outbox ops as one or more segments. Returns the number
/// of segments written. Ops are marked uploaded only after the backend
/// accepted the segment; a crash in between re-uploads to the same key next
/// time, where create-only `put` makes the retry idempotent-or-loud.
pub fn drain_outbox(
    db: &LibraryDatabase,
    backend: &dyn SyncBackend,
    ops_per_segment: usize,
) -> Result<usize, String> {
    let mut segments_written = 0usize;
    loop {
        let written = drain_outbox_batch(db, backend, ops_per_segment)?;
        if written == 0 {
            return Ok(segments_written);
        }
        segments_written += written;
    }
}

/// Upload at most one bounded segment. Runtime sync uses this so a large
/// outbox cannot monopolize the worker or outrun the blob batch published
/// immediately before it.
pub fn drain_outbox_batch(
    db: &LibraryDatabase,
    backend: &dyn SyncBackend,
    ops_per_segment: usize,
) -> Result<usize, String> {
    let batch = db.pending_ops(ops_per_segment.max(1))?;
    if batch.is_empty() {
        return Ok(0);
    }
    let seq = db.next_segment_seq()?;
    let ids: Vec<i64> = batch.iter().map(|(id, _)| *id).collect();
    let ops: Vec<_> = batch.into_iter().map(|(_, op)| op).collect();
    let bytes = encode_segment(&ops).map_err(|e| e.to_string())?;
    let key = segment_key(db.device_id(), seq);
    match backend.put(&key, &bytes) {
        Ok(()) => {}
        Err(super::backend::BackendError::AlreadyExists(_)) => {
            let existing = backend
                .get_limited(&key, MAX_SEGMENT_BYTES)
                .map_err(|error| format!("segment retry read {key}: {error}"))?;
            if existing.as_deref() != Some(bytes.as_slice()) {
                return Err(format!(
                    "segment sequence collision at {key}: existing immutable bytes differ"
                ));
            }
        }
        Err(error) => return Err(format!("segment upload {key}: {error}")),
    }
    let head = head_key(db.device_id());
    backend
        .put_replace(&head, seq.to_string().as_bytes())
        .map_err(|error| format!("segment head update {head}: {error}"))?;
    db.mark_ops_uploaded(&ids, seq)?;
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oplog::backend::MemoryBackend;
    use crate::oplog::replay::replay_backend;
    use tempfile::TempDir;

    fn open_db() -> LibraryDatabase {
        let tmp = TempDir::new().unwrap();
        let db = LibraryDatabase::open(tmp.path()).unwrap();
        std::mem::forget(tmp);
        db
    }

    #[test]
    fn drain_then_replay_reconstructs_mutations() {
        let db = open_db();
        let folder_id = db.create_folder("Art", None, None, None).unwrap();
        let file_id = db
            .insert_file(
                "hash_r",
                "image/png",
                5,
                None,
                None,
                None,
                None,
                false,
                "2026-01-01",
            )
            .unwrap();
        let entity_id = db
            .insert_entity(
                "hash_r",
                file_id,
                Some("img"),
                1,
                "2026-01-01",
                "2026-01-01",
            )
            .unwrap();
        db.add_folder_members(folder_id, &[entity_id]).unwrap();

        let backend = MemoryBackend::new();
        let segments = drain_outbox(&db, &backend, 2).unwrap();
        assert!(segments >= 1);
        // Everything drained; a second drain is a no-op.
        assert_eq!(drain_outbox(&db, &backend, 2).unwrap(), 0);

        let state = replay_backend(&backend).unwrap();
        let folder = state
            .folders
            .values()
            .find(|f| f.fields.get("name") == Some(&serde_json::json!("Art")))
            .expect("folder must replay");
        assert!(folder.members.contains("hash_r"));
    }

    #[test]
    fn sequence_collision_is_loud_not_overwriting() {
        let db = open_db();
        db.create_folder("A", None, None, None).unwrap();
        let backend = MemoryBackend::new();
        drain_outbox(&db, &backend, 16).unwrap();

        // Simulate a restored-from-backup outbox: pending op, regressed seq.
        db.with_write(|conn| {
            conn.execute(
                "UPDATE op_outbox
                 SET uploaded_seq = NULL, payload_json = '{\"name\":\"Different\"}'",
                [],
            )
            .map(|_| ())
        })
        .unwrap();
        let err = drain_outbox(&db, &backend, 16).unwrap_err();
        assert!(err.contains("sequence collision"), "got: {err}");
        // The remote segment is untouched.
        let state = replay_backend(&backend).unwrap();
        assert_eq!(state.folders.len(), 1);
    }

    #[test]
    fn identical_segment_retry_repairs_local_ack_and_head() {
        let db = open_db();
        db.create_folder("A", None, None, None).unwrap();
        let backend = MemoryBackend::new();
        drain_outbox(&db, &backend, 16).unwrap();
        db.with_write(|conn| {
            conn.execute("UPDATE op_outbox SET uploaded_seq = NULL", [])
                .map(|_| ())
        })
        .unwrap();

        assert_eq!(drain_outbox(&db, &backend, 16).unwrap(), 1);
        assert_eq!(db.pending_op_count().unwrap(), 0);
        assert_eq!(
            backend.get(&head_key(db.device_id())).unwrap().unwrap(),
            b"1"
        );
    }
}
