use std::io::Read;
use std::path::Path;

use crate::constants::MimeType;

use super::{ffmpeg, FileError, FileResult};

const HEADER_READ_SIZE: usize = 256;

type HeaderPair = (&'static [usize], &'static [&'static [u8]]);
type HeaderRule = &'static [HeaderPair];

static HEADERS_AND_MIME: &[(HeaderRule, MimeType)] = &[
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
    (
        &[(&[0], &[b"CWS", b"FWS", b"ZWS"])],
        MimeType::ApplicationFlash,
    ),
    (&[(&[0], &[b"FLV"])], MimeType::VideoFlv),
    (&[(&[0], &[b"%PDF"])], MimeType::ApplicationPdf),
    (
        &[(&[0], &[b"8BPS\x00\x01", b"8BPS\x00\x02"])],
        MimeType::ApplicationPsd,
    ),
    (&[(&[0], &[b"CSFCHUNK"])], MimeType::ApplicationClip),
    (&[(&[0], &[b"SAI-CANVAS"])], MimeType::ApplicationSai2),
    (&[(&[0], &[b"gimp xcf "])], MimeType::ApplicationXcf),
    (
        &[(&[38, 42, 58, 63], &[b"application/x-krita"])],
        MimeType::ApplicationKrita,
    ),
    (&[(&[0], &[b"PDN3"])], MimeType::ApplicationPaintDotNet),
    (
        &[(&[38, 43], &[b"application/epub+zip"])],
        MimeType::ApplicationEpub,
    ),
    (
        &[
            (&[4], &[b"FORM"]),
            (&[12], &[b"DJVU", b"DJVM", b"PM44", b"BM44", b"SDJV"]),
        ],
        MimeType::ApplicationDjvu,
    ),
    (&[(&[0], &[b"{\\rtf"])], MimeType::ApplicationRtf),
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
    (&[(&[0], &[b"fLaC"])], MimeType::AudioFlac),
    (
        &[(&[0], &[b"RIFF"]), (&[8], &[b"WAVE"])],
        MimeType::AudioWave,
    ),
    (&[(&[0], &[b"wvpk"])], MimeType::AudioWavpack),
    (&[(&[8], &[b"AVI "])], MimeType::VideoAvi),
    (
        &[(
            &[0],
            &[b"\x30\x26\xb2\x75\x8e\x66\xcf\x11\xa6\xd9\x00\xaa\x00\x62\xce\x6c"],
        )],
        MimeType::UndeterminedWm,
    ),
    (
        &[(&[0], &[b"\x4d\x5a\x90\x00\x03"])],
        MimeType::ApplicationWindowsExe,
    ),
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

fn is_png_animated(header_bytes: &[u8]) -> bool {
    let mut pos = 8;
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
    let reader = std::io::BufReader::new(file);
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
    let reader = std::io::BufReader::new(file);
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

pub async fn get_mime(path: &Path) -> FileResult<MimeType> {
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
                    return Ok(if ffmpeg::file_is_animated(path).await {
                        MimeType::AnimationJxl
                    } else {
                        MimeType::ImageJxl
                    });
                }
                MimeType::UndeterminedMp4 | MimeType::UndeterminedWm => {
                    match ffmpeg::get_mime(path).await {
                        Ok(detected) if detected != MimeType::ApplicationUnknown => {
                            return Ok(detected)
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
                MimeType::UndeterminedOle => return Ok(MimeType::ApplicationDoc),
                _ => return Ok(mime),
            }
        }
    }

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

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !matches!(ext.as_str(), "txt" | "log" | "json") {
        if let Ok(ffmpeg_mime) = ffmpeg::get_mime(path).await {
            if ffmpeg_mime != MimeType::ApplicationUnknown {
                return Ok(ffmpeg_mime);
            }
        }
    }

    Ok(MimeType::ApplicationUnknown)
}

pub fn is_image(mime: MimeType) -> bool {
    mime.is_image()
}

/// Quick extension-based pre-filter for import paths.
///
/// Rejects files whose extension can never map to an allowed MIME type,
/// avoiding the cost of reading and sniffing bytes for obviously unsupported
/// files (e.g. .txt, .html, .exe) during folder or drag-and-drop imports.
pub fn is_allowed_mime(mime: MimeType) -> bool {
    matches!(
        mime,
        MimeType::ImageJpeg
            | MimeType::ImagePng
            | MimeType::ImageGif
            | MimeType::ImageWebp
            | MimeType::ImageTiff
            | MimeType::ImageQoi
            | MimeType::ImageIcon
            | MimeType::ImageSvg
            | MimeType::ImageHeif
            | MimeType::ImageHeifSequence
            | MimeType::ImageHeic
            | MimeType::ImageHeicSequence
            | MimeType::ImageAvif
            | MimeType::ImageAvifSequence
            | MimeType::ImageBmp
            | MimeType::ImageJxl
            | MimeType::AnimationApng
            | MimeType::AnimationGif
            | MimeType::AnimationWebp
            | MimeType::AnimationJxl
            | MimeType::AnimationUgoira
            | MimeType::VideoAvi
            | MimeType::VideoFlv
            | MimeType::VideoMov
            | MimeType::VideoMp4
            | MimeType::VideoMkv
            | MimeType::VideoRealmedia
            | MimeType::VideoWebm
            | MimeType::VideoOgv
            | MimeType::VideoMpeg
            | MimeType::VideoWmv
            | MimeType::AudioM4a
            | MimeType::AudioMp3
            | MimeType::AudioRealmedia
            | MimeType::AudioOgg
            | MimeType::AudioFlac
            | MimeType::AudioWave
            | MimeType::AudioTrueaudio
            | MimeType::AudioWma
            | MimeType::AudioMkv
            | MimeType::AudioMp4
            | MimeType::AudioWavpack
            | MimeType::ApplicationFlash
            | MimeType::ApplicationCbz
            | MimeType::ApplicationClip
            | MimeType::ApplicationPsd
            | MimeType::ApplicationSai2
            | MimeType::ApplicationKrita
            | MimeType::ApplicationXcf
            | MimeType::ApplicationProcreate
            | MimeType::ApplicationPdf
            | MimeType::ApplicationDocx
            | MimeType::ApplicationXlsx
            | MimeType::ApplicationPptx
            | MimeType::ApplicationDoc
            | MimeType::ApplicationXls
            | MimeType::ApplicationPpt
            | MimeType::ApplicationEpub
            | MimeType::ApplicationDjvu
            | MimeType::ApplicationPaintDotNet
            | MimeType::ApplicationRtf
    )
}
