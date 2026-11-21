//! Paged documents rendered to pictures off the drawing thread: PDF through
//! hayro, DjVu through djvu-rs, and comic archives (a zip of page images).
//! Each open document lives on a thread of its own, which measures every
//! page when it opens and then draws the pages the viewer asks for, in the
//! order asked, skipping any request the viewer has since given up.
use super::geometry::PageSize;
use super::kind::RendererKind;
use anyhow::{Context as _, Result, anyhow, bail};
use futures::channel::oneshot;
use image::RgbaImage;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

/// A document whose pages can be measured and drawn.
pub trait PageSource {
    fn page_count(&self) -> usize;
    /// A page's natural size: PDF pages in points (CSS pixels at 100%),
    /// DjVu pages and comic pages in pixels.
    fn page_size(&mut self, index: usize) -> Result<PageSize>;
    /// Draws a page at exactly `width` × `height` pixels.
    fn render(&mut self, index: usize, width: u32, height: u32) -> Result<RgbaImage>;
}

/// Opens the paged document of this kind.
pub fn open(kind: RendererKind, path: &Path) -> Result<Box<dyn PageSource>> {
    Ok(match kind {
        RendererKind::Pdf => Box::new(Pdf::open(path)?),
        RendererKind::Djvu => Box::new(Djvu::open(path)?),
        RendererKind::Cbz => Box::new(Comic::open(path)?),
        other => bail!("{other:?} is not a paged document"),
    })
}

// ── PDF ─────────────────────────────────────────────────────────

pub struct Pdf {
    pdf: hayro::hayro_syntax::Pdf,
}

impl Pdf {
    pub fn open(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).context("read the PDF")?;
        let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|error| match error {
            hayro::hayro_syntax::LoadPdfError::Decryption(_) => {
                anyhow!("This PDF is encrypted.")
            }
            // i18n: an error the document view shows through t()
            hayro::hayro_syntax::LoadPdfError::Invalid => anyhow!("Could not open this PDF."),
        })?;
        if pdf.pages().is_empty() {
            bail!("This PDF has no pages.");
        }
        Ok(Self { pdf })
    }
}

impl PageSource for Pdf {
    fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    fn page_size(&mut self, index: usize) -> Result<PageSize> {
        let page = self
            .pdf
            .pages()
            .get(index)
            .ok_or_else(|| anyhow!("No page {}", index + 1))?;
        Ok(page.render_dimensions())
    }

    fn render(&mut self, index: usize, width: u32, height: u32) -> Result<RgbaImage> {
        let page = self
            .pdf
            .pages()
            .get(index)
            .ok_or_else(|| anyhow!("No page {}", index + 1))?;
        let (natural_width, natural_height) = page.render_dimensions();
        let cache = hayro::RenderCache::new();
        let pixmap = hayro::render(
            page,
            &cache,
            &hayro::hayro_interpret::InterpreterSettings::default(),
            &hayro::RenderSettings::default(),
            &hayro::PixmapSettings {
                // hayro truncates the scaled size; a quarter pixel more
                // makes it land on the size asked for.
                x_scale: (width as f32 + 0.25) / natural_width,
                y_scale: (height as f32 + 0.25) / natural_height,
                // `.page { background: #fff }`
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            },
        );
        let (out_width, out_height) = (pixmap.width() as u32, pixmap.height() as u32);
        // On an opaque white page premultiplied and straight alpha agree.
        RgbaImage::from_raw(out_width, out_height, pixmap.data_as_u8_slice().to_vec())
            // i18n: an error the document view shows through t()
            .ok_or_else(|| anyhow!("Could not render this PDF page."))
    }
}

// ── DjVu ────────────────────────────────────────────────────────

pub struct Djvu {
    document: djvu_rs::Document,
}

impl Djvu {
    pub fn open(path: &Path) -> Result<Self> {
        let document = djvu_rs::Document::open(path)
            .map_err(|error| anyhow!("Could not open this DjVu document: {error}"))?;
        if document.page_count() == 0 {
            bail!("This DjVu document has no pages.");
        }
        Ok(Self { document })
    }
}

