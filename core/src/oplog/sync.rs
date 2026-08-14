//! One sync cycle: push pending ops, pull and apply new peer segments.
//!
//! Peer segments are consumed strictly contiguously per device — a gap in
//! sequence numbers (transport still uploading an earlier segment) stops
//! ingestion for that device at the gap, never skips past it. Application
//! and cursor advancement commit in one transaction.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::db::LibraryDatabase;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use super::backend::SyncBackend;
use super::backend_fs::FsBackend;
use super::drain::{drain_outbox_batch, DEFAULT_OPS_PER_SEGMENT};
use super::remote_library::remote_library_root;
use super::segment::decode_segment;
use super::{OpRecord, OP_VERSION};

pub const MAX_REMOTE_SEGMENTS_PER_CYCLE: usize = 32;

pub const KV_SHARE_ROOT: &str = "sync_share_root";
pub const KV_LIBRARY_NAME: &str = "sync_library_name";
pub const KV_LAST_SUCCESS_AT: &str = "sync_last_success_at";
pub const KV_LAST_ERROR: &str = "sync_last_error";
pub const KV_LAST_REPORT: &str = "sync_last_report";

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SyncReport {
    pub segments_uploaded: usize,
    pub segments_consumed: usize,
    pub ops_applied: usize,
    pub blobs_uploaded: usize,
    pub blobs_downloaded: usize,
    pub waiting_for_prerequisites: bool,
    #[serde(skip)]
    applied_op_types: Vec<String>,
    #[serde(skip)]
    affected_folder_ids: Vec<i64>,
    #[serde(skip)]
    affected_smart_folder_ids: Vec<i64>,
    #[serde(skip)]
    affected_collection_ids: Vec<i64>,
}

pub fn binding(db: &LibraryDatabase) -> Result<Option<(PathBuf, String)>, String> {
    let root = db.kv_get(KV_SHARE_ROOT)?;
    let name = db.kv_get(KV_LIBRARY_NAME)?;
    match (root, name) {
        (Some(root), Some(name)) if !root.is_empty() && !name.is_empty() => {
            Ok(Some((PathBuf::from(root), name)))
        }
        _ => Ok(None),
    }
}

pub fn set_binding(db: &LibraryDatabase, share_root: &Path, name: &str) -> Result<(), String> {
    db.kv_set(KV_SHARE_ROOT, &share_root.display().to_string())?;
    db.kv_set(KV_LIBRARY_NAME, name)
}

pub fn clear_binding(db: &LibraryDatabase) -> Result<(), String> {
    db.kv_delete(KV_SHARE_ROOT)?;
    db.kv_delete(KV_LIBRARY_NAME)
}

fn valid_content_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn verify_content_hash(hash: &str, bytes: &[u8]) -> Result<(), String> {
    if !valid_content_hash(hash) {
        return Err(format!("invalid blob hash in sync object: {hash}"));
    }
    let actual = hex::encode(Sha256::digest(bytes));
    if actual != hash.to_ascii_lowercase() {
        return Err(format!(
            "sync blob failed hash verification: expected {hash}, got {actual}"
        ));
    }
    Ok(())
}

fn entity_blob_key(op: &OpRecord) -> Result<Option<(String, String)>, String> {
    if op.op_type != "entity_created" {
        return Ok(None);
    }
    let hash = op.entity_key.as_str();
    if !valid_content_hash(hash) {
        return Err(format!("remote entity has invalid content hash: {hash}"));
    }
    let mime = op
        .payload
        .get("mime")
        .and_then(|value| value.as_str())
        .unwrap_or("application/octet-stream");
    let ext = crate::blob_store::mime_to_extension(mime).to_string();
    let key = format!("blobs/f/{}/{}/{}.{}", &hash[0..2], &hash[2..4], hash, ext);
    Ok(Some((key, ext)))
}

