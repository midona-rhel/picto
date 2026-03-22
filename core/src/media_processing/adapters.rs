use std::path::Path;

use crate::constants::MimeType;

use super::analysis::{get_animation_properties, get_image_dimensions};
use super::detection::is_image;
use super::thumbnail::generate_image_thumbnail;
use super::{archive, ffmpeg, office, pdf, specialty, svg, FileError, FileInfo, FileResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaAdapterKind {
    Archive,
    ClipStudio,
    Krita,
    PaintDotNet,
    Procreate,
    Svg,
    Pdf,
    OfficePptx,
    Flash,
    Psd,
    Ugoira,
    Video,
    Animation,
    Image,
    Audio,
    Unsupported,
}

fn adapter_for(mime: MimeType) -> MediaAdapterKind {
    match mime {
        MimeType::ApplicationCbz | MimeType::ApplicationEpub => MediaAdapterKind::Archive,
        MimeType::ApplicationClip => MediaAdapterKind::ClipStudio,
        MimeType::ApplicationKrita => MediaAdapterKind::Krita,
        MimeType::ApplicationPaintDotNet => MediaAdapterKind::PaintDotNet,
        MimeType::ApplicationProcreate => MediaAdapterKind::Procreate,
        MimeType::ImageSvg => MediaAdapterKind::Svg,
        MimeType::ApplicationPdf => MediaAdapterKind::Pdf,
        MimeType::ApplicationPptx => MediaAdapterKind::OfficePptx,
        MimeType::ApplicationFlash => MediaAdapterKind::Flash,
        MimeType::ApplicationPsd => MediaAdapterKind::Psd,
        MimeType::AnimationUgoira => MediaAdapterKind::Ugoira,
        MimeType::AnimationGif | MimeType::AnimationApng | MimeType::AnimationWebp => {
            MediaAdapterKind::Animation
        }
        MimeType::AnimationJxl
        | MimeType::ImageHeifSequence
        | MimeType::ImageHeicSequence
        | MimeType::ImageAvifSequence => MediaAdapterKind::Video,
        _ if mime.is_video() => MediaAdapterKind::Video,
        _ if mime.is_audio() => MediaAdapterKind::Audio,
        _ if is_image(mime) => MediaAdapterKind::Image,
        _ => MediaAdapterKind::Unsupported,
    }
}

pub(crate) async fn populate_file_info(path: &Path, info: &mut FileInfo) {
    match adapter_for(info.mime) {
        MediaAdapterKind::Archive => {
            let is_epub = info.mime == MimeType::ApplicationEpub;
            if let Ok((w, h)) = archive::get_archive_resolution(path, is_epub) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::ClipStudio => {
            if let Ok(((w, h), dur, nf)) = specialty::get_clip_properties(path) {
                info.width = Some(w);
                info.height = Some(h);
                info.duration_ms = dur;
                info.num_frames = nf;
            }
        }
        MediaAdapterKind::Krita => {
            if let Ok((w, h)) = specialty::get_kra_properties(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::PaintDotNet => {
            if let Ok((w, h)) = specialty::get_paint_net_resolution(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::Procreate => {
            if let Ok((w, h)) = specialty::get_procreate_resolution(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::Svg => {
            if let Ok((w, h)) = svg::get_svg_resolution(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::Pdf => {
            if let Ok((_nw, (w, h))) = pdf::get_pdf_info(path) {
                info.width = w;
                info.height = h;
            }
        }
        MediaAdapterKind::OfficePptx => {
            let (_nw, (w, h)) = office::get_pptx_info(path);
            info.width = w;
            info.height = h;
        }
        MediaAdapterKind::Flash => {
            if let Ok(((w, h), dur, nf)) = specialty::get_flash_properties(path) {
                info.width = Some(w);
                info.height = Some(h);
                info.duration_ms = Some(dur);
                info.num_frames = Some(nf);
            }
        }
        MediaAdapterKind::Psd => {
            if let Ok((w, h)) = specialty::get_psd_resolution(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::Ugoira => {
            if let Ok(((w, h), dur, nf)) = specialty::get_ugoira_properties(path) {
                info.width = Some(w);
                info.height = Some(h);
                info.duration_ms = dur;
                info.num_frames = nf;
            }
        }
        MediaAdapterKind::Video => {
            if let Ok(props) = ffmpeg::get_video_properties(path).await {
                info.width = Some(props.width);
                info.height = Some(props.height);
                info.duration_ms = Some(props.duration_ms);
                info.num_frames = Some(props.num_frames as u32);
                info.has_audio = props.has_audio;
            }
        }
        MediaAdapterKind::Animation => {
            if let Ok((w, h, dur, nf)) = get_animation_properties(path, info.mime) {
                info.width = Some(w);
                info.height = Some(h);
                info.duration_ms = Some(dur);
                info.num_frames = Some(nf);
            }
        }
        MediaAdapterKind::Image => {
            if let Ok((w, h)) = get_image_dimensions(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaAdapterKind::Audio => {
            if let Ok(dur_ms) = ffmpeg::get_audio_duration_ms(path).await {
                info.duration_ms = Some(dur_ms);
            }
        }
        MediaAdapterKind::Unsupported => {}
    }
}

pub(crate) async fn generate_thumbnail_with_adapter(
    path: &Path,
    target_resolution: (u32, u32),
    mime: MimeType,
    duration_ms: Option<u64>,
    num_frames: Option<u32>,
    percentage_in: u32,
) -> FileResult<(Vec<u8>, String)> {
    let as_jpg =
        |r: FileResult<Vec<u8>>| -> FileResult<(Vec<u8>, String)> { r.map(|b| (b, "jpg".into())) };

    match adapter_for(mime) {
        MediaAdapterKind::Archive => {
            let is_epub = mime == MimeType::ApplicationEpub;
            as_jpg(archive::generate_thumbnail_from_archive(path, target_resolution, is_epub))
        }
        MediaAdapterKind::ClipStudio => {
            as_jpg(specialty::generate_thumbnail_from_clip(path, target_resolution))
        }
        MediaAdapterKind::Krita => {
            as_jpg(specialty::generate_thumbnail_from_krita(path, target_resolution))
        }
        MediaAdapterKind::PaintDotNet => {
            as_jpg(specialty::generate_thumbnail_from_paint_net(path, target_resolution))
        }
        MediaAdapterKind::Procreate => {
            as_jpg(specialty::generate_thumbnail_from_procreate(path, target_resolution))
        }
        MediaAdapterKind::Svg => as_jpg(svg::generate_thumbnail_from_svg(path, target_resolution)),
        MediaAdapterKind::Pdf => Err(FileError::Thumbnail(
            "PDF thumbnail generation not supported".to_string(),
        )),
        MediaAdapterKind::OfficePptx => {
            as_jpg(office::generate_thumbnail_from_office(path, target_resolution))
        }
        MediaAdapterKind::Flash => Err(FileError::Thumbnail(
            "Flash thumbnails not supported".to_string(),
        )),
        MediaAdapterKind::Psd | MediaAdapterKind::Image => generate_image_thumbnail(path, target_resolution),
        MediaAdapterKind::Ugoira => {
            let frame_index = num_frames
                .map(|nf| {
                    if nf > 1 {
                        ((percentage_in as f64 / 100.0) * (nf as f64 - 1.0)) as usize
                    } else {
                        0
                    }
                })
                .unwrap_or(0);
            as_jpg(specialty::generate_thumbnail_from_ugoira(
                path,
                target_resolution,
                frame_index,
            ))
        }
        MediaAdapterKind::Animation => generate_image_thumbnail(path, target_resolution),
        MediaAdapterKind::Video | MediaAdapterKind::Audio => {
            let dur = duration_ms.filter(|&ms| ms > 0);
            match ffmpeg::render_video_thumbnail(path, target_resolution, percentage_in, dur).await {
                Ok(bytes) => Ok((bytes, "jpg".into())),
                Err(_) => {
                    if percentage_in > 0 {
                        if let Ok(bytes) =
                            ffmpeg::render_video_thumbnail(path, target_resolution, 0, dur).await
                        {
                            return Ok((bytes, "jpg".into()));
                        }
                    }
                    Err(FileError::Thumbnail(format!(
                        "ffmpeg could not generate thumbnail for {:?}",
                        mime
                    )))
                }
            }
        }
        MediaAdapterKind::Unsupported => Err(FileError::Thumbnail(format!(
            "No thumbnail adapter for {:?}",
            mime
        ))),
    }
}
