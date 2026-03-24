//! Tag operations — apply, rename, delete, merge.

use crate::db::types::*;

use super::{target, ApplicationEngine, WriteChange};

/// Tag operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagOperation {
    Add,
    Remove,
}

impl ApplicationEngine {
    /// Add or remove tags from entities.
    /// Collections expand to EntityAndDescendants.
    pub fn apply_entity_tags(
        &self,
        target: EntityTarget,
        operation: TagOperation,
        tags: &[String],
    ) -> Result<TagChange, String> {
        let resolved = target::resolve(&self.db, &target)?;
        let expansion = ExpansionMode::EntityAndDescendants;

        let change = match resolved {
            target::ResolvedTarget::Ids(ids) => match operation {
                TagOperation::Add => self.db.add_tags(&ids, tags, expansion)?,
                TagOperation::Remove => self.db.remove_tags(&ids, tags, expansion)?,
            },
            target::ResolvedTarget::Query { view_query, exclusions } => match operation {
                TagOperation::Add => {
                    self.db.add_tags_bulk(&view_query, &exclusions, tags, expansion)?
                }
                TagOperation::Remove => {
                    self.db.remove_tags_bulk(&view_query, &exclusions, tags, expansion)?
                }
            },
        };
        self.commit_write(&WriteChange::from_tag(&change));
        Ok(change)
    }

    /// Rename a tag. Returns the surviving tag_id (may differ if merged into existing).
    pub fn rename_tag(&self, tag_id: i64, new_name: &str) -> Result<Option<i64>, String> {
        let result = self.db.rename_tag(tag_id, new_name)?;
        self.commit_write(&WriteChange {
            dirty_tag_ids: vec![tag_id],
            tags_changed: true,
            ..Default::default()
        });
        Ok(result)
    }

    /// Delete a tag and remove it from all entities. Returns affected entity_ids.
    pub fn delete_tag(&self, tag_id: i64) -> Result<Vec<i64>, String> {
        let affected = self.db.delete_tag(tag_id)?;
        self.commit_write(&WriteChange {
            entity_ids: affected.clone(),
            dirty_tag_ids: vec![tag_id],
            tags_changed: true,
            ..Default::default()
        });
        Ok(affected)
    }

    /// Merge one tag into another. Returns affected entity_ids.
    pub fn merge_tags(&self, from_tag_id: i64, to_tag_id: i64) -> Result<Vec<i64>, String> {
        let affected = self.db.merge_tags(from_tag_id, to_tag_id)?;
        self.commit_write(&WriteChange {
            entity_ids: affected.clone(),
            dirty_tag_ids: vec![from_tag_id, to_tag_id],
            tags_changed: true,
            ..Default::default()
        });
        Ok(affected)
    }
}
