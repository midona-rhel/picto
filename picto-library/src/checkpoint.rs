use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::schema::SCHEMA_FINGERPRINT;
use crate::Result;

pub const PROJECTION_IMPLEMENTATION_HASH: &str = "greenfield-projection-v1";

pub fn write(connection: &Connection, revision: u64, payload: &[u8]) -> Result<()> {
    let digest: [u8; 32] = Sha256::digest(payload).into();
    connection.execute(
        "INSERT INTO projection_checkpoint
             (singleton, schema_fingerprint, implementation_hash, database_revision, checksum, payload)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(singleton) DO UPDATE SET
             schema_fingerprint = excluded.schema_fingerprint,
             implementation_hash = excluded.implementation_hash,
             database_revision = excluded.database_revision,
             checksum = excluded.checksum,
             payload = excluded.payload",
        params![
            SCHEMA_FINGERPRINT,
            PROJECTION_IMPLEMENTATION_HASH,
            revision as i64,
            digest.as_slice(),
            payload
        ],
    )?;
    Ok(())
}

pub fn read(connection: &Connection, revision: u64) -> Result<Option<Vec<u8>>> {
    let result = connection.query_row(
        "SELECT schema_fingerprint, implementation_hash, database_revision, checksum, payload
         FROM projection_checkpoint WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        },
    );
    let Ok((schema, implementation, checkpoint_revision, checksum, payload)) = result else {
        return Ok(None);
    };
    if schema != SCHEMA_FINGERPRINT
        || implementation != PROJECTION_IMPLEMENTATION_HASH
        || checkpoint_revision != revision
        || Sha256::digest(&payload).as_slice() != checksum
    {
        return Ok(None);
    }
    Ok(Some(payload))
}
