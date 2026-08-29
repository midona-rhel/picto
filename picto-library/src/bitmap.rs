use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{LibraryError, Result};

pub const BITMAP_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BitmapDomain {
    Lifecycle = 1,
    Rating = 2,
    Tag = 3,
}

impl TryFrom<u8> for BitmapDomain {
    type Error = LibraryError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Lifecycle),
            2 => Ok(Self::Rating),
            3 => Ok(Self::Tag),
            _ => Err(LibraryError::InvalidState(format!(
                "unknown bitmap domain {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BitmapKey {
    pub domain: BitmapDomain,
    pub key_id: u32,
}

pub fn checksum(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

pub fn encode(bitmap: &RoaringBitmap) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(bitmap.serialized_size());
    bitmap.serialize_into(&mut payload)?;
    Ok(payload)
}

pub fn decode(payload: &[u8], expected_checksum: &[u8]) -> Result<RoaringBitmap> {
    if checksum(payload).as_slice() != expected_checksum {
        return Err(LibraryError::InvalidState(
            "bitmap checksum mismatch".into(),
        ));
    }
    Ok(RoaringBitmap::deserialize_from(&mut Cursor::new(payload))?)
}

fn split(bitmap: &RoaringBitmap) -> BTreeMap<u16, RoaringBitmap> {
    let mut shards = BTreeMap::new();
    for value in bitmap {
        let high = (value >> 16) as u16;
        shards
            .entry(high)
            .or_insert_with(RoaringBitmap::new)
            .insert(value & 0xffff);
    }
    shards
}

pub fn load(connection: &Connection, key: BitmapKey) -> Result<RoaringBitmap> {
    let mut statement = connection.prepare_cached(
        "SELECT high_bits, checksum, payload, cardinality, format_version
         FROM canonical_bitmap
         WHERE domain = ?1 AND key_id = ?2
         ORDER BY high_bits",
    )?;
    let rows = statement.query_map(params![key.domain as u8, key.key_id], |row| {
        Ok((
            row.get::<_, u16>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, i64>(3)? as u64,
            row.get::<_, u32>(4)?,
        ))
    })?;
    let mut bitmap = RoaringBitmap::new();
    for row in rows {
        let (high, expected_checksum, payload, cardinality, version) = row?;
        if version != BITMAP_FORMAT_VERSION {
            return Err(LibraryError::InvalidState(format!(
                "unsupported bitmap format {version}"
            )));
        }
        let shard = decode(&payload, &expected_checksum)?;
        if shard.len() != cardinality {
            return Err(LibraryError::InvalidState(
                "bitmap cardinality mismatch".into(),
            ));
        }
        for low in shard {
            bitmap.insert(((high as u32) << 16) | low);
        }
    }
    Ok(bitmap)
}

pub fn load_all(connection: &Connection) -> Result<HashMap<BitmapKey, RoaringBitmap>> {
    let mut statement = connection.prepare(
        "SELECT domain, key_id, high_bits, checksum, payload, cardinality, format_version
         FROM canonical_bitmap ORDER BY domain, key_id, high_bits",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, u8>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, u16>(2)?,
            row.get::<_, Vec<u8>>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, i64>(5)? as u64,
            row.get::<_, u32>(6)?,
        ))
    })?;
    let mut bitmaps = HashMap::new();
    for row in rows {
        let (domain, key_id, high, expected_checksum, payload, cardinality, version) = row?;
        if version != BITMAP_FORMAT_VERSION {
            return Err(LibraryError::InvalidState(format!(
                "unsupported bitmap format {version}"
            )));
        }
        let shard = decode(&payload, &expected_checksum)?;
        if shard.len() != cardinality {
            return Err(LibraryError::InvalidState(
                "bitmap cardinality mismatch".into(),
            ));
        }
        let key = BitmapKey {
            domain: domain.try_into()?,
            key_id,
        };
        let bitmap = bitmaps.entry(key).or_insert_with(RoaringBitmap::new);
        for low in shard {
            bitmap.insert(((high as u32) << 16) | low);
        }
    }
    Ok(bitmaps)
}

