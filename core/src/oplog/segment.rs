//! Immutable op segment encoding: framed, per-record CRC'd, with a trailing
//! whole-segment checksum. A damaged segment is detected and quarantined by
//! the caller — never half-applied.
//!
//! Layout: `b"PSEG"` magic, u16 LE format version, then per record a u32 LE
//! length + u32 LE CRC32 + JSON-encoded `OpRecord`, then a u32 zero sentinel
//! and the u32 CRC32 of all record bytes concatenated.

use super::OpRecord;

pub const SEGMENT_VERSION: u16 = 1;
pub const MAX_SEGMENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RECORDS_PER_SEGMENT: usize = 1024;
const MAGIC: &[u8; 4] = b"PSEG";

#[derive(thiserror::Error, Debug)]
pub enum SegmentError {
    #[error("corrupt segment: {0}")]
    Corrupt(&'static str),
    #[error("unknown segment version {0} — update required")]
    UnknownVersion(u16),
    #[error("op record encode failed: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("segment exceeds the {MAX_SEGMENT_BYTES}-byte limit")]
    SegmentTooLarge,
    #[error("segment contains more than {MAX_RECORDS_PER_SEGMENT} operations")]
    TooManyRecords,
    #[error("operation exceeds the {MAX_RECORD_BYTES}-byte limit")]
    RecordTooLarge,
    #[error("operation contains a non-canonical HLC")]
    InvalidHlc,
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}

pub fn encode_segment(ops: &[OpRecord]) -> Result<Vec<u8>, SegmentError> {
    if ops.len() > MAX_RECORDS_PER_SEGMENT {
        return Err(SegmentError::TooManyRecords);
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&SEGMENT_VERSION.to_le_bytes());
    let mut all_records = Vec::new();
    for op in ops {
        if !super::is_valid_hlc(&op.hlc) {
            return Err(SegmentError::InvalidHlc);
        }
        let bytes = serde_json::to_vec(op)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(SegmentError::RecordTooLarge);
        }
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32(&bytes).to_le_bytes());
        out.extend_from_slice(&bytes);
        all_records.extend_from_slice(&bytes);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&crc32(&all_records).to_le_bytes());
    if out.len() > MAX_SEGMENT_BYTES {
        return Err(SegmentError::SegmentTooLarge);
    }
    Ok(out)
}

