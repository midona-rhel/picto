//! The viewers of every kind of media other than images, Flash, video and
//! audio: PDF, DjVu and comic pages; text, Markdown, JSON, RTF and EPUB on
//! a page; fonts; and the view for files that cannot be shown. The media
//! surface asks [`kind::renderer_kind`] which viewer an item gets, and
//! opens a [`Document`] for an item of a document kind.
pub mod epub;
pub mod font;
pub mod geometry;
pub mod kind;
pub mod page_picture;
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
        PreviewReset,
        ScrollToStart,
        ScrollToEnd,
        ScrollPageUp,
        ScrollPageDown
    ]
);

/// The key context that a view adds while it shows a paged document. The
/// keys that scroll the pages (`scroll_bindings`) are bound in this context
/// only.
pub const PAGES_CONTEXT: &str = "Pages";

/// Returns the bindings of the keys that scroll a paged document: Home,
/// End, Page Up, Page Down, Space and Shift+Space. Space otherwise closes
/// the media view. These bindings must therefore be added after the
/// shortcut registry's bindings, so that they take precedence over them in
/// a view that shows pages.
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

/// Binds the keys that change the size of the font preview, in the media
/// view and in a detail window. These keys are not in the shortcut
/// registry. `=` or `+` (Shift+= on most layouts) makes the preview larger
/// and `-` makes it smaller, with Command or Control on the Mac and with
/// Control elsewhere. Command+0 (Control+0 elsewhere) is the registry's
/// Actual Size shortcut, which also sets the preview back to its first
/// size, so only Control+0 on the Mac is bound here for that. When the
/// open item is not a font, these keys change the interface zoom. The J
/// and L keys, which the registry gives to Previous Page and Next Page,
/// are bound by `crate::keymap`.
pub fn bind(cx: &mut App) {
    let modifiers: &[&str] = if cfg!(target_os = "macos") {
        &["Mod", "Ctrl"]
    } else {
        &["Mod"]
    };
    for scope in [Scope::Viewer, Scope::DetailWindow] {
        let predicate = scope.predicate();
        let context = predicate.as_deref();
        for modifier in modifiers {
            crate::keymap::bind_written(
                &format!("{modifier}+="),
                Box::new(PreviewLarger),
                context,
                cx,
            );
            crate::keymap::bind_written(
                &format!("{modifier}++"),
                Box::new(PreviewLarger),
                context,
                cx,
            );
            crate::keymap::bind_written(
                &format!("{modifier}+-"),
                Box::new(PreviewSmaller),
                context,
                cx,
            );
        }
        if cfg!(target_os = "macos") {
            crate::keymap::bind_written("Ctrl+0", Box::new(PreviewReset), context, cx);
        }
    }
}

/// How far a key scrolls the pages of a paged document: to the start, to
/// the end, or by about one viewport's height up or down.
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
    Unsupported { mime: String, scroll: ScrollHandle },
}

impl Document {
    /// Opens the file at `path`, which is of the document kind `kind`, and
    /// returns the view that shows it. The file is read and drawn on other
    /// threads, and the view shows what it has until they finish. With
    /// `inline` true, a paged document lays its pages out for the
    /// collection reader, which shows it as a member of a collection.
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
            // Every other kind has no viewer, so the item gets the view for
            // files that cannot be shown.
            _ => Document::Unsupported {
                mime: mime.to_owned(),
                scroll: ScrollHandle::new(),
            },
        }
    }

    /// Calls `changed` whenever the zoom of a paged document may have
    /// changed, so that the viewer toolbar can show the new zoom. Returns
    /// `None` for any other document, whose zoom never changes.
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

    /// Calls `progressed` when `presentable` begins to return true for a
    /// paged document, and again when `settled` begins to return true for
    /// it. Returns `None` for any other document, for which both return
    /// true from the start.
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

    /// Gives a paged document the item's thumbnail, which it draws on its
    /// first page until that page has been drawn. The caller calls this
    /// when the loading of the thumbnail has ended, and passes `None` when
    /// the thumbnail could not be loaded. Any other document ignores it.
    pub fn set_thumbnail(&self, thumbnail: Option<Arc<RenderImage>>, cx: &mut App) {
        if let Document::Paged(paged) = self {
            paged.update(cx, |paged, cx| paged.set_thumbnail(thumbnail, cx));
        }
    }

    /// Returns whether the document has what it needs to be drawn in place
    /// of what was shown before it. A paged document returns false until
    /// its pages have been measured and either the loading of the item's
    /// thumbnail has ended or its first page has been drawn
    /// (`PagedViewer::presentable`). Every other document returns true at
    /// once.
    pub fn presentable(&self, cx: &App) -> bool {
        match self {
            Document::Paged(paged) => paged.read(cx).presentable(),
            _ => true,
        }
    }

    /// Returns whether the document has drawn its content. A paged
    /// document returns false until the first drawing of one of its pages
    /// has arrived or has failed, or the document could not be opened
    /// (`PagedViewer::settled`). Every other document returns true at
    /// once.
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

    /// Returns whether this is a paged document. Only a paged document has
    /// pages that Home, End, Page Up, Page Down and Space scroll.
    pub fn paged(&self) -> bool {
        matches!(self, Document::Paged(_))
    }

    /// Returns whether the zoom controls of the viewer toolbar change this
    /// document's zoom. They do so for the pages of a PDF only.
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

    /// Carries out the Actual Size command: shows the pages of a PDF at
    /// 100% and sets a font preview back to 100%, the scale it opens with.
    /// Any other document does nothing.
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

    /// The document's view.
    pub fn element(&self) -> AnyElement {
        match self {
            Document::Paged(paged) => paged.clone().into_any_element(),
            Document::Reader(reader) => reader.clone().into_any_element(),
            Document::Font(font) => font.clone().into_any_element(),
            Document::Unsupported { mime, scroll } => {
                unsupported::view(scroll, mime).into_any_element()
            }
        }
    }
}

/// Runs `parse`, which reads a file that the app did not write, and
/// returns its result. A panic in `parse` is returned as the error "Could
/// not read this file." On GPUI's background threads a panic would
/// otherwise end the app, because it unwinds into the platform's thread
/// pool, which cannot unwind.
pub fn guarded<T>(parse: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(parse))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("Could not read this file.")))
}

/// Converts a decoded picture from RGBA to the BGRA byte order that GPUI
/// draws, and returns it as a picture GPUI can draw.
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
