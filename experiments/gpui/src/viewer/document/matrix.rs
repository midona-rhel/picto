//! The format matrix: every media type Picto accepts, the renderer it gets,
//! and a check that its first page or frame draws. The regular checks decode
//! the small fixtures in `assets/fixtures/documents` off-screen; the ignored
//! ones draw the real viewers through GPUI's headless renderer (they need a
//! GPU), and one takes a file from `PICTO_GPUI_MEDIA`.
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

/// The media types `core/src/media_processing/formats.rs` accepts.
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
    // Each document kind has the types the matrix in the README lists.
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
    // Word and PowerPoint documents take the unsupported view, as do the
    // design, 3D, office and other formats with no renderer.
    for mime in [
        "application/msword",
        "application/vnd.ms-powerpoint",
        "text/html",
        "model/gltf-binary",
        "application/x-procreate",
    ] {
        assert_eq!(renderer_kind(mime), Unsupported, "{mime}");
    }
    assert!(
        renderer_kind("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
            .is_document()
    );
}

// ── Checking a picture ─────────────────────────────────────────

/// The share of pixels further than 24 levels from the picture's most
/// common colour.
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

/// The picture has the size asked for and is not one flat colour.
fn assert_drawn(image: &RgbaImage, size: (u32, u32), what: &str) {
    assert_eq!((image.width(), image.height()), size, "{what}: size");
    let share = ink(image);
    assert!(share > 0.005, "{what}: blank ({share})");
}

// ── First pages, decoded off-screen ────────────────────────────

/// The first page of a paged document, drawn to fit a 400px square.
fn first_page(kind: RendererKind, path: &Path) -> (RgbaImage, (u32, u32)) {
    let mut source = pages::open(kind, path).unwrap();
    assert!(source.page_count() > 0);
    let natural = source.page_size(0).unwrap();
    let (width, height) = super::geometry::fit_page((400., 400.), natural).unwrap();
    let size = (width.round() as u32, height.round() as u32);
    (source.render(0, size.0, size.1).unwrap(), size)
}

#[test]
fn pdf_first_page_draws() {
    let mut pdf = pages::Pdf::open(&fixture("page.pdf")).unwrap();
    assert_eq!(pdf.page_count(), 1);
    // US Letter in points.
    assert_eq!(pdf.page_size(0).unwrap(), (612., 792.));
    let (image, size) = first_page(RendererKind::Pdf, &fixture("page.pdf"));
    assert_drawn(&image, size, "PDF");
}

#[test]
fn djvu_first_page_draws() {
    let (image, size) = first_page(RendererKind::Djvu, &fixture("page.djvu"));
    assert_drawn(&image, size, "DjVu");
}

#[test]
fn comic_pages_draw_in_natural_order() {
    let mut comic = pages::Comic::open(&fixture("comic.cbz")).unwrap();
    // 001, 002 and 010; the text file is not a page.
    assert_eq!(comic.page_count(), 3);
    assert_eq!(comic.page_size(0).unwrap(), (120., 180.));
    let first = comic.render(0, 120, 180).unwrap();
    // The first page's band is blue (60, 140, 220).
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
    // The image renderer decodes rasters with `image`, as GPUI's loader does.
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

/// A WOFF 1.0 file wrapping a TrueType font, every table compressed.
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

/// The bundled Fira Mono, as TrueType and wrapped as WOFF in `folder`.
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

// ── The viewers, drawn off-screen ──────────────────────────────

// The macOS text system and Metal draw these; other platforms have no
// headless renderer here yet.
#[cfg(target_os = "macos")]
mod gpu {
    use super::super::{Document, kind::RendererKind, kind::renderer_kind};
    use super::{assert_drawn, fixture, font_files, ink};

    use crate::theme;
    use gpui::{
        AnyWindowHandle, App, AppContext as _, Context, HeadlessAppContext, IntoElement,
        ParentElement as _, Render, RenderImage, Styled as _, Window, div, px, size,
    };
    use image::RgbaImage;
    use std::path::Path;
    use std::sync::Arc;

    const WIDTH: f32 = 900.;
    const HEIGHT: f32 = 700.;

    struct Root {
        document: Document,
        thumbnail: Option<Arc<RenderImage>>,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(self.document.element(self.thumbnail.clone(), cx))
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

    /// Whether the document has what it was loading.
    fn settled(document: &Document, cx: &App) -> bool {
        match document {
            Document::Paged(paged) => paged.read(cx).drawn(),
            Document::Reader(reader) => reader.read(cx).loaded(),
            Document::Font(font) => font.read(cx).settled(),
            Document::Unsupported { .. } => true,
        }
    }

    /// Opens a file in its viewer in an off-screen window, waits for it to
    /// load and draw, and returns the window's picture.
    pub fn render(
        cx: &mut HeadlessAppContext,
        path: &Path,
        mime: &str,
        thumbnail: Option<Arc<RenderImage>>,
    ) -> RgbaImage {
        let kind = renderer_kind(mime);
        assert!(kind.is_document(), "{mime} is not a document");
        let document =
            cx.update(|cx| Document::open(kind, path.to_owned(), mime, "Fixture".into(), cx));
        let window = cx
            .open_window(size(px(WIDTH), px(HEIGHT)), |_, cx| {
                cx.new(|_| Root {
                    document,
                    thumbnail,
                })
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
        // Lay out once more with what arrived, then once more for anything
        // measured in that frame.
        draw(cx);
        cx.run_until_parked();
        draw(cx);
        let picture = cx.capture_screenshot(handle).unwrap();
        cx.update_window(handle, |_, window, _| window.remove_window())
            .unwrap();
        picture
    }

    /// The picture is the window's size and its middle holds a drawing:
    /// at least 400 pixels apart from its most common colour, so a page
    /// with a few lines of text passes and a flat backdrop does not.
    fn assert_view(picture: &RgbaImage, what: &str) {
        let (width, height) = (picture.width(), picture.height());
        assert!(
            width as f32 >= WIDTH && height as f32 >= HEIGHT,
            "{what}: {width}×{height}"
        );
        // The footer and the backdrop are flat; the page in the middle is not.
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
            let picture = render(&mut cx, &path, mime, None);
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
        // Word files take the unsupported view: the thumbnail, the message
        // and the button.
        let thumbnail = super::super::render_image(RgbaImage::from_fn(160, 120, |x, _| {
            image::Rgba([(x * 255 / 160) as u8, 90, 200, 255])
        }));
        let picture = render(
            &mut cx,
            &fixture("missing.docx"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Some(thumbnail),
        );
        if let Some(out) = &out {
            picture.save(out.join("unsupported_docx.png")).unwrap();
        }
        assert_view(&picture, "unsupported");
        assert_eq!(
            renderer_kind(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            RendererKind::Docx
        );
    }

    /// The type of a real file, from its extension.
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

    /// `PICTO_GPUI_MEDIA=<file>` (with `PICTO_GPUI_MEDIA_MIME` to override
    /// the type its extension gives): decodes its first page or frame and,
    /// for documents, draws its viewer off-screen. `PICTO_GPUI_MATRIX_OUT`
    /// saves the pictures to a folder.
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
            let picture = render(&mut cx, &path, &mime, None);
            if let Some(out) = &out {
                picture.save(out.join("viewer.png")).unwrap();
            }
            assert_view(&picture, &mime);
            eprintln!("{mime}: viewer drawn");
        }
    }
}
