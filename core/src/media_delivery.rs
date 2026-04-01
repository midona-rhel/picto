//! Media delivery service — resolves stable asset URLs for entities.
//!
//! Filesystem paths remain backend internals. The public contract is entity +
//! asset role -> typed availability + `media://` URL.

use crate::blob_store::mime_to_extension;
use crate::db::types::EntityKind;
use crate::engine::assets::{AssetRole, EntityAssetResult};
use crate::engine::ApplicationEngine;
use crate::media_capabilities::capabilities_for_stored_media;

fn role_name(role: AssetRole) -> &'static str {
    match role {
        AssetRole::Thumbnail => "thumbnail",
        AssetRole::PreviewImage => "preview_image",
        AssetRole::OriginalMedia => "original_media",
        AssetRole::VideoStream => "video_stream",
    }
}

fn thumbnail_url(hash: &str) -> String {
    format!("media://localhost/thumb/{hash}.jpg")
}

fn file_url(hash: &str, mime_type: &str) -> String {
    let ext = mime_to_extension(mime_type);
    format!("media://localhost/file/{hash}.{ext}")
}

fn can_stream_video(mime_type: &str) -> bool {
    mime_type.starts_with("video/")
}

impl ApplicationEngine {
    pub fn resolve_media_asset(
        &self,
        entity_hash: &str,
        role: AssetRole,
    ) -> Result<EntityAssetResult, String> {
        let requested_role = role_name(role).to_string();
        let details = self.db().get_entity_details(entity_hash)?;
        let Some(details) = details else {
            return Ok(EntityAssetResult {
                role: requested_role,
                available: false,
                url: None,
                mime_type: None,
                source_entity_hash: None,
            });
        };

        let source_entity_hash = if details.entity_kind == EntityKind::Collection {
            self.db().get_primary_member_hash(entity_hash)?
        } else {
            Some(entity_hash.to_string())
        };

        let Some(source_entity_hash) = source_entity_hash else {
            return Ok(EntityAssetResult {
                role: requested_role,
                available: false,
                url: None,
                mime_type: None,
                source_entity_hash: None,
            });
        };

        let mime_type = details.mime_type.clone();
        let caps = capabilities_for_stored_media(&mime_type, details.frame_count);
        let url = match role {
            AssetRole::Thumbnail if caps.can_thumbnail() => {
                Some(thumbnail_url(&source_entity_hash))
            }
            AssetRole::PreviewImage if caps.can_preview_image => {
                if mime_type.starts_with("video/") {
                    Some(thumbnail_url(&source_entity_hash))
                } else {
                    Some(file_url(&source_entity_hash, &mime_type))
                }
            }
            AssetRole::OriginalMedia => Some(file_url(&source_entity_hash, &mime_type)),
            AssetRole::VideoStream if can_stream_video(&mime_type) => {
                Some(file_url(&source_entity_hash, &mime_type))
            }
            _ => None,
        };

        Ok(EntityAssetResult {
            role: requested_role,
            available: url.is_some(),
            url,
            mime_type: Some(mime_type),
            source_entity_hash: Some(source_entity_hash),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{can_stream_video, file_url, thumbnail_url};
    use crate::media_capabilities::capabilities_for_stored_media;

    #[test]
    fn image_and_video_roles_use_expected_urls() {
        let hash = "a".repeat(64);
        assert_eq!(
            thumbnail_url(&hash),
            format!("media://localhost/thumb/{hash}.jpg")
        );
        assert_eq!(
            file_url(&hash, "image/png"),
            format!("media://localhost/file/{hash}.png")
        );
        assert_eq!(
            file_url(&hash, "video/mp4"),
            format!("media://localhost/file/{hash}.mp4")
        );
    }

    #[test]
    fn role_capabilities_follow_mime_rules() {
        assert!(capabilities_for_stored_media("image/jpeg", Some(1)).can_thumbnail());
        assert!(capabilities_for_stored_media("video/mp4", None).can_thumbnail());
        assert!(!capabilities_for_stored_media("audio/flac", None).can_thumbnail());

        assert!(capabilities_for_stored_media("image/jpeg", Some(1)).can_preview_image);
        assert!(capabilities_for_stored_media("video/mp4", None).can_preview_image);
        assert!(!capabilities_for_stored_media("application/pdf", None).can_preview_image);

        assert!(can_stream_video("video/mp4"));
        assert!(!can_stream_video("image/jpeg"));
    }
}
