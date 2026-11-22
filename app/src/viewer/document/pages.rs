//! Opens paged documents and draws their pages as pictures, on other threads
//! than the one that draws the interface. A PDF is read with hayro and a
//! DjVu file with djvu-rs. A comic archive is a zip file of page images.
//! Each open document has a thread of its own (`PageWorker`). That thread
//! measures every page when it opens the document. It then draws the pages
//! that the viewer asks for, in the order in which they were asked for, and
//! does not draw a page whose request the viewer has dropped since.
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
    /// Returns the natural size of page `index`, counted from 0: for a PDF
    /// in points, each of which is drawn as one logical pixel at a scale
    /// of 100%, and for a DjVu file or a comic archive in pixels.
    fn page_size(&mut self, index: usize) -> Result<PageSize>;
    /// Draws page `index`, counted from 0, as a picture of exactly `width`
    /// × `height` pixels.
    fn render(&mut self, index: usize, width: u32, height: u32) -> Result<RgbaImage>;
}

/// Opens the file at `path` as a paged document of `kind`. It fails when
/// `kind` is not PDF, DjVu or comic archive, and when the file cannot be
/// opened as a document of that kind.
pub fn open(kind: RendererKind, path: &Path) -> Result<Box<dyn PageSource>> {
    Ok(match kind {
        RendererKind::Pdf => Box::new(Pdf::open(path)?),
        RendererKind::Djvu => Box::new(Djvu::open(path)?),
        RendererKind::Cbz => Box::new(Comic::open(path)?),
        other => bail!("{other:?} is not a paged document"),
    })
}

// The following opens and draws PDF files, with hayro.

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
            // i18n: the paged viewer translates this error where it draws it
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
                // hayro rounds the scaled size down to whole pixels. A
                // quarter of a pixel is added so that the picture is
                // exactly `width` × `height` pixels.
                x_scale: (width as f32 + 0.25) / natural_width,
                y_scale: (height as f32 + 0.25) / natural_height,
                // The page is drawn on white, the colour of the rectangle
                // that the viewer shows for a page without a drawing.
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            },
        );
        let (out_width, out_height) = (pixmap.width() as u32, pixmap.height() as u32);
        // hayro's pixels have their colours multiplied by their alpha.
        // Every pixel of a page drawn on white is opaque, and an opaque
        // pixel is the same in both forms, so the pixels are used as they
        // are.
        RgbaImage::from_raw(out_width, out_height, pixmap.data_as_u8_slice().to_vec())
            // i18n: the paged viewer translates this error where it draws it
            .ok_or_else(|| anyhow!("Could not render this PDF page."))
    }
}

// The following opens and draws DjVu files, with djvu-rs.

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

    /// Returns the page's size in pixels at the resolution that the file
    /// gives for the page, or at 96 dpi when that resolution is lower.
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
            // i18n: the paged viewer translates this error where it draws it
            .ok_or_else(|| anyhow!("Could not render this DjVu page."))
    }
}

// The following opens and draws comic archives, which are zip files of
// page images.

/// The file extensions, in lower case, of the archive entries that are the
/// pages of a comic.
const COMIC_EXTENSIONS: [&str; 8] = ["avif", "bmp", "gif", "jpeg", "jpg", "png", "svg", "webp"];

pub struct Comic {
    archive: zip::ZipArchive<std::fs::File>,
    /// The names of the archive's image entries in reading order
    /// (`comic_pages`).
    pages: Vec<String>,
    /// The index and the picture of the page that was decoded last. They
    /// are kept so that the page is not decoded again when it is drawn at
    /// another size or measured.
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

/// Returns the names in `names` that are the pages of a comic: the entries
/// that are not folders and whose extension is one of `COMIC_EXTENSIONS`.
/// They are sorted by `crate::text::natural_order`, which treats upper and
/// lower case as the same and compares a run of digits as a number, so
/// "Page 2" comes before "Page 10".
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

/// The largest number of bytes, 64 MB, that is read of one archive entry.
/// Comic archives and EPUB books come from the internet, and a small
/// archive can hold an entry that decompresses to far more than the
/// archive's own size.
pub const ENTRY_LIMIT: u64 = 64 << 20;
/// The number of bytes, 64 KB, that are read from the start of a comic page
/// to find the picture's size in its header.
const HEADER_LIMIT: u64 = 64 << 10;

/// Returns the content of the archive entry `name`. It fails when the
/// archive has no such entry, and when the entry is longer than `limit`
/// bytes. It never returns only the first part of an entry.
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

/// The largest width and the largest height in pixels of a picture that is
/// decoded from an archive. `PICTURE_BYTES` is the most memory, 512 MB,
/// that decoding one picture may allocate. A small file whose header gives
/// a larger picture than this is refused before the memory for it is
/// allocated.
const PICTURE_SIDE: u32 = 16_384;
const PICTURE_BYTES: u64 = 512 << 20;

/// Decodes the picture in `bytes`, the content of the archive entry `name`.
/// An entry whose name ends in `.svg` is drawn by resvg at its own size, or
/// smaller so that neither side is longer than [`MAX_RENDER_SIDE`]. Every
/// other entry is decoded by the `image` crate.
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
        // tiny-skia's pixels have their colours multiplied by their alpha,
        // and `RgbaImage` holds colours that are not. So each colour of a
        // partly transparent pixel is divided by the pixel's alpha.
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

