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
        provenance_mask: Option<u64>,
    ) -> Result<TagChange, String> {
        let resolved = target::resolve(&self.db, &target)?;
        let expansion = ExpansionMode::EntityAndDescendants;
        let provenance_mask = provenance_mask.unwrap_or(TAG_PROVENANCE_MANUAL);

        let change = match resolved {
            target::ResolvedTarget::Ids(ids) => match operation {
                TagOperation::Add => self.db.add_tags(&ids, tags, provenance_mask, expansion)?,
                TagOperation::Remove => self.db.remove_tags(&ids, tags, expansion)?,
            },
            target::ResolvedTarget::Query {
                view_query,
                exclusions,
            } => match operation {
                TagOperation::Add => self.db.add_tags_bulk(
                    &view_query,
                    &exclusions,
                    tags,
                    provenance_mask,
                    expansion,
                )?,
                TagOperation::Remove => {
                    self.db
                        .remove_tags_bulk(&view_query, &exclusions, tags, expansion)?
                }
            },
        };
        let mut write = WriteChange::from_tag(&change);
        write.entity_hashes = self.resolve_entity_hashes(&change.entity_ids);
        self.commit_write(&write);
        Ok(change)
    }

    /// Rename a tag. Returns the full structure change, including merge target when relevant.
    pub fn rename_tag(&self, tag_id: i64, new_name: &str) -> Result<TagStructureChange, String> {
        let result = self.db.rename_tag(tag_id, new_name)?;
        self.commit_write(&WriteChange {
            origin: "rename_tag".to_string(),
            entity_hashes: self.resolve_entity_hashes(&result.entity_ids),
            entity_ids: result.entity_ids.clone(),
            dirty_tag_ids: result.dirty_tag_ids.clone(),
            tag_structure_changed: true,
            ..Default::default()
        });
        Ok(result)
    }

    /// Delete a tag and remove it from all entities. Returns affected entity_ids.
    pub fn delete_tag(&self, tag_id: i64) -> Result<Vec<i64>, String> {
        let affected = self.db.delete_tag(tag_id)?;
        self.commit_write(&WriteChange {
            origin: "delete_tag".to_string(),
            entity_hashes: self.resolve_entity_hashes(&affected.entity_ids),
            entity_ids: affected.entity_ids.clone(),
            dirty_tag_ids: affected.dirty_tag_ids.clone(),
            tag_structure_changed: true,
            ..Default::default()
        });
        Ok(affected.entity_ids)
    }

    /// Merge one tag into another. Returns affected entity_ids.
    pub fn merge_tags(&self, from_tag_id: i64, to_tag_id: i64) -> Result<Vec<i64>, String> {
        let affected = self.db.merge_tags(from_tag_id, to_tag_id)?;
        self.commit_write(&WriteChange {
            origin: "merge_tags".to_string(),
            entity_hashes: self.resolve_entity_hashes(&affected.entity_ids),
            entity_ids: affected.entity_ids.clone(),
            dirty_tag_ids: affected.dirty_tag_ids.clone(),
            tag_structure_changed: true,
            ..Default::default()
        });
        Ok(affected.entity_ids)
    }

    pub fn manage_tag_alias(&self, from_tag_id: i64, to_tag_id: Option<i64>) -> Result<(), String> {
        self.db.manage_tag_alias(from_tag_id, to_tag_id)?;
        let mut dirty = vec![from_tag_id];
        if let Some(to_tag_id) = to_tag_id {
            dirty.push(to_tag_id);
        }
        self.commit_write(&WriteChange {
            origin: "manage_tag_alias".to_string(),
            dirty_tag_ids: dirty,
            tag_structure_changed: true,
            ..Default::default()
        });
        Ok(())
    }

    pub fn manage_tag_implication(
        &self,
        child_tag_id: i64,
        parent_tag_id: i64,
        add: bool,
    ) -> Result<(), String> {
        self.db
            .manage_tag_implication(child_tag_id, parent_tag_id, add)?;
        self.commit_write(&WriteChange {
            origin: "manage_tag_implication".to_string(),
            dirty_tag_ids: vec![child_tag_id, parent_tag_id],
            tag_structure_changed: true,
            ..Default::default()
        });
        Ok(())
    }

    pub fn set_tag_site_mask(&self, tag_id: i64, site_mask: u64) -> Result<(), String> {
        self.db.set_tag_site_mask(tag_id, site_mask)?;
        self.commit_write(&WriteChange {
            origin: "set_tag_site_mask".to_string(),
            dirty_tag_ids: vec![tag_id],
            tags_changed: true,
            ..Default::default()
        });
        Ok(())
    }

    pub fn ensure_tag(&self, tag_str: &str) -> Result<i64, String> {
        self.db.ensure_tag(tag_str)
    }

    pub fn find_tag_id(&self, tag_str: &str) -> Result<Option<i64>, String> {
        self.db.find_tag_id(tag_str)
    }

    pub fn get_tag_string(&self, tag_id: i64) -> Result<Option<String>, String> {
        self.db.get_tag_string(tag_id)
    }

    pub fn search_tags(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TagRecord>, String> {
        self.db.search_tags(query, limit, offset)
    }

    pub fn get_tag_relations(
        &self,
        tag_id: i64,
        relation_type: &str,
    ) -> Result<Vec<TagRelation>, String> {
        match relation_type {
            "aliases" => self.db.get_aliases_for_tag(tag_id),
            "implications" => self.db.get_implications_for_tag(tag_id),
            other => Err(format!("Invalid relation_type: {other}")),
        }
    }

    pub fn get_tags_paginated(
        &self,
        namespace: Option<String>,
        search: Option<String>,
        cursor: Option<String>,
        limit: i64,
    ) -> Result<Vec<TagRecord>, String> {
        self.db.get_tags_paginated(namespace, search, cursor, limit)
    }

    pub fn get_namespace_summary(&self) -> Result<Vec<NamespaceSummary>, String> {
        self.db.get_namespace_summary()
    }

    fn resolve_entity_hashes(&self, entity_ids: &[i64]) -> Vec<String> {
        self.db
            .resolve_entity_ids_to_hashes(entity_ids)
            .unwrap_or_default()
    }
}
