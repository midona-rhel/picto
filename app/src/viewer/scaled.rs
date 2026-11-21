//! Decodes an original image at the size it is shown at. A JPEG is decoded
//! with a reduced inverse DCT, at an eighth, a quarter or half of its size:
//! the smallest of these that still covers the size asked for. A 7000 × 9900
//! scan shown in a window then takes a sixteenth of the time and memory of
//! its full decode. Other formats have no reduced decode and are decoded
//! whole. The decoder is written in Rust only, so it is the same on macOS,
//! Windows and Linux.
use anyhow::{Result, anyhow};
use image::{DynamicImage, ImageDecoder as _};
use std::io::Cursor;
use std::path::Path;

/// The largest number of bytes that a reduced decode may allocate.
const DECODE_LIMIT: usize = 512 << 20;

/// Decodes the file at `path`. Where the format has a reduced decode, the
/// image is at least `size` (width, height) pixels; otherwise it is decoded
/// whole. `orient` applies the EXIF orientation, as GPUI's image loader
/// does.
pub fn decode(path: &Path, size: (u32, u32), orient: bool) -> Result<DynamicImage> {
    decode_bytes(&std::fs::read(path)?, size, orient)
}

/// The same as [`decode`], for a file whose bytes have already been read.
pub fn decode_bytes(bytes: &[u8], size: (u32, u32), orient: bool) -> Result<DynamicImage> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff])
        && let Some(image) = reduced_jpeg(bytes, size)?
    {
        let mut image = image;
        if orient {
            let mut header = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))?;
            image.apply_orientation(header.orientation()?);
        }
        return Ok(image);
    }
    let mut decoder = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    if orient {
        image.apply_orientation(orientation);
    }
    Ok(image)
}

/// Decodes a JPEG at the smallest reduced scale that covers `size`. Returns
/// `None` when the smallest scale that covers `size` is the full size, or
/// when the JPEG has a pixel format that the reduced decoder does not turn
/// into RGB.
fn reduced_jpeg(bytes: &[u8], size: (u32, u32)) -> Result<Option<DynamicImage>> {
    use jpeg_decoder::{Decoder, PixelFormat};
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_max_decoding_buffer_size(DECODE_LIMIT);
    decoder.read_info()?;
    let info = decoder
        .info()
        .ok_or_else(|| anyhow!("The JPEG has no header."))?;
    if !matches!(info.pixel_format, PixelFormat::RGB24 | PixelFormat::L8) {
        return Ok(None);
    }
    let requested = (
        size.0.clamp(1, u16::MAX as u32) as u16,
        size.1.clamp(1, u16::MAX as u32) as u16,
    );
    let (width, height) = decoder.scale(requested.0, requested.1)?;
    if (width, height) == (info.width, info.height) {
        return Ok(None);
    }
    let pixels = decoder.decode()?;
    let (width, height) = (width as u32, height as u32);
    let image = match info.pixel_format {
        PixelFormat::RGB24 => {
            image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::from)
        }
        _ => image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::from),
    };
    image
        .map(Some)
        .ok_or_else(|| anyhow!("The JPEG decoded to fewer pixels than it holds."))
}

/// Returns the size in pixels that an image of `natural` size takes when it
/// is fitted to `pane` (in points) at `scale_factor`. Returns `None` unless
/// that size is at most half of the image's own size in both directions,
/// because only then can a reduced decode supply it.
pub fn fitted_pixels(
    natural: (f32, f32),
    pane: (f32, f32),
    scale_factor: f32,
) -> Option<(u32, u32)> {
    if scale_factor <= 0. {
        return None;
    }
    let (_, _, width, height) = super::camera::contain(natural, pane)?;
    let pixels = (
        (width * scale_factor).ceil() as u32,
        (height * scale_factor).ceil() as u32,
    );
    (pixels.0 as f32 <= natural.0 / 2. && pixels.1 as f32 <= natural.1 / 2.).then_some(pixels)
}

#[cfg(test)]
mod tests {
    use super::{decode, fitted_pixels};

    fn photo(folder: &std::path::Path) -> std::path::PathBuf {
        let path = folder.join("photo.jpg");
        image::RgbImage::from_fn(1600, 1200, |x, y| {
            image::Rgb([(x / 7) as u8, (y / 5) as u8, 128])
        })
        .save(&path)
        .unwrap();
        path
    }

    #[test]
    fn a_jpeg_decodes_at_the_smallest_scale_that_covers_the_size() {
        let folder = tempfile::tempdir().unwrap();
        let path = photo(folder.path());
        // A quarter of the size (400 × 300) covers 350 × 260, and an eighth
        // would not.
        let image = decode(&path, (350, 260), true).unwrap();
        assert_eq!((image.width(), image.height()), (400, 300));
        // The colours are those of the photo: the pixel in the middle is close
        // to the same pixel of the whole decode.
        let pixel = image.to_rgb8().get_pixel(200, 150).0;
        let whole = image::open(&path).unwrap().to_rgb8().get_pixel(800, 600).0;
        assert!(
            pixel.iter().zip(whole).all(|(a, b)| a.abs_diff(b) <= 12),
            "{pixel:?} {whole:?}"
        );
        // When the size asked for is the image's own size or more, the image
        // is decoded whole.
        let image = decode(&path, (1600, 1200), true).unwrap();
        assert_eq!((image.width(), image.height()), (1600, 1200));
        // A format other than JPEG is decoded whole.
        let png = folder.path().join("photo.png");
        image::RgbImage::new(64, 32).save(&png).unwrap();
        assert_eq!(decode(&png, (8, 4), true).unwrap().width(), 64);
    }

    #[test]
    fn a_reduced_decode_serves_only_a_view_under_half_the_size() {
        // A 7016 × 9921 scan in a 1200 × 800 pane at 2× takes 1132 × 1600
        // pixels.
        assert_eq!(
            fitted_pixels((7016., 9921.), (1200., 800.), 2.),
            Some((1132, 1600))
        );
        // A 4000 × 3000 photo in the same pane needs 2134 × 1600 pixels, which
        // is more than half of its size.
        assert_eq!(fitted_pixels((4000., 3000.), (1200., 800.), 2.), None);
        assert_eq!(fitted_pixels((4000., 3000.), (0., 800.), 2.), None);
    }
}
