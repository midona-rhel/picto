//! PTR (Public Tag Repository) protocol data types.
//!
//! Wire format: zlib-compressed UTF-8 JSON tuples.
//! Envelope: `[SERIALISABLE_TYPE, VERSION, serialisable_info]`

use std::collections::BTreeMap;
use std::io::Read;

use serde::Serialize;

// ─── Serialisable type IDs ──────────────────────────────────────────

const SERIALISABLE_TYPE_CONTENT_UPDATE: u64 = 34;
const SERIALISABLE_TYPE_DEFINITIONS_UPDATE: u64 = 36;
const SERIALISABLE_TYPE_METADATA: u64 = 37;

// ─── Content type constants ─────────────────────────────────────────

pub const CONTENT_TYPE_MAPPINGS: u64 = 0;
pub const CONTENT_TYPE_TAG_SIBLINGS: u64 = 1;
pub const CONTENT_TYPE_TAG_PARENTS: u64 = 2;

// ─── Action constants ───────────────────────────────────────────────

pub const CONTENT_UPDATE_ADD: u64 = 0;
pub const CONTENT_UPDATE_DELETE: u64 = 1;

// ─── Definition type constants ──────────────────────────────────────

const DEFINITIONS_TYPE_HASHES: u64 = 0;
const DEFINITIONS_TYPE_TAGS: u64 = 1;

// ─── Metadata ───────────────────────────────────────────────────────

/// Metadata describes which update hashes are available at each index.
#[derive(Debug, Clone)]
pub struct PtrMetadata {
    /// update_index → entry
    pub updates: BTreeMap<u64, PtrMetadataEntry>,
    pub next_update_due: u64,
}

#[derive(Debug, Clone)]
pub struct PtrMetadataEntry {
    /// SHA256 hashes of the update files at this index.
    pub hashes: Vec<String>,
    pub begin: u64,
    pub end: u64,
}

// ─── Updates ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PtrUpdate {
    Definitions(DefinitionsUpdate),
    Content(ContentUpdate),
}

