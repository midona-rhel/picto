use crate::constants::MimeType;
use crate::media_processing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailBackend {
    Inline,
    GenericAdapter,
    Ffmpeg,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaCapabilities {
    pub ingest_supported: bool,
    pub thumbnail_backend: Option<ThumbnailBackend>,
    pub can_preview_image: bool,
    pub can_dominant_colors: bool,
    pub can_perceptual_hash: bool,
}

impl MediaCapabilities {
    pub const fn can_thumbnail(self) -> bool {
        self.thumbnail_backend.is_some()
    }

    pub const fn should_inline_thumbnail_on_ingest(self) -> bool {
        matches!(
            self.thumbnail_backend,
            Some(ThumbnailBackend::Inline | ThumbnailBackend::GenericAdapter)
        )
    }

    pub const fn requires_ffmpeg_thumbnail(self) -> bool {
        matches!(self.thumbnail_backend, Some(ThumbnailBackend::Ffmpeg))
    }
}

pub fn capabilities_for_detected_mime(mime: MimeType) -> MediaCapabilities {
    if !media_processing::is_allowed_mime(mime) {
        return MediaCapabilities {
            ingest_supported: false,
            thumbnail_backend: None,
            can_preview_image: false,
            can_dominant_colors: false,
            can_perceptual_hash: false,
        };
    }

    match mime {
        MimeType::ApplicationFlash => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Browser),
            can_preview_image: false,
            can_dominant_colors: false,
            can_perceptual_hash: false,
        },
        MimeType::AnimationGif
        | MimeType::AnimationApng
        | MimeType::AnimationWebp
        | MimeType::AnimationUgoira
        | MimeType::AnimationJxl
        | MimeType::ImageHeifSequence
        | MimeType::ImageHeicSequence
        | MimeType::ImageAvifSequence => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Ffmpeg),
            can_preview_image: true,
            can_dominant_colors: true,
            can_perceptual_hash: false,
        },
        m if is_special_thumbnail_detected_mime(m) => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::GenericAdapter),
            can_preview_image: false,
            can_dominant_colors: !is_document_thumbnail_detected_mime(m),
            can_perceptual_hash: false,
        },
        m if m.is_video() => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Ffmpeg),
            can_preview_image: true,
            can_dominant_colors: true,
            can_perceptual_hash: false,
        },
        m if m.is_audio() => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Ffmpeg),
            can_preview_image: false,
            can_dominant_colors: false,
            can_perceptual_hash: false,
        },
        m if is_static_raster_detected_mime(m) => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Inline),
            can_preview_image: true,
            can_dominant_colors: true,
            can_perceptual_hash: true,
        },
        _ => MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: None,
            can_preview_image: false,
            can_dominant_colors: false,
            can_perceptual_hash: false,
        },
    }
}

pub fn capabilities_for_stored_media(
    mime_type: &str,
    frame_count: Option<i64>,
) -> MediaCapabilities {
    if mime_type == "application/x-ugoira" {
        return MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::GenericAdapter),
            can_preview_image: true,
            can_dominant_colors: true,
            can_perceptual_hash: false,
        };
    }

    if mime_type == "application/x-shockwave-flash" {
        return MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Browser),
            can_preview_image: false,
            can_dominant_colors: false,
            can_perceptual_hash: false,
        };
    }

    if mime_type.starts_with("video/") {
        return MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Ffmpeg),
            can_preview_image: true,
            can_dominant_colors: true,
            can_perceptual_hash: false,
        };
    }

    if mime_type.starts_with("audio/") {
        return MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Ffmpeg),
            can_preview_image: false,
            can_dominant_colors: false,
            can_perceptual_hash: false,
        };
    }

    if is_special_thumbnail_mime(mime_type) {
        return MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::GenericAdapter),
            can_preview_image: false,
            can_dominant_colors: !is_document_thumbnail_mime(mime_type),
            can_perceptual_hash: false,
        };
    }

    if is_static_raster_mime(mime_type) {
        let animated = frame_count.unwrap_or(1) > 1 || matches!(mime_type, "image/apng");
        if animated {
            return MediaCapabilities {
                ingest_supported: true,
                thumbnail_backend: Some(ThumbnailBackend::Ffmpeg),
                can_preview_image: true,
                can_dominant_colors: true,
                can_perceptual_hash: false,
            };
        }
        return MediaCapabilities {
            ingest_supported: true,
            thumbnail_backend: Some(ThumbnailBackend::Inline),
            can_preview_image: true,
            can_dominant_colors: true,
            can_perceptual_hash: true,
        };
    }

    MediaCapabilities {
        ingest_supported: true,
        thumbnail_backend: None,
        can_preview_image: false,
        can_dominant_colors: false,
        can_perceptual_hash: false,
    }
}

fn is_static_raster_mime(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/jpeg"
            | "image/png"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/tiff"
            | "image/x-icon"
            | "image/qoi"
            | "image/heif"
            | "image/heic"
            | "image/avif"
            | "image/jxl"
            | "image/apng"
    )
}

