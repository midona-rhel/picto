//! Asset resolution — engine-facing asset contract.

use super::ApplicationEngine;
use ts_rs::TS;

/// Asset role requested by the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "snake_case")]
pub enum AssetRole {
    Thumbnail,
    PreviewImage,
    OriginalMedia,
    VideoStream,
}

/// Typed asset resolution result.
#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct EntityAssetResult {
    #[ts(type = "'thumbnail' | 'preview_image' | 'original_media' | 'video_stream'")]
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
    pub fn resolve_entity_asset(
        &self,
        entity_hash: &str,
        role: AssetRole,
    ) -> Result<EntityAssetResult, String> {
        self.resolve_media_asset(entity_hash, role)
    }
}
