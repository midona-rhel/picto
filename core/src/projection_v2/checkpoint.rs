//! Durable startup checkpoint for the rebuildable in-memory projection.

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use roaring::RoaringBitmap;
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use super::{
    color_is_finite, rebuild_all_color_roots, rebuild_all_mime_roots, validate_bitmap_ids,
    LabColorProjectionValue, NumericIndexes, ShardedMap, Shared, State,
};

const COMPONENT: &str = "projection-v2-roaring";
const MAGIC: &[u8; 8] = b"PCTOV2\0\x02";
const IMPLEMENTATION_MATERIAL: &[u8] =
    b"projection-v2-checkpoint-v11:lab-color-candidates:complete-immutable-state:portable-roaring";
const MAX_CHECKPOINT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRY_COUNT: usize = 100_000_000;

struct Metadata {
    schema_fingerprint: String,
    implementation_hash: String,
    database_revision: u64,
    checksum: String,
    health: String,
    checkpoint_path: Option<String>,
}

pub(super) fn load(connection: &Connection) -> Result<Option<State>, String> {
    let Some(expected_path) = checkpoint_path(connection)? else {
        return Ok(None);
    };
    let Some(metadata) = read_metadata(connection)? else {
        return Ok(None);
    };
    let schema_fingerprint = schema_fingerprint(connection)?;
    let implementation_hash = implementation_hash();
    let database_revision = crate::store::schema::revision(connection)
        .map_err(|error| format!("Failed to read checkpoint database revision: {error}"))?;
    if metadata.health != "healthy"
        || metadata.schema_fingerprint != schema_fingerprint
        || metadata.implementation_hash != implementation_hash
        || metadata.database_revision != database_revision
        || metadata.checkpoint_path.as_deref() != expected_path.to_str()
    {
        return Ok(None);
    }

    let file = fs::File::open(&expected_path)
        .map_err(|error| format!("Failed to open projection checkpoint: {error}"))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("Failed to inspect projection checkpoint: {error}"))?
        .len();
    if file_len > MAX_CHECKPOINT_BYTES {
        return Err("Projection checkpoint exceeds the size limit".to_string());
    }
    let bytes = fs::read(&expected_path)
        .map_err(|error| format!("Failed to read projection checkpoint: {error}"))?;
    let mut decoder = Decoder::new(&bytes);
    if decoder.take(MAGIC.len())? != MAGIC {
        return Err("Projection checkpoint has an invalid header".to_string());
    }
    let file_schema = hex::encode(decoder.take(32)?);
    let file_implementation = hex::encode(decoder.take(32)?);
    let file_revision = decoder.u64()?;
    let file_checksum = hex::encode(decoder.take(32)?);
    let payload_len = decoder.length()?;
    let payload = decoder.take(payload_len)?;
    decoder.finish()?;

    let actual_checksum = digest_hex(payload);
    if file_schema != metadata.schema_fingerprint
        || file_implementation != metadata.implementation_hash
        || file_revision != metadata.database_revision
        || file_checksum != metadata.checksum
        || actual_checksum != metadata.checksum
    {
        return Err("Projection checkpoint validation failed".to_string());
    }

    let state = decode_state(payload)?;
    validate_bitmap_ids(&state)?;
    Ok(Some(state))
}

pub(super) fn write(connection: &Connection, state: &State) -> Result<(), String> {
    let Some(path) = checkpoint_path(connection)? else {
        return Err("Projection checkpoints require a file-backed database".to_string());
    };
    validate_bitmap_ids(state)?;
    let schema_fingerprint = schema_fingerprint(connection)?;
    let implementation_hash = implementation_hash();
    let database_revision = crate::store::schema::revision(connection)
        .map_err(|error| format!("Failed to read checkpoint database revision: {error}"))?;
    let database_revision_sql = i64::try_from(database_revision)
        .map_err(|_| "Checkpoint database revision exceeds SQLite's range".to_string())?;
    let payload = encode_state(state)?;
    let checksum = digest_hex(&payload);

    let mut bytes = Vec::with_capacity(120 + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&decode_digest(&schema_fingerprint)?);
    bytes.extend_from_slice(&decode_digest(&implementation_hash)?);
    bytes.extend_from_slice(&database_revision.to_le_bytes());
    bytes.extend_from_slice(&decode_digest(&checksum)?);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);

    let parent = path
        .parent()
        .ok_or_else(|| "Projection checkpoint path has no parent directory".to_string())?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| format!("Failed to create projection checkpoint: {error}"))?;
    std::io::Write::write_all(&mut temporary, &bytes)
        .map_err(|error| format!("Failed to write projection checkpoint: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Failed to flush projection checkpoint: {error}"))?;
    temporary
        .persist(&path)
        .map_err(|error| format!("Failed to publish projection checkpoint: {}", error.error))?;

    connection
        .execute(
            "INSERT INTO projection_checkpoint (
                 component, schema_fingerprint, implementation_hash,
                 database_revision, checksum, health, checkpoint_path, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'healthy', ?6,
                       strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(component) DO UPDATE SET
                 schema_fingerprint = excluded.schema_fingerprint,
                 implementation_hash = excluded.implementation_hash,
                 database_revision = excluded.database_revision,
                 checksum = excluded.checksum,
                 health = excluded.health,
                 checkpoint_path = excluded.checkpoint_path,
                 updated_at = excluded.updated_at",
            rusqlite::params![
                COMPONENT,
                schema_fingerprint,
                implementation_hash,
                database_revision_sql,
                checksum,
                path.to_string_lossy().as_ref(),
            ],
        )
        .map_err(|error| format!("Failed to record projection checkpoint: {error}"))?;
    Ok(())
}