impl PageSource for Djvu {
    fn page_count(&self) -> usize {
        self.document.page_count()
    }

    /// The page at `max(96, dpi)`, as production renders it.
    fn page_size(&mut self, index: usize) -> Result<PageSize> {
        let page = self
            .document
            .page(index)
            .map_err(|error| anyhow!("{error}"))?;
        let dpi = page.dpi().max(96) as f32;
        let (width, height) = page.size_at_dpi(dpi);
        Ok((width as f32, height as f32))
    }

    fn render(&mut self, index: usize, width: u32, height: u32) -> Result<RgbaImage> {
        let page = self
            .document
            .page(index)
            .map_err(|error| anyhow!("{error}"))?;
        let pixmap = page
            .render_to_size(width, height)
            .map_err(|error| anyhow!("Could not render this DjVu page: {error}"))?;
        RgbaImage::from_raw(pixmap.width, pixmap.height, pixmap.data)
            // i18n: an error the document view shows through t()
            .ok_or_else(|| anyhow!("Could not render this DjVu page."))
    }
}

// ── Comic archives ──────────────────────────────────────────────

/// `IMAGE_EXTENSIONS` in `ArchiveDocumentViewer.tsx`.
const COMIC_EXTENSIONS: [&str; 8] = ["avif", "bmp", "gif", "jpeg", "jpg", "png", "svg", "webp"];

pub struct Comic {
    archive: zip::ZipArchive<std::fs::File>,
    /// The archive's image entries in reading order.
    pages: Vec<String>,
    /// The last decoded page, kept so a size query and the render that
    /// follows it decode once.
    decoded: Option<(usize, Arc<RgbaImage>)>,
}

impl Comic {
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).context("read the comic archive")?;
        let archive =
            zip::ZipArchive::new(file).map_err(|_| anyhow!("Could not open this CBZ."))?;
        let pages = comic_pages(archive.file_names());
        if pages.is_empty() {
            bail!("This CBZ has no readable pages.");
        }
        Ok(Self {
            archive,
            pages,
            decoded: None,
        })
    }

    fn decode(&mut self, index: usize) -> Result<Arc<RgbaImage>> {
        if let Some((decoded, image)) = &self.decoded
            && *decoded == index
        {
            return Ok(image.clone());
        }
        let name = self
            .pages
            .get(index)
            .ok_or_else(|| anyhow!("No page {}", index + 1))?
            .clone();
        let bytes = read_entry(&mut self.archive, &name, ENTRY_LIMIT)
            .map_err(|_| anyhow!("Could not read page {}", index + 1))?;
        let image = Arc::new(decode_picture(&name, &bytes)?);
        self.decoded = Some((index, image.clone()));
        Ok(image)
    }
}

/// The image entries of an archive, in natural order, ignoring case
/// (`localeCompare` with `numeric: true, sensitivity: 'base'`).
pub fn comic_pages<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut pages: Vec<String> = names
        .filter(|name| !name.ends_with('/'))
        .filter(|name| {
            name.rsplit_once('.').is_some_and(|(_, extension)| {
                COMIC_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            })
        })
        .map(str::to_owned)
        .collect();
    pages.sort_by(|left, right| crate::text::natural_order(left, right));
    pages
}

/// The most of an archive entry read: comic pages and EPUB chapters and
/// pictures come from the internet, and an entry can inflate to far more
/// than its archive holds.
pub const ENTRY_LIMIT: u64 = 64 << 20;
/// What is read of a page to find its size in its header.
const HEADER_LIMIT: u64 = 64 << 10;

/// An archive entry's bytes, up to `limit`; a longer entry is an error, not
/// its first part.
pub fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let entry = archive
        .by_name(name)
        .map_err(|_| anyhow!("{name} is missing"))?;
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("{name} is too large");
    }
    Ok(bytes)
}

/// The largest picture decoded from an archive, in pixels a side and bytes
/// decoded, so a small file that claims a vast picture is refused rather
/// than allocated.
const PICTURE_SIDE: u32 = 16_384;
const PICTURE_BYTES: u64 = 512 << 20;

