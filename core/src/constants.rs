//! Simplified constants for Picto media library.
//!
//! Keeps: MimeType enum, MIME string mappings, file type groupings.
//! Drops: all Hydrus protocol constants (ContentStatus, ContentType, ServiceType, etc.)

use std::collections::HashSet;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// MimeType enum — all file types we handle
// ---------------------------------------------------------------------------

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MimeType {
    // Images
    ImageJpeg = 1,
    ImagePng = 2,
    ImageBmp = 4,
    ImageIcon = 7,
    ImageWebp = 33,
    ImageTiff = 34,
    ImageSvg = 56,
    ImageHeif = 61,
    ImageHeifSequence = 62,
    ImageHeic = 63,
    ImageHeicSequence = 64,
    ImageAvif = 65,
    ImageAvifSequence = 66,
    ImageGif = 68,
    ImageQoi = 70,
    ImageJxl = 85,

    // Animations
    AnimationGif = 3,
    AnimationApng = 23,
    AnimationUgoira = 74,
    AnimationWebp = 83,
    AnimationJxl = 88,

    // Video
    VideoFlv = 9,
    VideoMp4 = 14,
    VideoMkv = 20,
    VideoWebm = 21,
    VideoMpeg = 25,
    VideoMov = 26,
    VideoAvi = 27,
    VideoWmv = 18,
    VideoOgv = 47,
    VideoRealmedia = 37,

    // Audio
    AudioMp3 = 13,
    AudioOgg = 15,
    AudioFlac = 16,
    AudioWma = 17,
    AudioM4a = 36,
    AudioRealmedia = 38,
    AudioTrueaudio = 39,
    AudioWave = 46,
    AudioMkv = 48,
    AudioMp4 = 49,
    AudioWavpack = 53,

    // Documents
    ApplicationPdf = 10,
    ApplicationEpub = 71,
    ApplicationDjvu = 72,
    ApplicationCbz = 73,
    ApplicationRtf = 75,
    ApplicationDocx = 76,
    ApplicationXlsx = 77,
    ApplicationPptx = 78,
    ApplicationDoc = 80,
    ApplicationXls = 81,
    ApplicationPpt = 82,
    TextPlain = 30,
    TextHtml = 8,

    // Project files
    ApplicationPsd = 35,
    ApplicationClip = 45,
    ApplicationSai2 = 54,
    ApplicationKrita = 55,
    ApplicationXcf = 57,
    ApplicationProcreate = 69,
    ApplicationPaintDotNet = 86,

    // Archives
    ApplicationZip = 11,
    ApplicationRar = 31,
    Application7z = 32,
    ApplicationGzip = 58,

    // Other
    ApplicationJson = 22,
    ApplicationYaml = 6,
    ApplicationFlash = 5,
    ApplicationWindowsExe = 52,
    ApplicationCbor = 51,

    // Undetermined (resolved during import)
    UndeterminedPng = 24,
    UndeterminedGif = 67,
    UndeterminedWm = 19,
    UndeterminedMp4 = 50,
    UndeterminedOle = 79,
    UndeterminedWebp = 84,
    UndeterminedJxl = 87,

    // Generic groupings
    GeneralImage = 41,
    GeneralAnimation = 44,
    GeneralVideo = 42,
    GeneralAudio = 40,
    GeneralApplication = 43,
    GeneralApplicationArchive = 59,
    GeneralImageProject = 60,

    // Fallback
    ApplicationOctetStream = 100,
    ApplicationUnknown = 101,
}