fn read_metadata(connection: &Connection) -> Result<Option<Metadata>, String> {
    connection
        .query_row(
            "SELECT schema_fingerprint, implementation_hash, database_revision,
                    checksum, health, checkpoint_path
             FROM projection_checkpoint WHERE component = ?1",
            [COMPONENT],
            |row| {
                let revision = row.get::<_, i64>(2)?;
                Ok(Metadata {
                    schema_fingerprint: row.get(0)?,
                    implementation_hash: row.get(1)?,
                    database_revision: u64::try_from(revision)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, revision))?,
                    checksum: row.get(3)?,
                    health: row.get(4)?,
                    checkpoint_path: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed to read projection checkpoint metadata: {error}"))
}

fn checkpoint_path(connection: &Connection) -> Result<Option<PathBuf>, String> {
    let mut statement = connection
        .prepare("PRAGMA database_list")
        .map_err(|error| format!("Failed to inspect checkpoint database path: {error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("Failed to inspect checkpoint database path: {error}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("Failed to inspect checkpoint database path: {error}"))?
    {
        if row.get::<_, String>(1).map_err(|error| error.to_string())? == "main" {
            let path = row.get::<_, String>(2).map_err(|error| error.to_string())?;
            if path.is_empty() || path == ":memory:" {
                return Ok(None);
            }
            return Ok(Some(PathBuf::from(format!("{path}.projection-v2"))));
        }
    }
    Ok(None)
}

fn schema_fingerprint(connection: &Connection) -> Result<String, String> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, COALESCE(sql, '')
             FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )
        .map_err(|error| format!("Failed to fingerprint checkpoint schema: {error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("Failed to fingerprint checkpoint schema: {error}"))?;
    let mut digest = Sha256::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("Failed to fingerprint checkpoint schema: {error}"))?
    {
        for column in 0..4 {
            let value = row
                .get::<_, String>(column)
                .map_err(|error| format!("Failed to fingerprint checkpoint schema: {error}"))?;
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    Ok(hex::encode(digest.finalize()))
}

fn implementation_hash() -> String {
    digest_hex(IMPLEMENTATION_MATERIAL)
}

fn digest_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn decode_digest(value: &str) -> Result<[u8; 32], String> {
    let bytes =
        hex::decode(value).map_err(|_| "Checkpoint digest is not hexadecimal".to_string())?;
    bytes
        .try_into()
        .map_err(|_| "Checkpoint digest has an invalid length".to_string())
}

fn encode_state(state: &State) -> Result<Vec<u8>, String> {
    let mut encoder = Encoder::default();
    for bitmap in state.lifecycle_bitmaps.iter() {
        encoder.bitmap(bitmap)?;
    }
    encoder.numeric(state);
    encoder.id_set(&state.media_ids);
    encoder.string_map(&state.media_mime_types);
    encoder.lab_color_map(&state.media_lab_colors);
    encoder.bitmap(&state.image_media_ids)?;
    encoder.bitmap(&state.all_image_roots)?;
    encoder.id_set(&state.collection_ids);
    encoder.bitmap_map(&state.collection_members)?;
    encoder.id_vec_map(&state.collection_orders);
    encoder.id_map(&state.media_to_root);
    encoder.bitmap_map(&state.folder_members)?;
    encoder.id_vec_map(&state.folder_orders);
    encoder.bitmap_map(&state.folder_bitmaps)?;
    encoder.id_vec_map(&state.root_owned_folders);
    encoder.u32_map(&state.root_folder_counts);
    encoder.bitmap(&state.categorized_roots)?;
    encoder.id_vec_map(&state.root_owned_tags);
    encoder.bitmap_map(&state.direct_tag_bitmaps)?;
    encoder.bitmap(&state.tagged_roots)?;
    encoder.bitmap_map(&state.smart_folder_bitmaps)?;
    Ok(encoder.bytes)
}

fn decode_state(bytes: &[u8]) -> Result<State, String> {
    let mut decoder = Decoder::new(bytes);
    let lifecycle_bitmaps = Arc::new([decoder.bitmap()?, decoder.bitmap()?, decoder.bitmap()?]);
    let numeric = Arc::new(decoder.numeric(&lifecycle_bitmaps)?);
    let mut state = State {
        lifecycle_bitmaps,
        numeric,
        media_ids: decoder.id_set()?.into(),
        media_mime_types: decoder.string_map()?,
        media_lab_colors: decoder.lab_color_map()?,
        root_lab_colors: ShardedMap::default(),
        color_lab_cell_roots: ShardedMap::default(),
        root_mime_types: ShardedMap::default(),
        exact_mime_roots: ShardedMap::default(),
        mime_family_roots: ShardedMap::default(),
        image_media_ids: decoder.bitmap()?.into(),
        all_image_roots: decoder.bitmap()?.into(),
        collection_ids: decoder.id_set()?.into(),
        collection_members: decoder.bitmap_map()?,
        collection_orders: decoder.id_vec_map()?,
        media_to_root: decoder.id_map()?,
        folder_members: decoder.bitmap_map()?,
        folder_orders: decoder.id_vec_map()?,
        folder_bitmaps: decoder.bitmap_map()?,
        root_owned_folders: decoder.id_vec_map()?,
        root_folder_counts: decoder.u32_map()?,
        categorized_roots: decoder.bitmap()?.into(),
        root_owned_tags: decoder.id_vec_map()?,
        direct_tag_bitmaps: decoder.bitmap_map()?,
        tagged_roots: decoder.bitmap()?.into(),
        smart_folder_bitmaps: decoder.bitmap_map()?,
    };
    decoder.finish()?;
    rebuild_all_mime_roots(&mut state);
    rebuild_all_color_roots(&mut state);
    Ok(state)
}

#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    fn len(&mut self, value: usize) {
        self.u64(value as u64);
    }

    fn bitmap(&mut self, bitmap: &RoaringBitmap) -> Result<(), String> {
        let mut bytes = Vec::with_capacity(bitmap.serialized_size());
        bitmap
            .serialize_into(&mut bytes)
            .map_err(|error| format!("Failed to serialize checkpoint bitmap: {error}"))?;
        self.len(bytes.len());
        self.bytes.extend_from_slice(&bytes);
        Ok(())
    }

    fn numeric(&mut self, state: &State) {
        let roots = state
            .lifecycle_bitmaps
            .iter()
            .fold(RoaringBitmap::new(), |roots, bitmap| roots | bitmap);
        self.len(roots.len() as usize);
        for bitmap_id in roots {
            let id = i64::from(bitmap_id);
            self.i64(id);
            encode_option_u64(self, state.numeric.total_size_bytes.get(bitmap_id));
            encode_option_u64(self, state.numeric.media_count.get(bitmap_id));
            encode_option_u64(self, state.numeric.display_duration_ms.get(bitmap_id));
            encode_option_u64(self, state.numeric.display_width.get(bitmap_id));
            encode_option_u64(self, state.numeric.display_height.get(bitmap_id));
            encode_option_u64(self, state.numeric.imported_at.get(bitmap_id));
            encode_option_u64(self, state.numeric.modified_at.get(bitmap_id));
            match state.numeric.rating.get(bitmap_id) {
                Some(value) => {
                    self.u8(1);
                    self.u8(value);
                }
                None => self.u8(0),
            }
        }
    }

    fn id_set(&mut self, set: &HashSet<i64>) {
        let mut values = set.iter().copied().collect::<Vec<_>>();
        values.sort_unstable();
        self.len(values.len());
        for value in values {
            self.i64(value);
        }
    }

    fn string(&mut self, value: &str) {
        self.len(value.len());
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn string_map(&mut self, map: &ShardedMap<i64, String>) {
        let mut entries = map.iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| **key);
        self.len(entries.len());
        for (key, value) in entries {
            self.i64(*key);
            self.string(value);
        }
    }

    fn lab_color_map(&mut self, map: &ShardedMap<i64, Shared<Vec<LabColorProjectionValue>>>) {
        let mut entries = map.iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| **key);
        self.len(entries.len());
        for (key, colors) in entries {
            self.i64(*key);
            self.len(colors.len());
            for color in colors.iter() {
                self.f64(color.l);
                self.f64(color.a);
                self.f64(color.b);
            }
        }
    }

    fn id_vec_map(&mut self, map: &ShardedMap<i64, Shared<Vec<i64>>>) {
        let mut entries = map.iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| **key);
        self.len(entries.len());
        for (key, values) in entries {
            self.i64(*key);
            self.len(values.len());
            for value in values.iter().copied() {
                self.i64(value);
            }
        }
    }

    fn id_map(&mut self, map: &ShardedMap<i64, i64>) {
        let mut entries = map
            .iter()
            .map(|(key, value)| (*key, *value))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| *key);
        self.len(entries.len());
        for (key, value) in entries {
            self.i64(key);
            self.i64(value);
        }
    }

    fn u32_map(&mut self, map: &ShardedMap<i64, u32>) {
        let mut entries = map
            .iter()
            .map(|(key, value)| (*key, *value))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| *key);
        self.len(entries.len());
        for (key, value) in entries {
            self.i64(key);
            self.u32(value);
        }
    }


    fn bitmap_map(&mut self, map: &ShardedMap<i64, Shared<RoaringBitmap>>) -> Result<(), String> {
        let mut entries = map.iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| **key);
        self.len(entries.len());
        for (key, bitmap) in entries {
            self.i64(*key);
            self.bitmap(bitmap)?;
        }
        Ok(())
    }
}