fn is_static_raster_detected_mime(mime: MimeType) -> bool {
    matches!(
        mime,
        MimeType::ImageJpeg
            | MimeType::ImagePng
            | MimeType::ImageGif
            | MimeType::ImageWebp
            | MimeType::ImageTiff
            | MimeType::ImageBmp
            | MimeType::ImageIcon
            | MimeType::ImageQoi
            | MimeType::ImageHeif
            | MimeType::ImageHeic
            | MimeType::ImageAvif
            | MimeType::ImageJxl
    )
}

fn is_special_thumbnail_detected_mime(mime: MimeType) -> bool {
    matches!(
        mime,
        MimeType::ApplicationCbz
            | MimeType::ApplicationClip
            | MimeType::ApplicationEpub
            | MimeType::ApplicationKrita
            | MimeType::ApplicationPaintDotNet
            | MimeType::ApplicationPptx
            | MimeType::ApplicationDocx
            | MimeType::ApplicationProcreate
            | MimeType::ApplicationPsd
            | MimeType::ImageSvg
    )
}

fn is_document_thumbnail_detected_mime(mime: MimeType) -> bool {
    matches!(
        mime,
        MimeType::ApplicationCbz
            | MimeType::ApplicationEpub
            | MimeType::ApplicationPptx
            | MimeType::ApplicationDocx
    )
}

fn is_special_thumbnail_mime(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "application/vnd.comicbook+zip"
            | "application/x-clip-studio-paint"
            | "application/epub+zip"
            | "application/x-krita"
            | "application/x-paint-dot-net"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/x-procreate"
            | "image/svg+xml"
            | "image/vnd.adobe.photoshop"
    )
}

fn is_document_thumbnail_mime(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "application/vnd.comicbook+zip"
            | "application/epub+zip"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    )
}

#[cfg(test)]
mod tests {
    use super::{capabilities_for_detected_mime, capabilities_for_stored_media, ThumbnailBackend};
    use crate::constants::MimeType;

    #[test]
    fn detected_video_uses_its_thumbnail_for_colors() {
        let caps = capabilities_for_detected_mime(MimeType::VideoMp4);
        assert_eq!(caps.thumbnail_backend, Some(ThumbnailBackend::Ffmpeg));
        assert!(caps.can_dominant_colors);
        assert!(!caps.can_perceptual_hash);
    }

    #[test]
    fn stored_ugoira_keeps_its_animation_adapter() {
        let caps = capabilities_for_stored_media("application/x-ugoira", Some(3));
        assert!(caps.ingest_supported);
        assert_eq!(
            caps.thumbnail_backend,
            Some(ThumbnailBackend::GenericAdapter)
        );
        assert!(caps.can_preview_image);
    }

    #[test]
    fn stored_animated_image_uses_its_thumbnail_for_colors() {
        let caps = capabilities_for_stored_media("image/gif", Some(12));
        assert_eq!(caps.thumbnail_backend, Some(ThumbnailBackend::Ffmpeg));
        assert!(caps.can_dominant_colors);
        assert!(!caps.can_perceptual_hash);
    }

    #[test]
    fn audio_uses_ffmpeg_waveform_without_color_analysis() {
        let detected = capabilities_for_detected_mime(MimeType::AudioMp3);
        let stored = capabilities_for_stored_media("audio/mpeg", None);
        assert_eq!(detected.thumbnail_backend, Some(ThumbnailBackend::Ffmpeg));
        assert_eq!(stored.thumbnail_backend, Some(ThumbnailBackend::Ffmpeg));
        assert!(!detected.can_dominant_colors);
        assert!(!stored.can_dominant_colors);
        assert!(!stored.can_perceptual_hash);
    }

    #[test]
    fn documents_keep_thumbnails_without_color_analysis() {
        for mime in [
            MimeType::ApplicationCbz,
            MimeType::ApplicationEpub,
            MimeType::ApplicationPptx,
            MimeType::ApplicationDocx,
        ] {
            let caps = capabilities_for_detected_mime(mime);
            assert!(caps.can_thumbnail());
            assert!(!caps.can_dominant_colors);
        }

        let project = capabilities_for_detected_mime(MimeType::ApplicationPsd);
        assert!(project.can_thumbnail());
        assert!(project.can_dominant_colors);
    }

    #[test]
    fn stored_static_raster_image_keeps_analysis() {
        let caps = capabilities_for_stored_media("image/png", Some(1));
        assert_eq!(caps.thumbnail_backend, Some(ThumbnailBackend::Inline));
        assert!(caps.can_dominant_colors);
        assert!(caps.can_perceptual_hash);
    }

    #[test]
    fn flash_thumbnails_are_owned_by_the_browser_renderer() {
        let detected = capabilities_for_detected_mime(MimeType::ApplicationFlash);
        let stored = capabilities_for_stored_media("application/x-shockwave-flash", Some(30));
        assert_eq!(detected.thumbnail_backend, Some(ThumbnailBackend::Browser));
        assert_eq!(stored.thumbnail_backend, Some(ThumbnailBackend::Browser));
        assert!(!stored.can_dominant_colors);
        assert!(!stored.can_perceptual_hash);
    }
}
