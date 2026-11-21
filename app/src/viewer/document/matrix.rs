//! Tests of the formats. They list every media type that Picto accepts with
//! the viewer it gets, and check that the first page or frame of each kind
//! of document can be drawn. The tests that always run decode the small
//! fixtures in `assets/fixtures/documents` without a window. The tests
//! marked `#[ignore]` draw the viewers through GPUI's headless renderer,
//! which needs a GPU. One of them takes its file from the environment
//! variable `PICTO_GPUI_MEDIA`.
use super::kind::{RendererKind, renderer_kind};
use super::pages::PageSource as _;
use super::{epub, font, pages, reader, rich};
use image::RgbaImage;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/fixtures/documents")
        .join(name)
}

/// The media types that the library accepts, copied from
/// `core/src/media_processing/formats.rs`.
const ACCEPTED: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/apng",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/tiff",
    "image/svg+xml",
    "image/x-icon",
    "image/x-icns",
    "image/heic",
    "image/heic-sequence",
    "image/heif",
    "image/heif-sequence",
    "image/avif",
    "image/avif-sequence",
    "image/jxl",
    "image/qoi",
    "application/x-ugoira",
    "text/x-base64",
    "application/x-insp",
    "image/x-svga",
    "application/x-pag",
    "application/vnd.lottie+zip",
    "image/vnd-ms.dds",
    "image/x-exr",
    "image/vnd.radiance",
    "image/x-tga",
    "image/x-ilbm",
    "video/mp4",
    "video/x-m4v",
    "video/webm",
    "video/x-matroska",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-flv",
    "video/x-f4v",
    "video/x-ms-wmv",
    "video/ogg",
    "video/mpeg",
    "video/vnd.rn-realvideo",
    "video/3gpp",
    "video/3gpp2",
    "video/mp2t",
    "audio/aac",
    "audio/flac",
    "audio/mp4",
    "audio/mpeg",
    "audio/ogg",
    "audio/opus",
    "audio/wav",
    "audio/x-ms-wma",
    "audio/x-matroska",
    "audio/wavpack",
    "audio/x-tta",
    "model/x-fbx",
    "model/obj",
    "model/x-3ds",
    "model/3mf",
    "model/vnd.collada+xml",
    "model/x-ifc",
    "model/ply",
    "model/stl",
    "model/gltf-binary",
    "model/gltf+json",
    "model/vrm",
    "application/x-affinity",
    "application/x-affinity-designer",
    "application/x-affinity-photo",
    "application/x-affinity-publisher",
    "application/postscript",
    "application/x-cinema4d",
    "application/x-coreldraw",
    "application/x-clip-studio-paint",
    "image/vnd.dwg",
    "application/x-omnigraffle",
    "application/vnd.adobe.indesign-idml-package",
    "application/x-indesign",
    "application/x-indesign-template",
    "application/x-mindnode",
    "image/vnd.adobe.photoshop",
    "image/vnd.adobe.photoshop.large",
    "image/vnd.adobe.photoshop.template",
    "application/x-pixelmator",
    "application/x-principle",
    "application/x-sketch",
    "application/x-sketch-template",
    "model/vnd.sketchup.skp",
    "application/x-adobe-xd",
    "application/x-xmind",
    "font/ttf",
    "font/collection",
    "font/otf",
    "font/woff",
    "image/x-hasselblad-3fr",
    "image/x-sony-arw",
    "image/x-canon-cr2",
    "image/x-canon-cr3",
    "image/x-canon-crw",
    "image/x-adobe-dng",
    "image/x-epson-erf",
    "image/x-minolta-mrw",
    "image/x-nikon-nef",
    "image/x-nikon-nrw",
    "image/x-olympus-orf",
    "image/x-pentax-pef",
    "image/x-fuji-raf",
    "image/x-raw",
    "image/x-panasonic-rw2",
    "image/x-sony-sr2",
    "image/x-samsung-srw",
    "image/x-sigma-x3f",
    "text/plain",
    "text/markdown",
    "application/json",
    "application/rtf",
    "application/x-iwork-keynote-sffkey",
    "application/x-iwork-numbers-sffnumbers",
    "application/x-iwork-pages-sffpages",
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.presentationml.template",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/x-edraw",
    "application/x-edraw-mindmap",
    "text/html",
    "multipart/related",
    "application/internet-shortcut",
    "application/zip",
    "application/x-cube-lut",
    "application/x-3dl-lut",
    "application/x-ies",
    "application/epub+zip",
    "application/vnd.comicbook+zip",
    "image/vnd.djvu",
    "application/x-shockwave-flash",
    "application/x-eps",
    "application/x-live-photo",
    "application/x-sai2",
    "application/x-krita",
    "image/x-xcf",
    "application/x-procreate",
    "application/x-paint-dot-net",
];