fn encode_option_u64(encoder: &mut Encoder, value: Option<u64>) {
    match value {
        Some(value) => {
            encoder.u8(1);
            encoder.u64(value);
        }
        None => encoder.u8(0),
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| "Projection checkpoint length overflow".to_string())?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| "Projection checkpoint is truncated".to_string())?;
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_bits(self.u64()?))
    }

    fn length(&mut self) -> Result<usize, String> {
        let length = usize::try_from(self.u64()?)
            .map_err(|_| "Projection checkpoint length is too large".to_string())?;
        if length > MAX_ENTRY_COUNT.max(self.bytes.len()) {
            return Err("Projection checkpoint length exceeds the limit".to_string());
        }
        Ok(length)
    }

    fn count(&mut self, minimum_bytes: usize) -> Result<usize, String> {
        let count = self.length()?;
        if count > MAX_ENTRY_COUNT
            || count.saturating_mul(minimum_bytes) > self.bytes.len() - self.position
        {
            return Err("Projection checkpoint entry count is invalid".to_string());
        }
        Ok(count)
    }

    fn finish(&self) -> Result<(), String> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err("Projection checkpoint has trailing data".to_string())
        }
    }

    fn bitmap(&mut self) -> Result<RoaringBitmap, String> {
        let length = self.length()?;
        let bytes = self.take(length)?;
        let mut cursor = Cursor::new(bytes);
        let bitmap = RoaringBitmap::deserialize_from(&mut cursor)
            .map_err(|error| format!("Projection checkpoint bitmap is invalid: {error}"))?;
        if cursor.position() != length as u64 {
            return Err("Projection checkpoint bitmap has trailing data".to_string());
        }
        Ok(bitmap)
    }

    fn numeric(&mut self, roots: &[RoaringBitmap; 3]) -> Result<NumericIndexes, String> {
        let count = self.count(16)?;
        let mut numeric = NumericIndexes::default();
        let mut seen = HashSet::with_capacity(count);
        for _ in 0..count {
            let id = self.i64()?;
            if !seen.insert(id)
                || !roots
                    .iter()
                    .any(|bitmap| u32::try_from(id).is_ok_and(|id| bitmap.contains(id)))
            {
                return Err("Projection checkpoint numeric root is invalid".to_string());
            }
            let bitmap_id = u32::try_from(id)
                .map_err(|_| "Projection checkpoint numeric root is invalid".to_string())?;
            if let Some(value) = self.option_u64()? {
                numeric.total_size_bytes.set(bitmap_id, value);
            }
            if let Some(value) = self.option_u64()? {
                numeric.media_count.set(bitmap_id, value);
            }
            if let Some(value) = self.option_u64()? {
                numeric.display_duration_ms.set(bitmap_id, value);
            }
            if let Some(value) = self.option_u64()? {
                numeric.display_width.set(bitmap_id, value);
            }
            if let Some(value) = self.option_u64()? {
                numeric.display_height.set(bitmap_id, value);
            }
            if let Some(value) = self.option_u64()? {
                numeric.imported_at.set(bitmap_id, value);
            }
            if let Some(value) = self.option_u64()? {
                numeric.modified_at.set(bitmap_id, value);
            }
            match self.u8()? {
                0 => {}
                1 => {
                    numeric.rating.set(bitmap_id, self.u8()?);
                }
                _ => return Err("Projection checkpoint rating marker is invalid".to_string()),
            }
        }
        Ok(numeric)
    }

    fn option_u64(&mut self) -> Result<Option<u64>, String> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err("Projection checkpoint option marker is invalid".to_string()),
        }
    }

    fn id_set(&mut self) -> Result<HashSet<i64>, String> {
        let count = self.count(8)?;
        let mut set = HashSet::with_capacity(count);
        for _ in 0..count {
            if !set.insert(self.i64()?) {
                return Err("Projection checkpoint contains a duplicate ID".to_string());
            }
        }
        Ok(set)
    }

    fn string(&mut self) -> Result<String, String> {
        let length = self.length()?;
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| "Projection checkpoint string is not UTF-8".to_string())
    }

    fn string_map(&mut self) -> Result<ShardedMap<i64, String>, String> {
        let count = self.count(16)?;
        let mut map = ShardedMap::default();
        for _ in 0..count {
            let key = self.i64()?;
            let value = self.string()?;
            insert_unique(&mut map, key, value)?;
        }
        Ok(map)
    }

    fn lab_color_map(
        &mut self,
    ) -> Result<ShardedMap<i64, Shared<Vec<LabColorProjectionValue>>>, String> {
        let count = self.count(16)?;
        let mut map = ShardedMap::default();
        for _ in 0..count {
            let key = self.i64()?;
            let color_count = self.count(24)?;
            let mut colors = Vec::with_capacity(color_count);
            for _ in 0..color_count {
                let color = LabColorProjectionValue {
                    l: self.f64()?,
                    a: self.f64()?,
                    b: self.f64()?,
                };
                if !color_is_finite(color) {
                    return Err("Projection checkpoint Lab color is invalid".to_string());
                }
                colors.push(color);
            }
            insert_unique(&mut map, key, colors.into())?;
        }
        Ok(map)
    }

    fn id_vec_map(&mut self) -> Result<ShardedMap<i64, Shared<Vec<i64>>>, String> {
        let count = self.count(16)?;
        let mut map = ShardedMap::default();
        for _ in 0..count {
            let key = self.i64()?;
            let value_count = self.count(8)?;
            let mut values = Vec::with_capacity(value_count);
            for _ in 0..value_count {
                values.push(self.i64()?);
            }
            if values.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err("Projection checkpoint tag list is not ordered".to_string());
            }
            insert_unique(&mut map, key, Shared::from(values))?;
        }
        Ok(map)
    }

    fn id_map(&mut self) -> Result<ShardedMap<i64, i64>, String> {
        let count = self.count(16)?;
        let mut map = ShardedMap::default();
        for _ in 0..count {
            let key = self.i64()?;
            let value = self.i64()?;
            insert_unique(&mut map, key, value)?;
        }
        Ok(map)
    }

    fn u32_map(&mut self) -> Result<ShardedMap<i64, u32>, String> {
        let count = self.count(12)?;
        let mut map = ShardedMap::default();
        for _ in 0..count {
            let key = self.i64()?;
            let value = self.u32()?;
            insert_unique(&mut map, key, value)?;
        }
        Ok(map)
    }


    fn bitmap_map(&mut self) -> Result<ShardedMap<i64, Shared<RoaringBitmap>>, String> {
        let count = self.count(16)?;
        let mut map = ShardedMap::default();
        for _ in 0..count {
            let key = self.i64()?;
            let value = Shared::from(self.bitmap()?);
            insert_unique(&mut map, key, value)?;
        }
        Ok(map)
    }
}

