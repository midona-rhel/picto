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
mod shell;
pub mod unsupported;

#[cfg(test)]
mod matrix;

use font::FontViewer;
use gpui::*;
use kind::RendererKind;
use paged::{PagedViewer, ZoomChanged};
use reader::ReaderViewer;
use std::path::PathBuf;
use std::sync::Arc;

actions!(
    document,
    [PreviousPage, NextPage, PreviewLarger, PreviewSmaller]
);

/// `document.previousPage` and `document.nextPage` (J and L), and the font
/// preview's Cmd+= and Cmd+−, while the viewer has the keyboard.
pub fn bind(cx: &mut App) {
    let viewer = Some("Viewer && !Flash");
    cx.bind_keys([
        KeyBinding::new("j", PreviousPage, viewer),
        KeyBinding::new("l", NextPage, viewer),
        KeyBinding::new("cmd-=", PreviewLarger, viewer),
        KeyBinding::new("cmd--", PreviewSmaller, viewer),
    ]);
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
    /// other threads; the view shows what it has meanwhile.
    pub fn open(
        kind: RendererKind,
        path: PathBuf,
        mime: &str,
        name: SharedString,
        cx: &mut App,
    ) -> Self {
        match kind {
            RendererKind::Pdf | RendererKind::Djvu | RendererKind::Cbz => {
                Document::Paged(cx.new(|cx| PagedViewer::new(kind, path, cx)))
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
