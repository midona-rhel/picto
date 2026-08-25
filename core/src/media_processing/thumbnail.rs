use std::io::BufReader;
use std::path::Path;

use image::GenericImageView;

use super::adapters::generate_thumbnail_with_adapter;
use super::{FileError, FileResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailScaleType {
    ScaleDownOnly = 0,
    ScaleToFit = 1,
    ScaleToFill = 2,
}

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

pub async fn generate_thumbnail_bytes(
    path: &Path,
    target_resolution: (u32, u32),
    mime: crate::constants::MimeType,
    duration_ms: Option<u64>,
    num_frames: Option<u32>,
    percentage_in: u32,
) -> FileResult<(Vec<u8>, String)> {
    generate_thumbnail_with_adapter(
        path,
        target_resolution,
        mime,
        duration_ms,
        num_frames,
        percentage_in,
    )
    .await
}

pub fn generate_thumbnail_from_decoded_image(
    decoded: &image::DynamicImage,
    target_resolution: (u32, u32),
) -> FileResult<(Vec<u8>, &'static str)> {
    let (tw, th) = get_thumbnail_resolution(
        (decoded.width(), decoded.height()),
        target_resolution,
        ThumbnailScaleType::ScaleDownOnly,
        100,
    );
    let resized = fast_resize(decoded, tw, th)?;
    encode_thumbnail(&resized)
}

/// Decode a JPEG at the smallest native IDCT scale that still covers the
/// thumbnail target. This avoids materializing the full-resolution raster.
pub(crate) fn generate_jpeg_thumbnail(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<(Vec<u8>, &'static str)> {
    let file = std::fs::File::open(path)?;
    let mut decoder = jpeg_decoder::Decoder::new(BufReader::new(file));
    decoder
        .scale(
            target_resolution.0.min(u16::MAX as u32) as u16,
            target_resolution.1.min(u16::MAX as u32) as u16,
        )
        .map_err(|error| FileError::Thumbnail(format!("JPEG scale failed: {error}")))?;
    let pixels = decoder
        .decode()
        .map_err(|error| FileError::Thumbnail(format!("JPEG decode failed: {error}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| FileError::Thumbnail("JPEG dimensions are unavailable".to_string()))?;
    let rgb = jpeg_pixels_to_rgb(pixels, info.pixel_format);
    let image = image::RgbImage::from_raw(info.width as u32, info.height as u32, rgb)
        .ok_or_else(|| FileError::Thumbnail("JPEG pixel buffer has invalid dimensions".into()))?;
    let (tw, th) = get_thumbnail_resolution(
        image.dimensions(),
        target_resolution,
        ThumbnailScaleType::ScaleDownOnly,
        100,
    );
    let resized = fast_resize_rgb(image, tw, th)?;
    encode_rgb_as_jpeg(&resized).map(|bytes| (bytes, "jpg"))
}

fn jpeg_pixels_to_rgb(pixels: Vec<u8>, format: jpeg_decoder::PixelFormat) -> Vec<u8> {
    match format {
        jpeg_decoder::PixelFormat::RGB24 => pixels,
        jpeg_decoder::PixelFormat::L8 => pixels
            .into_iter()
            .flat_map(|value| [value, value, value])
            .collect(),
        jpeg_decoder::PixelFormat::L16 => pixels
            .chunks_exact(2)
            .flat_map(|value| [value[0], value[0], value[0]])
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks_exact(4)
            .flat_map(|pixel| {
                let black = 1.0 - pixel[3] as f32 / 255.0;
                [
                    ((255 - pixel[0]) as f32 * black) as u8,
                    ((255 - pixel[1]) as f32 * black) as u8,
                    ((255 - pixel[2]) as f32 * black) as u8,
                ]
            })
            .collect(),
    }
}

fn fast_resize_rgb(image: image::RgbImage, width: u32, height: u32) -> FileResult<image::RgbImage> {
    use fast_image_resize as fr;

    let source = fr::images::Image::from_vec_u8(
        image.width(),
        image.height(),
        image.into_raw(),
        fr::PixelType::U8x3,
    )
    .map_err(|error| FileError::Thumbnail(format!("JPEG resize source failed: {error}")))?;
    let mut destination = fr::images::Image::new(width, height, fr::PixelType::U8x3);
    fr::Resizer::new()
        .resize(&source, &mut destination, None)
        .map_err(|error| FileError::Thumbnail(format!("JPEG resize failed: {error}")))?;
    image::RgbImage::from_raw(width, height, destination.into_vec())
        .ok_or_else(|| FileError::Thumbnail("JPEG resize buffer has invalid dimensions".into()))
}

fn encode_rgb_as_jpeg(rgb: &image::RgbImage) -> FileResult<Vec<u8>> {
    use image::ImageEncoder;

    let mut output = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 82).write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(output)
}

fn fast_resize(img: &image::DynamicImage, tw: u32, th: u32) -> FileResult<image::DynamicImage> {
    use fast_image_resize as fr;

    let src_w = img.width();
    let src_h = img.height();
    let dst_w = tw.max(1);
    let dst_h = th.max(1);

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

/// Encode an RgbaImage as JPEG by compositing alpha onto a white background.
fn encode_rgba_as_jpeg(rgba: &image::RgbaImage) -> FileResult<Vec<u8>> {
    let (w, h) = rgba.dimensions();
    let mut flattened = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut flattened, rgba, 0, 0);
    let rgb = image::DynamicImage::ImageRgba8(flattened).to_rgb8();

    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
    encoder.encode_image(&image::DynamicImage::ImageRgb8(rgb))?;
    Ok(out)
}

/// Backward-compatible entry point that converts to RGBA internally.
pub fn encode_thumbnail_jpeg(img: &image::DynamicImage) -> FileResult<Vec<u8>> {
    let rgba = img.to_rgba8();
    encode_rgba_as_jpeg(&rgba)
}

fn has_meaningful_alpha(rgba: &image::RgbaImage) -> bool {
    rgba.pixels().any(|p| p.0[3] < 255)
}

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
        encode_rgba_as_jpeg(&rgba).map(|bytes| (bytes, "jpg"))
    }
}

pub(crate) fn generate_image_thumbnail(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<(Vec<u8>, String)> {
    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(FileError::Io)?;
    let mut img = reader.decode()?;

    // Apply ICC color profile if present (converts to sRGB)
    if let Ok(raw_bytes) = std::fs::read(path) {
        img = apply_icc_profile_to_srgb(img, &raw_bytes);
    }

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

/// Extract ICC profile from raw image bytes and convert pixels to sRGB.
/// Returns the original image unchanged if no profile is found or conversion fails.
fn apply_icc_profile_to_srgb(img: image::DynamicImage, raw_bytes: &[u8]) -> image::DynamicImage {
    let icc_data = match extract_icc_profile(raw_bytes) {
        Some(data) => data,
        None => return img,
    };

    // Parse the ICC profile
    let src_profile = match lcms2::Profile::new_icc(&icc_data) {
        Ok(p) => p,
        Err(_) => return img,
    };

    let dst_profile = lcms2::Profile::new_srgb();

    // Check if it's already sRGB (skip conversion)
    // A rough heuristic: if the profile description contains "sRGB", skip.
    // lcms2 doesn't expose description easily, so just always convert — it's a no-op for sRGB anyway.

    let transform = match lcms2::Transform::new(
        &src_profile,
        lcms2::PixelFormat::RGBA_8,
        &dst_profile,
        lcms2::PixelFormat::RGBA_8,
        lcms2::Intent::Perceptual,
    ) {
        Ok(t) => t,
        Err(_) => return img,
    };

    let mut rgba = img.to_rgba8();
    let pixels: &mut [[u8; 4]] = unsafe {
        let ptr = rgba.as_mut_ptr() as *mut [u8; 4];
        let len = rgba.len() / 4;
        std::slice::from_raw_parts_mut(ptr, len)
    };

    transform.transform_in_place(pixels);

    image::DynamicImage::ImageRgba8(rgba)
}

/// Extract ICC profile data from JPEG APP2 markers or PNG iCCP chunk.
fn extract_icc_profile(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 {
        return None;
    }

    // JPEG: ICC profile in APP2 markers (0xFF 0xE2)
    if data[0] == 0xFF && data[1] == 0xD8 {
        return extract_icc_from_jpeg(data);
    }

    // PNG: iCCP chunk
    if data.starts_with(b"\x89PNG") {
        return extract_icc_from_png(data);
    }

    None
}

fn extract_icc_from_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    const ICC_MARKER: &[u8] = b"ICC_PROFILE\0";
    let mut chunks: Vec<(u8, u8, Vec<u8>)> = Vec::new(); // (seq, total, data)
    let mut pos = 2; // skip SOI

    while pos + 4 < data.len() {
        if data[pos] != 0xFF {
            break;
        }
        let marker = data[pos + 1];
        if marker == 0xD9 || marker == 0xDA {
            break; // EOI or SOS — stop scanning
        }
        let seg_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if seg_len < 2 || pos + 2 + seg_len > data.len() {
            break;
        }

        // APP2 marker with ICC_PROFILE header
        if marker == 0xE2 && seg_len > ICC_MARKER.len() + 2 {
            let payload = &data[pos + 4..pos + 2 + seg_len];
            if payload.starts_with(ICC_MARKER) {
                let seq = payload[ICC_MARKER.len()];
                let total = payload[ICC_MARKER.len() + 1];
                let icc_chunk = &payload[ICC_MARKER.len() + 2..];
                chunks.push((seq, total, icc_chunk.to_vec()));
            }
        }

        pos += 2 + seg_len;
    }

    if chunks.is_empty() {
        return None;
    }

    chunks.sort_by_key(|(seq, _, _)| *seq);
    let mut profile = Vec::new();
    for (_, _, chunk) in &chunks {
        profile.extend_from_slice(chunk);
    }

    if profile.len() < 128 {
        return None; // too small to be a valid ICC profile
    }

    Some(profile)
}

fn extract_icc_from_png(data: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 8; // skip PNG signature

    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        let chunk_data_start = pos + 8;
        let chunk_data_end = chunk_data_start + chunk_len;

        if chunk_data_end > data.len() {
            break;
        }

        if chunk_type == b"iCCP" {
            // iCCP: null-terminated profile name, compression method (0), compressed data
            let chunk_data = &data[chunk_data_start..chunk_data_end];
            if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                if null_pos + 2 <= chunk_data.len() {
                    let compressed = &chunk_data[null_pos + 2..]; // skip null + compression method byte
                    if let Ok(decompressed) = decompress_zlib(compressed) {
                        if decompressed.len() >= 128 {
                            return Some(decompressed);
                        }
                    }
                }
            }
        }

        if chunk_type == b"IDAT" {
            break; // stop at image data
        }

        pos = chunk_data_end + 4; // skip CRC
    }

    None
}

fn decompress_zlib(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut decoder = ZlibDecoder::new(data);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::{fast_resize, generate_jpeg_thumbnail};
    use image::{DynamicImage, GenericImageView, Rgb, RgbImage, Rgba, RgbaImage};

    #[test]
    fn simd_lanczos_thumbnail_matches_the_reference_resizer() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_fn(600, 400, |x, y| {
            Rgba([
                (x * 255 / 600) as u8,
                (y * 255 / 400) as u8,
                ((x * 13 + y * 29) & 0xff) as u8,
                255,
            ])
        }));
        let reference = source
            .resize_exact(180, 120, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let resized = fast_resize(&source, 180, 120).unwrap().to_rgba8();
        let (sum, maximum) = reference.as_raw().iter().zip(resized.as_raw()).fold(
            (0_u64, 0_u8),
            |(sum, maximum), (left, right)| {
                let difference = left.abs_diff(*right);
                (sum + u64::from(difference), maximum.max(difference))
            },
        );
        let mean = sum as f64 / reference.as_raw().len() as f64;

        assert!(mean < 1.0, "mean channel difference was {mean}");
        assert!(maximum <= 20, "maximum channel difference was {maximum}");
    }

    #[test]
    fn jpeg_thumbnail_decodes_at_reduced_resolution() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.jpg");
        DynamicImage::ImageRgb8(RgbImage::from_fn(1600, 1200, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        }))
        .save(&path)
        .unwrap();

        let (bytes, extension) = generate_jpeg_thumbnail(&path, (512, 512)).unwrap();
        let thumbnail = image::load_from_memory(&bytes).unwrap();

        assert_eq!(extension, "jpg");
        assert_eq!(thumbnail.dimensions(), (512, 384));
    }
}
