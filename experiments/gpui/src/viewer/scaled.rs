//! Originals decoded at the size they are shown. A JPEG is decoded with a
//! reduced inverse DCT (an eighth, a quarter or half of its size), the
//! smallest that still covers the size asked for, so a 7000 × 9900 scan
//! shown in a window costs a sixteenth of its full decode in time and
//! memory. Other formats have no reduced decode and are decoded whole. The
//! decoder is pure Rust, the same on macOS, Windows and Linux.
use anyhow::{Result, anyhow};
use image::{DynamicImage, ImageDecoder as _};
use std::io::Cursor;
use std::path::Path;

/// The most a reduced decode allocates.
const DECODE_LIMIT: usize = 512 << 20;

/// The file at `path`, at least `size` (width, height) where the format
/// allows it, else whole. `orient` applies the EXIF orientation, as GPUI's
/// image loader does.
pub fn decode(path: &Path, size: (u32, u32), orient: bool) -> Result<DynamicImage> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xff, 0xd8, 0xff])
        && let Some(image) = reduced_jpeg(&bytes, size)?
    {
        let mut image = image;
        if orient {
            let mut header = image::codecs::jpeg::JpegDecoder::new(Cursor::new(&bytes))?;
            image.apply_orientation(header.orientation()?);
        }
        return Ok(image);
    }
    let mut decoder = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    if orient {
        image.apply_orientation(orientation);
    }
    Ok(image)
}

/// A JPEG at the smallest reduced scale that covers `size`, or `None` when
/// it has no reduced decode worth making (a small file, or a pixel format
/// the reduced decoder does not turn into RGB).
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

/// The pixels an image of `natural` size takes fitted to `pane` (points)
/// at `scale_factor`, when that is under half its own size, so a reduced
/// decode can serve it.
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
        // A quarter (400 × 300) covers 350 × 260; an eighth would not.
        let image = decode(&path, (350, 260), true).unwrap();
        assert_eq!((image.width(), image.height()), (400, 300));
        // The colours are the photo's, not noise.
        let pixel = image.to_rgb8().get_pixel(200, 150).0;
        let whole = image::open(&path).unwrap().to_rgb8().get_pixel(800, 600).0;
        assert!(
            pixel.iter().zip(whole).all(|(a, b)| a.abs_diff(b) <= 12),
            "{pixel:?} {whole:?}"
        );
        // Asked for its own size or more, it decodes whole.
        let image = decode(&path, (1600, 1200), true).unwrap();
        assert_eq!((image.width(), image.height()), (1600, 1200));
        // Another format decodes whole.
        let png = folder.path().join("photo.png");
        image::RgbImage::new(64, 32).save(&png).unwrap();
        assert_eq!(decode(&png, (8, 4), true).unwrap().width(), 64);
    }

    #[test]
    fn a_reduced_decode_serves_only_a_view_under_half_the_size() {
        // A 7016 × 9921 scan in a 1200 × 800 pane at 2×: 1132 × 1600 pixels.
        assert_eq!(
            fitted_pixels((7016., 9921.), (1200., 800.), 2.),
            Some((1132, 1600))
        );
        // A 4000 × 3000 photo there needs 2134 × 1600: more than half.
        assert_eq!(fitted_pixels((4000., 3000.), (1200., 800.), 2.), None);
        assert_eq!(fitted_pixels((4000., 3000.), (0., 800.), 2.), None);
    }
}
