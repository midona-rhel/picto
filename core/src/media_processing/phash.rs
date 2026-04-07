use img_hash::{HasherConfig, ImageHash};

/// Hash size per dimension (16x16 = 256-bit hash).
const HASH_SIZE: u32 = 16;

/// Generate a perceptual hash for an image from raw bytes (decodes internally).
pub fn compute_phash(image_data: &[u8]) -> Result<ImageHash, image::ImageError> {
    let img = image::load_from_memory(image_data)?;
    compute_phash_from_image(&img)
}

/// Generate a perceptual hash from a pre-decoded image (avoids redundant decode).
pub fn compute_phash_from_image(
    img: &image::DynamicImage,
) -> Result<ImageHash, image::ImageError> {
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());

    let hasher = HasherConfig::new()
        .hash_size(HASH_SIZE, HASH_SIZE)
        .to_hasher();

    Ok(hasher.hash_image(
        &img_hash::image::RgbaImage::from_raw(w, h, rgba.into_raw())
            .expect("Failed to create image for hashing"),
    ))
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
}