/// Decodes a page picture: raster formats through `image`, SVG through
/// resvg at its own size, shrunk to [`MAX_RENDER_SIDE`] at most.
pub fn decode_picture(name: &str, bytes: &[u8]) -> Result<RgbaImage> {
    if name.to_ascii_lowercase().ends_with(".svg") {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
            .map_err(|error| anyhow!("Could not read this SVG page: {error}"))?;
        let natural = tree.size();
        let scale = (MAX_RENDER_SIDE as f32 / natural.width().max(natural.height())).min(1.);
        let size = natural
            .to_int_size()
            .scale_by(scale)
            .ok_or_else(|| anyhow!("This SVG page is empty."))?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
            .ok_or_else(|| anyhow!("This SVG page is empty."))?;
        let transform = resvg::tiny_skia::Transform::from_scale(
            size.width() as f32 / natural.width(),
            size.height() as f32 / natural.height(),
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        let mut data = pixmap.take();
        // tiny-skia's pixels are premultiplied.
        for pixel in data.chunks_exact_mut(4) {
            let alpha = pixel[3] as u32;
            if alpha > 0 && alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        return RgbaImage::from_raw(size.width(), size.height(), data)
            .ok_or_else(|| anyhow!("Could not render this SVG page."));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(PICTURE_SIDE);
    limits.max_image_height = Some(PICTURE_SIDE);
    limits.max_alloc = Some(PICTURE_BYTES);
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits);
    Ok(reader
        .decode()
        .map_err(|error| anyhow!("Could not decode {name}: {error}"))?
        .to_rgba8())
}

impl PageSource for Comic {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Read from the picture's header, so measuring a whole archive does
    /// not decode every page.
    fn page_size(&mut self, index: usize) -> Result<PageSize> {
        if let Some((decoded, image)) = &self.decoded
            && *decoded == index
        {
            return Ok((image.width() as f32, image.height() as f32));
        }
        let name = self
            .pages
            .get(index)
            .ok_or_else(|| anyhow!("No page {}", index + 1))?
            .clone();
        let unreadable = || anyhow!("Could not read page {}", index + 1);
        let mut head = Vec::new();
        self.archive
            .by_name(&name)
            .map_err(|_| unreadable())?
            .take(HEADER_LIMIT)
            .read_to_end(&mut head)?;
        if let Ok((width, height)) = image::ImageReader::new(Cursor::new(head))
            .with_guessed_format()
            .map_err(anyhow::Error::from)
            .and_then(|reader| Ok(reader.into_dimensions()?))
        {
            return Ok((width as f32, height as f32));
        }
        let bytes = read_entry(&mut self.archive, &name, ENTRY_LIMIT).map_err(|_| unreadable())?;
        let image = decode_picture(&name, &bytes)?;
        Ok((image.width() as f32, image.height() as f32))
    }

    fn render(&mut self, index: usize, width: u32, height: u32) -> Result<RgbaImage> {
        let image = self.decode(index)?;
        if (image.width(), image.height()) == (width, height) {
            return Ok((*image).clone());
        }
        Ok(image::imageops::resize(
            &*image,
            width.max(1),
            height.max(1),
            image::imageops::FilterType::CatmullRom,
        ))
    }
}

// ── JPEG XL ─────────────────────────────────────────────────────

/// A JPEG XL image decoded by jxl-oxide, the decoder `core/` makes its
/// thumbnails with.
pub fn decode_jpeg_xl(path: &Path) -> Result<RgbaImage> {
    super::guarded(|| decode_jpeg_xl_unguarded(path))
}

fn decode_jpeg_xl_unguarded(path: &Path) -> Result<RgbaImage> {
    let file = std::fs::File::open(path).context("read the JPEG XL image")?;
    let decoder = jxl_oxide::integration::JxlDecoder::new(file)
        .map_err(|error| anyhow!("Could not open this JPEG XL image: {error}"))?;
    Ok(image::DynamicImage::from_decoder(decoder)
        .map_err(|error| anyhow!("Could not decode this JPEG XL image: {error}"))?
        .to_rgba8())
}

// ── The document's thread ───────────────────────────────────────

/// A drawn page: its pixels, and the size it was asked for.
pub struct RenderedPage {
    pub pixels: (u32, u32),
    pub image: RgbaImage,
}

struct Request {
    index: usize,
    pixels: (u32, u32),
    reply: oneshot::Sender<Result<RenderedPage>>,
}

/// An open paged document on its own thread. Dropping it ends the thread.
pub struct PageWorker {
    requests: mpsc::Sender<Request>,
}

/// The largest side a page is drawn at, to bound memory at high zoom.
pub const MAX_RENDER_SIDE: u32 = 8192;

/// Every page's natural size. A page that cannot be measured takes the
/// size of the one before it, so one bad page does not hide the rest.
pub fn measure_pages(source: &mut dyn PageSource) -> Result<Vec<PageSize>> {
    let mut sizes: Vec<PageSize> = Vec::with_capacity(source.page_count());
    for index in 0..source.page_count() {
        match source.page_size(index) {
            Ok(size) if size.0 > 0. && size.1 > 0. => sizes.push(size),
            other => match sizes.last() {
                Some(previous) => sizes.push(*previous),
                None => {
                    other?;
                    bail!("The first page has no size.");
                }
            },
        }
    }
    if sizes.is_empty() {
        bail!("This document has no pages.");
    }
    Ok(sizes)
}

/// Opens a paged document and measures its pages, for a view that lays
/// the pages out before it shows them.
pub fn measure(kind: RendererKind, path: &Path) -> Result<Vec<PageSize>> {
    super::guarded(|| measure_pages(open(kind, path)?.as_mut()))
}

impl PageWorker {
    /// Opens the document on a new thread; the answer is the worker and every
    /// page's natural size, or why the document cannot be shown.
    pub fn open(
        kind: RendererKind,
        path: PathBuf,
    ) -> oneshot::Receiver<Result<(PageWorker, Vec<PageSize>)>> {
        let (opened, answer) = oneshot::channel();
        let (requests, inbox) = mpsc::channel::<Request>();
        let spawned = std::thread::Builder::new()
            .name("picto-document".into())
            .spawn(move || {
                let measured = open(kind, &path).and_then(|mut source| {
                    let sizes = measure_pages(source.as_mut())?;
                    Ok((source, sizes))
                });
                let (mut source, sizes) = match measured {
                    Ok(measured) => measured,
                    Err(error) => {
                        let _ = opened.send(Err(error));
                        return;
                    }
                };
                if opened.send(Ok((PageWorker { requests }, sizes))).is_err() {
                    return;
                }
                let mut queue = std::collections::VecDeque::new();
                loop {
                    if queue.is_empty() {
                        match inbox.recv() {
                            Ok(request) => queue.push_back(request),
                            Err(_) => return,
                        }
                    }
                    queue.extend(inbox.try_iter());
                    let Some(request) = queue.pop_front() else {
                        continue;
                    };
                    // The viewer gave this page up (scrolled away, zoomed,
                    // closed): it is not drawn.
                    if request.reply.is_canceled() {
                        continue;
                    }
                    let result = render_guarded(source.as_mut(), request.index, request.pixels);
                    let _ = request.reply.send(result);
                }
            });
        if let Err(error) = spawned {
            log_spawn_failure(&error);
        }
        answer
    }

    /// Asks for a page at `pixels`; the answer comes when it is drawn, or is
    /// dropped unanswered when the receiver is dropped first.
    pub fn render(
        &self,
        index: usize,
        pixels: (u32, u32),
    ) -> oneshot::Receiver<Result<RenderedPage>> {
        let (reply, answer) = oneshot::channel();
        let _ = self.requests.send(Request {
            index,
            pixels,
            reply,
        });
        answer
    }
}

fn log_spawn_failure(error: &std::io::Error) {
    eprintln!("picto: could not start a document thread: {error}");
}

/// Draws a page, turning a decoder's panic into an error for that page, so
/// the document's thread lives on and the other pages still draw.
fn render_guarded(
    source: &mut dyn PageSource,
    index: usize,
    pixels: (u32, u32),
) -> Result<RenderedPage> {
    let (width, height) = bound_render_size(pixels.0, pixels.1);
    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        source.render(index, width, height)
    }));
    match drawn {
        Ok(image) => Ok(RenderedPage {
            pixels,
            image: image?,
        }),
        Err(_) => Err(anyhow!("Could not render page {}.", index + 1)),
    }
}

