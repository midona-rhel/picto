//! Thumbnails the application's media worker cannot make, drawn here as the
//! Electron main process draws them (`flashThumbnailService.mjs`,
//! `pdfThumbnailService.mjs`): the first page of a PDF, DjVu or comic
//! archive, and a Flash movie five seconds in. Each is written beside the
//! library's own thumbnails as `<hash>.png`, where the media protocol and
//! this grid look for it.
use crate::viewer::document::kind::{RendererKind, renderer_kind};
use anyhow::{Result, anyhow};
use std::path::Path;
use std::time::{Duration, Instant};

/// `pdfThumbnail.ts` and `flashThumbnailService`: the picture fits 800 ×
/// 800 points, never enlarged past twice its size, drawn at the hidden
/// window's two pixels a point.
const BOX: f32 = 800.;
const MAX_SCALE: f32 = 2.;
const PIXELS_PER_POINT: f32 = 2.;
/// `FLASH_THUMBNAIL_SETTLE_MS` and `FLASH_THUMBNAIL_TIMEOUT_MS`.
const FLASH_SETTLE: Duration = Duration::from_millis(5_000);
const FLASH_TIMEOUT: Duration = Duration::from_millis(20_000);

/// Whether this app draws thumbnails of this type itself.
pub fn draws(mime: &str) -> bool {
    mime == "application/x-shockwave-flash"
        || matches!(
            renderer_kind(mime),
            RendererKind::Pdf | RendererKind::Djvu | RendererKind::Cbz
        )
}

/// The size a page of `width` × `height` points is drawn at, in pixels.
pub fn page_pixels(width: f32, height: f32) -> (u32, u32) {
    let scale = (BOX / width).min(BOX / height).min(MAX_SCALE) * PIXELS_PER_POINT;
    (
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    )
}

/// Draws the thumbnail of `original` (of type `mime`) to `output`, a
/// `.png`. Blocks for as long as the drawing takes: run it off the drawing
/// thread.
pub fn render(mime: &str, original: &Path, output: &Path) -> Result<()> {
    let picture = if mime == "application/x-shockwave-flash" {
        flash(original)?
    } else {
        let mut pages = crate::viewer::document::pages::open(renderer_kind(mime), original)?;
        let (width, height) = pages.page_size(0)?;
        let (width, height) = page_pixels(width, height);
        pages.render(0, width, height)?
    };
    let directory = output
        .parent()
        .ok_or_else(|| anyhow!("no thumbnail folder"))?;
    std::fs::create_dir_all(directory)?;
    // Written whole before it takes the thumbnail's name.
    let partial = output.with_extension(format!("{}.tmp", std::process::id()));
    picture.save_with_format(&partial, image::ImageFormat::Png)?;
    std::fs::rename(&partial, output)?;
    Ok(())
}

/// The movie's picture once it has played, muted, for the settling time.
fn flash(original: &Path) -> Result<image::RgbaImage> {
    let mut movie = crate::viewer::flash::Flash::open(original.to_path_buf());
    movie.toggle_mute();
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let frame = movie.frame.lock().unwrap().clone();
        match frame {
            Some(Err(error)) => return Err(anyhow!(error)),
            Some(Ok(picture)) if started.elapsed() >= FLASH_SETTLE => {
                // The stage's own size within the box, at two pixels a point.
                let (width, height) = page_pixels(
                    picture.width() as f32 / PIXELS_PER_POINT,
                    picture.height() as f32 / PIXELS_PER_POINT,
                );
                return Ok(image::imageops::resize(
                    &picture,
                    width.min(picture.width()),
                    height.min(picture.height()),
                    image::imageops::FilterType::Triangle,
                ));
            }
            _ if started.elapsed() >= FLASH_TIMEOUT => {
                return Err(anyhow!("the Flash movie drew nothing"));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{draws, page_pixels};

    #[test]
    fn pages_fit_the_box_at_two_pixels_a_point() {
        // A US letter page: 612 × 792 points fits 800 high.
        assert_eq!(page_pixels(612., 792.), (1236, 1600));
        // A tiny page is enlarged at most twice.
        assert_eq!(page_pixels(100., 50.), (400, 200));
    }

    #[test]
    fn the_types_drawn_here_are_those_the_media_worker_leaves() {
        assert!(draws("application/x-shockwave-flash"));
        assert!(draws("application/pdf"));
        assert!(!draws("image/png"));
        assert!(!draws("video/mp4"));
    }
}
