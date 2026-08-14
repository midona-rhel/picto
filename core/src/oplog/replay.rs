//! Deterministic replay: segments from all devices → one canonical truth
//! state. Ops are sorted into the `(hlc, device_id)` total order and applied
//! sequentially, which yields last-writer-wins per field for free. Tombstones
//! mark deletion; a later op targeting a tombstoned thing revives it
//! (add-wins — a wrongly-kept container is visible and cheap, a silently
//! dropped edit is not). Same segment set ⇒ byte-identical state and digest,
//! regardless of arrival order.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::backend::SyncBackend;
use super::segment::{decode_segment, SegmentError};
use super::{OpRecord, OP_VERSION};

#[derive(thiserror::Error, Debug)]
pub enum ReplayError {
    #[error("backend: {0}")]
    Backend(#[from] super::backend::BackendError),
    #[error("segment {key}: {source}")]
    Segment { key: String, source: SegmentError },
    #[error("op with unknown version {0} — update required, nothing applied past it")]
    UnknownOpVersion(i64),
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct EntityState {
    pub kind: String,
    pub deleted: bool,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub tags: BTreeSet<String>,
    /// Collection member order (collections only).
    pub members: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct ContainerState {
    pub deleted: bool,
    pub parent: Option<String>,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub members: BTreeSet<String>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct TagState {
    pub deleted: bool,
    pub alias_to: Option<String>,
    pub implies: BTreeSet<String>,
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// Canonical truth state built by replay. All maps are ordered so the JSON
/// serialization (and therefore the digest) is stable.
#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct TruthState {
    pub entities: BTreeMap<String, EntityState>,
    pub tags: BTreeMap<String, TagState>,
    pub folders: BTreeMap<String, ContainerState>,
    pub smart_folders: BTreeMap<String, ContainerState>,
    pub duplicate_decisions: BTreeMap<String, serde_json::Value>,
}

impl TruthState {
    /// Stable content digest for cross-device convergence checks.
    pub fn digest(&self) -> String {
        let json = serde_json::to_string(self).expect("truth state serializes");
        hex::encode(Sha256::digest(json.as_bytes()))
    }

    fn entity(&mut self, key: &str) -> &mut EntityState {
        self.entities.entry(key.to_string()).or_default()
    }

    /// Canonical tag key: `namespace:subtag` (empty namespace keeps a leading
    /// colon), matching `tag_op_key` on the write side.
    fn tag_key(raw: &str) -> String {
        match raw.find(':') {
            Some(idx) if idx > 0 => raw.to_string(),
            Some(_) => raw.to_string(),
            None => format!(":{raw}"),
        }
    }

    fn apply(&mut self, op: &OpRecord) {
        let key = op.entity_key.as_str();
        let p = &op.payload;
        match op.op_type.as_str() {
            "entity_created" => {
                let entity = self.entity(key);
                entity.deleted = false;
                entity.kind = p
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("single")
                    .to_string();
                if let Some(obj) = p.as_object() {
                    for (name, value) in obj {
                        if name == "kind" || name == "tags" {
                            continue;
                        }
                        entity.fields.insert(name.clone(), value.clone());
                    }
                }
                if let Some(tags) = p.get("tags").and_then(|v| v.as_array()) {
                    for tag in tags.iter().filter_map(|t| t.as_str()) {
                        entity.tags.insert(Self::tag_key(tag));
                    }
                }
            }
            "entity_status_changed" => {
                let entity = self.entity(key);
                entity.deleted = false;
                if let Some(status) = p.get("status") {
                    entity.fields.insert("status".into(), status.clone());
                }
            }
            "entity_updated" => {
                let entity = self.entity(key);
                entity.deleted = false;
                if let Some(obj) = p.as_object() {
                    for (name, value) in obj {
                        entity.fields.insert(name.clone(), value.clone());
                    }
                }
            }
            "entity_deleted" => {
                self.entity(key).deleted = true;
            }
            "entity_tags_added" | "entity_tags_removed" => {
                let add = op.op_type == "entity_tags_added";
                let entity = self.entity(key);
                entity.deleted = false;
                if let Some(tags) = p.get("tags").and_then(|v| v.as_array()) {
                    for tag in tags.iter().filter_map(|t| t.as_str()) {
                        let canonical = Self::tag_key(tag);
                        if add {
                            entity.tags.insert(canonical);
                        } else {
                            entity.tags.remove(&canonical);
                        }
                    }
                }
            }
            "tag_renamed" | "tag_merged" => {
                let new_key = if op.op_type == "tag_renamed" {
                    p.get("to").and_then(|v| v.as_str()).map(Self::tag_key)
                } else {
                    p.get("into")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                };
                if let Some(new_key) = new_key {
                    if let Some(state) = self.tags.remove(key) {
                        self.tags.entry(new_key.clone()).or_insert(state);
                    }
                    for entity in self.entities.values_mut() {
                        if entity.tags.remove(key) {
                            entity.tags.insert(new_key.clone());
                        }
                    }
                }
            }
            "tag_deleted" => {
                self.tags.remove(key);
                for entity in self.entities.values_mut() {
                    entity.tags.remove(key);
                }
            }
            "tag_alias_set" => {
                let tag = self.tags.entry(key.to_string()).or_default();
                tag.alias_to = p.get("to").and_then(|v| v.as_str()).map(|s| s.to_string());
            }
            "tag_implication_set" => {
                let parent = p.get("parent").and_then(|v| v.as_str());
                let add = p.get("add").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(parent) = parent {
                    let tag = self.tags.entry(key.to_string()).or_default();
                    if add {
                        tag.implies.insert(parent.to_string());
                    } else {
                        tag.implies.remove(parent);
                    }
                }
            }
            "folder_created"
            | "folder_updated"
            | "folder_moved"
            | "folder_deleted"
            | "folder_members_added"
            | "folder_members_removed" => {
                apply_container_op(&mut self.folders, "folder", op);
            }
            "smart_folder_created"
            | "smart_folder_updated"
            | "smart_folder_moved"
            | "smart_folder_deleted" => {
                apply_container_op(&mut self.smart_folders, "smart_folder", op);
            }
            "collection_created" => {
                let entity = self.entity(key);
                entity.deleted = false;
                entity.kind = "collection".into();
                if let Some(name) = p.get("name") {
                    entity.fields.insert("name".into(), name.clone());
                }
            }
            "collection_renamed" => {
                let entity = self.entity(key);
                entity.deleted = false;
                if let Some(name) = p.get("name") {
                    entity.fields.insert("name".into(), name.clone());
                }
            }
            "collection_split" => {
                self.entity(key).deleted = true;
            }
            "collection_members_added" | "collection_members_removed" => {
                let add = op.op_type == "collection_members_added";
                let entity = self.entity(key);
                entity.deleted = false;
                entity.kind = "collection".into();
                if let Some(members) = p.get("members").and_then(|v| v.as_array()) {
                    for member in members.iter().filter_map(|m| m.as_str()) {
                        if add {
                            if !entity.members.iter().any(|m| m == member) {
                                entity.members.push(member.to_string());
                            }
                        } else {
                            entity.members.retain(|m| m != member);
                        }
                    }
                }
            }
            "collection_members_reordered" => {
                let entity = self.entity(key);
                entity.deleted = false;
                if let Some(order) = p.get("order").and_then(|v| v.as_array()) {
                    entity.members = order
                        .iter()
                        .filter_map(|m| m.as_str().map(|s| s.to_string()))
                        .collect();
                }
            }
            "duplicate_decided" => {
                self.duplicate_decisions.insert(key.to_string(), p.clone());
            }
            // Forward compatibility: unknown op types within a known
            // op_version are ignored here; version gating happens in replay().
            _ => {}
        }
    }
}

fn apply_container_op(
    containers: &mut BTreeMap<String, ContainerState>,
    prefix: &str,
    op: &OpRecord,
) {
    let container = containers.entry(op.entity_key.clone()).or_default();
    let p = &op.payload;
    let suffix = &op.op_type[prefix.len() + 1..];
    match suffix {
        "created" | "updated" => {
            container.deleted = false;
            if let Some(obj) = p.as_object() {
                for (name, value) in obj {
                    if name == "parent" {
                        container.parent = value.as_str().map(|s| s.to_string());
                    } else {
                        container.fields.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        "moved" => {
            container.deleted = false;
            container.parent = p
                .get("parent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        "deleted" => container.deleted = true,
        "members_added" | "members_removed" => {
            container.deleted = false;
            let add = suffix == "members_added";
            if let Some(entities) = p.get("entities").and_then(|v| v.as_array()) {
                for entity in entities.iter().filter_map(|e| e.as_str()) {
                    if add {
                        container.members.insert(entity.to_string());
                    } else {
                        container.members.remove(entity);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Sort ops into the total order and apply. Errors on any op whose version is
/// newer than this build understands — nothing is guessed or dropped.
pub fn replay_ops(mut ops: Vec<OpRecord>) -> Result<TruthState, ReplayError> {
    if let Some(op) = ops.iter().find(|op| op.op_version > OP_VERSION) {
        return Err(ReplayError::UnknownOpVersion(op.op_version));
    }
    ops.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let mut state = TruthState::default();
    for op in &ops {
        state.apply(op);
    }
    Ok(state)
}

/// Load every segment under `oplog/` from the backend and replay.
pub fn replay_backend(backend: &dyn SyncBackend) -> Result<TruthState, ReplayError> {
    let mut ops = Vec::new();
    for key in backend.list("oplog/")? {
        let Some(bytes) = backend.get(&key)? else {
            continue;
        };
        let segment_ops =
            decode_segment(&bytes).map_err(|source| ReplayError::Segment { key, source })?;
        ops.extend(segment_ops);
    }
    replay_ops(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(
        hlc: &str,
        device: &str,
        op_type: &str,
        key: &str,
        payload: serde_json::Value,
    ) -> OpRecord {
        OpRecord {
            op_version: 1,
            op_type: op_type.into(),
            entity_key: key.into(),
            payload,
            hlc: hlc.into(),
            device_id: device.into(),
        }
    }

    fn two_device_history() -> Vec<OpRecord> {
        vec![
            op(
                "001-0000",
                "dev_a",
                "entity_created",
                "h1",
                serde_json::json!({"kind":"single","name":"one","status":1,"tags":["general:cat"]}),
            ),
            op(
                "002-0000",
                "dev_b",
                "entity_created",
                "h2",
                serde_json::json!({"kind":"single","name":"two","status":1}),
            ),
            op(
                "003-0000",
                "dev_a",
                "folder_created",
                "f-uuid",
                serde_json::json!({"name":"Art","parent":null}),
            ),
            op(
                "004-0000",
                "dev_b",
                "entity_tags_added",
                "h2",
                serde_json::json!({"tags":["artist:foo"]}),
            ),
            op(
                "005-0000",
                "dev_a",
                "folder_members_added",
                "f-uuid",
                serde_json::json!({"entities":["h1"]}),
            ),
            op(
                "006-0000",
                "dev_b",
                "entity_status_changed",
                "h1",
                serde_json::json!({"status":2}),
            ),
            op(
                "007-0000",
                "dev_a",
                "entity_updated",
                "h1",
                serde_json::json!({"rating":5}),
            ),
            op(
                "008-0000",
                "dev_b",
                "tag_renamed",
                "artist:foo",
                serde_json::json!({"to":"artist:bar"}),
            ),
        ]
    }

    #[test]
    fn replay_is_deterministic_under_shuffled_arrival() {
        let ops = two_device_history();
        let reference = replay_ops(ops.clone()).unwrap().digest();
        for _ in 0..50 {
            let mut shuffled = ops.clone();
            // Fisher–Yates with rand.
            for i in (1..shuffled.len()).rev() {
                let j = (rand::random::<u64>() as usize) % (i + 1);
                shuffled.swap(i, j);
            }
            assert_eq!(replay_ops(shuffled).unwrap().digest(), reference);
        }
    }

    #[test]
    fn later_writer_wins_per_field() {
        let ops = two_device_history();
        let state = replay_ops(ops).unwrap();
        let h1 = &state.entities["h1"];
        assert_eq!(h1.fields["status"], serde_json::json!(2));
        assert_eq!(h1.fields["rating"], serde_json::json!(5));
        // The rename rewrote the assignment on h2.
        assert!(state.entities["h2"].tags.contains("artist:bar"));
        assert!(!state.entities["h2"].tags.contains("artist:foo"));
    }

    #[test]
    fn concurrent_edit_resurrects_deleted_container() {
        // Device A deletes the folder; device B, concurrently but later in
        // the total order, adds a member. Add-wins: the folder lives.
        let ops = vec![
            op(
                "001-0000",
                "dev_a",
                "folder_created",
                "f1",
                serde_json::json!({"name":"Art"}),
            ),
            op(
                "002-0000",
                "dev_a",
                "folder_deleted",
                "f1",
                serde_json::json!({}),
            ),
            op(
                "002-0001",
                "dev_b",
                "folder_members_added",
                "f1",
                serde_json::json!({"entities":["h9"]}),
            ),
        ];
        let state = replay_ops(ops).unwrap();
        let folder = &state.folders["f1"];
        assert!(!folder.deleted, "edit after tombstone must resurrect");
        assert!(folder.members.contains("h9"));

        // And the reverse order: delete last in the total order → stays dead.
        let ops = vec![
            op(
                "001-0000",
                "dev_a",
                "folder_created",
                "f1",
                serde_json::json!({"name":"Art"}),
            ),
            op(
                "002-0000",
                "dev_b",
                "folder_members_added",
                "f1",
                serde_json::json!({"entities":["h9"]}),
            ),
            op(
                "003-0000",
                "dev_a",
                "folder_deleted",
                "f1",
                serde_json::json!({}),
            ),
        ];
        assert!(replay_ops(ops).unwrap().folders["f1"].deleted);
    }

    #[test]
    fn unknown_op_version_parks_the_whole_replay() {
        let mut ops = two_device_history();
        ops.push(OpRecord {
            op_version: 99,
            op_type: "from_the_future".into(),
            entity_key: "x".into(),
            payload: serde_json::json!({}),
            hlc: "009-0000".into(),
            device_id: "dev_c".into(),
        });
        assert!(matches!(
            replay_ops(ops),
            Err(ReplayError::UnknownOpVersion(99))
        ));
    }
}