impl MimeType {
    /// Standard MIME type string (e.g. "image/jpeg").
    pub fn mime_string(&self) -> &'static str {
        match self {
            Self::ImageJpeg => "image/jpeg",
            Self::ImagePng | Self::UndeterminedPng => "image/png",
            Self::ImageBmp => "image/bmp",
            Self::ImageIcon => "image/x-icon",
            Self::ImageWebp | Self::UndeterminedWebp => "image/webp",
            Self::ImageTiff => "image/tiff",
            Self::ImageSvg => "image/svg+xml",
            Self::ImageHeif | Self::ImageHeifSequence => "image/heif",
            Self::ImageHeic | Self::ImageHeicSequence => "image/heic",
            Self::ImageAvif | Self::ImageAvifSequence => "image/avif",
            Self::ImageGif | Self::UndeterminedGif => "image/gif",
            Self::ImageQoi => "image/qoi",
            Self::ImageJxl | Self::UndeterminedJxl => "image/jxl",

            Self::AnimationGif => "image/gif",
            Self::AnimationApng => "image/apng",
            Self::AnimationUgoira => "application/x-ugoira",
            Self::AnimationWebp => "image/webp",
            Self::AnimationJxl => "image/jxl",

            Self::VideoFlv => "video/x-flv",
            Self::VideoMp4 | Self::UndeterminedMp4 => "video/mp4",
            Self::VideoMkv => "video/x-matroska",
            Self::VideoWebm => "video/webm",
            Self::VideoMpeg => "video/mpeg",
            Self::VideoMov => "video/quicktime",
            Self::VideoAvi => "video/x-msvideo",
            Self::VideoWmv | Self::UndeterminedWm => "video/x-ms-wmv",
            Self::VideoOgv => "video/ogg",
            Self::VideoRealmedia => "video/vnd.rn-realvideo",

            Self::AudioMp3 => "audio/mpeg",
            Self::AudioOgg => "audio/ogg",
            Self::AudioFlac => "audio/flac",
            Self::AudioWma => "audio/x-ms-wma",
            Self::AudioM4a => "audio/mp4",
            Self::AudioRealmedia => "audio/vnd.rn-realaudio",
            Self::AudioTrueaudio => "audio/x-tta",
            Self::AudioWave => "audio/wav",
            Self::AudioMkv => "audio/x-matroska",
            Self::AudioMp4 => "audio/mp4",
            Self::AudioWavpack => "audio/wavpack",

            Self::ApplicationPdf => "application/pdf",
            Self::ApplicationEpub => "application/epub+zip",
            Self::ApplicationDjvu => "image/vnd.djvu",
            Self::ApplicationCbz => "application/vnd.comicbook+zip",
            Self::ApplicationRtf => "application/rtf",
            Self::ApplicationDocx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            Self::ApplicationXlsx => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            Self::ApplicationPptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::ApplicationDoc => "application/msword",
            Self::ApplicationXls => "application/vnd.ms-excel",
            Self::ApplicationPpt => "application/vnd.ms-powerpoint",
            Self::TextPlain => "text/plain",
            Self::TextHtml => "text/html",

            Self::ApplicationPsd => "image/vnd.adobe.photoshop",
            Self::ApplicationClip => "application/x-clip-studio-paint",
            Self::ApplicationSai2 => "application/x-sai2",
            Self::ApplicationKrita => "application/x-krita",
            Self::ApplicationXcf => "image/x-xcf",
            Self::ApplicationProcreate => "application/x-procreate",
            Self::ApplicationPaintDotNet => "application/x-paint-dot-net",

            Self::ApplicationZip => "application/zip",
            Self::ApplicationRar => "application/vnd.rar",
            Self::Application7z => "application/x-7z-compressed",
            Self::ApplicationGzip => "application/gzip",

            Self::ApplicationJson => "application/json",
            Self::ApplicationYaml => "application/x-yaml",
            Self::ApplicationFlash => "application/x-shockwave-flash",
            Self::ApplicationWindowsExe => "application/x-msdownload",
            Self::ApplicationCbor => "application/cbor",

            Self::UndeterminedOle => "application/x-ole-storage",

            Self::GeneralImage => "image/*",
            Self::GeneralAnimation => "image/*",
            Self::GeneralVideo => "video/*",
            Self::GeneralAudio => "audio/*",
            Self::GeneralApplication => "application/octet-stream",
            Self::GeneralApplicationArchive => "application/octet-stream",
            Self::GeneralImageProject => "application/octet-stream",

            Self::ApplicationOctetStream | Self::ApplicationUnknown => "application/octet-stream",
        }
    }

    /// File extension (without dot) for this MIME type.
    #[allow(dead_code)]
    pub fn extension(&self) -> &'static str {
        match self {
            Self::ImageJpeg => "jpg",
            Self::ImagePng | Self::UndeterminedPng => "png",
            Self::ImageBmp => "bmp",
            Self::ImageIcon => "ico",
            Self::ImageWebp | Self::UndeterminedWebp | Self::AnimationWebp => "webp",
            Self::ImageTiff => "tiff",
            Self::ImageSvg => "svg",
            Self::ImageHeif | Self::ImageHeifSequence => "heif",
            Self::ImageHeic | Self::ImageHeicSequence => "heic",
            Self::ImageAvif | Self::ImageAvifSequence => "avif",
            Self::ImageGif | Self::UndeterminedGif | Self::AnimationGif => "gif",
            Self::ImageQoi => "qoi",
            Self::ImageJxl | Self::UndeterminedJxl | Self::AnimationJxl => "jxl",
            Self::AnimationApng => "apng",
            Self::AnimationUgoira => "zip", // Ugoira is a zip of frames

            Self::VideoFlv => "flv",
            Self::VideoMp4 | Self::UndeterminedMp4 => "mp4",
            Self::VideoMkv => "mkv",
            Self::VideoWebm => "webm",
            Self::VideoMpeg => "mpeg",
            Self::VideoMov => "mov",
            Self::VideoAvi => "avi",
            Self::VideoWmv | Self::UndeterminedWm => "wmv",
            Self::VideoOgv => "ogv",
            Self::VideoRealmedia => "rm",

            Self::AudioMp3 => "mp3",
            Self::AudioOgg => "ogg",
            Self::AudioFlac => "flac",
            Self::AudioWma => "wma",
            Self::AudioM4a => "m4a",
            Self::AudioRealmedia => "ra",
            Self::AudioTrueaudio => "tta",
            Self::AudioWave => "wav",
            Self::AudioMkv => "mka",
            Self::AudioMp4 => "m4a",
            Self::AudioWavpack => "wv",

            Self::ApplicationPdf => "pdf",
            Self::ApplicationEpub => "epub",
            Self::ApplicationDjvu => "djvu",
            Self::ApplicationCbz => "cbz",
            Self::ApplicationRtf => "rtf",
            Self::ApplicationDocx => "docx",
            Self::ApplicationXlsx => "xlsx",
            Self::ApplicationPptx => "pptx",
            Self::ApplicationDoc => "doc",
            Self::ApplicationXls => "xls",
            Self::ApplicationPpt => "ppt",
            Self::TextPlain => "txt",
            Self::TextHtml => "html",

            Self::ApplicationPsd => "psd",
            Self::ApplicationClip => "clip",
            Self::ApplicationSai2 => "sai2",
            Self::ApplicationKrita => "kra",
            Self::ApplicationXcf => "xcf",
            Self::ApplicationProcreate => "procreate",
            Self::ApplicationPaintDotNet => "pdn",

            Self::ApplicationZip => "zip",
            Self::ApplicationRar => "rar",
            Self::Application7z => "7z",
            Self::ApplicationGzip => "gz",

            Self::ApplicationJson => "json",
            Self::ApplicationYaml => "yaml",
            Self::ApplicationFlash => "swf",
            Self::ApplicationWindowsExe => "exe",
            Self::ApplicationCbor => "cbor",

            _ => "bin",
        }
    }

    /// Whether this type can have a visual thumbnail generated.
    #[allow(dead_code)]
    pub fn has_thumbnail(&self) -> bool {
        types_with_thumbnails().contains(self)
    }

    /// Whether this is an image type (static, not animation).
    pub fn is_image(&self) -> bool {
        images().contains(self)
    }

    /// Whether this is an animation type.
    pub fn is_animation(&self) -> bool {
        animations().contains(self)
    }

    /// Whether this is a video type.
    pub fn is_video(&self) -> bool {
        videos().contains(self)
    }

    /// Whether this is an audio type.
    pub fn is_audio(&self) -> bool {
        audios().contains(self)
    }

    /// Whether this type can have resolution (width/height).
    #[allow(dead_code)]
    pub fn has_resolution(&self) -> bool {
        self.is_image()
            || self.is_animation()
            || self.is_video()
            || matches!(
                self,
                Self::ApplicationPsd
                    | Self::ApplicationClip
                    | Self::ApplicationSai2
                    | Self::ApplicationKrita
                    | Self::ApplicationXcf
                    | Self::ApplicationProcreate
                    | Self::ApplicationPaintDotNet
                    | Self::ApplicationPdf
            )
    }

    /// Whether this type can have duration.
    #[allow(dead_code)]
    pub fn has_duration(&self) -> bool {
        self.is_animation() || self.is_video() || self.is_audio()
    }
}