#[derive(Debug, Clone, Default)]
pub struct DefinitionsUpdate {
    /// hash_id → hex hash string
    pub hash_ids_to_hashes: Vec<(u64, String)>,
    /// tag_id → tag string (e.g. "character:reimu" or "scenery")
    pub tag_ids_to_tags: Vec<(u64, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct ContentUpdate {
    /// Tag mappings: (tag_id, [hash_ids]) — ADD
    pub mappings_add: Vec<(u64, Vec<u64>)>,
    /// Tag mappings: (tag_id, [hash_ids]) — DELETE
    pub mappings_delete: Vec<(u64, Vec<u64>)>,
    /// Tag siblings: (old_tag_id, new_tag_id) — ADD
    pub siblings_add: Vec<(u64, u64)>,
    /// Tag siblings: (old_tag_id, new_tag_id) — DELETE
    pub siblings_delete: Vec<(u64, u64)>,
    /// Tag parents: (child_tag_id, parent_tag_id) — ADD
    pub parents_add: Vec<(u64, u64)>,
    /// Tag parents: (child_tag_id, parent_tag_id) — DELETE
    pub parents_delete: Vec<(u64, u64)>,
}

// ─── Progress reporting ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PtrSyncProgress {
    pub updates_total: u64,
    pub updates_processed: u64,
    pub tags_added: u64,
    pub siblings_added: u64,
    pub parents_added: u64,
    pub current_update_index: u64,
    pub latest_server_index: u64,
    pub starting_index: u64,
    /// Current sync phase: "metadata", "downloading", "definitions", "processing", or ""
    pub phase: String,
    /// True if this event is a periodic heartbeat (counters may be unchanged).
    pub heartbeat: bool,
    /// Milliseconds elapsed since sync started.
    pub elapsed_ms: u64,
    /// Total content row ops (adds/deletes across mappings/siblings/parents)
    /// scheduled for the current chunk write.
    pub content_rows_total: u64,
    /// Content row ops already committed for the current chunk write.
    pub content_rows_written: u64,
    /// Total DB write batches planned for the current chunk write.
    pub content_batches_total: u32,
    /// DB write batches already committed for the current chunk write.
    pub content_batches_done: u32,
    /// Hash hex strings that had content changes (for incremental overlay rebuild).
    /// Not serialized to the frontend.
    #[serde(skip)]
    pub changed_hashes: Vec<String>,
    /// True when changed_hashes was truncated to avoid excessive memory usage.
    /// Not serialized to the frontend.
    #[serde(skip)]
    pub changed_hashes_truncated: bool,
    /// True when sibling/parent relations changed in this sync run (PBI-028).
    /// Forces full overlay rebuild instead of incremental.
    #[serde(skip)]
    pub tag_graph_changed: bool,
}

impl Default for PtrSyncProgress {
    fn default() -> Self {
        Self {
            updates_total: 0,
            updates_processed: 0,
            tags_added: 0,
            siblings_added: 0,
            parents_added: 0,
            current_update_index: 0,
            latest_server_index: 0,
            starting_index: 0,
            phase: String::new(),
            heartbeat: false,
            elapsed_ms: 0,
            content_rows_total: 0,
            content_rows_written: 0,
            content_batches_total: 0,
            content_batches_done: 0,
            changed_hashes: Vec::new(),
            changed_hashes_truncated: false,
            tag_graph_changed: false,
        }
    }
}

// ─── Hydrus serializable type IDs (additional) ─────────────────────

const SERIALISABLE_TYPE_DICTIONARY: u64 = 21;

// ─── Deserialization ────────────────────────────────────────────────

/// Decompress network bytes: try zlib, then raw (uncompressed).
pub fn decompress_network_bytes(data: &[u8]) -> Result<String, String> {
    // Try zlib first
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut output = String::new();
    match decoder.read_to_string(&mut output) {
        Ok(_) => return Ok(output),
        Err(_) => {}
    }

    // Try raw UTF-8 (some responses may not be compressed)
    String::from_utf8(data.to_vec()).map_err(|e| format!("Failed to decode response: {}", e))
}

/// Unwrap a Hydrus SerialisableDictionary (type 21) envelope.
///
/// The PTR server wraps responses in `DumpHydrusArgsToNetworkBytes`, producing:
/// ```text
/// [21, 2, [
///   [(metatype_key, key_data), (metatype_value, value_data)],
///   ...
/// ]]
/// ```
/// Where metatype: 0 = JSON, 1 = hex bytes, 2 = nested serialisable [type, ver, data].
/// This extracts the value for the given string key.
fn unwrap_network_dict<'a>(
    val: &'a serde_json::Value,
    key: &str,
) -> Result<&'a serde_json::Value, String> {
    let arr = val.as_array().ok_or("Expected array")?;
    if arr.len() < 3 {
        return Err("Expected [type, version, info]".into());
    }

    let type_id = arr[0].as_u64().ok_or("Bad type")?;
    if type_id != SERIALISABLE_TYPE_DICTIONARY {
        // Not a dict wrapper — return as-is
        return Ok(val);
    }

    // Type 21: info = list of [(metatype, key_data), (metatype, value_data)] pairs
    let pairs = arr[2]
        .as_array()
        .ok_or("Dict: bad info (expected array of pairs)")?;

    for pair in pairs {
        let p = pair.as_array().ok_or("Dict: bad pair")?;
        if p.len() < 2 {
            continue;
        }

        // Each element is (metatype, data) — a 2-element array
        let meta_key = p[0].as_array().ok_or("Dict: bad meta_key")?;
        if meta_key.len() < 2 {
            continue;
        }

        // metatype 0 = JSON_OK, key_data is the string key
        let key_str = meta_key[1].as_str().unwrap_or("");
        if key_str == key {
            let meta_value = p[1].as_array().ok_or("Dict: bad meta_value")?;
            if meta_value.len() < 2 {
                return Err("Dict: meta_value too short".into());
            }
            // metatype 2 = HYDRUS_SERIALISABLE → value is [type, version, data]
            // metatype 0 = JSON_OK → value is the raw JSON
            return Ok(&meta_value[1]);
        }
    }

    Err(format!("Key '{}' not found in dictionary", key))
}

