//! Every way of viewing media other than images, Flash, video and audio
//! (`src/features/viewer/document/`): PDF, DjVu and comic pages, text,
//! Markdown, JSON, RTF and EPUB on a page, fonts, and the view for files
//! that cannot be shown. The media surface asks [`kind::renderer_kind`]
//! what an item gets and hands document kinds to [`Document`].
pub mod epub;
pub mod font;
pub mod geometry;
pub mod kind;
pub mod paged;
pub mod pages;
pub mod reader;
pub mod rich;
pub mod rtf;
mod shell;
pub mod unsupported;

#[cfg(test)]
mod matrix;

use crate::keymap::Scope;
use font::FontViewer;
use gpui::*;
use kind::RendererKind;
use paged::{PagedViewer, Progressed, ZoomChanged};
use reader::ReaderViewer;
use std::path::PathBuf;
use std::sync::Arc;

actions!(
    document,
    [
        PreviousPage,
        NextPage,
        PreviewLarger,
        PreviewSmaller,
        ScrollToStart,
        ScrollToEnd,
        ScrollPageUp,
        ScrollPageDown
    ]
);

/// The key context a host adds while it shows a document's pages, for the
/// keys that scroll them.
pub const PAGES_CONTEXT: &str = "Pages";

/// Home, End, Page Up, Page Down and Space scroll a document's pages as
/// they scroll any page. Space otherwise closes the media view, so these
/// are made after the registry's bindings, which they then take precedence
/// over in a host that shows pages.
pub fn scroll_bindings() -> Vec<KeyBinding> {
    let mut bindings = Vec::new();
    for scope in [Scope::Viewer, Scope::DetailWindow] {
        let predicate = scope
            .predicate()
            .map(|predicate| format!("{predicate} && {PAGES_CONTEXT}"));
        let predicate = predicate.as_deref();
        bindings.push(KeyBinding::new("home", ScrollToStart, predicate));
        bindings.push(KeyBinding::new("end", ScrollToEnd, predicate));
        bindings.push(KeyBinding::new("pageup", ScrollPageUp, predicate));
        bindings.push(KeyBinding::new("pagedown", ScrollPageDown, predicate));
        bindings.push(KeyBinding::new("space", ScrollPageDown, predicate));
        bindings.push(KeyBinding::new("shift-space", ScrollPageUp, predicate));
    }
    bindings
}

/// The font preview's Cmd+= and Cmd+− (Ctrl off macOS), which
/// `FontViewer.tsx` handles itself rather than through the shortcut
/// registry, in each host that shows a document. The registry's J and L
/// pages are bound by `crate::keymap`.
pub fn bind(cx: &mut App) {
    let (larger, smaller) = if cfg!(target_os = "macos") {
        ("cmd-=", "cmd--")
    } else {
        ("ctrl-=", "ctrl--")
    };
    let mut bindings = Vec::new();
    for scope in [Scope::Viewer, Scope::DetailWindow] {
        let predicate = scope.predicate();
        bindings.push(KeyBinding::new(larger, PreviewLarger, predicate.as_deref()));
        bindings.push(KeyBinding::new(
            smaller,
            PreviewSmaller,
            predicate.as_deref(),
        ));
    }
    cx.bind_keys(bindings);
}

/// A scroll of a document's pages by a key.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scroll {
    Start,
    End,
    PageUp,
    PageDown,
}

/// An open document and the view that shows it.
pub enum Document {
    Paged(Entity<PagedViewer>),
    Reader(Entity<ReaderViewer>),
    Font(Entity<FontViewer>),
    Unsupported {
        mime: String,
        path: PathBuf,
        scroll: ScrollHandle,
    },
}

impl Document {
    /// Opens a file of a document kind. Reading and rendering start on
    /// other threads; the view shows what it has meanwhile. `inline` lays a
    /// paged document's pages out for the collection reader.
    pub fn open(
        kind: RendererKind,
        path: PathBuf,
        mime: &str,
        name: SharedString,
        inline: bool,
        cx: &mut App,
    ) -> Self {
        match kind {
            RendererKind::Pdf | RendererKind::Djvu | RendererKind::Cbz => {
                Document::Paged(cx.new(|cx| PagedViewer::new(kind, path, inline, cx)))
            }
            RendererKind::TextDocument | RendererKind::Epub => {
                let mime = mime.to_owned();
                Document::Reader(cx.new(|cx| ReaderViewer::new(kind, path, mime, cx)))
            }
            RendererKind::Font => {
                let mime = mime.to_owned();
                Document::Font(cx.new(|cx| FontViewer::new(path, name, mime, cx)))
            }
            // Word and PowerPoint, by decision, and every type without a
            // renderer.
            _ => Document::Unsupported {
                mime: mime.to_owned(),
                path,
                scroll: ScrollHandle::new(),
            },
        }
    }