    /// Returns the size in pixels of the page's picture. The size is read
    /// from the picture's header in the first 64 KB of the entry
    /// (`HEADER_LIMIT`), so that measuring a whole archive does not decode
    /// every page. When the size cannot be read from the header, the whole
    /// picture is decoded.
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

// The following decodes JPEG XL images, which the media surface and the
// duplicates screen show as pictures.

/// Decodes the JPEG XL image at `path` with jxl-oxide, which is also the
/// decoder that the crate in `core/` depends on. A panic in the decoder is
/// returned as an error.
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

// The following is the thread that holds one open document and draws its
// pages.

/// The drawing of one page. `pixels` is the width and height that were
/// asked for. `image` is the picture, which is smaller than `pixels` when a
/// side of `pixels` is longer than `MAX_RENDER_SIDE`.
pub struct RenderedPage {
    pub pixels: (u32, u32),
    pub image: RgbaImage,
}

struct Request {
    index: usize,
    pixels: (u32, u32),
    reply: oneshot::Sender<Result<RenderedPage>>,
}

/// The handle of the thread that holds one open paged document. The viewer
/// asks the thread for drawings of pages through it (`render`). Dropping
/// the handle ends the thread.
pub struct PageWorker {
    requests: mpsc::Sender<Request>,
}

/// The largest width and the largest height in pixels of the picture that a
/// page is drawn as. It limits the memory that one page takes at a high
/// zoom.
pub const MAX_RENDER_SIDE: u32 = 8192;

/// Returns the natural size of every page of `source`. A page that cannot
/// be measured, or whose width or height is not above 0, gets the size of
/// the page before it, so that one damaged page does not keep the other
/// pages from being shown. It fails when the first page cannot be measured
/// and when the document has no pages.
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

/// Opens the paged document at `path` and returns the natural size of every
/// page. The collection reader calls it to lay out the pages of a member
/// that is a paged document before that member's viewer exists.
pub fn measure(kind: RendererKind, path: &Path) -> Result<Vec<PageSize>> {
    super::guarded(|| measure_pages(open(kind, path)?.as_mut()))
}

impl PageWorker {
    /// Starts a thread that opens the document at `path` and measures its
    /// pages. The receiver that is returned gets the thread's handle and
    /// the natural size of every page, or the error with which opening or
    /// measuring failed. When the thread cannot be started, or ends
    /// without an answer, the receiver is cancelled.
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
                    // The viewer has dropped the receiver of this request,
                    // because the page was scrolled away, the zoom changed
                    // or the viewer was closed. The page is not drawn.
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

    /// Asks the thread to draw page `index` at `pixels`, a width and a
    /// height in pixels. The receiver that is returned gets the drawing, or
    /// the error with which drawing failed. When the caller drops the
    /// receiver before the thread reaches the request, the thread does not
    /// draw the page.
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

/// Draws page `index` of `source` at `pixels`, scaled down by
/// `bound_render_size` when that is too large. A panic in the decoder is
/// returned as an error for that page, so that the document's thread keeps
/// running and can draw the other pages.
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

/// Returns `width` × `height` scaled down, with the same proportions, so
/// that neither side is longer than [`MAX_RENDER_SIDE`]. A size within that
/// limit is returned unchanged. Each side returned is at least 1.
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

    /// A document of three pages for the tests. Its second page cannot be
    /// measured, and drawing it panics.
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
        // The page that cannot be measured gets the size of the page before
        // it.
        assert_eq!(
            measure_pages(&mut source).unwrap(),
            vec![(100., 200.), (100., 200.), (102., 200.)]
        );
        // Drawing the page whose decoder panics returns an error, and the
        // page after it can still be drawn.
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
        // A GIF of a few bytes whose header gives a width and a height of
        // 65535 pixels.
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0]);
        gif.extend_from_slice(&[0x2c, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 2, 0, 0x3b]);
        assert!(decode_picture("page.gif", &gif).is_err());
        // An SVG page that is larger still is drawn with its longer side at
        // `MAX_RENDER_SIDE` pixels.
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
