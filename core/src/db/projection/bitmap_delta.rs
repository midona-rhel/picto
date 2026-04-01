//! Bitmap delta log persistence — append, replay, compaction.
//!
//! Delta log is a simple binary format appended after each write batch.
//! On startup, the latest snapshot is loaded and deltas are replayed.
//! If either is missing or corrupt, a full rebuild from authoritative
//! tables is performed — bitmaps are fully disposable.

use std::io::Write;
use std::path::Path;

use super::bitmaps::{BitmapDelta, BitmapKey, BitmapStore};

/// Append deltas to the delta log file.
pub fn append_deltas(log_path: &Path, deltas: &[BitmapDelta]) -> std::io::Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    for delta in deltas {
        let key_bytes = serialize_key(&delta.key);
        let op: u8 = if delta.insert { 1 } else { 0 };
        file.write_all(&(key_bytes.len() as u16).to_le_bytes())?;
        file.write_all(&key_bytes)?;
        file.write_all(&[op])?;
        file.write_all(&delta.entity_id.to_le_bytes())?;
    }
    Ok(())
}

/// Replay deltas from a log file into the bitmap store.
pub fn replay_deltas(log_path: &Path, store: &BitmapStore) -> std::io::Result<usize> {
    if !log_path.exists() {
        return Ok(0);
    }
    let data = std::fs::read(log_path)?;
    let mut cursor = 0;
    let mut count = 0;

    while cursor + 2 < data.len() {
        let key_len = u16::from_le_bytes([data[cursor], data[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + key_len + 1 + 4 > data.len() {
            break; // truncated entry
        }
        let key_bytes = &data[cursor..cursor + key_len];
        cursor += key_len;
        let op = data[cursor];
        cursor += 1;
        let entity_id = u32::from_le_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]);
        cursor += 4;

        if let Some(key) = deserialize_key(key_bytes) {
            if op == 1 {
                store.insert(&key, entity_id);
            } else {
                store.remove(&key, entity_id);
            }
            count += 1;
        }
    }
    // Drain the deltas that were just replayed (they came from the log, not new mutations)
    store.drain_deltas();
    Ok(count)
}

/// Flush pending deltas to the log file.
pub fn flush_deltas(log_path: &Path, store: &BitmapStore) -> std::io::Result<usize> {
    let deltas = store.drain_deltas();
    let count = deltas.len();
    if !deltas.is_empty() {
        append_deltas(log_path, &deltas)?;
    }
    Ok(count)
}

fn serialize_key(key: &BitmapKey) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9);
    match key {
        BitmapKey::Status(v) => {
            buf.push(0);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::Tag(v) => {
            buf.push(1);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::ImpliedTag(v) => {
            buf.push(2);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::EffectiveTag(v) => {
            buf.push(3);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::Folder(v) => {
            buf.push(4);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::SmartFolder(v) => {
            buf.push(5);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::Tagged => {
            buf.push(6);
        }
        BitmapKey::CollectionMember => {
            buf.push(7);
        }
    }
    buf
}

fn deserialize_key(bytes: &[u8]) -> Option<BitmapKey> {
    if bytes.is_empty() {
        return None;
    }
    let tag = bytes[0];
    let val = if bytes.len() >= 9 {
        Some(i64::from_le_bytes(bytes[1..9].try_into().ok()?))
    } else {
        None
    };
    match tag {
        0 => Some(BitmapKey::Status(val?)),
        1 => Some(BitmapKey::Tag(val?)),
        2 => Some(BitmapKey::ImpliedTag(val?)),
        3 => Some(BitmapKey::EffectiveTag(val?)),
        4 => Some(BitmapKey::Folder(val?)),
        5 => Some(BitmapKey::SmartFolder(val?)),
        6 => Some(BitmapKey::Tagged),
        7 => Some(BitmapKey::CollectionMember),
        _ => None,
    }
}
