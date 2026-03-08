//! Selection controller — thin entry point delegating to focused modules.

use std::collections::HashMap;

use crate::sqlite::SqliteDatabase;
use crate::types::{SelectionQuerySpec, SelectionSummary};

pub struct SelectionController;

impl SelectionController {
    pub async fn add_tags_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        tag_strings: Vec<String>,
    ) -> Result<usize, String> {
        super::mutations::add_tags_selection(db, selection, tag_strings).await
    }

    pub async fn remove_tags_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        tag_strings: Vec<String>,
    ) -> Result<usize, String> {
        super::mutations::remove_tags_selection(db, selection, tag_strings).await
    }

    pub async fn update_rating_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        rating: Option<i64>,
    ) -> Result<usize, String> {
        super::mutations::update_rating_selection(db, selection, rating).await
    }

    pub async fn set_notes_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        notes: HashMap<String, String>,
    ) -> Result<usize, String> {
        super::mutations::set_notes_selection(db, selection, notes).await
    }

    pub async fn set_source_urls_selection(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
        urls: Vec<String>,
    ) -> Result<usize, String> {
        super::mutations::set_source_urls_selection(db, selection, urls).await
    }

    pub async fn get_selection_summary(
        db: &SqliteDatabase,
        selection: SelectionQuerySpec,
    ) -> Result<SelectionSummary, String> {
        super::summary::get_selection_summary(db, selection).await
    }
}