#[test]
fn every_accepted_type_has_a_renderer() {
    use RendererKind::*;
    let mut by_kind = std::collections::BTreeMap::<String, Vec<&str>>::new();
    for mime in ACCEPTED {
        by_kind
            .entry(format!("{:?}", renderer_kind(mime)))
            .or_default()
            .push(mime);
    }
    // Each kind of document gets exactly these media types.
    assert_eq!(by_kind["Pdf"], ["application/pdf"]);
    assert_eq!(by_kind["Epub"], ["application/epub+zip"]);
    assert_eq!(by_kind["Cbz"], ["application/vnd.comicbook+zip"]);
    assert_eq!(by_kind["Djvu"], ["image/vnd.djvu"]);
    assert_eq!(by_kind["JpegXl"], ["image/jxl"]);
    assert_eq!(
        by_kind["TextDocument"],
        [
            "text/plain",
            "text/markdown",
            "application/json",
            "application/rtf"
        ]
    );
    assert_eq!(
        by_kind["Font"],
        ["font/ttf", "font/collection", "font/otf", "font/woff"]
    );
    assert_eq!(by_kind["Video"].len(), 15);
    assert_eq!(by_kind["Audio"].len(), 11);
    // A format that has no viewer, such as an office document, a design
    // file or a 3D model, gets the view for files that cannot be shown.
    for mime in [
        "application/msword",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/vnd.ms-powerpoint",
        "text/html",
        "model/gltf-binary",
        "application/x-procreate",
    ] {
        assert_eq!(renderer_kind(mime), Unsupported, "{mime}");
    }
}

// The following checks that something has been drawn in a picture.

/// Returns the share of the picture's pixels in which a colour channel
/// differs by more than 24 levels from the picture's most common colour.
fn ink(image: &RgbaImage) -> f32 {
    let mut counts = std::collections::HashMap::<[u8; 3], usize>::new();
    for pixel in image.pixels() {
        *counts
            .entry([pixel[0] & 0xf0, pixel[1] & 0xf0, pixel[2] & 0xf0])
            .or_default() += 1;
    }
    let common = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(colour, _)| colour)
        .unwrap_or_default();
    let differing = image
        .pixels()
        .filter(|pixel| {
            (0..3).any(|channel| (pixel[channel] as i32 - (common[channel] as i32 + 8)).abs() > 24)
        })
        .count();
    differing as f32 / (image.width() * image.height()).max(1) as f32
}

/// Asserts that `image` has the size `size` and is not of one colour only:
/// more than 0.5% of its pixels differ from its most common colour (`ink`).
/// `what` names the picture in the message of a failure.
fn assert_drawn(image: &RgbaImage, size: (u32, u32), what: &str) {
    assert_eq!((image.width(), image.height()), size, "{what}: size");
    let share = ink(image);
    assert!(share > 0.005, "{what}: blank ({share})");
}

// The following tests decode the first page of each kind of document
// without a window.

/// Draws the first page of the paged document at `path` as large as fits
/// into a square of 400 pixels, and returns the picture and its size.
fn first_page(kind: RendererKind, path: &Path) -> (RgbaImage, (u32, u32)) {
    let mut source = pages::open(kind, path).unwrap();
    assert!(source.page_count() > 0);
    let natural = source.page_size(0).unwrap();
    let (_, _, width, height) = crate::viewer::camera::contain(natural, (400., 400.)).unwrap();
    let size = (width.round() as u32, height.round() as u32);
    (source.render(0, size.0, size.1).unwrap(), size)
}