pub fn replace(
    transaction: &Transaction<'_>,
    revision: u64,
    key: BitmapKey,
    bitmap: &RoaringBitmap,
) -> Result<usize> {
    let next = split(bitmap);
    let mut existing_statement = transaction.prepare_cached(
        "SELECT high_bits, checksum FROM canonical_bitmap
         WHERE domain = ?1 AND key_id = ?2",
    )?;
    let existing = existing_statement
        .query_map(params![key.domain as u8, key.key_id], |row| {
            Ok((row.get::<_, u16>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<HashMap<_, _>, _>>()?;

    let mut changed = 0;
    for (high, shard) in &next {
        let payload = encode(shard)?;
        let digest = checksum(&payload);
        if existing
            .get(high)
            .is_some_and(|current| current.as_slice() == digest)
        {
            continue;
        }
        transaction.execute(
            "INSERT INTO canonical_bitmap
                 (domain, key_id, high_bits, revision, cardinality, format_version, checksum, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(domain, key_id, high_bits) DO UPDATE SET
                 revision = excluded.revision,
                 cardinality = excluded.cardinality,
                 format_version = excluded.format_version,
                 checksum = excluded.checksum,
                 payload = excluded.payload",
            params![
                key.domain as u8,
                key.key_id,
                *high,
                revision as i64,
                shard.len() as i64,
                BITMAP_FORMAT_VERSION,
                digest.as_slice(),
                payload
            ],
        )?;
        changed += 1;
    }

    for high in existing.keys().filter(|high| !next.contains_key(high)) {
        transaction.execute(
            "DELETE FROM canonical_bitmap
             WHERE domain = ?1 AND key_id = ?2 AND high_bits = ?3",
            params![key.domain as u8, key.key_id, *high],
        )?;
        changed += 1;
    }
    Ok(changed)
}

pub fn replace_shards(
    transaction: &Transaction<'_>,
    revision: u64,
    key: BitmapKey,
    bitmap: &RoaringBitmap,
    high_bits: impl IntoIterator<Item = u16>,
) -> Result<usize> {
    let mut changed = 0;
    let mut delete = transaction.prepare_cached(
        "DELETE FROM canonical_bitmap
         WHERE domain = ?1 AND key_id = ?2 AND high_bits = ?3",
    )?;
    let mut upsert = transaction.prepare_cached(
        "INSERT INTO canonical_bitmap
             (domain, key_id, high_bits, revision, cardinality, format_version, checksum, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(domain, key_id, high_bits) DO UPDATE SET
             revision = excluded.revision,
             cardinality = excluded.cardinality,
             format_version = excluded.format_version,
             checksum = excluded.checksum,
             payload = excluded.payload
         WHERE canonical_bitmap.checksum != excluded.checksum",
    )?;
    for high in high_bits {
        let start = (high as u32) << 16;
        let end = start | u16::MAX as u32;
        let shard = bitmap
            .range(start..=end)
            .map(|value| value & 0xffff)
            .collect::<RoaringBitmap>();
        if shard.is_empty() {
            changed += delete.execute(params![key.domain as u8, key.key_id, high])?;
            continue;
        }
        let payload = encode(&shard)?;
        let digest = checksum(&payload);
        changed += upsert.execute(params![
            key.domain as u8,
            key.key_id,
            high,
            revision as i64,
            shard.len() as i64,
            BITMAP_FORMAT_VERSION,
            digest.as_slice(),
            payload
        ])?;
    }
    Ok(changed)
}

pub fn revision(transaction: &Transaction<'_>, key: BitmapKey) -> Result<Option<u64>> {
    transaction
        .query_row(
            "SELECT MAX(revision) FROM canonical_bitmap WHERE domain = ?1 AND key_id = ?2",
            params![key.domain as u8, key.key_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map(|value| value.flatten().map(|revision| revision as u64))
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_round_trip_preserves_full_u32_ids() {
        let values = [1, 65_535, 65_536, 98_765, u32::MAX - 1];
        let bitmap = values.into_iter().collect::<RoaringBitmap>();
        let mut rebuilt = RoaringBitmap::new();
        for (high, shard) in split(&bitmap) {
            for low in shard {
                rebuilt.insert(((high as u32) << 16) | low);
            }
        }
        assert_eq!(bitmap, rebuilt);
    }

    #[test]
    fn targeted_replacement_changes_only_requested_shards() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(include_str!("schema_v1.sql"))
            .unwrap();
        let transaction = connection.unchecked_transaction().unwrap();
        let key = BitmapKey {
            domain: BitmapDomain::Tag,
            key_id: 7,
        };
        let original = [1, 65_537].into_iter().collect::<RoaringBitmap>();
        replace(&transaction, 1, key, &original).unwrap();
        let updated = [2, 65_537].into_iter().collect::<RoaringBitmap>();
        replace_shards(&transaction, 2, key, &updated, [0]).unwrap();
        transaction.commit().unwrap();
        assert_eq!(load(&connection, key).unwrap(), updated);
    }
}
