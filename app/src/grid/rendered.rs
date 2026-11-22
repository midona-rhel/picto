//! Thumbnails that the application's media worker cannot make and that this
//! app draws itself: the first page of a PDF, of a DjVu document or of a
//! comic archive, a Flash movie after it has played for five seconds, the
//! first page of a text, Markdown, JSON or RTF document, and the first
//! chapter of an EPUB as the document viewer shows it. Each thumbnail is
//! written into the folder of the library's own thumbnails as `<hash>.png`.
//! The grid looks for it there, and the application's own thumbnail work
//! finds it there and stops retrying.
use crate::viewer::document::kind::{RendererKind, renderer_kind};
use anyhow::{Result, anyhow};
use std::path::Path;
use std::time::{Duration, Instant};

/// The side, in points, of the square that a thumbnail's picture is scaled
/// to fit. The picture is never enlarged to more than `MAX_SCALE` times its
/// size, and it is drawn with `PIXELS_PER_POINT` pixels for each point.
const BOX: f32 = 800.;
const MAX_SCALE: f32 = 2.;
const PIXELS_PER_POINT: f32 = 2.;
/// How long a Flash movie plays before its picture is taken, and how long
/// the app waits for a movie that draws nothing before it gives up.
const FLASH_SETTLE: Duration = Duration::from_millis(5_000);
const FLASH_TIMEOUT: Duration = Duration::from_millis(20_000);

/// Whether this app draws the thumbnail of a file of the type `mime` itself.
pub fn draws(mime: &str) -> bool {
    mime == "application/x-shockwave-flash"
        || matches!(
            renderer_kind(mime),
            RendererKind::Pdf
                | RendererKind::Djvu
                | RendererKind::Cbz
                | RendererKind::TextDocument
                | RendererKind::Epub
        )
}

/// Returns the size in pixels at which a page of `width` × `height` points
/// is drawn.
pub fn page_pixels(width: f32, height: f32) -> (u32, u32) {
    let scale = (BOX / width).min(BOX / height).min(MAX_SCALE) * PIXELS_PER_POINT;
    (
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    )
}

/// Draws the thumbnail of `original`, a file of the type `mime`, and writes
/// it to `output` as a PNG. The call blocks for as long as the drawing
/// takes, so it must run on a background thread.
pub fn render(mime: &str, original: &Path, output: &Path) -> Result<()> {
    use crate::viewer::document::{epub, page_picture, reader};
    let picture = if mime == "application/x-shockwave-flash" {
        flash(original)?
    } else if renderer_kind(mime) == RendererKind::TextDocument {
        let (blocks, _) = reader::load_text(original, mime)?;
        page_picture::draw(&blocks, reader::PageStyle::Text)
    } else if renderer_kind(mime) == RendererKind::Epub {
        // The thumbnail shows the first chapter of the book's spine, which is
        // the chapter the book viewer opens on.
        let first = epub::chapters(original)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("the book has no chapters"))?;
        page_picture::draw(&epub::chapter(original, &first)?, reader::PageStyle::Book)
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
    // The picture is written to a temporary file, which is renamed to the
    // thumbnail's name once it is complete. A temporary file that could not
    // be completed is deleted.
    let partial = output.with_extension(format!("{}.tmp", std::process::id()));
    let saved = picture
        .save_with_format(&partial, image::ImageFormat::Png)
        .map_err(anyhow::Error::from)
        .and_then(|()| Ok(std::fs::rename(&partial, output)?));
    if saved.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    saved
}

/// Returns the picture of a Flash movie once it has played, muted, for
/// `FLASH_SETTLE`.
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
                // The picture's size in points is its size in pixels divided by
                // `PIXELS_PER_POINT`. The picture is scaled down to the size
                // at which a page of that many points is drawn, and it is
                // never enlarged.
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
    use super::page_pixels;

    #[test]
    fn pages_fit_the_box_at_two_pixels_a_point() {
        // A US letter page of 612 × 792 points is scaled to 800 points high,
        // which is 1600 pixels.
        assert_eq!(page_pixels(612., 792.), (1236, 1600));
        // A small page is enlarged to at most twice its size in points.
        assert_eq!(page_pixels(100., 50.), (400, 200));
    }
}