// ---------------------------------------------------------------------------
// File type groupings
// ---------------------------------------------------------------------------

static IMAGES: OnceLock<HashSet<MimeType>> = OnceLock::new();
static ANIMATIONS: OnceLock<HashSet<MimeType>> = OnceLock::new();
static VIDEOS: OnceLock<HashSet<MimeType>> = OnceLock::new();
static AUDIOS: OnceLock<HashSet<MimeType>> = OnceLock::new();
static TYPES_WITH_THUMBNAILS: OnceLock<HashSet<MimeType>> = OnceLock::new();

fn images() -> &'static HashSet<MimeType> {
    IMAGES.get_or_init(|| {
        [
            MimeType::ImageJpeg,
            MimeType::ImagePng,
            MimeType::ImageBmp,
            MimeType::ImageIcon,
            MimeType::ImageWebp,
            MimeType::ImageTiff,
            MimeType::ImageSvg,
            MimeType::ImageHeif,
            MimeType::ImageHeifSequence,
            MimeType::ImageHeic,
            MimeType::ImageHeicSequence,
            MimeType::ImageAvif,
            MimeType::ImageAvifSequence,
            MimeType::ImageGif,
            MimeType::ImageQoi,
            MimeType::ImageJxl,
        ]
        .into_iter()
        .collect()
    })
}

