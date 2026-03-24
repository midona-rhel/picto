//! Asset resolution — resolve_entity_asset.
//!
//! The engine resolves WHICH entity+file to ask about.
//! The media delivery service resolves HOW to serve it.

use serde::Serialize;

use super::ApplicationEngine;

/// Asset role requested by the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetRole {
    Thumbnail,
    PreviewImage,
    OriginalMedia,
}

/// Typed asset resolution result.
#[derive(Debug, Clone, Serialize)]
pub struct EntityAssetResult {
    pub role: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_entity_hash: Option<String>,
}

impl ApplicationEngine {
    /// Resolve an asset for an entity by role.
    /// For collections, resolves the primary member first.
    /// TODO: delegate URL construction to the media delivery service
    pub fn resolve_entity_asset(
        &self,
        entity_hash: &str,
        role: AssetRole,
    ) -> Result<EntityAssetResult, String> {
        let role_str = match role {
            AssetRole::Thumbnail => "thumbnail",
            AssetRole::PreviewImage => "preview_image",
            AssetRole::OriginalMedia => "original_media",
        };

        let details = self.db.get_entity_details(entity_hash)?;
        let Some(details) = details else {
            return Ok(EntityAssetResult {
                role: role_str.to_string(),
                available: false,
                url: None,
                mime_type: None,
                source_entity_hash: None,
            });
        };

        // For collections, the asset comes from the primary member.
        let source_hash = if details.entity_kind == crate::db::types::EntityKind::Collection {
            self.db
                .get_primary_member_hash(entity_hash)?
                .unwrap_or_else(|| entity_hash.to_string())
        } else {
            entity_hash.to_string()
        };

        Ok(EntityAssetResult {
            role: role_str.to_string(),
            available: true,
            url: None, // TODO: delegate to media delivery service for URL construction
            mime_type: Some(details.mime_type.clone()),
            source_entity_hash: Some(source_hash),
        })
    }
}
