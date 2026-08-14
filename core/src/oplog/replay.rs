//! Deterministic replay: segments from all devices → one canonical truth
//! state. Ops are sorted into the `(hlc, device_id)` total order and applied
//! sequentially, which yields last-writer-wins per field for free. Hard
//! deletion is delete-wins against partial edits because live storage removes
//! the record and its payload; only a later explicit full create can recreate
//! it. Same segment set ⇒ byte-identical state and digest, regardless of
//! arrival order.

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
    #[error("unsupported op version {0} — update required, nothing applied")]
    UnknownOpVersion(i64),
    #[error("unknown op type {0} — update required, nothing applied")]
    UnknownOpType(String),
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct EntityState {
    pub created: bool,
    pub kind: String,
    pub deleted: bool,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub tags: BTreeSet<String>,
    /// Collection member order (collections only).
    pub members: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct ContainerState {
    pub created: bool,
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
            "entity_created" | "entity_recreated" => {
                if op.op_type == "entity_created"
                    && self
                        .entities
                        .get(key)
                        .is_some_and(|entity| entity.created || entity.deleted)
                {
                    return;
                }
                let entity = self.entity(key);
                *entity = EntityState::default();
                entity.created = true;
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
                if entity.deleted {
                    return;
                }
                if let Some(status) = p.get("status") {
                    entity.fields.insert("status".into(), status.clone());
                }
            }
            "entity_updated" => {
                let entity = self.entity(key);
                if entity.deleted {
                    return;
                }
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
                if entity.deleted {
                    return;
                }
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
                if self.entities.get(key).is_some_and(|entity| entity.created) {
                    return;
                }
                let entity = self.entity(key);
                *entity = EntityState::default();
                entity.created = true;
                entity.deleted = false;
                entity.kind = "collection".into();
                if let Some(name) = p.get("name") {
                    entity.fields.insert("name".into(), name.clone());
                }
            }
            "collection_renamed" => {
                let entity = self.entity(key);
                if entity.deleted {
                    return;
                }
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
                if entity.deleted {
                    return;
                }
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
                if entity.deleted {
                    return;
                }
                if let Some(order) = p.get("order").and_then(|v| v.as_array()) {
                    entity.members = order
                        .iter()
                        .filter_map(|m| m.as_str().map(|s| s.to_string()))
                        .collect();
                }
            }
            "duplicate_decided" => {
                self.duplicate_decisions
                    .insert(super::normalized_pair_key(key), p.clone());
            }
            _ => unreachable!("operation vocabulary is validated before replay"),
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
        "created" => {
            if container.created {
                return;
            }
            *container = ContainerState::default();
            container.created = true;
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
        "updated" => {
            if container.deleted {
                return;
            }
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
            if container.deleted {
                return;
            }
            container.parent = p
                .get("parent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        "deleted" => container.deleted = true,
        "members_added" | "members_removed" => {
            if container.deleted {
                return;
            }
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
    if let Some(op) = ops.iter().find(|op| op.op_version != OP_VERSION) {
        return Err(ReplayError::UnknownOpVersion(op.op_version));
    }
    if let Some(op) = ops
        .iter()
        .find(|op| !super::is_supported_op_type(&op.op_type))
    {
        return Err(ReplayError::UnknownOpType(op.op_type.clone()));
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
        if !key.ends_with(".seg") {
            continue;
        }
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
    fn hard_delete_ignores_partial_edits_and_duplicate_container_creates() {
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
        assert!(state.folders["f1"].deleted);
        assert!(!state.folders["f1"].members.contains("h9"));

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
                "002-0001",
                "dev_a",
                "folder_deleted",
                "f1",
                serde_json::json!({}),
            ),
            op(
                "003-0000",
                "dev_a",
                "folder_created",
                "f1",
                serde_json::json!({"name":"Recreated"}),
            ),
        ];
        let folder = &replay_ops(ops).unwrap().folders["f1"];
        assert!(folder.deleted);
        assert_eq!(folder.fields["name"], serde_json::json!("Art"));
        assert!(folder.members.contains("h9"));
    }

    #[test]
    fn explicit_entity_recreation_resets_truth() {
        let entity = replay_ops(vec![
            op(
                "001-0000",
                "a",
                "entity_created",
                "h1",
                serde_json::json!({"name":"old","tags":["general:old"]}),
            ),
            op(
                "002-0000",
                "a",
                "entity_updated",
                "h1",
                serde_json::json!({"rating":5}),
            ),
            op(
                "003-0000",
                "a",
                "entity_deleted",
                "h1",
                serde_json::json!({}),
            ),
            op(
                "004-0000",
                "a",
                "entity_recreated",
                "h1",
                serde_json::json!({"name":"new"}),
            ),
        ])
        .unwrap();
        assert_eq!(entity.entities["h1"].fields.len(), 1);
        assert!(entity.entities["h1"].tags.is_empty());
        assert!(!entity.entities["h1"].deleted);
    }

    #[test]
    fn first_create_cannot_cross_an_existing_tombstone() {
        let state = replay_ops(vec![
            op(
                "001-0000",
                "a",
                "entity_deleted",
                "h1",
                serde_json::json!({}),
            ),
            op(
                "002-0000",
                "b",
                "entity_created",
                "h1",
                serde_json::json!({"name":"stale import"}),
            ),
        ])
        .unwrap();

        assert!(state.entities["h1"].deleted);
        assert!(!state.entities["h1"].created);
    }

    #[test]
    fn collection_create_is_idempotent_after_split() {
        let collection = replay_ops(vec![
            op(
                "001-0000",
                "a",
                "collection_created",
                "c1",
                serde_json::json!({"name":"old"}),
            ),
            op(
                "002-0000",
                "a",
                "collection_members_added",
                "c1",
                serde_json::json!({"members":["h1"]}),
            ),
            op(
                "003-0000",
                "a",
                "collection_split",
                "c1",
                serde_json::json!({}),
            ),
            op(
                "004-0000",
                "a",
                "collection_created",
                "c1",
                serde_json::json!({"name":"new"}),
            ),
        ])
        .unwrap();
        assert!(collection.entities["c1"].deleted);
        assert_eq!(collection.entities["c1"].members, vec!["h1"]);
        assert_eq!(collection.entities["c1"].fields["name"], "old");
    }

    #[test]
    fn duplicate_create_is_idempotent_and_does_not_reset_metadata() {
        let state = replay_ops(vec![
            op(
                "001-0000",
                "dev_a",
                "entity_created",
                "h1",
                serde_json::json!({"kind":"single","name":"Original"}),
            ),
            op(
                "002-0000",
                "dev_a",
                "entity_updated",
                "h1",
                serde_json::json!({"rating":5}),
            ),
            op(
                "003-0000",
                "dev_b",
                "entity_created",
                "h1",
                serde_json::json!({"kind":"single","name":"Duplicate import"}),
            ),
        ])
        .unwrap();

        assert_eq!(state.entities["h1"].fields["name"], "Original");
        assert_eq!(state.entities["h1"].fields["rating"], 5);
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

    #[test]
    fn older_op_version_also_parks_the_whole_replay() {
        let mut ops = two_device_history();
        ops[0].op_version = 0;
        assert!(matches!(
            replay_ops(ops),
            Err(ReplayError::UnknownOpVersion(0))
        ));
    }

    #[test]
    fn reversed_duplicate_pair_keys_share_one_replay_decision() {
        let state = replay_ops(vec![
            op(
                "001-0000",
                "a",
                "duplicate_decided",
                "left|right",
                serde_json::json!({"action":"keep_both"}),
            ),
            op(
                "002-0000",
                "b",
                "duplicate_decided",
                "right|left",
                serde_json::json!({"action":"not_duplicate"}),
            ),
        ])
        .unwrap();
        assert_eq!(state.duplicate_decisions.len(), 1);
        assert_eq!(
            state.duplicate_decisions["left|right"]["action"],
            "not_duplicate"
        );
    }

    #[test]
    fn unknown_op_type_parks_the_whole_replay() {
        let mut ops = two_device_history();
        ops.push(op(
            "009-0000",
            "dev_c",
            "entity_cretaed",
            "x",
            serde_json::json!({}),
        ));
        assert!(matches!(
            replay_ops(ops),
            Err(ReplayError::UnknownOpType(op_type)) if op_type == "entity_cretaed"
        ));
    }
}
