//! File processing utilities for Picto.
//!
//! Provides MIME detection by file header bytes, file info extraction (size, dimensions,
//! duration, frames, audio, word count), thumbnail generation with SIMD-accelerated
//! resizing, and hash computation.
//!
//! Adapted from the Hydrus file handling modules with simplifications:
//! - Dropped perceptual hash (use img_hash crate in Phase 4)
//! - Dropped Hydrus filesystem layout (we use content packs)
//! - Uses fast_image_resize for 10x faster thumbnail resizing

pub mod archive;
pub mod blurhash;
pub mod colors;
pub mod ffmpeg;
pub mod ffmpeg_path;
pub mod gallery_dl_path;
pub mod office;
pub mod pdf;
pub mod specialty;
pub mod svg;

use std::io::{BufReader, Read};
use std::path::Path;

use image::GenericImageView;
use sha2::{Digest as Sha2Digest, Sha256};

use crate::constants::MimeType;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(thiserror::Error, Debug)]
pub enum FileError {
    #[error("Zero-size file: {0}")]
    ZeroSizeFile(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFile(String),
    #[error("Damaged or unusual file: {0}")]
    DamagedOrUnusualFile(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("Hash error: {0}")]
    Hash(String),
    #[error("Thumbnail error: {0}")]
    Thumbnail(String),
}

pub type FileResult<T> = Result<T, FileError>;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default thumbnail bounding box (longest side)
pub const DEFAULT_THUMBNAIL_DIMENSIONS: (u32, u32) = (500, 500);

/// Number of bytes to read for header-based MIME detection
const HEADER_READ_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// Header-based MIME detection
// ---------------------------------------------------------------------------

/// A single header check: (offsets, byte_patterns). At least one offset+pattern must match.
type HeaderPair = (&'static [usize], &'static [&'static [u8]]);

/// A complete header rule: all pairs must match.
type HeaderRule = &'static [HeaderPair];

/// (header_rule, mime_type) — order matters! First match wins.
static HEADERS_AND_MIME: &[(HeaderRule, MimeType)] = &[
    // Images
    (&[(&[0], &[b"\xff\xd8"])], MimeType::ImageJpeg),
    (&[(&[0], &[b"\x89PNG"])], MimeType::UndeterminedPng),
    (
        &[(&[0], &[b"GIF87a", b"GIF89a"])],
        MimeType::UndeterminedGif,
    ),
    (&[(&[8], &[b"WEBP"])], MimeType::UndeterminedWebp),
    (&[(&[0], &[b"II*\x00", b"MM\x00*"])], MimeType::ImageTiff),
    (&[(&[0], &[b"BM"])], MimeType::ImageBmp),
    (
        &[(&[0], &[b"\x00\x00\x01\x00", b"\x00\x00\x02\x00"])],
        MimeType::ImageIcon,
    ),
    (&[(&[0], &[b"qoif"])], MimeType::ImageQoi),
    (
        &[(
            &[0],
            &[b"\xff\x0a", b"\x00\x00\x00\x0cJXL \x0d\x0a\x87\x0a"],
        )],
        MimeType::UndeterminedJxl,
    ),
    // Flash / FLV
    (
        &[(&[0], &[b"CWS", b"FWS", b"ZWS"])],
        MimeType::ApplicationFlash,
    ),
    (&[(&[0], &[b"FLV"])], MimeType::VideoFlv),
    // Documents
    (&[(&[0], &[b"%PDF"])], MimeType::ApplicationPdf),
    (
        &[(&[0], &[b"8BPS\x00\x01", b"8BPS\x00\x02"])],
        MimeType::ApplicationPsd,
    ),
    (&[(&[0], &[b"CSFCHUNK"])], MimeType::ApplicationClip),
    (&[(&[0], &[b"SAI-CANVAS"])], MimeType::ApplicationSai2),
    (&[(&[0], &[b"gimp xcf "])], MimeType::ApplicationXcf),
    // Krita — MUST come before generic ZIP
    (
        &[(&[38, 42, 58, 63], &[b"application/x-krita"])],
        MimeType::ApplicationKrita,
    ),
    (&[(&[0], &[b"PDN3"])], MimeType::ApplicationPaintDotNet),
    // EPUB — also before generic ZIP
    (
        &[(&[38, 43], &[b"application/epub+zip"])],
        MimeType::ApplicationEpub,
    ),
    // DJVU — two-part header
    (
        &[
            (&[4], &[b"FORM"]),
            (&[12], &[b"DJVU", b"DJVM", b"PM44", b"BM44", b"SDJV"]),
        ],
        MimeType::ApplicationDjvu,
    ),
    // RTF
    (&[(&[0], &[b"{\\rtf"])], MimeType::ApplicationRtf),
    // Archives (generic ZIP after format-specific checks)
    (
        &[(&[0], &[b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"])],
        MimeType::ApplicationZip,
    ),
    (&[(&[0], &[b"7z\xbc\xaf\x27\x1c"])], MimeType::Application7z),
    (
        &[(
            &[0],
            &[
                b"\x52\x61\x72\x21\x1a\x07\x00",
                b"\x52\x61\x72\x21\x1a\x07\x01\x00",
            ],
        )],
        MimeType::ApplicationRar,
    ),
    (&[(&[0], &[b"\x1f\x8b"])], MimeType::ApplicationGzip),
    // ISOBMFF: AVIF before HEIF before generic MP4
    (&[(&[4], &[b"ftypavif"])], MimeType::ImageAvif),
    (&[(&[4], &[b"ftypavis"])], MimeType::ImageAvifSequence),
    (
        &[(&[4], &[b"ftypmif1"]), (&[16, 20, 24], &[b"avif"])],
        MimeType::ImageAvif,
    ),
    (
        &[(&[4], &[b"ftypheic", b"ftypheix", b"ftypheim", b"ftypheis"])],
        MimeType::ImageHeic,
    ),
    (
        &[(&[4], &[b"ftyphevc", b"ftyphevx", b"ftyphevm", b"ftyphevs"])],
        MimeType::ImageHeicSequence,
    ),
    (&[(&[4], &[b"ftypmif1"])], MimeType::ImageHeif),
    (&[(&[4], &[b"ftypmsf1"])], MimeType::ImageHeifSequence),
    (
        &[(
            &[4],
            &[
                b"ftypmp4",
                b"ftypisom",
                b"ftypM4V",
                b"ftypMSNV",
                b"ftypavc1",
                b"ftypavc1",
                b"ftypFACE",
                b"ftypdash",
            ],
        )],
        MimeType::UndeterminedMp4,
    ),
    (&[(&[4], &[b"ftypqt"])], MimeType::VideoMov),
    // Audio
    (&[(&[0], &[b"fLaC"])], MimeType::AudioFlac),
    (
        &[(&[0], &[b"RIFF"]), (&[8], &[b"WAVE"])],
        MimeType::AudioWave,
    ),
    (&[(&[0], &[b"wvpk"])], MimeType::AudioWavpack),
    // Video
    (&[(&[8], &[b"AVI "])], MimeType::VideoAvi),
    // Windows Media (undetermined WMA/WMV)
    (
        &[(
            &[0],
            &[b"\x30\x26\xb2\x75\x8e\x66\xcf\x11\xa6\xd9\x00\xaa\x00\x62\xce\x6c"],
        )],
        MimeType::UndeterminedWm,
    ),
    // Windows EXE
    (
        &[(&[0], &[b"\x4d\x5a\x90\x00\x03"])],
        MimeType::ApplicationWindowsExe,
    ),
    // OLE compound documents
    (
        &[(
            &[0],
            &[
                b"\x31\xbe\x00\x00",
                b"PO^Q",
                b"\xfe\x37\x00\x23",
                b"\xdb\xa5\x2d\x00\x00\x00",
                b"\xdb\xa5\x2d\x00",
            ],
        )],
        MimeType::ApplicationDoc,
    ),
    (
        &[(&[0], &[b"\xed\xde\xad\x0b", b"\x0b\xad\xde\xad"])],
        MimeType::ApplicationPpt,
    ),
    (
        &[(&[0], &[b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"])],
        MimeType::UndeterminedOle,
    ),
];

fn passes_header_pair(offsets: &[usize], headers: &[&[u8]], file_bytes: &[u8]) -> bool {
    for &offset in offsets {
        for header in headers {
            let end = offset + header.len();
            if end <= file_bytes.len() && &file_bytes[offset..end] == *header {
                return true;
            }
        }
    }
    false
}

fn passes_header_rule(rule: HeaderRule, file_bytes: &[u8]) -> bool {
    for &(offsets, headers) in rule {
        if !passes_header_pair(offsets, headers, file_bytes) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Animation detection helpers
// ---------------------------------------------------------------------------

/// Check if a PNG has an acTL chunk (APNG).
fn is_png_animated(header_bytes: &[u8]) -> bool {
    let mut pos = 8; // skip PNG signature
    while pos + 12 <= header_bytes.len() {
        let chunk_len = u32::from_be_bytes([
            header_bytes[pos],
            header_bytes[pos + 1],
            header_bytes[pos + 2],
            header_bytes[pos + 3],
        ]) as usize;
        let chunk_type = &header_bytes[pos + 4..pos + 8];
        if chunk_type == b"acTL" {
            let num_frames = u32::from_be_bytes([
                header_bytes[pos + 8],
                header_bytes[pos + 9],
                header_bytes[pos + 10],
                header_bytes[pos + 11],
            ]);
            return num_frames > 1;
        }
        pos += 12 + chunk_len;
    }
    false
}

fn is_gif_animated(path: &Path) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let reader = BufReader::new(file);
    let decoder = match image::codecs::gif::GifDecoder::new(reader) {
        Ok(d) => d,
        Err(_) => return false,
    };
    use image::AnimationDecoder;
    decoder.into_frames().take(2).count() > 1
}

fn is_webp_animated(path: &Path) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let reader = BufReader::new(file);
    let decoder = match image::codecs::webp::WebPDecoder::new(reader) {
        Ok(d) => d,
        Err(_) => return false,
    };
    use image::AnimationDecoder;
    decoder.into_frames().take(2).count() > 1
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let has_match = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    has_match(b"<html")
        || has_match(b"<HTML")
        || has_match(b"<!DOCTYPE html")
        || has_match(b"<!DOCTYPE HTML")
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let has_match = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    has_match(b"<svg")
        || has_match(b"<SVG")
        || has_match(b"<!DOCTYPE svg")
        || has_match(b"<!DOCTYPE SVG")
}

// ---------------------------------------------------------------------------
// MIME detection
// ---------------------------------------------------------------------------

/// Detect MIME type from file header bytes.
pub fn get_mime(path: &Path) -> FileResult<MimeType> {
    let size = std::fs::metadata(path)
        .map_err(|e| FileError::NotFound(format!("{}: {}", path.display(), e)))?
        .len();

    if size == 0 {
        return Err(FileError::ZeroSizeFile(path.display().to_string()));
    }

    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; HEADER_READ_SIZE];
    let bytes_read = file.read(&mut header)?;
    let header = &header[..bytes_read];

    for &(rule, mime) in HEADERS_AND_MIME {
        if passes_header_rule(rule, header) {
            match mime {
                MimeType::ApplicationZip => return Ok(MimeType::ApplicationZip),
                MimeType::UndeterminedPng => {
                    // Read more bytes for APNG detection — acTL can appear after
                    // large metadata chunks that exceed the initial 256-byte header.
                    let mut extended = vec![0u8; 8192];
                    use std::io::Seek;
                    let _ = file.seek(std::io::SeekFrom::Start(0));
                    let ext_read = file.read(&mut extended).unwrap_or(0);
                    return Ok(if is_png_animated(&extended[..ext_read]) {
                        MimeType::AnimationApng
                    } else {
                        MimeType::ImagePng
                    });
                }
                MimeType::UndeterminedGif => {
                    return Ok(if is_gif_animated(path) {
                        MimeType::AnimationGif
                    } else {
                        MimeType::ImageGif
                    });
                }
                MimeType::UndeterminedWebp => {
                    return Ok(if is_webp_animated(path) {
                        MimeType::AnimationWebp
                    } else {
                        MimeType::ImageWebp
                    });
                }
                MimeType::UndeterminedJxl => {
                    return Ok(if ffmpeg::file_is_animated(path) {
                        MimeType::AnimationJxl
                    } else {
                        MimeType::ImageJxl
                    });
                }
                MimeType::UndeterminedMp4 | MimeType::UndeterminedWm => {
                    match ffmpeg::get_mime(path) {
                        Ok(detected) if detected != MimeType::ApplicationUnknown => {
                            return Ok(detected);
                        }
                        _ => {
                            return Ok(if mime == MimeType::UndeterminedMp4 {
                                MimeType::VideoMp4
                            } else {
                                MimeType::VideoWmv
                            });
                        }
                    }
                }
                MimeType::UndeterminedOle => {
                    // TODO: Port OLE file inspection
                    return Ok(MimeType::ApplicationDoc);
                }
                _ => return Ok(mime),
            }
        }
    }

    // Fallback heuristics: JSON, HTML, SVG
    if header.starts_with(b"{") || header.starts_with(b"[") {
        let contents = std::fs::read(path)?;
        if serde_json::from_slice::<serde_json::Value>(&contents).is_ok() {
            return Ok(MimeType::ApplicationJson);
        }
    }

    if looks_like_html(header) {
        return Ok(MimeType::TextHtml);
    }

    if looks_like_svg(header) {
        return Ok(MimeType::ImageSvg);
    }

    // Final ffmpeg fallback
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !matches!(ext.as_str(), "txt" | "log" | "json") {
        if let Ok(ffmpeg_mime) = ffmpeg::get_mime(path) {
            if ffmpeg_mime != MimeType::ApplicationUnknown {
                return Ok(ffmpeg_mime);
            }
        }
    }

    Ok(MimeType::ApplicationUnknown)
}

// ---------------------------------------------------------------------------
// Type predicate helpers
// ---------------------------------------------------------------------------

pub fn is_image(mime: MimeType) -> bool {
    mime.is_image()
}

pub fn is_animation(mime: MimeType) -> bool {
    mime.is_animation()
}

pub fn is_video(mime: MimeType) -> bool {
    mime.is_video()
}

pub fn is_audio(mime: MimeType) -> bool {
    mime.is_audio()
}

pub fn definitely_has_audio(mime: MimeType) -> bool {
    is_audio(mime) || mime == MimeType::ApplicationFlash
}

/// Check if a MIME type is allowed for import.
pub fn is_allowed_mime(mime: MimeType) -> bool {
    matches!(
        mime,
        // Images
        MimeType::ImageJpeg | MimeType::ImagePng | MimeType::ImageGif
            | MimeType::ImageWebp | MimeType::ImageTiff | MimeType::ImageQoi
            | MimeType::ImageIcon | MimeType::ImageSvg | MimeType::ImageHeif
            | MimeType::ImageHeifSequence | MimeType::ImageHeic
            | MimeType::ImageHeicSequence | MimeType::ImageAvif
            | MimeType::ImageAvifSequence | MimeType::ImageBmp | MimeType::ImageJxl
        // Animations
            | MimeType::AnimationApng | MimeType::AnimationGif
            | MimeType::AnimationWebp | MimeType::AnimationJxl
            | MimeType::AnimationUgoira
        // Video
            | MimeType::VideoAvi | MimeType::VideoFlv | MimeType::VideoMov
            | MimeType::VideoMp4 | MimeType::VideoMkv | MimeType::VideoRealmedia
            | MimeType::VideoWebm | MimeType::VideoOgv | MimeType::VideoMpeg
            | MimeType::VideoWmv
        // Audio
            | MimeType::AudioM4a | MimeType::AudioMp3 | MimeType::AudioRealmedia
            | MimeType::AudioOgg | MimeType::AudioFlac | MimeType::AudioWave
            | MimeType::AudioTrueaudio | MimeType::AudioWma | MimeType::AudioMkv
            | MimeType::AudioMp4 | MimeType::AudioWavpack
        // Applications
            | MimeType::ApplicationFlash | MimeType::ApplicationCbz
            | MimeType::ApplicationClip | MimeType::ApplicationPsd
            | MimeType::ApplicationSai2 | MimeType::ApplicationKrita
            | MimeType::ApplicationXcf | MimeType::ApplicationProcreate
            | MimeType::ApplicationPdf | MimeType::ApplicationDocx
            | MimeType::ApplicationXlsx | MimeType::ApplicationPptx
            | MimeType::ApplicationDoc | MimeType::ApplicationXls
            | MimeType::ApplicationPpt | MimeType::ApplicationEpub
            | MimeType::ApplicationDjvu | MimeType::ApplicationPaintDotNet
            | MimeType::ApplicationRtf | MimeType::ApplicationZip
            | MimeType::ApplicationRar | MimeType::Application7z
            | MimeType::ApplicationGzip
    )
}

// ---------------------------------------------------------------------------
// File info extraction
// ---------------------------------------------------------------------------

/// File information extracted from a file.
#[derive(Debug, Clone)]
pub struct FileInfo {
    #[allow(dead_code)]
    pub size: u64,
    pub mime: MimeType,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub num_frames: Option<u32>,
    pub has_audio: bool,
    #[allow(dead_code)]
    pub num_words: Option<u32>,
}

/// Extract file info: size, dimensions, duration, frames, audio, word count.
pub fn get_file_info(path: &Path, mime: Option<MimeType>) -> FileResult<FileInfo> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| FileError::NotFound(format!("{}: {}", path.display(), e)))?;
    let size = metadata.len();

    if size == 0 {
        return Err(FileError::ZeroSizeFile(path.display().to_string()));
    }

    let mime = match mime {
        Some(m) => m,
        None => get_mime(path)?,
    };

    if !is_allowed_mime(mime) {
        if mime == MimeType::TextHtml {
            return Err(FileError::UnsupportedFile("Looks like HTML".to_string()));
        } else if mime == MimeType::ApplicationJson {
            return Err(FileError::UnsupportedFile("Looks like JSON".to_string()));
        } else if mime == MimeType::ApplicationUnknown {
            return Err(FileError::UnsupportedFile("Unknown filetype!".to_string()));
        } else {
            return Err(FileError::UnsupportedFile(
                "Filetype is not permitted!".to_string(),
            ));
        }
    }

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut duration_ms: Option<u64> = None;
    let mut num_frames: Option<u32> = None;
    let mut num_words: Option<u32> = None;
    let mut has_audio = definitely_has_audio(mime);

    if mime == MimeType::ApplicationCbz || mime == MimeType::ApplicationEpub {
        let is_epub = mime == MimeType::ApplicationEpub;
        if let Ok((w, h)) = archive::get_archive_resolution(path, is_epub) {
            width = Some(w);
            height = Some(h);
        }
    } else if mime == MimeType::ApplicationClip {
        if let Ok(((w, h), dur, nf)) = specialty::get_clip_properties(path) {
            width = Some(w);
            height = Some(h);
            duration_ms = dur;
            num_frames = nf;
        }
    } else if mime == MimeType::ApplicationKrita {
        if let Ok((w, h)) = specialty::get_kra_properties(path) {
            width = Some(w);
            height = Some(h);
        }
    } else if mime == MimeType::ApplicationPaintDotNet {
        if let Ok((w, h)) = specialty::get_paint_net_resolution(path) {
            width = Some(w);
            height = Some(h);
        }
    } else if mime == MimeType::ApplicationProcreate {
        if let Ok((w, h)) = specialty::get_procreate_resolution(path) {
            width = Some(w);
            height = Some(h);
        }
    } else if mime == MimeType::ImageSvg {
        if let Ok((w, h)) = svg::get_svg_resolution(path) {
            width = Some(w);
            height = Some(h);
        }
    } else if mime == MimeType::ApplicationPdf {
        if let Ok((nw, (w, h))) = pdf::get_pdf_info(path) {
            num_words = nw;
            width = w;
            height = h;
        }
    } else if mime == MimeType::ApplicationPptx {
        let (nw, (w, h)) = office::get_pptx_info(path);
        num_words = nw;
        width = w;
        height = h;
    } else if mime == MimeType::ApplicationDocx {
        num_words = office::get_docx_info(path);
    } else if matches!(
        mime,
        MimeType::ApplicationDoc | MimeType::ApplicationPpt | MimeType::ApplicationXls
    ) {
        if let Ok(nw) = office::ole_document_word_count(path) {
            num_words = nw;
        }
    } else if mime == MimeType::ApplicationFlash {
        if let Ok(((w, h), dur, nf)) = specialty::get_flash_properties(path) {
            width = Some(w);
            height = Some(h);
            duration_ms = Some(dur);
            num_frames = Some(nf);
        }
    } else if mime == MimeType::ApplicationPsd {
        if let Ok((w, h)) = specialty::get_psd_resolution(path) {
            width = Some(w);
            height = Some(h);
        }
    } else if mime == MimeType::AnimationUgoira {
        if let Ok(((w, h), dur, nf)) = specialty::get_ugoira_properties(path) {
            width = Some(w);
            height = Some(h);
            duration_ms = dur;
            num_frames = nf;
        }
    } else if is_video(mime)
        || matches!(
            mime,
            MimeType::ImageHeifSequence
                | MimeType::ImageHeicSequence
                | MimeType::ImageAvifSequence
                | MimeType::AnimationJxl
        )
    {
        if let Ok((res, dur_ms, nframes, audio)) = ffmpeg::get_ffmpeg_video_properties(path, false)
        {
            width = Some(res.0);
            height = Some(res.1);
            duration_ms = Some(dur_ms);
            num_frames = Some(nframes as u32);
            has_audio = audio;
        }
    } else if is_animation(mime) {
        if let Ok(props) = get_animation_properties(path, mime) {
            width = Some(props.0);
            height = Some(props.1);
            duration_ms = Some(props.2);
            num_frames = Some(props.3);
        }
    } else if is_image(mime) {
        if let Ok(dims) = get_image_dimensions(path) {
            width = Some(dims.0);
            height = Some(dims.1);
        }
    } else if is_audio(mime) {
        if let Ok(dur_ms) = ffmpeg::get_audio_duration_ms(path) {
            duration_ms = Some(dur_ms);
        }
    }

    Ok(FileInfo {
        size,
        mime,
        width,
        height,
        duration_ms,
        num_frames,
        has_audio,
        num_words,
    })
}

/// Get image dimensions using header-only decode.
fn get_image_dimensions(path: &Path) -> FileResult<(u32, u32)> {
    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let (w, h) = reader.into_dimensions()?;
    Ok((w, h))
}

/// Get animation properties (width, height, duration_ms, num_frames).
fn get_animation_properties(path: &Path, mime: MimeType) -> FileResult<(u32, u32, u64, u32)> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    use image::AnimationDecoder;

    match mime {
        MimeType::AnimationGif => {
            let decoder = image::codecs::gif::GifDecoder::new(reader).map_err(FileError::Image)?;
            let (w, h) = image::ImageDecoder::dimensions(&decoder);
            let mut frame_count = 0u32;
            let mut total_delay_ms = 0u64;
            for f in decoder.into_frames().flatten() {
                frame_count += 1;
                let (num, den) = f.delay().numer_denom_ms();
                total_delay_ms += num as u64 / den.max(1) as u64;
            }
            Ok((w, h, total_delay_ms, frame_count))
        }
        MimeType::AnimationWebp => {
            let decoder =
                image::codecs::webp::WebPDecoder::new(reader).map_err(FileError::Image)?;
            let (w, h) = image::ImageDecoder::dimensions(&decoder);
            let mut frame_count = 0u32;
            let mut total_delay_ms = 0u64;
            for f in decoder.into_frames().flatten() {
                frame_count += 1;
                let (num, den) = f.delay().numer_denom_ms();
                total_delay_ms += num as u64 / den.max(1) as u64;
            }
            Ok((w, h, total_delay_ms, frame_count))
        }
        MimeType::AnimationApng => {
            let decoder = image::codecs::png::PngDecoder::new(reader).map_err(FileError::Image)?;
            if decoder.is_apng().unwrap_or(false) {
                let (w, h) = image::ImageDecoder::dimensions(&decoder);
                let apng = decoder.apng().map_err(FileError::Image)?;
                let mut frame_count = 0u32;
                let mut total_delay_ms = 0u64;
                for f in apng.into_frames().flatten() {
                    frame_count += 1;
                    let (num, den) = f.delay().numer_denom_ms();
                    total_delay_ms += num as u64 / den.max(1) as u64;
                }
                Ok((w, h, total_delay_ms, frame_count))
            } else {
                let (w, h) = image::ImageDecoder::dimensions(&decoder);
                Ok((w, h, 0, 1))
            }
        }
        _ => {
            let img = image::open(path)?;
            let (w, h) = img.dimensions();
            Ok((w, h, 0, 1))
        }
    }
}

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

/// Compute SHA-256 hash of a byte buffer.
pub fn get_hash_from_bytes(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

// ---------------------------------------------------------------------------
// Thumbnail resolution calculation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailScaleType {
    ScaleDownOnly = 0,
    ScaleToFit = 1,
    #[allow(dead_code)]
    ScaleToFill = 2,
}

/// Calculate thumbnail resolution preserving aspect ratio within bounding box.
pub fn get_thumbnail_resolution(
    image_resolution: (u32, u32),
    bounding_dimensions: (u32, u32),
    scale_type: ThumbnailScaleType,
    thumbnail_dpr_percent: u32,
) -> (u32, u32) {
    let (mut im_width, mut im_height) = image_resolution;
    let (mut bounding_width, mut bounding_height) = bounding_dimensions;

    if thumbnail_dpr_percent != 100 {
        let dpr = thumbnail_dpr_percent as f64 / 100.0;
        bounding_height = (bounding_height as f64 * dpr) as u32;
        bounding_width = (bounding_width as f64 * dpr) as u32;
    }

    if im_width == 0 || im_height == 0 {
        im_width = bounding_width;
        im_height = bounding_width;
    }

    if scale_type == ThumbnailScaleType::ScaleDownOnly
        && bounding_width >= im_width
        && bounding_height >= im_height
    {
        return (im_width, im_height);
    }

    let width_ratio = im_width as f64 / bounding_width as f64;
    let height_ratio = im_height as f64 / bounding_height as f64;
    let image_is_wider = width_ratio > height_ratio;
    let image_is_taller = height_ratio > width_ratio;
    let image_ratio = im_width as f64 / im_height as f64;

    let mut thumbnail_width = bounding_width as f64;
    let mut thumbnail_height = bounding_height as f64;

    match scale_type {
        ThumbnailScaleType::ScaleDownOnly | ThumbnailScaleType::ScaleToFit => {
            if image_is_taller {
                thumbnail_width = im_width as f64 / height_ratio;
            } else if image_is_wider {
                thumbnail_height = im_height as f64 / width_ratio;
            }
        }
        ThumbnailScaleType::ScaleToFill => {
            if image_is_taller {
                thumbnail_height = bounding_width as f64 * (1.0 / image_ratio).min(5.0);
            } else if image_is_wider {
                thumbnail_width = bounding_height as f64 * image_ratio.min(5.0);
            }
        }
    }

    let tw = (thumbnail_width as i64).max(1) as u32;
    let th = (thumbnail_height as i64).max(1) as u32;
    (tw, th)
}

// ---------------------------------------------------------------------------
// Thumbnail generation (with SIMD-accelerated resize via fast_image_resize)
// ---------------------------------------------------------------------------

/// Resize an image using fast_image_resize (SIMD-accelerated, ~10x faster than image crate).
fn fast_resize(img: &image::DynamicImage, tw: u32, th: u32) -> FileResult<image::DynamicImage> {
    use fast_image_resize as fr;

    let src_w = img.width();
    let src_h = img.height();
    let dst_w = tw.max(1);
    let dst_h = th.max(1);

    // Convert to RGBA8 for consistent handling
    let rgba = img.to_rgba8();
    let src_image =
        fr::images::Image::from_vec_u8(src_w, src_h, rgba.into_raw(), fr::PixelType::U8x4)
            .map_err(|e| FileError::Thumbnail(format!("fast_image_resize src error: {}", e)))?;

    let mut dst_image = fr::images::Image::new(dst_w, dst_h, fr::PixelType::U8x4);

    let mut resizer = fr::Resizer::new();
    resizer
        .resize(&src_image, &mut dst_image, None)
        .map_err(|e| FileError::Thumbnail(format!("fast_image_resize error: {}", e)))?;

    let dst_buf = dst_image.into_vec();
    let result = image::RgbaImage::from_raw(tw, th, dst_buf)
        .ok_or_else(|| FileError::Thumbnail("Failed to create image from resized data".into()))?;

    Ok(image::DynamicImage::ImageRgba8(result))
}

/// Generate a thumbnail for a file. Returns `(bytes, extension)` — PNG for
/// transparent images, JPEG otherwise.
pub fn generate_thumbnail_bytes(
    path: &Path,
    target_resolution: (u32, u32),
    mime: MimeType,
    duration_ms: Option<u64>,
    num_frames: Option<u32>,
    percentage_in: u32,
) -> FileResult<(Vec<u8>, String)> {
    // Helper: wrap specialty generators that always produce JPEG bytes.
    let as_jpg =
        |r: FileResult<Vec<u8>>| -> FileResult<(Vec<u8>, String)> { r.map(|b| (b, "jpg".into())) };

    if mime == MimeType::ApplicationCbz || mime == MimeType::ApplicationEpub {
        let is_epub = mime == MimeType::ApplicationEpub;
        return as_jpg(archive::generate_thumbnail_from_archive(
            path,
            target_resolution,
            is_epub,
        ));
    } else if mime == MimeType::ApplicationClip {
        return as_jpg(specialty::generate_thumbnail_from_clip(
            path,
            target_resolution,
        ));
    } else if mime == MimeType::ApplicationKrita {
        return as_jpg(specialty::generate_thumbnail_from_krita(
            path,
            target_resolution,
        ));
    } else if mime == MimeType::ApplicationPaintDotNet {
        return as_jpg(specialty::generate_thumbnail_from_paint_net(
            path,
            target_resolution,
        ));
    } else if mime == MimeType::ApplicationProcreate {
        return as_jpg(specialty::generate_thumbnail_from_procreate(
            path,
            target_resolution,
        ));
    } else if mime == MimeType::ApplicationPsd {
        return as_jpg(specialty::generate_thumbnail_from_psd(
            path,
            target_resolution,
        ));
    } else if mime == MimeType::ImageSvg {
        return as_jpg(svg::generate_thumbnail_from_svg(path, target_resolution));
    } else if mime == MimeType::ApplicationPdf {
        return as_jpg(pdf::generate_thumbnail_from_pdf(path, target_resolution));
    } else if mime == MimeType::ApplicationPptx {
        return as_jpg(office::generate_thumbnail_from_office(
            path,
            target_resolution,
        ));
    } else if mime == MimeType::ApplicationFlash {
        return Err(FileError::Thumbnail(
            "Flash thumbnails not supported".to_string(),
        ));
    } else if is_image(mime) || mime == MimeType::AnimationWebp {
        return generate_image_thumbnail(path, target_resolution);
    } else if mime == MimeType::AnimationUgoira {
        let frame_index = num_frames
            .map(|nf| {
                if nf > 1 {
                    ((percentage_in as f64 / 100.0) * (nf as f64 - 1.0)) as usize
                } else {
                    0
                }
            })
            .unwrap_or(0);
        return as_jpg(specialty::generate_thumbnail_from_ugoira(
            path,
            target_resolution,
            frame_index,
        ));
    } else {
        // Animations (non-webp) and video: use ffmpeg
        if is_animation(mime) {
            return generate_image_thumbnail(path, target_resolution);
        }

        match ffmpeg::render_video_frame_to_png(
            path,
            target_resolution,
            percentage_in,
            num_frames.unwrap_or(1) as u64,
            duration_ms.unwrap_or(0),
        ) {
            Ok(bytes) => return Ok((bytes, "jpg".into())),
            Err(_) => {
                if percentage_in > 0 {
                    if let Ok(bytes) = ffmpeg::render_video_frame_to_png(
                        path,
                        target_resolution,
                        0,
                        num_frames.unwrap_or(1) as u64,
                        duration_ms.unwrap_or(0),
                    ) {
                        return Ok((bytes, "jpg".into()));
                    }
                }
                return Err(FileError::Thumbnail(format!(
                    "ffmpeg could not generate thumbnail for {:?}",
                    mime
                )));
            }
        }
    }
}

/// Encode a DynamicImage as JPEG. Used by specialty thumbnail generators
/// (archive, office, etc.) that don't need alpha preservation.
pub fn encode_thumbnail_jpeg(img: &image::DynamicImage) -> FileResult<Vec<u8>> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    // JPEG has no alpha channel. Composite transparency onto white so
    // transparent thumbnails don't render with dark artifacts.
    let mut flattened = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut flattened, &rgba, 0, 0);
    let rgb = image::DynamicImage::ImageRgba8(flattened).to_rgb8();

    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
    encoder.encode_image(&image::DynamicImage::ImageRgb8(rgb))?;
    Ok(out)
}

/// Detect whether an RGBA image has any meaningful transparency.
fn has_meaningful_alpha(rgba: &image::RgbaImage) -> bool {
    rgba.pixels().any(|p| p.0[3] < 255)
}

/// Encode a DynamicImage, preserving transparency when present.
/// Returns `(bytes, extension)` — `"png"` for images with alpha, `"jpg"` otherwise.
pub fn encode_thumbnail(img: &image::DynamicImage) -> FileResult<(Vec<u8>, &'static str)> {
    let rgba = img.to_rgba8();
    if has_meaningful_alpha(&rgba) {
        use image::ImageEncoder;
        let mut out = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut out);
        encoder.write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )?;
        Ok((out, "png"))
    } else {
        encode_thumbnail_jpeg(img).map(|bytes| (bytes, "jpg"))
    }
}

/// Generate thumbnail from an image file using SIMD-accelerated resize.
/// Returns `(bytes, extension)` — PNG for transparent images, JPEG otherwise.
fn generate_image_thumbnail(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<(Vec<u8>, String)> {
    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let img = reader.decode()?;
    let (orig_w, orig_h) = img.dimensions();

    let (tw, th) = get_thumbnail_resolution(
        (orig_w, orig_h),
        target_resolution,
        ThumbnailScaleType::ScaleDownOnly,
        100,
    );

    let thumbnail = fast_resize(&img, tw, th)?;
    encode_thumbnail(&thumbnail).map(|(bytes, ext)| (bytes, ext.to_string()))
}

/// Decompression bomb check.
pub fn is_decompression_bomb(path: &Path) -> FileResult<bool> {
    const MAX_IMAGE_PIXELS: u64 = (512 * 1024 * 1024) / 3;

    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(FileError::Io)?;

    match reader.into_dimensions() {
        Ok((w, h)) => Ok(w as u64 * h as u64 > MAX_IMAGE_PIXELS),
        Err(_) => Ok(false),
    }
}