pub fn decode_segment(data: &[u8]) -> Result<Vec<OpRecord>, SegmentError> {
    if data.len() > MAX_SEGMENT_BYTES {
        return Err(SegmentError::SegmentTooLarge);
    }
    if data.len() < 6 || &data[0..4] != MAGIC {
        return Err(SegmentError::Corrupt("bad magic"));
    }
    let version = u16::from_le_bytes([data[4], data[5]]);
    if version != SEGMENT_VERSION {
        return Err(SegmentError::UnknownVersion(version));
    }

    let mut ops = Vec::new();
    let mut all_records = Vec::new();
    let mut cursor = 6usize;
    loop {
        let Some(len_bytes) = data.get(cursor..cursor + 4) else {
            return Err(SegmentError::Corrupt("truncated before sentinel"));
        };
        let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
        cursor += 4;
        if len == 0 {
            break;
        }
        if len > MAX_RECORD_BYTES {
            return Err(SegmentError::RecordTooLarge);
        }
        if ops.len() >= MAX_RECORDS_PER_SEGMENT {
            return Err(SegmentError::TooManyRecords);
        }
        let Some(crc_bytes) = data.get(cursor..cursor + 4) else {
            return Err(SegmentError::Corrupt("truncated record header"));
        };
        let expected_crc = u32::from_le_bytes(crc_bytes.try_into().unwrap());
        cursor += 4;
        let Some(payload) = data.get(cursor..cursor + len) else {
            return Err(SegmentError::Corrupt("truncated record payload"));
        };
        cursor += len;
        if crc32(payload) != expected_crc {
            return Err(SegmentError::Corrupt("record crc mismatch"));
        }
        let op: OpRecord = serde_json::from_slice(payload)
            .map_err(|_| SegmentError::Corrupt("record decode failed"))?;
        if !super::is_valid_hlc(&op.hlc) {
            return Err(SegmentError::InvalidHlc);
        }
        all_records.extend_from_slice(payload);
        ops.push(op);
    }
    let Some(total_bytes) = data.get(cursor..cursor + 4) else {
        return Err(SegmentError::Corrupt("missing segment checksum"));
    };
    let expected_total = u32::from_le_bytes(total_bytes.try_into().unwrap());
    cursor += 4;
    if crc32(&all_records) != expected_total {
        return Err(SegmentError::Corrupt("segment crc mismatch"));
    }
    if cursor != data.len() {
        return Err(SegmentError::Corrupt("trailing bytes"));
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ops() -> Vec<OpRecord> {
        (0..3)
            .map(|i| OpRecord {
                op_version: 1,
                op_type: "entity_status_changed".into(),
                entity_key: format!("hash_{i}"),
                payload: serde_json::json!({ "status": i }),
                hlc: format!("000000000000{i}-0000"),
                device_id: "dev_a".into(),
            })
            .collect()
    }

    #[test]
    fn round_trip() {
        let ops = sample_ops();
        let encoded = encode_segment(&ops).unwrap();
        assert_eq!(decode_segment(&encoded).unwrap(), ops);
    }

    #[test]
    fn corruption_is_detected_never_partially_applied() {
        let ops = sample_ops();
        let mut encoded = encode_segment(&ops).unwrap();
        // Flip one byte in the middle of a record payload.
        let mid = encoded.len() / 2;
        encoded[mid] ^= 0xFF;
        assert!(matches!(
            decode_segment(&encoded),
            Err(SegmentError::Corrupt(_))
        ));
    }

    #[test]
    fn truncation_is_detected() {
        let ops = sample_ops();
        let encoded = encode_segment(&ops).unwrap();
        assert!(decode_segment(&encoded[..encoded.len() - 6]).is_err());
    }

    #[test]
    fn unknown_version_is_parked_not_guessed() {
        let mut encoded = encode_segment(&sample_ops()).unwrap();
        encoded[4] = 0xEE;
        assert!(matches!(
            decode_segment(&encoded),
            Err(SegmentError::UnknownVersion(_))
        ));
    }

    #[test]
    fn malformed_hlc_is_rejected_before_it_reaches_sqlite() {
        let mut ops = sample_ops();
        ops[0].hlc = "zzzz".into();
        assert!(matches!(
            encode_segment(&ops),
            Err(SegmentError::InvalidHlc)
        ));

        let payload = serde_json::to_vec(&ops[0]).unwrap();
        let mut checksummed = Vec::new();
        checksummed.extend_from_slice(MAGIC);
        checksummed.extend_from_slice(&SEGMENT_VERSION.to_le_bytes());
        checksummed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        checksummed.extend_from_slice(&crc32(&payload).to_le_bytes());
        checksummed.extend_from_slice(&payload);
        checksummed.extend_from_slice(&0u32.to_le_bytes());
        checksummed.extend_from_slice(&crc32(&payload).to_le_bytes());
        assert!(matches!(
            decode_segment(&checksummed),
            Err(SegmentError::InvalidHlc)
        ));
    }

    #[test]
    fn segment_limits_are_enforced() {
        let ops = vec![sample_ops()[0].clone(); MAX_RECORDS_PER_SEGMENT + 1];
        assert!(matches!(
            encode_segment(&ops),
            Err(SegmentError::TooManyRecords)
        ));
        assert!(matches!(
            decode_segment(&vec![0; MAX_SEGMENT_BYTES + 1]),
            Err(SegmentError::SegmentTooLarge)
        ));
    }
}