fn upload_pending_blobs(
    db: &LibraryDatabase,
    blob_store: &crate::blob_store::BlobStore,
    backend: &dyn SyncBackend,
) -> Result<usize, String> {
    let mut uploaded = 0usize;
    let mut seen = std::collections::HashSet::new();
    for (_, op) in db.pending_ops(DEFAULT_OPS_PER_SEGMENT)? {
        let Some((key, ext)) = entity_blob_key(&op)? else {
            continue;
        };
        if !seen.insert(op.entity_key.clone()) {
            continue;
        }
        let bytes = blob_store
            .read_original(&op.entity_key, Some(&ext))
            .map_err(|e| e.to_string())?;
        verify_content_hash(&op.entity_key, &bytes)?;
        match backend.put(&key, &bytes) {
            Ok(()) => uploaded += 1,
            Err(super::backend::BackendError::AlreadyExists(_)) => {}
            Err(e) => return Err(format!("blob upload {key}: {e}")),
        }
    }
    Ok(uploaded)
}

fn hydrate_required_blobs(
    ops: &[OpRecord],
    blob_store: &crate::blob_store::BlobStore,
    backend: &dyn SyncBackend,
) -> Result<(usize, usize), String> {
    let mut downloaded = 0usize;
    let mut pending = 0usize;
    let mut seen = std::collections::HashSet::new();
    for op in ops {
        let Some((key, ext)) = entity_blob_key(op)? else {
            continue;
        };
        if !seen.insert(op.entity_key.clone()) {
            continue;
        }
        if blob_store
            .find_original(&op.entity_key, Some(&ext))
            .map_err(|e| e.to_string())?
            .is_some()
        {
            let bytes = blob_store
                .read_original(&op.entity_key, Some(&ext))
                .map_err(|e| e.to_string())?;
            verify_content_hash(&op.entity_key, &bytes)?;
            continue;
        }
        let Some(bytes) = backend.get(&key).map_err(|e| e.to_string())? else {
            pending += 1;
            continue;
        };
        verify_content_hash(&op.entity_key, &bytes)?;
        blob_store
            .write_original(&op.entity_key, &bytes, Some(&ext))
            .map_err(|e| e.to_string())?;
        downloaded += 1;
    }
    Ok((downloaded, pending))
}

/// One complete synchronization pass. Blobs move first in both directions so
/// no metadata segment can reference bytes that this cycle has not published
/// or hydrated and verified.
pub fn sync_cycle(
    db: &LibraryDatabase,
    blob_store: &crate::blob_store::BlobStore,
    backend: &dyn SyncBackend,
) -> Result<SyncReport, String> {
    let blobs_uploaded = upload_pending_blobs(db, blob_store, backend)?;
    let mut report = sync_once_inner(db, backend, Some(blob_store))?;
    report.blobs_uploaded = blobs_uploaded;
    Ok(report)
}

/// Run the bound library's canonical cycle and persist its latest observable
/// result. The remote is never deleted or overwritten here.
pub fn run_bound_sync(
    db: &LibraryDatabase,
    blob_store: &crate::blob_store::BlobStore,
) -> Result<SyncReport, String> {
    let Some((share_root, name)) = binding(db)? else {
        return Err("This library is not connected to a sync folder".to_string());
    };
    let result = (|| {
        let backend = FsBackend::open(&remote_library_root(&share_root, &name))
            .map_err(|error| format!("Cannot open sync folder: {error}"))?;
        sync_cycle(db, blob_store, &backend)
    })();

    match &result {
        Ok(report) => {
            db.kv_set(KV_LAST_SUCCESS_AT, &chrono::Utc::now().to_rfc3339())?;
            db.kv_delete(KV_LAST_ERROR)?;
            db.kv_set(
                KV_LAST_REPORT,
                &serde_json::to_string(report).map_err(|error| error.to_string())?,
            )?;
            if report.ops_applied > 0 {
                crate::events::emit_state_changed("folder_sync", sync_change_impact(report));
            }
        }
        Err(error) => {
            let _ = db.kv_set(KV_LAST_ERROR, error);
        }
    }
    result
}