/// Parse a metadata response from decompressed JSON.
///
/// The server wraps the response as:
/// `[21, 1, [[], [["metadata_slice", [37, 1, [entries, next_update_due]]]]]]`
pub fn parse_metadata(json_str: &str) -> Result<PtrMetadata, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Metadata JSON parse error: {}", e))?;

    // Unwrap the type 21 dict wrapper if present
    let metadata_val = unwrap_network_dict(&val, "metadata_slice")?;

    let arr = metadata_val.as_array().ok_or("Metadata: expected array")?;
    if arr.len() < 3 {
        return Err("Metadata: expected [type, version, info]".into());
    }

    let type_id = arr[0].as_u64().ok_or("Metadata: bad type")?;
    if type_id != SERIALISABLE_TYPE_METADATA {
        return Err(format!(
            "Expected metadata type {}, got {}",
            SERIALISABLE_TYPE_METADATA, type_id
        ));
    }

    // info = [metadata_entries, next_update_due]
    let info = arr[2].as_array().ok_or("Metadata: bad info")?;
    if info.len() < 2 {
        return Err("Metadata: info should be [entries, next_update_due]".into());
    }

    let entries_arr = info[0].as_array().ok_or("Metadata: bad entries array")?;
    let next_update_due = info[1].as_u64().unwrap_or(0);

    let mut updates = BTreeMap::new();
    for entry in entries_arr {
        let e = entry.as_array().ok_or("Metadata: bad entry")?;
        if e.len() < 4 {
            continue;
        }
        let update_index = e[0].as_u64().ok_or("Metadata: bad index")?;
        let hashes_arr = e[1].as_array().ok_or("Metadata: bad hashes")?;
        let begin = e[2].as_u64().unwrap_or(0);
        let end = e[3].as_u64().unwrap_or(0);

        let hashes: Vec<String> = hashes_arr
            .iter()
            .filter_map(|h| h.as_str().map(|s| s.to_string()))
            .collect();

        updates.insert(update_index, PtrMetadataEntry { hashes, begin, end });
    }

    Ok(PtrMetadata {
        updates,
        next_update_due,
    })
}

/// Parse an update response (either DefinitionsUpdate or ContentUpdate).
pub fn parse_update(json_str: &str) -> Result<PtrUpdate, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Update JSON parse error: {}", e))?;

    let arr = val.as_array().ok_or("Update: expected array")?;
    if arr.len() < 3 {
        return Err("Update: expected [type, version, info]".into());
    }

    let type_id = arr[0].as_u64().ok_or("Update: bad type")?;

    match type_id {
        SERIALISABLE_TYPE_DEFINITIONS_UPDATE => parse_definitions_update(&arr[2]),
        SERIALISABLE_TYPE_CONTENT_UPDATE => parse_content_update(&arr[2]),
        _ => Err(format!("Unknown update type: {}", type_id)),
    }
}

fn parse_definitions_update(info: &serde_json::Value) -> Result<PtrUpdate, String> {
    let entries = info.as_array().ok_or("DefinitionsUpdate: bad info")?;

    let mut update = DefinitionsUpdate::default();

    for entry in entries {
        let e = entry.as_array().ok_or("DefinitionsUpdate: bad entry")?;
        if e.len() < 2 {
            continue;
        }
        let def_type = e[0].as_u64().ok_or("DefinitionsUpdate: bad def_type")?;
        let defs = e[1].as_array().ok_or("DefinitionsUpdate: bad defs")?;

        match def_type {
            DEFINITIONS_TYPE_HASHES => {
                for def in defs {
                    let d = def.as_array().ok_or("DefinitionsUpdate: bad hash def")?;
                    if d.len() >= 2 {
                        let id = d[0].as_u64().ok_or("DefinitionsUpdate: bad hash id")?;
                        let hex = d[1]
                            .as_str()
                            .ok_or("DefinitionsUpdate: bad hash hex")?
                            .to_string();
                        update.hash_ids_to_hashes.push((id, hex));
                    }
                }
            }
            DEFINITIONS_TYPE_TAGS => {
                for def in defs {
                    let d = def.as_array().ok_or("DefinitionsUpdate: bad tag def")?;
                    if d.len() >= 2 {
                        let id = d[0].as_u64().ok_or("DefinitionsUpdate: bad tag id")?;
                        let tag = d[1]
                            .as_str()
                            .ok_or("DefinitionsUpdate: bad tag string")?
                            .to_string();
                        update.tag_ids_to_tags.push((id, tag));
                    }
                }
            }
            _ => {} // Unknown definition type, skip
        }
    }

    Ok(PtrUpdate::Definitions(update))
}