    /// Calls `changed` whenever the document's zoom changes, for the
    /// toolbar.
    pub fn observe_zoom<T: 'static>(
        &self,
        cx: &mut Context<T>,
        changed: impl Fn(&mut T, &mut Context<T>) + 'static,
    ) -> Option<Subscription> {
        match self {
            Document::Paged(paged) => {
                Some(cx.subscribe(paged, move |this, _, _: &ZoomChanged, cx| changed(this, cx)))
            }
            _ => None,
        }
    }

    /// Calls `progressed` when a paged document becomes presentable or draws
    /// its first page.
    pub fn observe_progress<T: 'static>(
        &self,
        cx: &mut Context<T>,
        progressed: impl Fn(&mut T, &mut Context<T>) + 'static,
    ) -> Option<Subscription> {
        match self {
            Document::Paged(paged) => {
                Some(cx.subscribe(paged, move |this, _, _: &Progressed, cx| {
                    progressed(this, cx)
                }))
            }
            _ => None,
        }
    }

    /// Hands the item's thumbnail, once its load has settled, to a view that
    /// shows it first.
    pub fn set_thumbnail(&self, thumbnail: Option<Arc<RenderImage>>, cx: &mut App) {
        if let Document::Paged(paged) = self {
            paged.update(cx, |paged, cx| paged.set_thumbnail(thumbnail, cx));
        }
    }

    /// Whether the document has something to show without a gap. Only
    /// paged documents wait; the others draw at once.
    pub fn presentable(&self, cx: &App) -> bool {
        match self {
            Document::Paged(paged) => paged.read(cx).presentable(),
            _ => true,
        }
    }

    /// Whether the document has drawn what it opened to (for a paged one,
    /// its first page in view).
    pub fn settled(&self, cx: &App) -> bool {
        match self {
            Document::Paged(paged) => paged.read(cx).settled(),
            _ => true,
        }
    }

    pub fn scroll(&self, scroll: Scroll, cx: &mut App) {
        if let Document::Paged(paged) = self {
            paged.update(cx, |paged, cx| match scroll {
                Scroll::Start => paged.scroll_to_end(false, cx),
                Scroll::End => paged.scroll_to_end(true, cx),
                Scroll::PageUp => paged.scroll_page(false, cx),
                Scroll::PageDown => paged.scroll_page(true, cx),
            });
        }
    }

    /// Whether this is a paged document, whose keys scroll its pages.
    pub fn paged(&self) -> bool {
        matches!(self, Document::Paged(_))
    }

    /// Whether the viewer toolbar's zoom controls drive this document: PDF
    /// pages only, as `usesRendererZoom` says.
    pub fn zoomable(&self, cx: &App) -> bool {
        matches!(self, Document::Paged(paged) if paged.read(cx).zoomable())
    }

    pub fn percent(&self, cx: &App) -> f32 {
        match self {
            Document::Paged(paged) => paged.read(cx).percent(),
            _ => 100.,
        }
    }

    pub fn fit(&self, cx: &mut App) {
        if let Document::Paged(paged) = self {
            paged.update(cx, |paged, cx| paged.fit(cx));
        }
    }

    /// Cmd+0: a PDF page at 100%, a font preview at its own size.
    pub fn actual_size(&self, cx: &mut App) {
        match self {
            Document::Paged(paged) => paged.update(cx, |paged, cx| paged.actual_size(cx)),
            Document::Font(font) => font.update(cx, |font, cx| font.reset_scale(cx)),
            _ => {}
        }
    }

    pub fn set_percent(&self, percent: f32, cx: &mut App) {
        if let Document::Paged(paged) = self {
            paged.update(cx, |paged, cx| paged.set_percent(percent, cx));
        }
    }

    pub fn step_zoom(&self, zoom_in: bool, cx: &mut App) {
        if let Document::Paged(paged) = self {
            paged.update(cx, |paged, cx| paged.step_zoom(zoom_in, cx));
        }
    }

    pub fn previous_page(&self, cx: &mut App) {
        match self {
            Document::Paged(paged) => paged.update(cx, |paged, cx| paged.previous_page(cx)),
            Document::Reader(reader) => reader.update(cx, |reader, cx| reader.previous_page(cx)),
            _ => {}
        }
    }

    pub fn next_page(&self, cx: &mut App) {
        match self {
            Document::Paged(paged) => paged.update(cx, |paged, cx| paged.next_page(cx)),
            Document::Reader(reader) => reader.update(cx, |reader, cx| reader.next_page(cx)),
            _ => {}
        }
    }

    pub fn preview_size(&self, larger: bool, cx: &mut App) {
        if let Document::Font(font) = self {
            font.update(cx, |font, cx| font.step_scale(larger, cx));
        }
    }

    /// The document's view. The unsupported view shows the thumbnail the
    /// media surface has loaded.
    pub fn element(&self, thumbnail: Option<Arc<RenderImage>>, cx: &App) -> AnyElement {
        match self {
            Document::Paged(paged) => paged.clone().into_any_element(),
            Document::Reader(reader) => reader.clone().into_any_element(),
            Document::Font(font) => font.clone().into_any_element(),
            Document::Unsupported { mime, path, scroll } => {
                unsupported::view(scroll, thumbnail, mime, path.clone(), cx).into_any_element()
            }
        }
    }
}

/// Runs a parser on a file that came from outside. A panic in it fails
/// like any other error: on GPUI's background threads a panic would end the
/// app, as it unwinds into the platform's thread pool, which cannot unwind.
pub fn guarded<T>(parse: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(parse))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("Could not read this file.")))
}

/// A decoded picture in the BGRA order GPUI draws.
pub fn render_image(mut image: image::RgbaImage) -> Arc<RenderImage> {
    for pixel in image.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        image
    )]))
}

#[cfg(test)]
mod tests {
    use super::guarded;

    #[test]
    fn a_parser_that_panics_fails_like_any_other() {
        let result: anyhow::Result<()> = guarded(|| panic!("a damaged file"));
        assert_eq!(result.unwrap_err().to_string(), "Could not read this file.");
        assert_eq!(guarded(|| Ok(7)).unwrap(), 7);
    }
}
