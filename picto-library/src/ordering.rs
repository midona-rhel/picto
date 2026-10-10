use rusqlite::{params, Connection, Transaction};

use crate::bitmap::checksum;
use crate::{LibraryError, Result};

/// Orders are stored in fixed-size chunks so an edit rewrites only the chunks
/// it touched. Appending to a 200k-item folder used to rewrite an 800 KB blob
/// (and log it to the WAL); it now rewrites the last 4 KB chunk.
pub const ORDER_CHUNK_LEN: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OrderOwnerKind {
    Collection = 1,
    Folder = 2,
}

fn encode_chunk(values: &[u32]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(values.len() * 4);
    for value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload
}

fn decode_chunk(payload: &[u8], expected_checksum: &[u8], into: &mut Vec<u32>) -> Result<()> {
    if checksum(payload).as_slice() != expected_checksum {
        return Err(LibraryError::InvalidState(
            "ordered membership checksum mismatch".into(),
        ));
    }
    if !payload.len().is_multiple_of(4) || payload.len() > ORDER_CHUNK_LEN * 4 {
        return Err(LibraryError::InvalidState(
            "ordered membership chunk length mismatch".into(),
        ));
    }
    into.extend(
        payload
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes)),
    );
    Ok(())
}

fn require_unique(values: &[u32], invalid: impl FnOnce(String) -> LibraryError) -> Result<()> {
    // Every rewrite of a large folder order checks this, so the check is a
    // dense bitset over the ids (local ids are compact) rather than a hash set.
    let words = values.iter().max().map_or(0, |max| (*max as usize >> 6) + 1);
    let duplicate = if words <= values.len().max(1 << 16) {
        let mut seen = vec![0u64; words];
        values.iter().any(|value| {
            let (word, bit) = (*value as usize >> 6, 1u64 << (value & 63));
            let present = seen[word] & bit != 0;
            seen[word] |= bit;
            present
        })
    } else {
        let mut unique = std::collections::HashSet::with_capacity(values.len());
        values.iter().any(|value| !unique.insert(*value))
    };
    if duplicate {
        return Err(invalid("ordered membership contains duplicate IDs".into()));
    }
    Ok(())
}

/// Reassemble every stored order. Rows arrive sorted by owner and chunk.
pub fn load_all(
    connection: &Connection,
    mut each: impl FnMut(OrderOwnerKind, u32, Vec<u32>) -> Result<()>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT owner_kind, owner_id, chunk_index, checksum, payload
         FROM ordered_membership_chunk
         ORDER BY owner_kind, owner_id, chunk_index",
    )?;
    let mut rows = statement.query([])?;
    let mut current: Option<(u8, u32, Vec<u32>)> = None;
    let mut finish = |entry: Option<(u8, u32, Vec<u32>)>| -> Result<()> {
        if let Some((kind, owner_id, values)) = entry {
            require_unique(&values, LibraryError::InvalidState)?;
            each(owner_kind(kind)?, owner_id, values)?;
        }
        Ok(())
    };
    let mut index = 0_usize;
    while let Some(row) = rows.next()? {
        if index.is_multiple_of(256) {
            crate::query_request::check()?;
        }
        index += 1;
        let kind = row.get::<_, u8>(0)?;
        let owner_id = row.get::<_, u32>(1)?;
        let chunk_index = row.get::<_, i64>(2)? as usize;
        let digest = row.get_ref(3)?.as_blob().map_err(rusqlite::Error::from)?;
        let payload = row.get_ref(4)?.as_blob().map_err(rusqlite::Error::from)?;
        if current
            .as_ref()
            .is_none_or(|(current_kind, current_owner, _)| {
                (*current_kind, *current_owner) != (kind, owner_id)
            })
        {
            finish(current.take())?;
            current = Some((kind, owner_id, Vec::new()));
        }
        let (_, _, values) = current.as_mut().expect("current order");
        if chunk_index * ORDER_CHUNK_LEN != values.len() {
            return Err(LibraryError::InvalidState(
                "ordered membership chunks are not contiguous".into(),
            ));
        }
        decode_chunk(payload, digest, values)?;
    }
    finish(current.take())
}