fn sync_change_impact(
    report: &SyncReport,
) -> crate::runtime_contract::change_builder::ChangeImpact {
    use crate::runtime_contract::state_change::Domain;

    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new();
    if !report.affected_folder_ids.is_empty() {
        impact = impact
            .add_domain(Domain::Folders)
            .folder_ids(report.affected_folder_ids.clone());
    }
    if !report.affected_smart_folder_ids.is_empty() {
        impact = impact
            .add_domain(Domain::SmartFolders)
            .smart_folder_ids(report.affected_smart_folder_ids.clone());
    }
    let mut exact_scopes = report
        .affected_folder_ids
        .iter()
        .map(|id| format!("folder:{id}"))
        .collect::<Vec<_>>();
    exact_scopes.extend(
        report
            .affected_collection_ids
            .iter()
            .map(|id| format!("collection:{id}")),
    );
    if !exact_scopes.is_empty() {
        impact = impact.extra_grid_scopes(exact_scopes);
    }
    let membership_changed = report.applied_op_types.iter().any(|op_type| {
        matches!(
            op_type.as_str(),
            "folder_members_added"
                | "folder_members_removed"
                | "collection_members_added"
                | "collection_members_removed"
                | "collection_split"
        )
    });
    if membership_changed && !report.affected_folder_ids.is_empty() {
        impact = impact.folder_membership_changed(report.affected_folder_ids.clone());
    }
    for op_type in &report.applied_op_types {
        impact = match op_type.as_str() {
            "entity_created" | "entity_deleted" | "entity_status_changed" => impact
                .add_domains(&[Domain::Files, Domain::Sidebar])
                .status_changed()
                .status_sensitive_grid_scopes_changed(),
            "entity_updated" => impact
                .add_domain(Domain::Files)
                .media_metadata_changed()
                .all_smart_folder_scopes_changed(),
            "entity_tags_added" | "entity_tags_removed" => impact
                .add_domains(&[Domain::Files, Domain::Tags, Domain::Sidebar])
                .tags_changed()
                .all_smart_folder_scopes_changed(),
            "tag_renamed"
            | "tag_merged"
            | "tag_deleted"
            | "tag_alias_set"
            | "tag_implication_set" => impact
                .add_domains(&[Domain::Tags, Domain::Sidebar])
                .tag_structure_changed_fact()
                .all_smart_folder_scopes_changed(),
            "folder_created" | "folder_updated" | "folder_moved" | "folder_deleted" => {
                impact.add_domains(&[Domain::Folders, Domain::Sidebar])
            }
            "folder_members_added" | "folder_members_removed" => {
                impact.add_domains(&[Domain::Files, Domain::Folders, Domain::Sidebar])
            }
            "smart_folder_created"
            | "smart_folder_updated"
            | "smart_folder_moved"
            | "smart_folder_deleted" => {
                impact.add_domains(&[Domain::SmartFolders, Domain::Sidebar])
            }
            "collection_created"
            | "collection_split"
            | "collection_members_added"
            | "collection_members_removed" => impact
                .add_domains(&[Domain::Files, Domain::Folders, Domain::Sidebar])
                .status_changed()
                .status_sensitive_grid_scopes_changed()
                .all_smart_folder_scopes_changed(),
            "collection_renamed" => impact
                .add_domains(&[Domain::Files, Domain::Folders])
                .media_metadata_changed(),
            "collection_members_reordered" => impact
                .add_domains(&[Domain::Files, Domain::Folders])
                .grid_reorder(),
            "duplicate_decided" => impact.add_domains(&[Domain::Files, Domain::Sidebar]),
            _ => impact,
        };
    }
    impact.compiler_batch_done = Some(true);
    impact
}

#[derive(Default)]
struct SyncScopeImpact {
    folder_ids: BTreeSet<i64>,
    smart_folder_ids: BTreeSet<i64>,
    collection_ids: BTreeSet<i64>,
}

impl SyncScopeImpact {
    fn merge(&mut self, other: Self) {
        self.folder_ids.extend(other.folder_ids);
        self.smart_folder_ids.extend(other.smart_folder_ids);
        self.collection_ids.extend(other.collection_ids);
    }
}

