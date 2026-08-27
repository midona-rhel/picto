//! Durable canonical bitmap and ordering encodings.
//!
//! SQLite owns these payloads. In-memory projections deserialize the same
//! portable representation and may always be rebuilt from canonical rows.

use std::collections::BTreeMap;
use std::io::Cursor;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

const BITMAP_FORMAT_VERSION: i64 = 1;
const ORDER_FORMAT_VERSION: i64 = 1;
const SHARD_SHIFT: u32 = 16;
const SHARD_MASK: u32 = (1 << SHARD_SHIFT) - 1;

pub const LIFECYCLE_ACTIVE_KEY: i64 = 1;
pub const LIFECYCLE_INBOX_KEY: i64 = 2;
pub const LIFECYCLE_TRASH_KEY: i64 = 3;
pub const RATING_UNRATED_KEY: i64 = 0;

pub const fn rating_key(rating: Option<u8>) -> i64 {
    match rating {
        Some(value) => value as i64 + 1,
        None => RATING_UNRATED_KEY,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum BitmapDomain {
    Lifecycle = 1,
    Rating = 2,
    Tag = 3,
    Folder = 4,
    GroupMember = 5,
    RootKind = 100,
    Mime = 101,
    MimeFamily = 102,
    MediaKind = 103,
    MediaFlags = 104,
    Availability = 105,
    ColorLabCell = 106,
    SmartFolder = 107,
}

impl BitmapDomain {
    pub const fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedBitmapShard {
    pub shard: u16,
    pub cardinality: u64,
    pub checksum: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedOrder {
    pub cardinality: u64,
    pub checksum: String,
    pub payload: Vec<u8>,
}

pub fn encode_shards(bitmap: &RoaringBitmap) -> Result<Vec<EncodedBitmapShard>, String> {
    let mut shards = BTreeMap::<u16, RoaringBitmap>::new();
    for id in bitmap {
        let shard = (id >> SHARD_SHIFT) as u16;
        shards.entry(shard).or_default().insert(id & SHARD_MASK);
    }

    shards
        .into_iter()
        .map(|(shard, values)| {
            let mut payload = Vec::with_capacity(values.serialized_size());
            values
                .serialize_into(&mut payload)
                .map_err(|error| format!("Failed to serialize bitmap shard: {error}"))?;
            Ok(EncodedBitmapShard {
                shard,
                cardinality: values.len(),
                checksum: checksum(&payload),
                payload,
            })
        })
        .collect()
}

pub fn decode_shard(encoded: &EncodedBitmapShard) -> Result<RoaringBitmap, String> {
    if checksum(&encoded.payload) != encoded.checksum {
        return Err(format!("Bitmap shard {} checksum mismatch", encoded.shard));
    }
    let values = RoaringBitmap::deserialize_from(&mut Cursor::new(&encoded.payload))
        .map_err(|error| format!("Invalid bitmap shard {}: {error}", encoded.shard))?;
    if values.len() != encoded.cardinality {
        return Err(format!(
            "Bitmap shard {} cardinality mismatch: expected {}, decoded {}",
            encoded.shard,
            encoded.cardinality,
            values.len()
        ));
    }
    if values.max().is_some_and(|value| value > SHARD_MASK) {
        return Err(format!(
            "Bitmap shard {} contains an invalid local ID",
            encoded.shard
        ));
    }

    let prefix = u32::from(encoded.shard) << SHARD_SHIFT;
    Ok(values
        .iter()
        .map(|value| prefix | value)
        .collect::<RoaringBitmap>())
}

pub fn replace_bitmap(
    transaction: &Transaction<'_>,
    domain: BitmapDomain,
    key_id: i64,
    revision: i64,
    bitmap: &RoaringBitmap,
) -> rusqlite::Result<()> {
    transaction.execute(
        "DELETE FROM canonical_bitmap WHERE domain = ?1 AND key_id = ?2",
        params![domain.as_i64(), key_id],
    )?;
    let shards = encode_shards(bitmap).map_err(invalid_data)?;
    let mut statement = transaction.prepare_cached(
        "INSERT INTO canonical_bitmap (
             domain, key_id, shard, revision, cardinality,
             format_version, checksum, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for shard in shards {
        statement.execute(params![
            domain.as_i64(),
            key_id,
            i64::from(shard.shard),
            revision,
            shard.cardinality as i64,
            BITMAP_FORMAT_VERSION,
            shard.checksum,
            shard.payload,
        ])?;
    }
    Ok(())
}

pub fn load_bitmap(
    connection: &Connection,
    domain: BitmapDomain,
    key_id: i64,
) -> rusqlite::Result<RoaringBitmap> {
    let mut statement = connection.prepare_cached(
        "SELECT shard, cardinality, checksum, payload, format_version
         FROM canonical_bitmap
         WHERE domain = ?1 AND key_id = ?2
         ORDER BY shard",
    )?;
    let rows = statement.query_map(params![domain.as_i64(), key_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Vec<u8>>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut result = RoaringBitmap::new();
    for row in rows {
        let (shard, cardinality, checksum, payload, version) = row?;
        if version != BITMAP_FORMAT_VERSION {
            return Err(invalid_data(format!(
                "Unsupported bitmap format version {version}"
            )));
        }
        let shard = u16::try_from(shard).map_err(|_| invalid_data("Invalid bitmap shard"))?;
        let cardinality =
            u64::try_from(cardinality).map_err(|_| invalid_data("Invalid bitmap cardinality"))?;
        result |= decode_shard(&EncodedBitmapShard {
            shard,
            cardinality,
            checksum,
            payload,
        })
        .map_err(invalid_data)?;
    }
    Ok(result)
}

pub fn load_domain(
    connection: &Connection,
    domain: BitmapDomain,
) -> rusqlite::Result<BTreeMap<i64, RoaringBitmap>> {
    let mut statement = connection.prepare_cached(
        "SELECT key_id, shard, cardinality, checksum, payload, format_version
         FROM canonical_bitmap
         WHERE domain = ?1
         ORDER BY key_id, shard",
    )?;
    let rows = statement.query_map([domain.as_i64()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    let mut result = BTreeMap::<i64, RoaringBitmap>::new();
    for row in rows {
        let (key_id, shard, cardinality, checksum, payload, version) = row?;
        if version != BITMAP_FORMAT_VERSION {
            return Err(invalid_data(format!(
                "Unsupported bitmap format version {version}"
            )));
        }
        let shard = u16::try_from(shard).map_err(|_| invalid_data("Invalid bitmap shard"))?;
        let cardinality =
            u64::try_from(cardinality).map_err(|_| invalid_data("Invalid bitmap cardinality"))?;
        let decoded = decode_shard(&EncodedBitmapShard {
            shard,
            cardinality,
            checksum,
            payload,
        })
        .map_err(invalid_data)?;
        *result.entry(key_id).or_default() |= decoded;
    }
    Ok(result)
}

pub fn intern_key(
    transaction: &Transaction<'_>,
    domain: BitmapDomain,
    value: &str,
) -> rusqlite::Result<u32> {
    if let Some(key_id) = transaction
        .query_row(
            "SELECT key_id FROM canonical_bitmap_key
             WHERE domain = ?1 AND value = ?2",
            params![domain.as_i64(), value],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    {
        return u32::try_from(key_id).map_err(|_| invalid_data("Bitmap key exceeds u32"));
    }

    transaction.execute(
        "INSERT INTO canonical_bitmap_key_allocator(domain, next_key_id)
         VALUES (?1, 1)
         ON CONFLICT(domain) DO NOTHING",
        [domain.as_i64()],
    )?;
    let next: i64 = transaction.query_row(
        "SELECT next_key_id FROM canonical_bitmap_key_allocator WHERE domain = ?1",
        [domain.as_i64()],
        |row| row.get(0),
    )?;
    let key_id = u32::try_from(next).map_err(|_| invalid_data("Bitmap key space exhausted"))?;
    transaction.execute(
        "UPDATE canonical_bitmap_key_allocator
         SET next_key_id = next_key_id + 1 WHERE domain = ?1",
        [domain.as_i64()],
    )?;
    transaction.execute(
        "INSERT INTO canonical_bitmap_key(domain, key_id, value) VALUES (?1, ?2, ?3)",
        params![domain.as_i64(), i64::from(key_id), value],
    )?;
    Ok(key_id)
}

pub fn encode_order(ids: &[u32]) -> EncodedOrder {
    let mut payload = Vec::with_capacity(16 + ids.len() * size_of::<u32>());
    payload.extend_from_slice(b"PORD");
    payload.extend_from_slice(&(ORDER_FORMAT_VERSION as u32).to_le_bytes());
    payload.extend_from_slice(&(ids.len() as u64).to_le_bytes());
    for id in ids {
        payload.extend_from_slice(&id.to_le_bytes());
    }
    EncodedOrder {
        cardinality: ids.len() as u64,
        checksum: checksum(&payload),
        payload,
    }
}

pub fn decode_order(encoded: &EncodedOrder) -> Result<Vec<u32>, String> {
    if checksum(&encoded.payload) != encoded.checksum {
        return Err("Ordering checksum mismatch".to_string());
    }
    if encoded.payload.len() < 16 || &encoded.payload[0..4] != b"PORD" {
        return Err("Invalid ordering payload".to_string());
    }
    let version = u32::from_le_bytes(encoded.payload[4..8].try_into().unwrap());
    if version != ORDER_FORMAT_VERSION as u32 {
        return Err(format!("Unsupported ordering format version {version}"));
    }
    let count = u64::from_le_bytes(encoded.payload[8..16].try_into().unwrap());
    if count != encoded.cardinality {
        return Err("Ordering cardinality mismatch".to_string());
    }
    let count = usize::try_from(count).map_err(|_| "Ordering is too large".to_string())?;
    let expected = 16usize
        .checked_add(
            count
                .checked_mul(size_of::<u32>())
                .ok_or("Ordering is too large")?,
        )
        .ok_or("Ordering is too large")?;
    if encoded.payload.len() != expected {
        return Err("Ordering payload length mismatch".to_string());
    }
    Ok(encoded.payload[16..]
        .chunks_exact(size_of::<u32>())
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect())
}

pub fn replace_order(
    transaction: &Transaction<'_>,
    owner_kind: &str,
    owner_id: i64,
    revision: i64,
    ids: &[u32],
) -> rusqlite::Result<()> {
    let encoded = encode_order(ids);
    transaction.execute(
        "INSERT INTO canonical_order (
             owner_kind, owner_id, revision, cardinality,
             format_version, checksum, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(owner_kind, owner_id) DO UPDATE SET
             revision = excluded.revision,
             cardinality = excluded.cardinality,
             format_version = excluded.format_version,
             checksum = excluded.checksum,
             payload = excluded.payload",
        params![
            owner_kind,
            owner_id,
            revision,
            encoded.cardinality as i64,
            ORDER_FORMAT_VERSION,
            encoded.checksum,
            encoded.payload,
        ],
    )?;
    Ok(())
}

pub fn replace_ordered_membership(
    transaction: &Transaction<'_>,
    owner_kind: &str,
    owner_id: i64,
    revision: i64,
    ids: &[u32],
) -> rusqlite::Result<()> {
    let members = ids.iter().copied().collect::<RoaringBitmap>();
    if members.len() != ids.len() as u64 {
        return Err(invalid_data("Ordering contains duplicate member IDs"));
    }
    let domain = match owner_kind {
        "group" => BitmapDomain::GroupMember,
        "folder" => BitmapDomain::Folder,
        _ => return Err(invalid_data("Invalid ordered membership owner")),
    };
    replace_bitmap(transaction, domain, owner_id, revision, &members)?;
    replace_order(transaction, owner_kind, owner_id, revision, ids)
}

pub fn load_order(
    connection: &Connection,
    owner_kind: &str,
    owner_id: i64,
) -> rusqlite::Result<Option<Vec<u32>>> {
    connection
        .query_row(
            "SELECT cardinality, checksum, payload, format_version
             FROM canonical_order WHERE owner_kind = ?1 AND owner_id = ?2",
            params![owner_kind, owner_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(cardinality, checksum, payload, version)| {
            if version != ORDER_FORMAT_VERSION {
                return Err(invalid_data(format!(
                    "Unsupported ordering format version {version}"
                )));
            }
            let cardinality = u64::try_from(cardinality)
                .map_err(|_| invalid_data("Invalid ordering cardinality"))?;
            decode_order(&EncodedOrder {
                cardinality,
                checksum,
                payload,
            })
            .map_err(invalid_data)
        })
        .transpose()
}

/// Explicit membership for test seeding: canonical ownership is supplied by
/// the fixture instead of being derived from relationship rows.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestMembership {
    pub tags: Vec<(i64, Vec<u32>)>,
    pub folders: Vec<(i64, Vec<u32>)>,
    pub groups: Vec<(i64, Vec<u32>)>,
}

#[cfg(test)]
pub(crate) fn seed_test_state(
    transaction: &Transaction<'_>,
    membership: &TestMembership,
) -> rusqlite::Result<()> {
    let revision = transaction.query_row(
        "SELECT revision FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let mut lifecycles = BTreeMap::<i64, RoaringBitmap>::from([
        (LIFECYCLE_ACTIVE_KEY, RoaringBitmap::new()),
        (LIFECYCLE_INBOX_KEY, RoaringBitmap::new()),
        (LIFECYCLE_TRASH_KEY, RoaringBitmap::new()),
    ]);
    let mut ratings = BTreeMap::<i64, RoaringBitmap>::new();
    {
        let mut statement = transaction.prepare(
            "SELECT root.item_id, root.lifecycle, metadata.rating
             FROM library_root root
             LEFT JOIN root_metadata metadata ON metadata.root_item_id = root.item_id
             ORDER BY root.item_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (item_id, lifecycle, rating) = row?;
            let item_id = u32::try_from(item_id)
                .map_err(|_| invalid_data("Test fixture root ID exceeds the bitmap domain"))?;
            let lifecycle_key = match lifecycle.as_str() {
                "active" => LIFECYCLE_ACTIVE_KEY,
                "inbox" => LIFECYCLE_INBOX_KEY,
                "trash" => LIFECYCLE_TRASH_KEY,
                _ => return Err(invalid_data("Test fixture has an invalid lifecycle")),
            };
            lifecycles.get_mut(&lifecycle_key).unwrap().insert(item_id);
            let rating = rating
                .map(u8::try_from)
                .transpose()
                .map_err(|_| invalid_data("Test fixture has an invalid rating"))?;
            ratings
                .entry(rating_key(rating))
                .or_default()
                .insert(item_id);
        }
    }

    let mut tags = BTreeMap::<i64, RoaringBitmap>::new();
    for (tag_id, roots) in &membership.tags {
        tags.entry(*tag_id)
            .or_default()
            .extend(roots.iter().copied());
    }

    let mut folders = BTreeMap::<i64, RoaringBitmap>::new();
    let mut folder_orders = BTreeMap::<i64, Vec<u32>>::new();
    for (folder_id, roots) in &membership.folders {
        folders
            .entry(*folder_id)
            .or_default()
            .extend(roots.iter().copied());
        folder_orders
            .entry(*folder_id)
            .or_default()
            .extend(roots.iter().copied());
    }

    let mut groups = BTreeMap::<i64, Vec<u32>>::new();
    {
        let mut statement = transaction.prepare(
            "SELECT item_id FROM library_item WHERE kind = 'collection' ORDER BY item_id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        for row in rows {
            groups.insert(row?, Vec::new());
        }
    }
    for (group_id, media_ids) in &membership.groups {
        groups.insert(*group_id, media_ids.clone());
    }

    transaction.execute(
        "DELETE FROM canonical_bitmap WHERE domain BETWEEN ?1 AND ?2",
        params![
            BitmapDomain::Lifecycle.as_i64(),
            BitmapDomain::GroupMember.as_i64()
        ],
    )?;
    transaction.execute(
        "DELETE FROM canonical_order WHERE owner_kind IN ('group', 'folder')",
        [],
    )?;
    for (key, bitmap) in lifecycles {
        replace_bitmap(transaction, BitmapDomain::Lifecycle, key, revision, &bitmap)?;
    }
    for (key, bitmap) in ratings {
        replace_bitmap(transaction, BitmapDomain::Rating, key, revision, &bitmap)?;
    }
    for (key, bitmap) in tags {
        replace_bitmap(transaction, BitmapDomain::Tag, key, revision, &bitmap)?;
    }
    for (key, bitmap) in folders {
        replace_bitmap(transaction, BitmapDomain::Folder, key, revision, &bitmap)?;
    }
    for (folder_id, order) in folder_orders {
        replace_order(transaction, "folder", folder_id, revision, &order)?;
    }
    for (group_id, members) in groups {
        replace_ordered_membership(transaction, "group", group_id, revision, &members)?;
    }
    Ok(())
}

fn checksum(payload: &[u8]) -> String {
    hex::encode(Sha256::digest(payload))
}

fn invalid_data(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Blob,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message.into(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::schema;

    #[test]
    fn sharded_portable_bitmap_round_trips_exactly() {
        let bitmap = RoaringBitmap::from_iter([0, 1, 65_535, 65_536, 65_537, u32::MAX]);
        let encoded = encode_shards(&bitmap).unwrap();
        assert_eq!(encoded.len(), 3);
        let decoded = encoded
            .iter()
            .try_fold(RoaringBitmap::new(), |mut all, shard| {
                all |= decode_shard(shard)?;
                Ok::<_, String>(all)
            })
            .unwrap();
        assert_eq!(decoded, bitmap);
    }

    #[test]
    fn bitmap_checksum_rejects_corruption() {
        let mut encoded = encode_shards(&RoaringBitmap::from_iter([1, 2, 3]))
            .unwrap()
            .remove(0);
        encoded.payload[0] ^= 0xff;
        assert!(decode_shard(&encoded).unwrap_err().contains("checksum"));
    }

    #[test]
    fn ordering_round_trips_and_rejects_corruption() {
        let ids = vec![9, 2, u32::MAX, 4];
        let encoded = encode_order(&ids);
        assert_eq!(decode_order(&encoded).unwrap(), ids);

        let mut corrupted = encoded;
        corrupted.payload[16] ^= 1;
        assert!(decode_order(&corrupted).unwrap_err().contains("checksum"));
    }

    #[test]
    fn sqlite_round_trip_is_revisioned_and_exact() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema::create_canonical_v1(&mut connection).unwrap();
        let bitmap = RoaringBitmap::from_iter([7, 65_536, 65_539, 300_000]);
        let order = vec![300_000, 7, 65_539, 65_536];

        let transaction = connection.transaction().unwrap();
        replace_bitmap(&transaction, BitmapDomain::GroupMember, 42, 1, &bitmap).unwrap();
        replace_order(&transaction, "group", 42, 1, &order).unwrap();
        transaction.commit().unwrap();

        assert_eq!(
            load_bitmap(&connection, BitmapDomain::GroupMember, 42).unwrap(),
            bitmap
        );
        assert_eq!(load_order(&connection, "group", 42).unwrap(), Some(order));
        let revisions: Vec<i64> = connection
            .prepare("SELECT DISTINCT revision FROM canonical_bitmap")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(revisions, vec![1]);
    }

    #[test]
    fn sqlite_load_rejects_payload_corruption() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema::create_canonical_v1(&mut connection).unwrap();
        let transaction = connection.transaction().unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Tag,
            9,
            1,
            &RoaringBitmap::from_iter([1, 2, 3]),
        )
        .unwrap();
        transaction.commit().unwrap();
        connection
            .execute(
                "UPDATE canonical_bitmap SET payload = zeroblob(length(payload))",
                [],
            )
            .unwrap();

        assert!(load_bitmap(&connection, BitmapDomain::Tag, 9).is_err());
    }

    #[test]
    fn categorical_keys_are_stable_and_never_reused() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema::create_canonical_v1(&mut connection).unwrap();
        let transaction = connection.transaction().unwrap();
        let jpeg = intern_key(&transaction, BitmapDomain::Mime, "image/jpeg").unwrap();
        assert_eq!(
            intern_key(&transaction, BitmapDomain::Mime, "image/jpeg").unwrap(),
            jpeg
        );
        transaction
            .execute(
                "DELETE FROM canonical_bitmap_key WHERE domain = ?1 AND key_id = ?2",
                params![BitmapDomain::Mime.as_i64(), i64::from(jpeg)],
            )
            .unwrap();
        let png = intern_key(&transaction, BitmapDomain::Mime, "image/png").unwrap();
        assert_eq!(png, jpeg + 1);
        transaction.commit().unwrap();
    }

    #[test]
    fn ordered_membership_rejects_duplicates_and_round_trips() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema::create_canonical_v1(&mut connection).unwrap();
        let transaction = connection.transaction().unwrap();
        assert!(replace_ordered_membership(&transaction, "group", 5, 1, &[9, 9]).is_err());
        replace_ordered_membership(&transaction, "group", 5, 1, &[8, 3, 13]).unwrap();
        transaction.commit().unwrap();

        assert_eq!(
            load_bitmap(&connection, BitmapDomain::GroupMember, 5).unwrap(),
            RoaringBitmap::from_iter([3, 8, 13])
        );
        assert_eq!(
            load_order(&connection, "group", 5).unwrap(),
            Some(vec![8, 3, 13])
        );
    }
}