fn owner_kind(value: u8) -> Result<OrderOwnerKind> {
    match value {
        value if value == OrderOwnerKind::Collection as u8 => Ok(OrderOwnerKind::Collection),
        value if value == OrderOwnerKind::Folder as u8 => Ok(OrderOwnerKind::Folder),
        _ => Err(LibraryError::InvalidState(format!(
            "unknown ordered membership kind {value}"
        ))),
    }
}

pub fn load(
    connection: &Connection,
    owner_kind: OrderOwnerKind,
    owner_id: u32,
) -> Result<Option<Vec<u32>>> {
    let mut statement = connection.prepare_cached(
        "SELECT chunk_index, checksum, payload FROM ordered_membership_chunk
         WHERE owner_kind = ?1 AND owner_id = ?2 ORDER BY chunk_index",
    )?;
    let mut rows = statement.query(params![owner_kind as u8, owner_id])?;
    let mut values = None::<Vec<u32>>;
    while let Some(row) = rows.next()? {
        let values = values.get_or_insert_with(Vec::new);
        if row.get::<_, i64>(0)? as usize * ORDER_CHUNK_LEN != values.len() {
            return Err(LibraryError::InvalidState(
                "ordered membership chunks are not contiguous".into(),
            ));
        }
        decode_chunk(
            row.get_ref(2)?.as_blob().map_err(rusqlite::Error::from)?,
            row.get_ref(1)?.as_blob().map_err(rusqlite::Error::from)?,
            values,
        )?;
    }
    if let Some(values) = &values {
        require_unique(values, LibraryError::InvalidState)?;
    }
    Ok(values)
}

/// Store `values` as the order of one owner, rewriting only the chunks whose
/// content changed. An empty order keeps one empty chunk, so "empty" and
/// "absent" stay distinct. Returns whether anything was written.
pub fn replace(
    transaction: &Transaction<'_>,
    _revision: u64,
    owner_kind: OrderOwnerKind,
    owner_id: u32,
    values: &[u32],
) -> Result<bool> {
    require_unique(values, LibraryError::InvalidInput)?;
    let stored = {
        let mut statement = transaction.prepare_cached(
            "SELECT chunk_index, checksum FROM ordered_membership_chunk
             WHERE owner_kind = ?1 AND owner_id = ?2",
        )?;
        let rows = statement.query_map(params![owner_kind as u8, owner_id], |row| {
            Ok((row.get::<_, i64>(0)? as usize, row.get::<_, Vec<u8>>(1)?))
        })?;
        rows.collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?
    };
    let chunks = if values.is_empty() {
        vec![&values[..0]]
    } else {
        values.chunks(ORDER_CHUNK_LEN).collect::<Vec<_>>()
    };
    let mut changed = false;
    for (chunk_index, chunk) in chunks.iter().enumerate() {
        let payload = encode_chunk(chunk);
        let digest = checksum(&payload);
        if stored
            .get(&chunk_index)
            .is_some_and(|current| current.as_slice() == digest)
        {
            continue;
        }
        transaction
            .prepare_cached(
                "INSERT INTO ordered_membership_chunk
                     (owner_kind, owner_id, chunk_index, checksum, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(owner_kind, owner_id, chunk_index) DO UPDATE SET
                     checksum = excluded.checksum,
                     payload = excluded.payload",
            )?
            .execute(params![
                owner_kind as u8,
                owner_id,
                chunk_index as i64,
                digest.as_slice(),
                payload
            ])?;
        changed = true;
    }
    if stored.keys().any(|index| *index >= chunks.len()) {
        transaction.execute(
            "DELETE FROM ordered_membership_chunk
             WHERE owner_kind = ?1 AND owner_id = ?2 AND chunk_index >= ?3",
            params![owner_kind as u8, owner_id, chunks.len() as i64],
        )?;
        changed = true;
    }
    Ok(changed)
}

pub fn delete(
    transaction: &Transaction<'_>,
    owner_kind: OrderOwnerKind,
    owner_id: u32,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM ordered_membership_chunk WHERE owner_kind = ?1 AND owner_id = ?2",
        params![owner_kind as u8, owner_id],
    )?;
    Ok(())
}