#[test]
fn pdf_first_page_draws() {
    let mut pdf = pages::Pdf::open(&fixture("page.pdf")).unwrap();
    assert_eq!(pdf.page_count(), 1);
    // The page is US Letter, which is 612 × 792 points.
    assert_eq!(pdf.page_size(0).unwrap(), (612., 792.));
    let (image, size) = first_page(RendererKind::Pdf, &fixture("page.pdf"));
    assert_drawn(&image, size, "PDF");
}

#[test]
fn a_multi_page_pdf_measures_and_draws_every_page() {
    let path = fixture("pages.pdf");
    // The three pages are US Letter, A5 and US Letter turned sideways.
    assert_eq!(
        pages::measure(RendererKind::Pdf, &path).unwrap(),
        vec![(612., 792.), (420., 595.), (792., 612.)]
    );
    // The document's thread draws the pages that are asked for, in the
    // order asked. It does not draw a page whose receiver was dropped
    // before the thread reached the request.
    let (worker, sizes) =
        futures::executor::block_on(pages::PageWorker::open(RendererKind::Pdf, path))
            .unwrap()
            .unwrap();
    assert_eq!(sizes.len(), 3);
    let given_up = worker.render(0, (612, 792));
    let second = worker.render(1, (210, 298));
    let third = worker.render(2, (396, 306));
    drop(given_up);
    let second = futures::executor::block_on(second).unwrap().unwrap();
    let third = futures::executor::block_on(third).unwrap().unwrap();
    assert_drawn(&second.image, (210, 298), "PDF page 2");
    assert_drawn(&third.image, (396, 306), "PDF page 3");
    // Page 2 has a red band and page 3 has a green band, so the colours
    // show that each request drew its own page.
    let red = second.image.get_pixel(105, 298 - 220);
    assert!(red[0] > 150 && red[1] < 100, "page 2: {red:?}");
    let green = third.image.get_pixel(198, 306 - 215);
    assert!(green[1] > 120 && green[0] < 100, "page 3: {green:?}");
}

#[test]
fn djvu_first_page_draws() {
    let (image, size) = first_page(RendererKind::Djvu, &fixture("page.djvu"));
    assert_drawn(&image, size, "DjVu");
}

#[test]
fn comic_pages_draw_in_natural_order() {
    let mut comic = pages::Comic::open(&fixture("comic.cbz")).unwrap();
    // The pages are the entries 001, 002 and 010. The text file in the
    // archive is not a page.
    assert_eq!(comic.page_count(), 3);
    assert_eq!(comic.page_size(0).unwrap(), (120., 180.));
    let first = comic.render(0, 120, 180).unwrap();
    // The first page has a blue band, of the colour (60, 140, 220).
    let band = first.get_pixel(60, 20);
    assert!(band[2] > 200 && band[0] < 100, "page 001 first: {band:?}");
    let (image, size) = first_page(RendererKind::Cbz, &fixture("comic.cbz"));
    assert_drawn(&image, size, "CBZ");
}

#[test]
fn jpeg_xl_decodes() {
    let image = pages::decode_jpeg_xl(&fixture("image.jxl")).unwrap();
    assert_drawn(&image, (96, 64), "JPEG XL");
}

#[test]
fn gif_and_svg_first_frames_decode() {
    // Raster images are decoded with the `image` crate, which GPUI's image
    // loader also uses.
    let gif = image::open(fixture("animation.gif")).unwrap().to_rgba8();
    assert_drawn(&gif, (48, 32), "GIF");
    let svg =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fixtures/vector.svg"))
            .unwrap();
    let svg = pages::decode_picture("vector.svg", &svg).unwrap();
    let size = (svg.width(), svg.height());
    assert_drawn(&svg, size, "SVG");
}

