//! Tag operations — split from db/mod.rs, same `impl LibraryDatabase`.

use super::*;

impl LibraryDatabase {
    // ── Tag operations ───────────────────────────────────────────

    pub fn add_tags(
        &self,
        entity_ids: &[i64],
        tag_strings: &[String],
        provenance_mask: u64,
        expansion: ExpansionMode,
    ) -> Result<TagChange, String> {
        self.with_write(|conn| {
            let change =
                write::tags::add_tags(conn, entity_ids, tag_strings, provenance_mask, expansion)?;
            if !change.tags_added.is_empty() {
                let hashes = entity_hashes_for_ids(conn, &change.entity_ids)?;
                emit_per_entity(
                    conn,
                    &self.device_id,
                    "entity_tags_added",
                    &hashes,
                    &serde_json::json!({ "tags": change.tags_added, "provenance": provenance_mask.to_string() }),
                )?;
            }
            Ok(change)
        })
    }

    pub fn remove_tags(
        &self,
        entity_ids: &[i64],
        tag_strings: &[String],
        expansion: ExpansionMode,
    ) -> Result<TagChange, String> {
        self.with_write(|conn| {
            let change = write::tags::remove_tags(conn, entity_ids, tag_strings, expansion)?;
            if !change.tags_removed.is_empty() {
                let hashes = entity_hashes_for_ids(conn, &change.entity_ids)?;
                emit_per_entity(
                    conn,
                    &self.device_id,
                    "entity_tags_removed",
                    &hashes,
                    &serde_json::json!({ "tags": change.tags_removed }),
                )?;
            }
            Ok(change)
        })
    }

    pub fn rename_tag(&self, tag_id: i64, new_name: &str) -> Result<TagStructureChange, String> {
        self.with_write(|conn| {
            let old_key = tag_op_key(conn, tag_id)?;
            let change = write::tags::rename_tag(conn, tag_id, new_name)?;
            if let Some(old_key) = old_key {
                match change.merged_into_tag_id {
                    Some(target_id) => {
                        let into = tag_op_key(conn, target_id)?;
                        crate::oplog::record_op(
                            conn,
                            &self.device_id,
                            "tag_merged",
                            &old_key,
                            &serde_json::json!({ "into": into }),
                        )?;
                    }
                    None => {
                        crate::oplog::record_op(
                            conn,
                            &self.device_id,
                            "tag_renamed",
                            &old_key,
                            &serde_json::json!({ "to": new_name }),
                        )?;
                    }
                }
            }
            Ok(change)
        })
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<TagStructureChange, String> {
        self.with_write(|conn| {
            let key = tag_op_key(conn, tag_id)?;
            let change = write::tags::delete_tag(conn, tag_id)?;
            if let Some(key) = key {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "tag_deleted",
                    &key,
                    &serde_json::json!({}),
                )?;
            }
            Ok(change)
        })
    }

    pub fn merge_tags(
        &self,
        from_tag_id: i64,
        to_tag_id: i64,
    ) -> Result<TagStructureChange, String> {
        self.with_write(|conn| {
            let from_key = tag_op_key(conn, from_tag_id)?;
            let into = tag_op_key(conn, to_tag_id)?;
            let change = write::tags::merge_tags(conn, from_tag_id, to_tag_id)?;
            if let Some(from_key) = from_key {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "tag_merged",
                    &from_key,
                    &serde_json::json!({ "into": into }),
                )?;
            }
            Ok(change)
        })
    }

    pub fn manage_tag_alias(&self, from_tag_id: i64, to_tag_id: Option<i64>) -> Result<(), String> {
        self.with_write(|conn| {
            let from_key = tag_op_key(conn, from_tag_id)?;
            let to_key = match to_tag_id {
                Some(id) => tag_op_key(conn, id)?,
                None => None,
            };
            write::tags::manage_alias(conn, from_tag_id, to_tag_id)?;
            if let Some(from_key) = from_key {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "tag_alias_set",
                    &from_key,
                    &serde_json::json!({ "to": to_key }),
                )?;
            }
            Ok(())
        })
    }

    pub fn manage_tag_implication(
        &self,
        child_tag_id: i64,
        parent_tag_id: i64,
        add: bool,
    ) -> Result<(), String> {
        self.with_write(|conn| {
            let child_key = tag_op_key(conn, child_tag_id)?;
            let parent_key = tag_op_key(conn, parent_tag_id)?;
            write::tags::manage_implication(conn, child_tag_id, parent_tag_id, add)?;
            if let Some(child_key) = child_key {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "tag_implication_set",
                    &child_key,
                    &serde_json::json!({ "parent": parent_key, "add": add }),
                )?;
            }
            Ok(())
        })
    }

    pub fn set_tag_site_mask(&self, tag_id: i64, site_mask: u64) -> Result<(), String> {
        self.with_write(|conn| {
            write::tags::set_tag_site_mask(conn, tag_id, site_mask)?;
            if let Some(key) = tag_op_key(conn, tag_id)? {
                // Masks cross boundaries as decimal strings (PBI-598 rule).
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "tag_site_mask_set",
                    &key,
                    &serde_json::json!({ "site_mask": site_mask.to_string() }),
                )?;
            }
            Ok(())
        })
    }

    pub fn ensure_tag(&self, tag_str: &str) -> Result<i64, String> {
        self.with_write(|conn| write::tags::ensure_tag(conn, tag_str))
    }
}
