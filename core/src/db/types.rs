//! Shared types for the database boundary.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Single,
    Collection,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Single => "single",
            EntityKind::Collection => "collection",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "single" => Ok(EntityKind::Single),
            "collection" => Ok(EntityKind::Collection),
            other => Err(format!("Invalid entity_kind: {other}")),
        }
    }
}

/// How a command expands entity targets to include collection members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionMode {
    EntityOnly,
    DescendantsOnly,
    EntityAndDescendants,
}

#[derive(Debug, Default)]
pub struct EntityChange {
    pub entity_ids: Vec<i64>,
    pub entity_hashes: Vec<String>,
}

#[derive(Debug, Default)]
pub struct StatusChange {
    pub entity_ids: Vec<i64>,
    pub entity_hashes: Vec<String>,
    pub new_status: i64,
}

#[derive(Debug, Default)]
pub struct TagChange {
    pub entity_ids: Vec<i64>,
    pub tag_ids: Vec<i64>,
    pub tags_added: Vec<String>,
    pub tags_removed: Vec<String>,
}

#[derive(Debug, Default)]
pub struct FolderMembershipChange {
    pub folder_id: i64,
    pub entity_ids: Vec<i64>,
}

#[derive(Debug, Default)]
pub struct CollectionMembershipChange {
    pub collection_id: i64,
    pub added: Vec<i64>,
    pub removed: Vec<i64>,
}