#[test]
fn text_documents_read_into_blocks() {
    for (name, mime, expected) in [
        ("notes.txt", "text/plain", "\ta tab-indented line"),
        ("readme.md", "text/markdown", "inline code"),
        ("data.json", "application/json", "\"ok\": true"),
        ("letter.rtf", "application/rtf", "café"),
    ] {
        assert_eq!(renderer_kind(mime), RendererKind::TextDocument);
        let (blocks, truncated) = reader::load_text(&fixture(name), mime).unwrap();
        assert!(!truncated);
        let text = rich::text_of(&blocks);
        assert!(text.contains(expected), "{name}: {text}");
    }
}

#[test]
fn epub_chapters_read_with_their_pictures() {
    let chapters = epub::chapters(&fixture("book.epub")).unwrap();
    assert_eq!(chapters, ["OEBPS/text/one.xhtml", "OEBPS/text/two.xhtml"]);
    let blocks = epub::chapter(&fixture("book.epub"), &chapters[0]).unwrap();
    assert!(rich::text_of(&blocks).contains("Chapter One"));
    let picture = blocks.iter().find_map(|block| match block {
        rich::Block::Picture(picture) => Some(picture.size),
        _ => None,
    });
    assert_eq!(picture, Some((160., 90.)));
}

/// Returns a WOFF 1.0 file that holds the TrueType font `sfnt`. Each table
/// is stored compressed when that makes it smaller.
fn pack_woff(sfnt: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let read_u32 = |at: usize| u32::from_be_bytes(sfnt[at..at + 4].try_into().unwrap());
    let count = u16::from_be_bytes(sfnt[4..6].try_into().unwrap()) as usize;
    let mut directory = Vec::new();
    let mut data = Vec::new();
    let mut offset = 44 + 20 * count;
    for index in 0..count {
        let at = 12 + 16 * index;
        let (tag, checksum, start, length) = (
            read_u32(at),
            read_u32(at + 4),
            read_u32(at + 8) as usize,
            read_u32(at + 12) as usize,
        );
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&sfnt[start..start + length]).unwrap();
        let compressed = encoder.finish().unwrap();
        let stored = if compressed.len() < length {
            compressed
        } else {
            sfnt[start..start + length].to_vec()
        };
        for value in [
            tag,
            offset as u32,
            stored.len() as u32,
            length as u32,
            checksum,
        ] {
            directory.extend_from_slice(&value.to_be_bytes());
        }
        offset += stored.len().next_multiple_of(4);
        data.extend_from_slice(&stored);
        data.resize(data.len().next_multiple_of(4), 0);
    }
    let mut woff = Vec::new();
    woff.extend_from_slice(b"wOFF");
    woff.extend_from_slice(&read_u32(0).to_be_bytes());
    woff.extend_from_slice(&((44 + directory.len() + data.len()) as u32).to_be_bytes());
    woff.extend_from_slice(&(count as u16).to_be_bytes());
    woff.extend_from_slice(&[0; 2]);
    woff.extend_from_slice(&(sfnt.len() as u32).to_be_bytes());
    woff.extend_from_slice(&[0; 24]);
    woff.extend_from_slice(&directory);
    woff.extend_from_slice(&data);
    woff
}

/// Returns two files of the bundled font Fira Mono, each with its media
/// type: the TrueType file in the app's assets, and a WOFF file made from
/// it, which is written into `folder`.
fn font_files(folder: &Path) -> Vec<(PathBuf, &'static str)> {
    let ttf = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/FiraMono.ttf");
    let woff = folder.join("FiraMono.woff");
    std::fs::write(&woff, pack_woff(&std::fs::read(&ttf).unwrap())).unwrap();
    vec![(ttf, "font/ttf"), (woff, "font/woff")]
}

#[test]
fn fonts_load_and_have_outlines() {
    let folder = tempfile::tempdir().unwrap();
    for (path, mime) in font_files(folder.path()) {
        assert_eq!(renderer_kind(mime), RendererKind::Font);
        let loaded = font::load(&path).unwrap();
        assert!(loaded.families.iter().any(|family| family == "Fira Mono"));
        let face = ttf_parser::Face::parse(&loaded.data, 0).unwrap();
        let glyph = face.glyph_index('A').unwrap();
        let bounds = face.glyph_bounding_box(glyph).unwrap();
        assert!(bounds.width() > 0 && bounds.height() > 0, "{mime}");
    }
}

// The following tests draw the viewers in a window that is not shown.