/// Shrinks a render size, keeping its proportions, so neither side passes
/// [`MAX_RENDER_SIDE`].
pub fn bound_render_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height).max(1);
    if longest <= MAX_RENDER_SIDE {
        return (width.max(1), height.max(1));
    }
    let scale = MAX_RENDER_SIDE as f32 / longest as f32;
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RENDER_SIDE, PageSource, bound_render_size, comic_pages, decode_picture, measure_pages,
        read_entry, render_guarded,
    };
    use anyhow::{Result, bail};
    use image::RgbaImage;

    /// Three pages; the second cannot be measured and panics when drawn.
    struct Flaky;

    impl PageSource for Flaky {
        fn page_count(&self) -> usize {
            3
        }

        fn page_size(&mut self, index: usize) -> Result<(f32, f32)> {
            match index {
                1 => bail!("broken page"),
                _ => Ok((100. + index as f32, 200.)),
            }
        }

        fn render(&mut self, index: usize, width: u32, height: u32) -> Result<RgbaImage> {
            assert_ne!(index, 1, "the decoder fails on this page");
            Ok(RgbaImage::new(width, height))
        }
    }

    #[test]
    fn one_bad_page_neither_hides_nor_stops_the_others() {
        let mut source = Flaky;
        // The page that cannot be measured takes the size before it.
        assert_eq!(
            measure_pages(&mut source).unwrap(),
            vec![(100., 200.), (100., 200.), (102., 200.)]
        );
        // A page whose decoder panics fails alone; the next still draws.
        assert!(render_guarded(&mut source, 1, (10, 20)).is_err());
        let next = render_guarded(&mut source, 2, (10, 20)).unwrap();
        assert_eq!((next.image.width(), next.image.height()), (10, 20));
    }
    use std::cmp::Ordering;

    #[test]
    fn comic_pages_are_images_in_natural_order() {
        let pages = comic_pages(
            [
                "Page 10.png",
                "page 2.JPG",
                "notes.txt",
                "folder/",
                "Page 1.webp",
                "cover.svg",
            ]
            .into_iter(),
        );
        assert_eq!(
            pages,
            ["cover.svg", "Page 1.webp", "page 2.JPG", "Page 10.png"]
        );
        assert_eq!(crate::text::natural_order("a02", "a2"), Ordering::Equal);
        assert_eq!(crate::text::natural_order("a9", "a10"), Ordering::Less);
    }

    #[test]
    fn render_sizes_are_bounded() {
        assert_eq!(bound_render_size(1000, 500), (1000, 500));
        assert_eq!(bound_render_size(16384, 8192), (8192, 4096));
        assert_eq!(bound_render_size(0, 0), (1, 1));
    }

    #[test]
    fn a_picture_that_claims_a_vast_size_is_refused_or_shrunk() {
        // A GIF of a few bytes that claims 65535 pixels a side.
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0]);
        gif.extend_from_slice(&[0x2c, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 2, 0, 0x3b]);
        assert!(decode_picture("page.gif", &gif).is_err());
        // An SVG page as large is drawn at the largest render size.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1000000" height="500000"><rect width="10" height="10"/></svg>"#;
        let page = decode_picture("page.svg", svg).unwrap();
        assert_eq!(
            (page.width(), page.height()),
            (MAX_RENDER_SIDE, MAX_RENDER_SIDE / 2)
        );
    }

    #[test]
    fn an_archive_entry_past_its_limit_is_refused() {
        use std::io::Write;
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("page.png", options).unwrap();
        archive.write_all(&vec![0; 1 << 16]).unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(
            read_entry(&mut archive, "page.png", 1 << 16).unwrap().len(),
            1 << 16
        );
        assert!(read_entry(&mut archive, "page.png", (1 << 16) - 1).is_err());
        assert!(read_entry(&mut archive, "../page.png", 1 << 16).is_err());
    }
}