fn resolve_scope_impact(db: &LibraryDatabase, ops: &[OpRecord]) -> Result<SyncScopeImpact, String> {
    db.with_read(|conn| {
        let mut impact = SyncScopeImpact::default();
        let mut folder = conn.prepare_cached("SELECT folder_id FROM folder WHERE uuid = ?1")?;
        let mut smart =
            conn.prepare_cached("SELECT smart_folder_id FROM smart_folder WHERE uuid = ?1")?;
        let mut entity = conn.prepare_cached(
            "SELECT entity_id, parent_collection_entity_id, entity_kind
             FROM media_entity WHERE entity_hash = ?1",
        )?;
        for op in ops {
            if op.op_type.starts_with("folder_") {
                if let Some(id) = folder
                    .query_row([&op.entity_key], |row| row.get::<_, i64>(0))
                    .optional()?
                {
                    impact.folder_ids.insert(id);
                }
            } else if op.op_type.starts_with("smart_folder_") {
                if let Some(id) = smart
                    .query_row([&op.entity_key], |row| row.get::<_, i64>(0))
                    .optional()?
                {
                    impact.smart_folder_ids.insert(id);
                }
            } else if let Some((entity_id, parent_id, kind)) = entity
                .query_row([&op.entity_key], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .optional()?
            {
                if kind == "collection" {
                    impact.collection_ids.insert(entity_id);
                } else if let Some(collection_id) = parent_id {
                    impact.collection_ids.insert(collection_id);
                }
            }
        }
        let mut collection_folders = conn.prepare_cached(
            "SELECT DISTINCT folder_id FROM folder_member
             WHERE entity_id = ?1 OR entity_id IN (
                 SELECT entity_id FROM media_entity WHERE parent_collection_entity_id = ?1
             )",
        )?;
        for collection_id in &impact.collection_ids {
            let rows = collection_folders.query_map([collection_id], |row| row.get::<_, i64>(0))?;
            for row in rows {
                impact.folder_ids.insert(row?);
            }
        }
        Ok(impact)
    })
}

/// Serialize every trigger (startup, periodic, and manual) through the same
/// cycle so two callers cannot publish the same segment concurrently.
pub async fn run_serialized_sync(
    db: Arc<LibraryDatabase>,
    blob_store: Arc<crate::blob_store::BlobStore>,
    cycle_lock: Arc<tokio::sync::Mutex<()>>,
) -> Result<SyncReport, String> {
    let _guard = cycle_lock.lock().await;
    tokio::task::spawn_blocking(move || run_bound_sync(&db, &blob_store))
        .await
        .map_err(|error| format!("sync worker failed: {error}"))?
}

/// Parse `oplog/<device>/<seq>.seg` → `(device, seq)`.
fn parse_segment_key(key: &str) -> Option<(&str, i64)> {
    let rest = key.strip_prefix("oplog/")?;
    let (device, file) = rest.split_once('/')?;
    let seq = file.strip_suffix(".seg")?.parse().ok()?;
    Some((device, seq))
}

fn sync_once_inner(
    db: &LibraryDatabase,
    backend: &dyn SyncBackend,
    blob_store: Option<&crate::blob_store::BlobStore>,
) -> Result<SyncReport, String> {
    let mut report = SyncReport {
        segments_uploaded: drain_outbox_batch(db, backend, DEFAULT_OPS_PER_SEGMENT)?,
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

    let initial_cursors = per_device
        .keys()
        .map(|device| Ok((device.clone(), db.ingest_cursor(device)?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut cursors = initial_cursors.clone();
    let mut device_order = per_device.keys().cloned().collect::<Vec<_>>();
    if !device_order.is_empty() {
        let rotation = initial_cursors
            .values()
            .fold(0usize, |sum, cursor| sum.wrapping_add(*cursor as usize))
            % device_order.len();
        device_order.rotate_left(rotation);
    }
    let mut new_ops: Vec<OpRecord> = Vec::new();
    let mut candidate_segments = 0usize;
    while candidate_segments < MAX_REMOTE_SEGMENTS_PER_CYCLE {
        let mut progressed = false;
        for device in &device_order {
            if candidate_segments >= MAX_REMOTE_SEGMENTS_PER_CYCLE {
                break;
            }
            let segments = &per_device[device];
            let Some(cursor) = cursors.get_mut(device) else {
                continue;
            };
            let Some(key) = segments.get(&(*cursor + 1)) else {
                continue;
            };
            let Some(bytes) = backend.get(key).map_err(|e| e.to_string())? else {
                continue;
            };
            let ops =
                decode_segment(&bytes).map_err(|e| format!("quarantined segment {key}: {e}"))?;
            new_ops.extend(ops);
            *cursor += 1;
            candidate_segments += 1;
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    let cursor_updates = cursors
        .into_iter()
        .filter(|(device, cursor)| initial_cursors.get(device).is_some_and(|old| cursor > old))
        .collect::<Vec<_>>();

    if new_ops.is_empty() {
        return Ok(report);
    }
    if let Some(op) = new_ops.iter().find(|op| op.op_version > OP_VERSION) {
        return Err(format!(
            "peer op version {} is newer than this build supports — update required",
            op.op_version
        ));
    }
    if let Some(blob_store) = blob_store {
        let (downloaded, pending) = hydrate_required_blobs(&new_ops, blob_store, backend)?;
        report.blobs_downloaded = downloaded;
        report.waiting_for_prerequisites = pending > 0;
        if pending > 0 {
            return Ok(report);
        }
    }
    new_ops.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let mut scope_impact = resolve_scope_impact(db, &new_ops)?;
    match db.apply_remote_ops(&new_ops, &cursor_updates)? {
        Some(applied) => {
            scope_impact.merge(resolve_scope_impact(db, &new_ops)?);
            report.ops_applied = applied;
            report.segments_consumed = candidate_segments;
            report.applied_op_types = new_ops.iter().map(|op| op.op_type.clone()).collect();
            report.affected_folder_ids = scope_impact.folder_ids.into_iter().collect();
            report.affected_smart_folder_ids = scope_impact.smart_folder_ids.into_iter().collect();
            report.affected_collection_ids = scope_impact.collection_ids.into_iter().collect();
        }
        None => report.waiting_for_prerequisites = true,
    }
    Ok(report)
}

#[cfg(test)]
pub fn sync_once(db: &LibraryDatabase, backend: &dyn SyncBackend) -> Result<SyncReport, String> {
    sync_once_inner(db, backend, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oplog::backend::{BackendError, MemoryBackend};
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct BlobBeforeSegmentBackend {
        inner: MemoryBackend,
        expected_blob_key: String,
        writes: Mutex<Vec<String>>,
    }

    impl BlobBeforeSegmentBackend {
        fn new(expected_blob_key: String) -> Self {
            Self {
                inner: MemoryBackend::new(),
                expected_blob_key,
                writes: Mutex::new(Vec::new()),
            }
        }
    }

    impl SyncBackend for BlobBeforeSegmentBackend {
        fn put(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
            if key.starts_with("oplog/") && self.inner.get(&self.expected_blob_key)?.is_none() {
                return Err(BackendError::Io(
                    "metadata was published before its blob".to_string(),
                ));
            }
            self.inner.put(key, bytes)?;
            self.writes.lock().unwrap().push(key.to_string());
            Ok(())
        }

        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, BackendError> {
            self.inner.get(key)
        }

        fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
            self.inner.list(prefix)
        }

        fn delete(&self, key: &str) -> Result<(), BackendError> {
            self.inner.delete(key)
        }
    }

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

        // Device A creates a folder.
        dev_a.create_folder("Art", None, None, None).unwrap();

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

    #[test]
    fn remote_ingestion_is_bounded_and_resumes_from_its_cursor() {
        let backend = MemoryBackend::new();
        let target = open_device("bounded-remote-target");
        let total_segments = MAX_REMOTE_SEGMENTS_PER_CYCLE + 3;
        for seq in 1..=total_segments {
            let op = OpRecord {
                op_version: OP_VERSION,
                op_type: "folder_created".into(),
                entity_key: format!("remote-folder-{seq}"),
                payload: serde_json::json!({"name": format!("Remote {seq}")}),
                hlc: format!("{seq:013x}-0000"),
                device_id: "bounded-remote-source".into(),
            };
            backend
                .put(
                    &crate::oplog::drain::segment_key("bounded-remote-source", seq as i64),
                    &crate::oplog::segment::encode_segment(&[op]).unwrap(),
                )
                .unwrap();
        }
        let peer_op = OpRecord {
            op_version: OP_VERSION,
            op_type: "folder_created".into(),
            entity_key: "other-peer-folder".into(),
            payload: serde_json::json!({"name": "Other peer"}),
            hlc: "0000000000001-0000".into(),
            device_id: "zz-remote-peer".into(),
        };
        backend
            .put(
                &crate::oplog::drain::segment_key("zz-remote-peer", 1),
                &crate::oplog::segment::encode_segment(&[peer_op]).unwrap(),
            )
            .unwrap();

        let first = sync_once(&target, &backend).unwrap();
        assert_eq!(first.segments_consumed, MAX_REMOTE_SEGMENTS_PER_CYCLE);
        assert_eq!(
            target.ingest_cursor("bounded-remote-source").unwrap(),
            (MAX_REMOTE_SEGMENTS_PER_CYCLE - 1) as i64
        );
        assert_eq!(target.ingest_cursor("zz-remote-peer").unwrap(), 1);
        assert_eq!(folder_names(&target).len(), MAX_REMOTE_SEGMENTS_PER_CYCLE);

        let second = sync_once(&target, &backend).unwrap();
        assert_eq!(second.segments_consumed, 4);
        assert_eq!(folder_names(&target).len(), total_segments + 1);
    }

    #[test]
    fn remote_membership_impact_targets_exact_open_grids() {
        let report = SyncReport {
            ops_applied: 2,
            applied_op_types: vec![
                "folder_members_added".into(),
                "collection_members_removed".into(),
            ],
            affected_folder_ids: vec![7],
            affected_collection_ids: vec![11],
            ..Default::default()
        };

        let impact = sync_change_impact(&report);

        assert_eq!(impact.folder_membership_changed, Some(vec![7]));
        let scopes = impact.extra_grid_scopes.unwrap();
        assert!(scopes.contains(&"folder:7".to_string()));
        assert!(scopes.contains(&"collection:11".to_string()));
        assert!(scopes.contains(&"system:active".to_string()));
    }

    #[test]
    fn sync_cycle_publishes_verified_blob_before_metadata_segment() {
        let temp = TempDir::new().unwrap();
        let db = LibraryDatabase::open_with_device_id(temp.path(), "order-a".into()).unwrap();
        let blob_store = crate::blob_store::BlobStore::open(temp.path()).unwrap();
        let bytes = b"sync ordering fixture";
        let hash = hex::encode(Sha256::digest(bytes));
        blob_store
            .write_original(&hash, bytes, Some("png"))
            .unwrap();
        let device_id = db.device_id().to_string();
        db.with_write(|conn| {
            crate::oplog::record_op(
                conn,
                &device_id,
                "entity_created",
                &hash,
                &serde_json::json!({"mime": "image/png", "size": bytes.len()}),
            )
        })
        .unwrap();

        let blob_key = format!("blobs/f/{}/{}/{}.png", &hash[0..2], &hash[2..4], hash);
        let backend = BlobBeforeSegmentBackend::new(blob_key.clone());
        let report = sync_cycle(&db, &blob_store, &backend).unwrap();

        assert_eq!(report.blobs_uploaded, 1);
        assert_eq!(report.segments_uploaded, 1);
        let writes = backend.writes.lock().unwrap();
        assert_eq!(writes.first(), Some(&blob_key));
        assert!(writes.get(1).is_some_and(|key| key.starts_with("oplog/")));
    }

    #[test]
    fn sync_cycle_uploads_one_bounded_metadata_batch() {
        let temp = TempDir::new().unwrap();
        let db = LibraryDatabase::open_with_device_id(temp.path(), "bounded-a".into()).unwrap();
        let blob_store = crate::blob_store::BlobStore::open(temp.path()).unwrap();
        let backend = MemoryBackend::new();
        let device_id = db.device_id().to_string();
        db.with_write(|conn| {
            for index in 0..=DEFAULT_OPS_PER_SEGMENT {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "folder_created",
                    &format!("folder-{index}"),
                    &serde_json::json!({"name": format!("Folder {index}")}),
                )?;
            }
            Ok(())
        })
        .unwrap();

        let report = sync_cycle(&db, &blob_store, &backend).unwrap();

        assert_eq!(report.segments_uploaded, 1);
        assert_eq!(db.pending_op_count().unwrap(), 1);
        assert_eq!(backend.list("oplog/bounded-a/").unwrap().len(), 1);
    }

    #[test]
    fn bound_sync_persists_success_report_and_clears_previous_error() {
        let library_root = TempDir::new().unwrap();
        let share_root = TempDir::new().unwrap();
        let db =
            LibraryDatabase::open_with_device_id(library_root.path(), "status-a".into()).unwrap();
        let blob_store = crate::blob_store::BlobStore::open(library_root.path()).unwrap();
        set_binding(&db, share_root.path(), "Remote").unwrap();
        db.kv_set(KV_LAST_ERROR, "old failure").unwrap();

        let report = run_bound_sync(&db, &blob_store).unwrap();

        assert_eq!(report, SyncReport::default());
        assert!(db.kv_get(KV_LAST_SUCCESS_AT).unwrap().is_some());
        assert_eq!(db.kv_get(KV_LAST_ERROR).unwrap(), None);
        let persisted: SyncReport =
            serde_json::from_str(&db.kv_get(KV_LAST_REPORT).unwrap().unwrap()).unwrap();
        assert_eq!(persisted, report);
    }

    #[test]
    fn unbound_sync_returns_error_without_persisting_a_false_failure() {
        let library_root = TempDir::new().unwrap();
        let db =
            LibraryDatabase::open_with_device_id(library_root.path(), "status-b".into()).unwrap();
        let blob_store = crate::blob_store::BlobStore::open(library_root.path()).unwrap();

        let error = run_bound_sync(&db, &blob_store).unwrap_err();

        assert!(error.contains("not connected"));
        assert_eq!(db.kv_get(KV_LAST_SUCCESS_AT).unwrap(), None);
        assert_eq!(db.kv_get(KV_LAST_ERROR).unwrap(), None);
    }

    #[test]
    fn sync_cycle_rejects_corrupt_remote_bytes_without_writing_them() {
        let source_root = TempDir::new().unwrap();
        let target_root = TempDir::new().unwrap();
        let source =
            LibraryDatabase::open_with_device_id(source_root.path(), "corrupt-a".into()).unwrap();
        let target =
            LibraryDatabase::open_with_device_id(target_root.path(), "corrupt-b".into()).unwrap();
        let blob_store = crate::blob_store::BlobStore::open(target_root.path()).unwrap();
        let expected = b"expected bytes";
        let hash = hex::encode(Sha256::digest(expected));
        let key = format!("blobs/f/{}/{}/{}.png", &hash[0..2], &hash[2..4], hash);
        let backend = MemoryBackend::new();
        let device_id = source.device_id().to_string();
        source
            .with_write(|conn| {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "entity_created",
                    &hash,
                    &serde_json::json!({"mime": "image/png", "size": expected.len()}),
                )
            })
            .unwrap();
        sync_once(&source, &backend).unwrap();
        backend.put(&key, b"corrupt bytes").unwrap();

        let error = sync_cycle(&target, &blob_store, &backend).unwrap_err();

        assert!(error.contains("failed hash verification"));
        assert!(blob_store.read_original(&hash, Some("png")).is_err());
    }

    #[test]
    fn sync_cycle_rejects_corrupt_local_bytes_without_uploading_them() {
        let temp = TempDir::new().unwrap();
        let db = LibraryDatabase::open_with_device_id(temp.path(), "corrupt-local".into()).unwrap();
        let blob_store = crate::blob_store::BlobStore::open(temp.path()).unwrap();
        let hash = hex::encode(Sha256::digest(b"expected bytes"));
        blob_store
            .write_original(&hash, b"corrupt bytes", Some("png"))
            .unwrap();
        let key = format!("blobs/f/{}/{}/{}.png", &hash[0..2], &hash[2..4], hash);
        let backend = MemoryBackend::new();
        let device_id = db.device_id().to_string();
        db.with_write(|conn| {
            crate::oplog::record_op(
                conn,
                &device_id,
                "entity_created",
                &hash,
                &serde_json::json!({"mime": "image/png"}),
            )
        })
        .unwrap();

        let error = sync_cycle(&db, &blob_store, &backend).unwrap_err();

        assert!(error.contains("failed hash verification"));
        assert!(backend.get(&key).unwrap().is_none());
    }

    #[test]
    fn missing_remote_reference_rolls_back_and_leaves_cursor_pending() {
        let backend = MemoryBackend::new();
        let dev_a = open_device("pending-a");
        let dev_b = open_device("pending-b");
        let folder_uuid = "folder-remote";
        let entity_hash = "a".repeat(64);
        let device_id = dev_a.device_id().to_string();
        dev_a
            .with_write(|conn| {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "folder_created",
                    folder_uuid,
                    &serde_json::json!({"name": "Remote"}),
                )?;
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "folder_members_added",
                    folder_uuid,
                    &serde_json::json!({"entities": [entity_hash]}),
                )
            })
            .unwrap();
        sync_once(&dev_a, &backend).unwrap();

        let report = sync_once(&dev_b, &backend).unwrap();
        assert!(report.waiting_for_prerequisites);
        assert_eq!(report.ops_applied, 0);
        assert_eq!(dev_b.ingest_cursor("pending-a").unwrap(), 0);
        assert!(folder_names(&dev_b).is_empty(), "the batch must roll back");

        let file_id = dev_b
            .insert_file(
                &entity_hash,
                "image/png",
                1,
                Some(1),
                Some(1),
                None,
                None,
                false,
                "2026-01-01",
            )
            .unwrap();
        dev_b
            .insert_single(
                &entity_hash,
                file_id,
                Some("ready"),
                1,
                "2026-01-01",
                "2026-01-01",
            )
            .unwrap();

        let report = sync_once(&dev_b, &backend).unwrap();
        assert!(!report.waiting_for_prerequisites);
        assert_eq!(report.ops_applied, 2);
        assert_eq!(dev_b.ingest_cursor("pending-a").unwrap(), 1);
        let membership_count: i64 = dev_b
            .with_read(|conn| {
                conn.query_row("SELECT COUNT(*) FROM folder_member", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(membership_count, 1);
    }

    #[test]
    fn unknown_current_version_op_stops_without_advancing_cursor() {
        let backend = MemoryBackend::new();
        let dev_a = open_device("unknown-a");
        let dev_b = open_device("unknown-b");
        let device_id = dev_a.device_id().to_string();
        dev_a
            .with_write(|conn| {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "future_truth_operation",
                    "future-key",
                    &serde_json::json!({}),
                )
            })
            .unwrap();
        sync_once(&dev_a, &backend).unwrap();

        let error = sync_once(&dev_b, &backend).unwrap_err();

        assert!(error.contains("unknown remote op type"));
        assert!(error.contains("update required"));
        assert_eq!(dev_b.ingest_cursor("unknown-a").unwrap(), 0);
    }

    #[test]
    fn entity_segment_waits_for_verified_blob_then_materializes() {
        let source_root = TempDir::new().unwrap();
        let target_root = TempDir::new().unwrap();
        let source =
            LibraryDatabase::open_with_device_id(source_root.path(), "blob-a".into()).unwrap();
        let target =
            LibraryDatabase::open_with_device_id(target_root.path(), "blob-b".into()).unwrap();
        let target_blobs = crate::blob_store::BlobStore::open(target_root.path()).unwrap();
        let backend = MemoryBackend::new();
        let bytes = b"remote original";
        let hash = hex::encode(Sha256::digest(bytes));
        let device_id = source.device_id().to_string();
        source
            .with_write(|conn| {
                crate::oplog::record_op(
                    conn,
                    &device_id,
                    "entity_created",
                    &hash,
                    &serde_json::json!({
                        "mime": "image/png",
                        "size": bytes.len(),
                        "status": 1
                    }),
                )
            })
            .unwrap();
        sync_once(&source, &backend).unwrap();

        let report = sync_cycle(&target, &target_blobs, &backend).unwrap();
        assert!(report.waiting_for_prerequisites);
        assert_eq!(target.ingest_cursor("blob-a").unwrap(), 0);
        let entity_count: i64 = target
            .with_read(|conn| {
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(entity_count, 0);

        let blob_key = format!("blobs/f/{}/{}/{}.png", &hash[0..2], &hash[2..4], hash);
        backend.put(&blob_key, bytes).unwrap();
        let report = sync_cycle(&target, &target_blobs, &backend).unwrap();

        assert_eq!(report.blobs_downloaded, 1);
        assert_eq!(report.ops_applied, 1);
        assert_eq!(target.ingest_cursor("blob-a").unwrap(), 1);
        assert_eq!(
            target_blobs.read_original(&hash, Some("png")).unwrap(),
            bytes
        );
        let queued_work: Vec<String> = target
            .with_read(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT work_type FROM deferred_work_item WHERE entity_hash = ?1 ORDER BY work_type",
                )?;
                let rows = stmt
                    .query_map([&hash], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            queued_work,
            vec![
                "dominant_colors".to_string(),
                "perceptual_hash".to_string(),
                "thumbnail".to_string()
            ]
        );
    }
}
