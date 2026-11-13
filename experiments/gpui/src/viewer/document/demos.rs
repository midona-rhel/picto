//! The format matrix's fixtures as entries of the Media Examples menu, so
//! each document viewer can be opened without a library file of its type.
use crate::session::Session;
use gpui::*;

/// Each sample's file name, media type, menu label and contents.
pub const SAMPLES: [(&str, &str, &str, &[u8]); 10] = [
    (
        "pages.pdf",
        "application/pdf",
        "PDF",
        include_bytes!("../../../assets/fixtures/documents/pages.pdf"),
    ),
    (
        "page.djvu",
        "image/vnd.djvu",
        "DjVu",
        include_bytes!("../../../assets/fixtures/documents/page.djvu"),
    ),
    (
        "comic.cbz",
        "application/vnd.comicbook+zip",
        "Comic (CBZ)",
        include_bytes!("../../../assets/fixtures/documents/comic.cbz"),
    ),
    (
        "book.epub",
        "application/epub+zip",
        "Book (EPUB)",
        include_bytes!("../../../assets/fixtures/documents/book.epub"),
    ),
    (
        "notes.txt",
        "text/plain",
        "Plain Text",
        include_bytes!("../../../assets/fixtures/documents/notes.txt"),
    ),
    (
        "readme.md",
        "text/markdown",
        "Markdown",
        include_bytes!("../../../assets/fixtures/documents/readme.md"),
    ),
    (
        "data.json",
        "application/json",
        "JSON",
        include_bytes!("../../../assets/fixtures/documents/data.json"),
    ),
    (
        "letter.rtf",
        "application/rtf",
        "RTF",
        include_bytes!("../../../assets/fixtures/documents/letter.rtf"),
    ),
    (
        "FiraMono.ttf",
        "font/ttf",
        "Font",
        include_bytes!("../../../assets/FiraMono.ttf"),
    ),
    (
        "image.jxl",
        "image/jxl",
        "JPEG XL",
        include_bytes!("../../../assets/fixtures/documents/image.jxl"),
    ),
];

actions!(
    document_sample,
    [
        OpenPdf,
        OpenDjvu,
        OpenComic,
        OpenBook,
        OpenText,
        OpenMarkdown,
        OpenJson,
        OpenRtf,
        OpenFont,
        OpenJpegXl
    ]
);

/// The samples' menu entries, in the order of [`SAMPLES`].
pub fn menu_items() -> Vec<MenuItem> {
    let label = |index: usize| SAMPLES[index].2;
    vec![
        MenuItem::action(label(0), OpenPdf),
        MenuItem::action(label(1), OpenDjvu),
        MenuItem::action(label(2), OpenComic),
        MenuItem::action(label(3), OpenBook),
        MenuItem::action(label(4), OpenText),
        MenuItem::action(label(5), OpenMarkdown),
        MenuItem::action(label(6), OpenJson),
        MenuItem::action(label(7), OpenRtf),
        MenuItem::action(label(8), OpenFont),
        MenuItem::action(label(9), OpenJpegXl),
    ]
}

/// Opens the chosen sample through the session, as the SVG and Flash
/// examples do.
pub fn handle<E: InteractiveElement>(root: E, session: &Entity<Session>) -> E {
    fn open<A: Action, E: InteractiveElement>(
        root: E,
        session: &Entity<Session>,
        index: usize,
    ) -> E {
        let session = session.clone();
        root.on_action(move |_: &A, _, cx| {
            let (name, mime, _, _) = SAMPLES[index];
            session.update(cx, |session, cx| session.show_demo(name, mime, cx));
        })
    }
    let root = open::<OpenPdf, E>(root, session, 0);
    let root = open::<OpenDjvu, E>(root, session, 1);
    let root = open::<OpenComic, E>(root, session, 2);
    let root = open::<OpenBook, E>(root, session, 3);
    let root = open::<OpenText, E>(root, session, 4);
    let root = open::<OpenMarkdown, E>(root, session, 5);
    let root = open::<OpenJson, E>(root, session, 6);
    let root = open::<OpenRtf, E>(root, session, 7);
    let root = open::<OpenFont, E>(root, session, 8);
    open::<OpenJpegXl, E>(root, session, 9)
}
