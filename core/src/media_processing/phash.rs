use img_hash::ImageHash;
use rustdct::DCTplanner;

/// Hash size per dimension (16x16 = 256-bit hash).
const HASH_SIZE: u32 = 16;
const DCT_SIZE: u32 = HASH_SIZE * 4;

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
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pixel| pixel[0])
            .collect(),
        image::DynamicImage::ImageRgb8(buffer) => {
            buffer
                .as_raw()
                .as_chunks::<3>()
                .0
                .iter()
                .map(|pixel| rgb_luma(pixel))
                .collect()
        }
        image::DynamicImage::ImageRgba8(buffer) => {
            buffer
                .as_raw()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| rgb_luma(pixel))
                .collect()
        }
        _ => img
            .to_rgba8()
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| rgb_luma(pixel))
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
    let mut resized = fr::images::Image::new(DCT_SIZE, DCT_SIZE, fr::PixelType::U8);
    fr::Resizer::new()
        .resize(&source, &mut resized, None)
        .expect("non-zero image dimensions and fixed pHash target are valid");

    Ok(perceptual_signature(&resized.into_vec()))
}

fn perceptual_signature(luma: &[u8]) -> ImageHash {
    let global = dct_phash(luma);
    let detail = detail_mask(luma);
    let mut bytes = Vec::with_capacity(global.as_bytes().len() + detail.len());
    bytes.extend_from_slice(global.as_bytes());
    bytes.extend_from_slice(&detail);
    ImageHash::from_bytes(&bytes).expect("fixed global and detail masks are valid")
}

/// Standard low-frequency DCT pHash. The previous implementation accidentally
/// used img_hash's default horizontal-gradient algorithm, so it was a dHash
/// despite the persisted field and duplicate pipeline calling it pHash.
fn dct_phash(luma: &[u8]) -> ImageHash {
    let side = DCT_SIZE as usize;
    debug_assert_eq!(luma.len(), side * side);

    let mut planner = DCTplanner::new();
    let dct = planner.plan_dct2(side);
    let mut rows = vec![0.0_f32; side * side];
    let mut scratch = vec![0.0_f32; side];
    for (source, target) in luma.chunks_exact(side).zip(rows.chunks_exact_mut(side)) {
        let mut input = source.iter().map(|value| *value as f32).collect::<Vec<_>>();
        dct.process_dct2(&mut input, &mut scratch);
        target.copy_from_slice(&scratch);
    }

    let mut coefficients = vec![0.0_f32; side * side];
    let mut column = vec![0.0_f32; side];
    for x in 0..side {
        for y in 0..side {
            column[y] = rows[y * side + x];
        }
        dct.process_dct2(&mut column, &mut scratch);
        for y in 0..side {
            coefficients[y * side + x] = scratch[y];
        }
    }

    let mut low_frequencies = Vec::with_capacity((HASH_SIZE * HASH_SIZE - 1) as usize);
    for y in 0..HASH_SIZE as usize {
        for x in 0..HASH_SIZE as usize {
            if x != 0 || y != 0 {
                low_frequencies.push(coefficients[y * side + x]);
            }
        }
    }
    let middle = low_frequencies.len() / 2;
    low_frequencies.select_nth_unstable_by(middle, f32::total_cmp);
    let median = low_frequencies[middle];

    let mut bytes = vec![0_u8; (HASH_SIZE * HASH_SIZE / 8) as usize];
    let mut bit = 1usize; // Keep the DC bit fixed; it represents average brightness.
    for y in 0..HASH_SIZE as usize {
        for x in 0..HASH_SIZE as usize {
            if x == 0 && y == 0 {
                continue;
            }
            if coefficients[y * side + x] > median {
                bytes[bit / 8] |= 1 << (7 - bit % 8);
            }
            bit += 1;
        }
    }
    ImageHash::from_bytes(&bytes).expect("fixed 256-bit pHash is valid")
}

/// A compact edge-occupancy mask. The 64px low-pass input suppresses encoder
/// noise; the 16x16 cells retain where local structure actually exists.
fn detail_mask(luma: &[u8]) -> Vec<u8> {
    let side = DCT_SIZE as usize;
    let cell_side = side / HASH_SIZE as usize;
    let mut energy = vec![0.0_f32; (HASH_SIZE * HASH_SIZE) as usize];
    for cell_y in 0..HASH_SIZE as usize {
        for cell_x in 0..HASH_SIZE as usize {
            let mut sum = 0.0_f32;
            let mut count = 0_u32;
            let start_x = (cell_x * cell_side).max(1);
            let start_y = (cell_y * cell_side).max(1);
            let end_x = ((cell_x + 1) * cell_side).min(side - 1);
            let end_y = ((cell_y + 1) * cell_side).min(side - 1);
            for y in start_y..end_y {
                for x in start_x..end_x {
                    let left = i16::from(luma[y * side + x - 1]);
                    let right = i16::from(luma[y * side + x + 1]);
                    let top = i16::from(luma[(y - 1) * side + x]);
                    let bottom = i16::from(luma[(y + 1) * side + x]);
                    sum += f32::from((right - left).abs() + (bottom - top).abs());
                    count += 1;
                }
            }
            energy[cell_y * HASH_SIZE as usize + cell_x] = sum / count.max(1) as f32;
        }
    }

    let mut ordered = energy.clone();
    let threshold_index = ordered.len() * 3 / 5;
    ordered.select_nth_unstable_by(threshold_index, f32::total_cmp);
    let threshold = ordered[threshold_index].max(6.0);
    let mut bytes = vec![0_u8; energy.len() / 8];
    for (bit, value) in energy.into_iter().enumerate() {
        if value > threshold {
            bytes[bit / 8] |= 1 << (7 - bit % 8);
        }
    }
    bytes
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
    use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageBuffer, ImageFormat, Rgba};
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
        assert_eq!(rgb_hash, "f0gAFSlIVBUAQKqa/8+on3/Kqp8BQKqaf8+qn39KKJ8GCwIWlhcNJQsMEha0tDg9aFhWUKy0MTFYWPDQ5WDhYQ==");
    }

    #[test]
    fn compression_noise_is_closer_than_a_localized_edit() {
        let original = DynamicImage::ImageRgba8(ImageBuffer::from_fn(512, 384, |x, y| {
            Rgba([
                ((x * 7 + y * 3) % 256) as u8,
                ((x * 2 + y * 11) % 256) as u8,
                ((x * 13 + y * 5) % 256) as u8,
                255,
            ])
        }));
        let original_hash = super::compute_phash_from_image(&original).unwrap();

        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 65)
            .encode_image(&original)
            .unwrap();
        let compressed = image::load_from_memory(&jpeg).unwrap();
        let compressed_hash = super::compute_phash_from_image(&compressed).unwrap();

        let mut edited = original.to_rgba8();
        for y in 120..264 {
            for x in 180..332 {
                edited.put_pixel(x, y, Rgba([250, 20, 180, 255]));
            }
        }
        let edited_hash =
            super::compute_phash_from_image(&DynamicImage::ImageRgba8(edited)).unwrap();

        assert!(
            original_hash.dist(&compressed_hash) < original_hash.dist(&edited_hash),
            "distributed compression noise should be less significant than a localized structural edit"
        );
    }
}