fn parse_content_update(info: &serde_json::Value) -> Result<PtrUpdate, String> {
    let entries = info.as_array().ok_or("ContentUpdate: bad info")?;

    let mut update = ContentUpdate::default();

    for entry in entries {
        let e = entry.as_array().ok_or("ContentUpdate: bad entry")?;
        if e.len() < 2 {
            continue;
        }
        let content_type = e[0].as_u64().ok_or("ContentUpdate: bad content_type")?;
        let actions = e[1].as_array().ok_or("ContentUpdate: bad actions")?;

        for action_entry in actions {
            let ae = action_entry
                .as_array()
                .ok_or("ContentUpdate: bad action entry")?;
            if ae.len() < 2 {
                continue;
            }
            let action = ae[0].as_u64().ok_or("ContentUpdate: bad action")?;
            let data = ae[1].as_array().ok_or("ContentUpdate: bad data")?;

            match (content_type, action) {
                (CONTENT_TYPE_MAPPINGS, CONTENT_UPDATE_ADD) => {
                    for item in data {
                        if let Some(mapping) = parse_mapping(item) {
                            update.mappings_add.push(mapping);
                        }
                    }
                }
                (CONTENT_TYPE_MAPPINGS, CONTENT_UPDATE_DELETE) => {
                    for item in data {
                        if let Some(mapping) = parse_mapping(item) {
                            update.mappings_delete.push(mapping);
                        }
                    }
                }
                (CONTENT_TYPE_TAG_SIBLINGS, CONTENT_UPDATE_ADD) => {
                    for item in data {
                        if let Some(pair) = parse_tag_pair(item) {
                            update.siblings_add.push(pair);
                        }
                    }
                }
                (CONTENT_TYPE_TAG_SIBLINGS, CONTENT_UPDATE_DELETE) => {
                    for item in data {
                        if let Some(pair) = parse_tag_pair(item) {
                            update.siblings_delete.push(pair);
                        }
                    }
                }
                (CONTENT_TYPE_TAG_PARENTS, CONTENT_UPDATE_ADD) => {
                    for item in data {
                        if let Some(pair) = parse_tag_pair(item) {
                            update.parents_add.push(pair);
                        }
                    }
                }
                (CONTENT_TYPE_TAG_PARENTS, CONTENT_UPDATE_DELETE) => {
                    for item in data {
                        if let Some(pair) = parse_tag_pair(item) {
                            update.parents_delete.push(pair);
                        }
                    }
                }
                _ => {} // Pending/rescind actions or unknown content types — skip
            }
        }
    }

    Ok(PtrUpdate::Content(update))
}

/// Parse a mapping: [tag_id, [hash_id, hash_id, ...]]
fn parse_mapping(val: &serde_json::Value) -> Option<(u64, Vec<u64>)> {
    let arr = val.as_array()?;
    if arr.len() < 2 {
        return None;
    }
    let tag_id = arr[0].as_u64()?;
    let hash_ids: Vec<u64> = arr[1]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64())
        .collect();
    Some((tag_id, hash_ids))
}

/// Parse a tag pair: [tag_id_a, tag_id_b]
fn parse_tag_pair(val: &serde_json::Value) -> Option<(u64, u64)> {
    let arr = val.as_array()?;
    if arr.len() < 2 {
        return None;
    }
    Some((arr[0].as_u64()?, arr[1].as_u64()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_metadata() {
        let json = r#"[37, 1, [
            [
                [0, ["abc123", "def456"], 0, 1000],
                [1, ["ghi789"], 1000, 2000]
            ],
            1234567890
        ]]"#;

        let meta = parse_metadata(json).unwrap();
        assert_eq!(meta.updates.len(), 2);
        assert_eq!(meta.next_update_due, 1234567890);
        assert_eq!(meta.updates[&0].hashes, vec!["abc123", "def456"]);
        assert_eq!(meta.updates[&1].hashes, vec!["ghi789"]);
    }

    #[test]
    fn test_parse_definitions_update() {
        let json = r#"[36, 1, [
            [0, [[1, "aabbccdd"], [2, "eeff0011"]]],
            [1, [[100, "character:reimu"], [101, "scenery"]]]
        ]]"#;

        let update = parse_update(json).unwrap();
        match update {
            PtrUpdate::Definitions(def) => {
                assert_eq!(def.hash_ids_to_hashes.len(), 2);
                assert_eq!(def.tag_ids_to_tags.len(), 2);
                assert_eq!(def.tag_ids_to_tags[0], (100, "character:reimu".to_string()));
            }
            _ => panic!("Expected DefinitionsUpdate"),
        }
    }

    #[test]
    fn test_parse_content_update() {
        let json = r#"[34, 1, [
            [0, [
                [0, [[10, [1, 2, 3]], [11, [4, 5]]]],
                [1, [[12, [6]]]]
            ]],
            [1, [
                [0, [[20, 21]]]
            ]]
        ]]"#;

        let update = parse_update(json).unwrap();
        match update {
            PtrUpdate::Content(cu) => {
                assert_eq!(cu.mappings_add.len(), 2);
                assert_eq!(cu.mappings_add[0], (10, vec![1, 2, 3]));
                assert_eq!(cu.mappings_delete.len(), 1);
                assert_eq!(cu.siblings_add.len(), 1);
                assert_eq!(cu.siblings_add[0], (20, 21));
            }
            _ => panic!("Expected ContentUpdate"),
        }
    }
}
