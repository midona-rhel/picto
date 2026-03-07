//! Tag orchestration — search, add/remove, autocomplete, and tag info
//! retrieval for dispatch handlers.
//!
//! Delegates to `sqlite::tags` for DB operations and `tags.rs` for
//! normalization.

use crate::sqlite::tags::FileTagInfo;
use crate::sqlite::SqliteDatabase;
use crate::tags;
use crate::types::{FileInfoSlim, TagInfo};

fn tag_display_key(namespace: &str, subtag: &str) -> String {
    if namespace.is_empty() {
        subtag.to_string()
    } else {
        format!("{}:{}", namespace, subtag)
    }
}

fn file_tag_to_tag_info(t: &FileTagInfo) -> TagInfo {
    TagInfo {
        tag_id: t.tag_id,
        display: tag_display_key(&t.namespace, &t.subtag),
        namespace: t.namespace.clone(),
        subtag: t.subtag.clone(),
        file_count: 0,
        read_only: t.source != "local",
    }
}

pub struct TagController;

impl TagController {
    pub async fn search_tags(
        db: &SqliteDatabase,
        query: String,
        limit: Option<usize>,
    ) -> Result<Vec<TagInfo>, String> {
        let limit = limit.unwrap_or(20) as i64;
        let results = db.search_tags(&query, limit).await?;
        Ok(results
            .into_iter()
            .map(|t| TagInfo {
                tag_id: t.tag_id,
                display: tag_display_key(&t.namespace, &t.subtag),
                namespace: t.namespace,
                subtag: t.subtag,
                file_count: t.file_count,
                read_only: false,
            })
            .collect())
    }

    // PBI-038: Paged tag search with counts for picker.
    pub async fn search_tags_paged(
        db: &SqliteDatabase,
        query: String,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<(String, String, i64)>, String> {
        let limit = limit.unwrap_or(200) as i64;
        let offset = offset.unwrap_or(0) as i64;
        let results = db.search_tags_paged(&query, limit, offset).await?;
        Ok(results
            .into_iter()
            .map(|t| {
                (
                    tag_display_key(&t.namespace, &t.subtag),
                    t.namespace,
                    t.file_count,
                )
            })
            .collect())
    }

    pub async fn get_all_tags_with_counts(
        db: &SqliteDatabase,
    ) -> Result<Vec<(String, String, i64)>, String> {
        let tags = db.get_all_tags_with_counts().await?;
        Ok(tags
            .into_iter()
            .map(|t| {
                (
                    tag_display_key(&t.namespace, &t.subtag),
                    t.namespace,
                    t.file_count,
                )
            })
            .collect())
    }

    pub async fn get_entity_tags(
        db: &SqliteDatabase,
        hash: String,
    ) -> Result<Vec<TagInfo>, String> {
        let results = db.get_entity_tags(&hash).await?;
        Ok(results.iter().map(file_tag_to_tag_info).collect())
    }

    pub async fn find_files_by_tags(
        db: &SqliteDatabase,
        tag_strings: Vec<String>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<FileInfoSlim>, String> {
        if tag_strings.is_empty() {
            return Ok(Vec::new());
        }

        let limit = limit.unwrap_or(500);
        let offset = offset.unwrap_or(0);

        let all_hashes = db.find_files_by_tags(&tag_strings, true).await?;
        let page_hashes: Vec<String> = all_hashes.into_iter().skip(offset).take(limit).collect();

        if page_hashes.is_empty() {
            return Ok(Vec::new());
        }

        let files = db.batch_get_metadata_slim(page_hashes).await?;
        Ok(files.into_iter().map(FileInfoSlim::from).collect())
    }

    pub async fn add_tags(
        db: &SqliteDatabase,
        hash: String,
        tag_strings: Vec<String>,
    ) -> Result<Vec<String>, String> {
        let mut applied = Vec::new();
        for (ns, st) in tags::parse_tags(&tag_strings) {
            db.tag_entity(&hash, &ns, &st, "local").await?;
            applied.push(tags::combine_tag(&ns, &st));
        }
        Ok(applied)
    }

    pub async fn remove_tags(
        db: &SqliteDatabase,
        hash: String,
        tag_strings: Vec<String>,
    ) -> Result<(), String> {
        for (ns, st) in tags::parse_tags(&tag_strings) {
            db.untag_entity(&hash, &ns, &st).await?;
        }
        Ok(())
    }

    pub async fn add_tags_batch(
        db: &SqliteDatabase,
        hashes: Vec<String>,
        tag_strings: Vec<String>,
    ) -> Result<(), String> {
        if tag_strings.is_empty() || hashes.is_empty() {
            return Ok(());
        }
        db.add_tags_batch(&hashes, &tag_strings).await?;
        Ok(())
    }

    pub async fn remove_tags_batch(
        db: &SqliteDatabase,
        hashes: Vec<String>,
        tag_strings: Vec<String>,
    ) -> Result<(), String> {
        if tag_strings.is_empty() || hashes.is_empty() {
            return Ok(());
        }
        db.remove_tags_batch(&hashes, &tag_strings).await?;
        Ok(())
    }
}