/// Migration: split each single-blob order into chunks.
pub(crate) fn migrate_single_blob_orders(connection: &Connection) -> Result<()> {
    connection.execute_batch(ORDER_CHUNK_SCHEMA)?;
    let legacy = {
        let mut statement = connection
            .prepare("SELECT owner_kind, owner_id, checksum, payload FROM ordered_membership")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, u8>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut insert = connection.prepare(
        "INSERT INTO ordered_membership_chunk
             (owner_kind, owner_id, chunk_index, checksum, payload)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (kind, owner_id, digest, payload) in legacy {
        if checksum(&payload).as_slice() != digest.as_slice() || payload.len() < 4 {
            return Err(LibraryError::InvalidState(
                "ordered membership checksum mismatch".into(),
            ));
        }
        let mut values = Vec::new();
        decode_chunk_unchecked(&payload[4..], &mut values)?;
        let chunks = if values.is_empty() {
            vec![&values[..0]]
        } else {
            values.chunks(ORDER_CHUNK_LEN).collect::<Vec<_>>()
        };
        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            let payload = encode_chunk(chunk);
            insert.execute(params![
                kind,
                owner_id,
                chunk_index as i64,
                checksum(&payload).as_slice(),
                payload
            ])?;
        }
    }
    connection.execute_batch("DROP TABLE ordered_membership;")?;
    Ok(())
}

fn decode_chunk_unchecked(payload: &[u8], into: &mut Vec<u32>) -> Result<()> {
    if !payload.len().is_multiple_of(4) {
        return Err(LibraryError::InvalidState(
            "ordered membership length mismatch".into(),
        ));
    }
    into.extend(
        payload
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes)),
    );
    Ok(())
}

pub(crate) const ORDER_CHUNK_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS ordered_membership_chunk (
    owner_kind INTEGER NOT NULL CHECK (owner_kind IN (1, 2)),
    owner_id INTEGER NOT NULL,
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    checksum BLOB NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY(owner_kind, owner_id, chunk_index)
) WITHOUT ROWID, STRICT;";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_are_found_with_compact_and_sparse_ids() {
        let invalid = |message: String| LibraryError::InvalidInput(message);
        assert!(require_unique(&[], invalid).is_ok());
        assert!(require_unique(&[5, 900_000, 63, 64], invalid).is_ok());
        assert!(require_unique(&[5, 900_000, 63, 5], invalid).is_err());
        // Ids far beyond the order's length take the hash-set path.
        assert!(require_unique(&[u32::MAX, 1, u32::MAX - 1], invalid).is_ok());
        assert!(require_unique(&[u32::MAX, 1, u32::MAX], invalid).is_err());
    }

    #[test]
    fn an_append_rewrites_only_the_last_chunk_and_a_shrink_drops_the_tail() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(ORDER_CHUNK_SCHEMA).unwrap();
        let transaction = connection.transaction().unwrap();
        let mut ids = (1..=2_500_u32).collect::<Vec<_>>();
        assert!(replace(&transaction, 1, OrderOwnerKind::Folder, 4, &ids).unwrap());
        assert!(!replace(&transaction, 2, OrderOwnerKind::Folder, 4, &ids).unwrap());

        ids.push(9_999);
        let writes = transaction.total_changes();
        assert!(replace(&transaction, 3, OrderOwnerKind::Folder, 4, &ids).unwrap());
        assert_eq!(
            transaction.total_changes() - writes,
            1,
            "only the last chunk is rewritten"
        );
        assert_eq!(
            load(&transaction, OrderOwnerKind::Folder, 4)
                .unwrap()
                .unwrap(),
            ids
        );

        ids.truncate(10);
        assert!(replace(&transaction, 4, OrderOwnerKind::Folder, 4, &ids).unwrap());
        assert_eq!(
            load(&transaction, OrderOwnerKind::Folder, 4)
                .unwrap()
                .unwrap(),
            ids
        );
        assert!(replace(&transaction, 5, OrderOwnerKind::Folder, 4, &[]).unwrap());
        assert_eq!(
            load(&transaction, OrderOwnerKind::Folder, 4).unwrap(),
            Some(Vec::new())
        );
        assert!(replace(&transaction, 6, OrderOwnerKind::Folder, 4, &[7, 7]).is_err());
    }
}
