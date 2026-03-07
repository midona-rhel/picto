//! Single-transaction file import with tags.

use rusqlite::Connection;

use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::compilers::CompilerEvent;
use crate::sqlite::files::{self, NewFile};
use crate::tags::db as tags_db;
use crate::sqlite::SqliteDatabase;

/// Options for importing a file.
pub struct ImportOptions {
    pub hash: String,
    pub name: Option<String>,
    pub size: i64,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub blurhash: Option<String>,
    pub status: i64,
    pub notes: Option<String>,
    pub source_urls: Option<Vec<String>>,
    pub dominant_color_hex: Option<String>,
    pub dominant_palette_blob: Option<Vec<u8>>,
    pub tags: Vec<(String, String)>, // (namespace, subtag)
    pub tag_source: String,
    pub colors: Vec<(String, f32, f32, f32)>, // (hex, l, a, b)
}

/// Result of a single-transaction import.
pub struct ImportResult {
    pub file_id: i64,
    pub tag_ids: Vec<i64>,
    pub was_duplicate: bool,
}

pub fn import_file_with_tags(
    conn: &mut Connection,
    opts: &ImportOptions,
) -> rusqlite::Result<ImportResult> {
    if files::file_exists(conn, &opts.hash)? {
        let file_id = conn.query_row(
            "SELECT file_id FROM file WHERE hash = ?1",
            [&opts.hash],
            |row| row.get::<_, i64>(0),
        )?;
        return Ok(ImportResult {
            file_id,
            tag_ids: Vec::new(),
            was_duplicate: true,
        });
    }

    let tx = conn.transaction()?;

    let now = chrono::Utc::now().to_rfc3339();
    let urls_json = opts
        .source_urls
        .as_ref()
        .map(|urls| serde_json::to_string(urls).unwrap_or_default());

    let new_file = NewFile {
        hash: opts.hash.clone(),
        name: opts.name.clone(),
        size: opts.size,
        mime: opts.mime.clone(),
        width: opts.width,
        height: opts.height,
        duration_ms: opts.duration_ms,
        num_frames: opts.num_frames,
        has_audio: opts.has_audio,
        blurhash: opts.blurhash.clone(),
        status: opts.status,
        imported_at: now,
        notes: opts.notes.clone(),
        source_urls_json: urls_json,
        dominant_color_hex: opts.dominant_color_hex.clone(),
        dominant_palette_blob: opts.dominant_palette_blob.clone(),
    };

    let file_id = files::insert_file(&tx, &new_file)?;

    let mut tag_ids = Vec::new();
    for (ns, st) in &opts.tags {
        let tag_id = tags_db::get_or_create_tag(&tx, ns, st)?;
        tags_db::tag_entity(&tx, file_id, tag_id, &opts.tag_source)?;
        tag_ids.push(tag_id);
    }

    if !opts.colors.is_empty() {
        files::save_file_colors(&tx, file_id, &opts.colors)?;
    }

    tx.commit()?;

    Ok(ImportResult {
        file_id,
        tag_ids,
        was_duplicate: false,
    })
}

impl SqliteDatabase {
    pub async fn import_file(&self, opts: ImportOptions) -> Result<ImportResult, String> {
        let bitmaps = self.bitmaps.clone();
        let hash_index = self.hash_index.clone();
        let hash = opts.hash.clone();
        let status = opts.status;

        let result = self
            .with_conn_mut(move |conn| import_file_with_tags(conn, &opts))
            .await?;

        if !result.was_duplicate {
            hash_index.insert(hash, result.file_id);
            bitmaps.insert(&BitmapKey::Status(status), result.file_id as u32);

            for &tag_id in &result.tag_ids {
                bitmaps.insert(&BitmapKey::Tag(tag_id), result.file_id as u32);
            }

            self.emit_compiler_event(CompilerEvent::FileInserted {
                file_id: result.file_id,
            });
        }

        Ok(result)
    }
}