fn animations() -> &'static HashSet<MimeType> {
    ANIMATIONS.get_or_init(|| {
        [
            MimeType::AnimationGif,
            MimeType::AnimationApng,
            MimeType::AnimationUgoira,
            MimeType::AnimationWebp,
            MimeType::AnimationJxl,
        ]
        .into_iter()
        .collect()
    })
}

fn videos() -> &'static HashSet<MimeType> {
    VIDEOS.get_or_init(|| {
        [
            MimeType::VideoFlv,
            MimeType::VideoMp4,
            MimeType::VideoMkv,
            MimeType::VideoWebm,
            MimeType::VideoMpeg,
            MimeType::VideoMov,
            MimeType::VideoAvi,
            MimeType::VideoWmv,
            MimeType::VideoOgv,
            MimeType::VideoRealmedia,
        ]
        .into_iter()
        .collect()
    })
}

fn audios() -> &'static HashSet<MimeType> {
    AUDIOS.get_or_init(|| {
        [
            MimeType::AudioMp3,
            MimeType::AudioOgg,
            MimeType::AudioFlac,
            MimeType::AudioWma,
            MimeType::AudioM4a,
            MimeType::AudioRealmedia,
            MimeType::AudioTrueaudio,
            MimeType::AudioWave,
            MimeType::AudioMkv,
            MimeType::AudioMp4,
            MimeType::AudioWavpack,
        ]
        .into_iter()
        .collect()
    })
}

fn types_with_thumbnails() -> &'static HashSet<MimeType> {
    TYPES_WITH_THUMBNAILS.get_or_init(|| {
        let mut s: HashSet<MimeType> = HashSet::new();
        s.extend(images());
        s.extend(animations());
        s.extend(videos());
        s.extend([
            MimeType::ApplicationPsd,
            MimeType::ApplicationClip,
            MimeType::ApplicationSai2,
            MimeType::ApplicationKrita,
            MimeType::ApplicationXcf,
            MimeType::ApplicationProcreate,
            MimeType::ApplicationPaintDotNet,
            MimeType::ApplicationPdf,
            MimeType::ApplicationCbz,
        ]);
        s
    })
}

/// Initialize all static grouping sets. Call once at startup.
pub fn init_groupings() {
    images();
    animations();
    videos();
    audios();
    types_with_thumbnails();
}