// These tests draw with the macOS text system and with Metal. The other
// platforms have no headless renderer in this crate yet, so the tests
// exist on macOS only.
#[cfg(target_os = "macos")]
mod gpu {
    use super::super::{Document, kind::RendererKind, kind::renderer_kind};
    use super::{assert_drawn, fixture, font_files, ink};

    use crate::theme;
    use gpui::{
        AnyWindowHandle, App, AppContext as _, Context, HeadlessAppContext, IntoElement,
        ParentElement as _, Render, Styled as _, Window, div, px, size,
    };
    use image::RgbaImage;
    use std::path::Path;
    use std::sync::Arc;

    const WIDTH: f32 = 900.;
    const HEIGHT: f32 = 700.;

    struct Root {
        document: Document,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().relative().size_full().child(self.document.element())
        }
    }

    fn context() -> HeadlessAppContext {
        let text_system: Arc<dyn gpui::PlatformTextSystem> =
            Arc::new(gpui_apple::AppleTextSystem::new());
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(crate::assets::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.allow_parking();
        cx.update(|cx: &mut App| {
            cx.text_system()
                .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                    "../../../assets/FiraMono.ttf"
                ))])
                .unwrap();
            cx.set_global(theme::DARK);
        });
        cx
    }

    /// Returns whether the document has finished loading what it shows
    /// first.
    fn settled(document: &Document, cx: &App) -> bool {
        match document {
            Document::Paged(paged) => paged.read(cx).settled(),
            Document::Reader(reader) => reader.read(cx).loaded(),
            Document::Font(font) => font.read(cx).settled(),
            Document::Unsupported { .. } => true,
        }
    }

    /// Opens the file at `path` in its viewer in a window that is not
    /// shown, waits up to 30 seconds for the viewer to load the file, and
    /// returns a picture of the window.
    pub fn render(cx: &mut HeadlessAppContext, path: &Path, mime: &str) -> RgbaImage {
        let kind = renderer_kind(mime);
        assert!(kind.is_document(), "{mime} is not a document");
        let document = cx
            .update(|cx| Document::open(kind, path.to_owned(), mime, "Fixture".into(), false, cx));
        let window = cx
            .open_window(size(px(WIDTH), px(HEIGHT)), |_, cx| {
                cx.new(|_| Root { document })
            })
            .unwrap();
        let handle: AnyWindowHandle = window.into();
        let draw = |cx: &mut HeadlessAppContext| {
            cx.update_window(handle, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .unwrap();
        };
        let started = std::time::Instant::now();
        loop {
            draw(cx);
            cx.run_until_parked();
            let ready = cx
                .read_window(&window, |root, cx| settled(&root.read(cx).document, cx))
                .unwrap();
            if ready || started.elapsed() > std::time::Duration::from_secs(30) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // The window is drawn once more, which lays out the content that
        // arrived, and then again, which lays out anything that was
        // measured in the frame before.
        draw(cx);
        cx.run_until_parked();
        draw(cx);
        let picture = cx.capture_screenshot(handle).unwrap();
        cx.update_window(handle, |_, window, _| window.remove_window())
            .unwrap();
        picture
    }

    /// Asserts that `picture` is at least as large as the window and that
    /// something is drawn in its middle: at least 400 pixels there differ
    /// from the most common colour. A page with a few lines of text
    /// passes, and a backdrop of one colour does not.
    fn assert_view(picture: &RgbaImage, what: &str) {
        let (width, height) = (picture.width(), picture.height());
        assert!(
            width as f32 >= WIDTH && height as f32 >= HEIGHT,
            "{what}: {width}×{height}"
        );
        // The footer and the backdrop are each of one colour, so only the
        // middle of the picture, where the page is, is checked.
        let middle =
            image::imageops::crop_imm(picture, width / 4, height / 8, width / 2, height / 2)
                .to_image();
        let drawn = ink(&middle) * (middle.width() * middle.height()) as f32;
        assert!(drawn >= 400., "{what}: blank ({drawn} pixels drawn)");
    }

    #[test]
    #[ignore = "draws through the GPU"]
    fn viewers_render_off_screen() {
        let mut cx = context();
        let folder = tempfile::tempdir().unwrap();
        let out = std::env::var_os("PICTO_GPUI_MATRIX_OUT").map(std::path::PathBuf::from);
        let mut cases: Vec<(std::path::PathBuf, &str)> = vec![
            (fixture("page.pdf"), "application/pdf"),
            (fixture("pages.pdf"), "application/pdf"),
            (fixture("page.djvu"), "image/vnd.djvu"),
            (fixture("comic.cbz"), "application/vnd.comicbook+zip"),
            (fixture("notes.txt"), "text/plain"),
            (fixture("readme.md"), "text/markdown"),
            (fixture("data.json"), "application/json"),
            (fixture("letter.rtf"), "application/rtf"),
            (fixture("book.epub"), "application/epub+zip"),
        ];
        cases.extend(font_files(folder.path()));
        for (path, mime) in cases {
            let picture = render(&mut cx, &path, mime);
            if let Some(out) = &out {
                let name = path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .replace('.', "_");
                picture.save(out.join(format!("{name}.png"))).unwrap();
            }
            assert_view(&picture, mime);
        }
        // A type that has no viewer gets the view for files that cannot be
        // shown, which is the document shell with a message.
        let picture = render(&mut cx, &fixture("missing.zip"), "application/zip");
        if let Some(out) = &out {
            picture.save(out.join("unsupported_zip.png")).unwrap();
        }
        assert_view(&picture, "unsupported");
    }

    /// Returns the media type that the extension of `path` stands for, or
    /// `application/octet-stream` for an extension that is not listed.
    fn mime_of(path: &Path) -> &'static str {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "pdf" => "application/pdf",
            "djvu" | "djv" => "image/vnd.djvu",
            "cbz" => "application/vnd.comicbook+zip",
            "epub" => "application/epub+zip",
            "txt" => "text/plain",
            "md" | "markdown" => "text/markdown",
            "json" => "application/json",
            "rtf" => "application/rtf",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "ttc" => "font/collection",
            "woff" => "font/woff",
            "jxl" => "image/jxl",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => "application/octet-stream",
        }
    }

    /// Checks the file that the environment variable `PICTO_GPUI_MEDIA`
    /// names. The file's media type comes from its extension, or from
    /// `PICTO_GPUI_MEDIA_MIME` when that variable is set. The test decodes
    /// the file's first page or frame. For a document it also draws the
    /// viewer in a window that is not shown. When `PICTO_GPUI_MATRIX_OUT`
    /// names a folder, the pictures are saved there.
    #[test]
    #[ignore = "takes a real file"]
    fn renders_a_real_file() {
        let path = std::path::PathBuf::from(
            std::env::var_os("PICTO_GPUI_MEDIA").expect("PICTO_GPUI_MEDIA=<file>"),
        );
        let mime =
            std::env::var("PICTO_GPUI_MEDIA_MIME").unwrap_or_else(|_| mime_of(&path).to_owned());
        let out = std::env::var_os("PICTO_GPUI_MATRIX_OUT").map(std::path::PathBuf::from);
        let kind = renderer_kind(&mime);
        let first = match kind {
            RendererKind::Pdf | RendererKind::Djvu | RendererKind::Cbz => {
                Some(super::first_page(kind, &path).0)
            }
            RendererKind::JpegXl => Some(super::super::pages::decode_jpeg_xl(&path).unwrap()),
            RendererKind::Image => Some(image::open(&path).unwrap().to_rgba8()),
            _ => None,
        };
        if let Some(first) = first {
            let size = (first.width(), first.height());
            if let Some(out) = &out {
                first.save(out.join("first.png")).unwrap();
            }
            assert_drawn(&first, size, &mime);
            eprintln!("{mime}: first page or frame {}×{}", size.0, size.1);
        }
        if kind.is_document() {
            let mut cx = context();
            let picture = render(&mut cx, &path, &mime);
            if let Some(out) = &out {
                picture.save(out.join("viewer.png")).unwrap();
            }
            assert_view(&picture, &mime);
            eprintln!("{mime}: viewer drawn");
        }
    }
}