fn insert_unique<K, V>(map: &mut ShardedMap<K, V>, key: K, value: V) -> Result<(), String>
where
    K: Clone + Eq + std::hash::Hash + super::ProjectionKey,
    V: Clone,
{
    if map.insert(key, value).is_some() {
        Err("Projection checkpoint contains a duplicate map key".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use roaring::RoaringBitmap;
    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::checkpoint_path;
    use crate::app::Lifecycle;
    use crate::canonical_bitmap::{
        replace_bitmap, BitmapDomain, LIFECYCLE_ACTIVE_KEY, LIFECYCLE_INBOX_KEY,
        LIFECYCLE_TRASH_KEY, RATING_UNRATED_KEY,
    };
    use crate::projection_v2::ProjectionStore;

    fn fixture() -> (TempDir, Connection, ProjectionStore) {
        let directory = tempfile::tempdir().unwrap();
        let mut connection = Connection::open(directory.path().join("library.sqlite")).unwrap();
        crate::store::schema::create_canonical_v1(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO media_file
                     (file_id, file_hash, mime_type, size_bytes, created_at)
                 VALUES (1, 'checkpoint-file', 'image/png', 10, 'now');
                 INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                 VALUES (1, 'checkpoint-item', 'media', 'now', 'now');
                 INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                 VALUES (1, 1, 'now', 'now');
                 INSERT INTO library_root (item_id, lifecycle) VALUES (1, 'active');
                 INSERT INTO root_metadata (root_item_id, name, updated_at)
                 VALUES (1, 'Checkpoint item', 'now');",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_ACTIVE_KEY,
            1,
            &RoaringBitmap::from_iter([1]),
        )
        .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_INBOX_KEY,
            1,
            &RoaringBitmap::new(),
        )
        .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_TRASH_KEY,
            1,
            &RoaringBitmap::new(),
        )
        .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Rating,
            RATING_UNRATED_KEY,
            1,
            &RoaringBitmap::from_iter([1]),
        )
        .unwrap();
        transaction.commit().unwrap();
        let projection = ProjectionStore::from_connection(&connection).unwrap();
        (directory, connection, projection)
    }

    #[test]
    fn corruption_rebuilds_from_sqlite() {
        let (_directory, mut connection, projection) = fixture();
        projection.write_checkpoint(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = 1",
                [],
            )
            .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_ACTIVE_KEY,
            2,
            &RoaringBitmap::new(),
        )
        .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_TRASH_KEY,
            2,
            &RoaringBitmap::from_iter([1]),
        )
        .unwrap();
        transaction.commit().unwrap();
        fs::write(checkpoint_path(&connection).unwrap().unwrap(), b"corrupt").unwrap();

        let initialized = ProjectionStore::initialize(&connection).unwrap();

        assert_eq!(initialized.lifecycle_bitmap(Lifecycle::Trash).len(), 1);
        assert!(initialized.active_bitmap().is_empty());
    }

    #[test]
    fn revision_mismatch_rebuilds_from_sqlite() {
        let (_directory, mut connection, projection) = fixture();
        projection.write_checkpoint(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute_batch(
                "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = 1;
                 UPDATE library_meta SET revision = revision + 1 WHERE singleton = 1;",
            )
            .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_ACTIVE_KEY,
            2,
            &RoaringBitmap::new(),
        )
        .unwrap();
        replace_bitmap(
            &transaction,
            BitmapDomain::Lifecycle,
            LIFECYCLE_TRASH_KEY,
            2,
            &RoaringBitmap::from_iter([1]),
        )
        .unwrap();
        transaction.commit().unwrap();

        let initialized = ProjectionStore::initialize(&connection).unwrap();

        assert_eq!(initialized.lifecycle_bitmap(Lifecycle::Trash).len(), 1);
        assert!(initialized.active_bitmap().is_empty());
    }

    #[test]
    fn exact_checkpoint_restores_complete_state() {
        let (_directory, connection, projection) = fixture();
        projection.write_checkpoint(&connection).unwrap();
        connection
            .execute(
                "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = 1",
                [],
            )
            .unwrap();

        let initialized = ProjectionStore::initialize(&connection).unwrap();

        assert_eq!(initialized.lifecycle_bitmap(Lifecycle::Active).len(), 1);
        assert!(initialized.trash_bitmap().is_empty());
        assert_eq!(initialized.root_for_media(1), Some(1));
        assert!(initialized
            .selection_snapshot()
            .all_media_are_images(&RoaringBitmap::from_iter([1])));
        assert_eq!(
            initialized.mime_bitmap("image/png"),
            RoaringBitmap::from_iter([1])
        );
        assert_eq!(
            initialized.mime_family_bitmap("image"),
            RoaringBitmap::from_iter([1])
        );
    }
}
