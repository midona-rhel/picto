//! Sync operation log foundations: stable identity, hybrid logical clock,
//! device identity, and the durable op outbox.
//!
//! Every truth mutation records an op row in `op_outbox` inside the same
//! transaction as the mutation itself (`with_write` provides the atomicity),
//! so an acknowledged mutation and its op are durable together or not at all.
//! The outbox is device-local operational state; a later sync stage drains it
//! into immutable remote segments.

use std::sync::Mutex;
use std::sync::OnceLock;

use rusqlite::Connection;

pub mod backend;
pub mod backend_fs;
pub mod conflict;
pub mod drain;
pub mod remote_library;
pub mod replay;
pub mod segment;
pub mod sync;

/// Version stamped on every op record. Readers park unknown versions.
pub const OP_VERSION: i64 = 1;

/// Operations understood by every path that emits, applies, or restores the
/// current protocol version. Adding an operation requires updating this one
/// vocabulary and its materializers together.
pub(crate) fn is_supported_op_type(op_type: &str) -> bool {
    matches!(
        op_type,
        "entity_created"
            | "entity_recreated"
            | "entity_status_changed"
            | "entity_updated"
            | "entity_deleted"
            | "entity_tags_added"
            | "entity_tags_removed"
            | "tag_renamed"
            | "tag_merged"
            | "tag_deleted"
            | "tag_alias_set"
            | "tag_implication_set"
            | "folder_created"
            | "folder_updated"
            | "folder_moved"
            | "folder_deleted"
            | "folder_members_added"
            | "folder_members_removed"
            | "smart_folder_created"
            | "smart_folder_updated"
            | "smart_folder_moved"
            | "smart_folder_deleted"
            | "collection_created"
            | "collection_renamed"
            | "collection_members_added"
            | "collection_members_removed"
            | "collection_members_reordered"
            | "collection_split"
            | "duplicate_decided"
            | "subscription_group_created"
            | "subscription_group_updated"
            | "subscription_group_deleted"
            | "subscription_created"
            | "subscription_updated"
            | "subscription_deleted"
            | "subscription_query_created"
            | "subscription_query_updated"
            | "subscription_query_deleted"
    )
}

pub(crate) fn normalized_pair_key(key: &str) -> String {
    let Some((left, right)) = key.split_once('|') else {
        return key.to_owned();
    };
    if left <= right {
        format!("{left}|{right}")
    } else {
        format!("{right}|{left}")
    }
}

/// One truth mutation, as stored in the outbox and shipped in segments.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OpRecord {
    pub op_version: i64,
    pub op_type: String,
    pub entity_key: String,
    pub payload: serde_json::Value,
    pub hlc: String,
    pub device_id: String,
}

impl OpRecord {
    /// Total-order sort key: `(hlc, device_id)`. HLCs are strictly monotonic
    /// per device, so this orders every op pair deterministically.
    pub fn sort_key(&self) -> (&str, &str) {
        (&self.hlc, &self.device_id)
    }
}

/// Canonical HLC encoding used for durable lexical ordering.
pub(crate) fn is_valid_hlc(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 18
        && bytes[13] == b'-'
        && bytes[..13]
            .iter()
            .chain(&bytes[14..])
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Generate a stable random identity token (32 lowercase hex chars).
pub fn new_uuid() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// Next hybrid-logical-clock stamp: `<unix_ms hex 13>-<counter hex 4>`.
/// Lexicographic order equals causal order for stamps from one device;
/// the counter keeps stamps monotonic when the wall clock stalls or steps
/// backwards.
pub fn next_hlc() -> String {
    static LAST: Mutex<(u64, u64)> = Mutex::new((0, 0));
    let wall_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut last = LAST.lock().unwrap();
    if wall_ms > last.0 {
        *last = (wall_ms, 0);
    } else {
        last.1 += 1;
        if last.1 > 0xffff {
            last.0 += 1;
            last.1 = 0;
        }
    }
    format!("{:013x}-{:04x}", last.0, last.1)
}

/// This installation's stable device identity.
///
/// Stored outside any library root (a synced library must never carry a
/// device id), under `~/.picto/device-id`; created on first use.
/// TODO: resolve from the Electron app-data directory once the host passes
/// one through `initialize`.
pub fn device_id() -> String {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let Some(dir) = home_dir().map(|h| h.join(".picto")) else {
            return new_uuid();
        };
        let path = dir.join("device-id");
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        let id = new_uuid();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, &id);
        id
    })
    .clone()
}

fn home_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let var = "USERPROFILE";
    #[cfg(not(windows))]
    let var = "HOME";
    std::env::var_os(var).map(std::path::PathBuf::from)
}

/// Record an op in the outbox. Must be called on the connection of the
/// `with_write` transaction that performs the mutation the op describes.
pub fn record_op(
    conn: &Connection,
    device_id: &str,
    op_type: &str,
    entity_key: &str,
    payload: &serde_json::Value,
) -> rusqlite::Result<()> {
    if !is_supported_op_type(op_type) {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "unknown local op type {op_type}"
        )));
    }
    let op_type = conflict::local_op_type(conn, op_type, entity_key)?;
    let hlc = conflict::next_durable_hlc(conn)?;
    let created_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO op_outbox (op_version, op_type, entity_key, payload_json, hlc, device_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            OP_VERSION,
            &op_type,
            entity_key,
            payload.to_string(),
            &hlc,
            device_id,
            created_at,
        ],
    )?;
    let op = OpRecord {
        op_version: OP_VERSION,
        op_type,
        entity_key: entity_key.to_owned(),
        payload: payload.clone(),
        hlc,
        device_id: device_id.to_owned(),
    };
    conflict::record_local_op(conn, &op)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_32_hex_and_unique() {
        let a = new_uuid();
        let b = new_uuid();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn hlc_is_strictly_monotonic() {
        let mut prev = next_hlc();
        for _ in 0..1000 {
            let next = next_hlc();
            assert!(next > prev, "{next} must sort after {prev}");
            prev = next;
        }
    }

    #[test]
    fn device_id_is_stable_within_process() {
        assert_eq!(device_id(), device_id());
    }

    #[test]
    fn hlc_validation_requires_the_canonical_fixed_width_encoding() {
        assert!(is_valid_hlc("0019c7a5f1234-000f"));
        assert!(!is_valid_hlc("19c7a5f1234-000f"));
        assert!(!is_valid_hlc("0019C7A5F1234-000f"));
        assert!(!is_valid_hlc("0019c7a5f1234-zzzz"));
    }

    #[test]
    fn operation_vocabulary_is_explicit() {
        assert!(is_supported_op_type("entity_recreated"));
        assert!(is_supported_op_type("subscription_query_updated"));
        assert!(!is_supported_op_type("entity_cretaed"));
    }
}
