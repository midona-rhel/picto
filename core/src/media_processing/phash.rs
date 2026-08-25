use img_hash::{HasherConfig, ImageHash};

/// Hash size per dimension (16x16 = 256-bit hash).
const HASH_SIZE: u32 = 16;
const RESIZE_WIDTH: u32 = HASH_SIZE + 1;
const RESIZE_HEIGHT: u32 = HASH_SIZE;

/// Convert to the luminance bytes used by img_hash 3.2 without first expanding
/// an RGB image into a full-resolution RGBA allocation.
///
/// img_hash 3.2 uses image 0.23 internally. Its float coefficients and
/// truncation are retained deliberately so the new algorithm differs only in
/// the SIMD Lanczos implementation, not in color conversion.
fn legacy_luma_bytes(img: &image::DynamicImage) -> Vec<u8> {
    fn rgb_luma(pixel: &[u8]) -> u8 {
        (0.2126_f32 * pixel[0] as f32 + 0.7152_f32 * pixel[1] as f32 + 0.0722_f32 * pixel[2] as f32)
            as u8
    }

    match img {
        image::DynamicImage::ImageLuma8(buffer) => buffer.as_raw().clone(),
        image::DynamicImage::ImageLumaA8(buffer) => buffer
            .as_raw()
            .chunks_exact(2)
            .map(|pixel| pixel[0])
            .collect(),
        image::DynamicImage::ImageRgb8(buffer) => {
            buffer.as_raw().chunks_exact(3).map(rgb_luma).collect()
        }
        image::DynamicImage::ImageRgba8(buffer) => {
            buffer.as_raw().chunks_exact(4).map(rgb_luma).collect()
        }
        _ => img
            .to_rgba8()
            .as_raw()
            .chunks_exact(4)
            .map(rgb_luma)
            .collect(),
    }
}

/// Generate a perceptual hash for an image from raw bytes (decodes internally).
pub fn compute_phash(image_data: &[u8]) -> Result<ImageHash, image::ImageError> {
    let img = image::load_from_memory(image_data)?;
    compute_phash_from_image(&img)
}

/// Generate a perceptual hash from a pre-decoded image (avoids redundant decode).
pub fn compute_phash_from_image(img: &image::DynamicImage) -> Result<ImageHash, image::ImageError> {
    use fast_image_resize as fr;

    let source = fr::images::Image::from_vec_u8(
        img.width(),
        img.height(),
        legacy_luma_bytes(img),
        fr::PixelType::U8,
    )
    .expect("dynamic image dimensions match its pixel buffer");
    let mut resized = fr::images::Image::new(RESIZE_WIDTH, RESIZE_HEIGHT, fr::PixelType::U8);
    fr::Resizer::new()
        .resize(&source, &mut resized, None)
        .expect("non-zero image dimensions and fixed pHash target are valid");

    let grayscale =
        img_hash::image::GrayImage::from_raw(RESIZE_WIDTH, RESIZE_HEIGHT, resized.into_vec())
            .expect("pHash resize buffer has the requested dimensions");

    let hasher = HasherConfig::new()
        .hash_size(HASH_SIZE, HASH_SIZE)
        .to_hasher();
    Ok(hasher.hash_image(&grayscale))
}

/// Compute phash and return as base64 string for DB storage.
pub fn compute_phash_base64(image_data: &[u8]) -> Result<String, image::ImageError> {
    let hash = compute_phash(image_data)?;
    Ok(hash.to_base64())
}

/// Compute phash from a pre-decoded image and return as base64 string.
pub fn compute_phash_base64_from_image(
    img: &image::DynamicImage,
) -> Result<String, image::ImageError> {
    let hash = compute_phash_from_image(img)?;
    Ok(hash.to_base64())
}

#[cfg(test)]
mod tests {
    use super::{compute_phash_base64, compute_phash_base64_from_image};
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::io::Cursor;

    #[test]
    fn bytes_and_decoded_image_produce_same_hash() {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_fn(16, 16, |x, y| {
            let value = ((x + y) * 8) as u8;
            Rgba([value, 255 - value, value / 2, 255])
        }));

        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("encode png");

        let from_bytes = compute_phash_base64(&bytes).expect("hash from bytes");
        let from_image = compute_phash_base64_from_image(&image).expect("hash from image");
        assert_eq!(from_bytes, from_image);
    }

    #[test]
    fn optimized_hash_is_stable_across_supported_eight_bit_buffers() {
        let rgb = DynamicImage::ImageRgb8(ImageBuffer::from_fn(257, 193, |x, y| {
            image::Rgb([
                ((x * 17 + y * 3) % 256) as u8,
                ((x * 5 + y * 29) % 256) as u8,
                ((x * 11 + y * 7) % 256) as u8,
            ])
        }));
        let rgba = DynamicImage::ImageRgba8(rgb.to_rgba8());

        let rgb_hash = compute_phash_base64_from_image(&rgb).unwrap();
        let rgba_hash = compute_phash_base64_from_image(&rgba).unwrap();

        assert_eq!(rgb_hash, rgba_hash);
        assert_eq!(rgb_hash, "2WQmqwhKigKiAClIC5KggClQChQCRUBBUZAVBRHJTLI=");
    }
}
