use std::io::Cursor;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::bitmap::checksum;
use crate::{LibraryError, Result};

pub const ORDER_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OrderOwnerKind {
    Collection = 1,
    Folder = 2,
}

pub fn encode(values: &[u32]) -> Result<Vec<u8>> {
    if values.windows(2).any(|window| window[0] == window[1]) {
        return Err(LibraryError::InvalidInput(
            "ordered membership contains adjacent duplicates".into(),
        ));
    }
    let mut unique = std::collections::HashSet::with_capacity(values.len());
    if values.iter().any(|value| !unique.insert(*value)) {
        return Err(LibraryError::InvalidInput(
            "ordered membership contains duplicate IDs".into(),
        ));
    }
    let mut payload = Vec::with_capacity(4 + values.len() * 4);
    payload.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    Ok(payload)
}

pub fn decode(payload: &[u8], expected_checksum: &[u8]) -> Result<Vec<u32>> {
    if checksum(payload).as_slice() != expected_checksum {
        return Err(LibraryError::InvalidState(
            "ordered membership checksum mismatch".into(),
        ));
    }
    if payload.len() < 4 {
        return Err(LibraryError::InvalidState(
            "ordered membership payload is truncated".into(),
        ));
    }
    let mut cursor = Cursor::new(payload);
    let mut count_bytes = [0u8; 4];
    std::io::Read::read_exact(&mut cursor, &mut count_bytes)?;
    let count = u32::from_le_bytes(count_bytes) as usize;
    if payload.len() != 4 + count * 4 {
        return Err(LibraryError::InvalidState(
            "ordered membership length mismatch".into(),
        ));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let mut bytes = [0u8; 4];
        std::io::Read::read_exact(&mut cursor, &mut bytes)?;
        values.push(u32::from_le_bytes(bytes));
    }
    let mut unique = std::collections::HashSet::with_capacity(values.len());
    if values.iter().any(|value| !unique.insert(*value)) {
        return Err(LibraryError::InvalidState(
            "ordered membership contains duplicate IDs".into(),
        ));
    }
    Ok(values)
}

pub fn load(
    connection: &Connection,
    owner_kind: OrderOwnerKind,
    owner_id: u32,
) -> Result<Option<Vec<u32>>> {
    let row = connection
        .query_row(
            "SELECT cardinality, format_version, checksum, payload
             FROM ordered_membership WHERE owner_kind = ?1 AND owner_id = ?2",
            params![owner_kind as u8, owner_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, u32>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .optional()?;
    row.map(|(cardinality, version, digest, payload)| {
        if version != ORDER_FORMAT_VERSION {
            return Err(LibraryError::InvalidState(format!(
                "unsupported ordered membership format {version}"
            )));
        }
        let values = decode(&payload, &digest)?;
        if values.len() != cardinality {
            return Err(LibraryError::InvalidState(
                "ordered membership cardinality mismatch".into(),
            ));
        }
        Ok(values)
    })
    .transpose()
}

pub fn replace(
    transaction: &Transaction<'_>,
    revision: u64,
    owner_kind: OrderOwnerKind,
    owner_id: u32,
    values: &[u32],
) -> Result<bool> {
    let payload = encode(values)?;
    let digest = checksum(&payload);
    let existing = transaction
        .query_row(
            "SELECT checksum FROM ordered_membership WHERE owner_kind = ?1 AND owner_id = ?2",
            params![owner_kind as u8, owner_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    if existing.is_some_and(|current| current.as_slice() == digest) {
        return Ok(false);
    }
    transaction.execute(
        "INSERT INTO ordered_membership
             (owner_kind, owner_id, revision, cardinality, format_version, checksum, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(owner_kind, owner_id) DO UPDATE SET
             revision = excluded.revision,
             cardinality = excluded.cardinality,
             format_version = excluded.format_version,
             checksum = excluded.checksum,
             payload = excluded.payload",
        params![
            owner_kind as u8,
            owner_id,
            revision as i64,
            values.len() as i64,
            ORDER_FORMAT_VERSION,
            digest.as_slice(),
            payload
        ],
    )?;
    Ok(true)
}

pub fn delete(
    transaction: &Transaction<'_>,
    owner_kind: OrderOwnerKind,
    owner_id: u32,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM ordered_membership WHERE owner_kind = ?1 AND owner_id = ?2",
        params![owner_kind as u8, owner_id],
    )?;
    Ok(())
}
