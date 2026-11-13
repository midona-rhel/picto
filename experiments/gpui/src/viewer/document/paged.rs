//! PDF, DjVu and comic archives as one continuous column of pages
//! (`PdfViewer.tsx`, `DjvuViewer.tsx` and the CBZ half of
//! `ArchiveDocumentViewer.tsx` show one page at a time; here the pages
//! scroll). Every page is measured when the document opens, so the column
//! has its final shape at once: a page not drawn yet is a blank sheet of
//! exactly its size, and the first page shows the item's thumbnail until it
//! is drawn. Pages are drawn on the document's own thread at the size they
//! take on screen, only those in or near view, and a few are kept. PDF pages
//! zoom from the viewer toolbar.
//!
//! In the collection reader the pages run inline instead: as wide as the
//! member's frame, one under the other, scrolled by the reader.
use super::geometry::{self, Column, PageSize, ScaleMode};
use super::kind::RendererKind;
use super::pages::{PageWorker, RenderedPage};
use super::shell;
use crate::{theme, ui};
use gpui::*;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// The zoom changed, or whether the document zooms at all.
pub struct ZoomChanged;

/// The viewer can be shown (its pages are measured and the first page has
/// a picture), or its first page has been drawn.
pub struct Progressed;

/// `.viewport`'s padding around the column.
pub const PADDING: f32 = 24.;
/// The space between pages (`--space-4`).
pub const GAP: f32 = 16.;
/// The space between pages inline in the collection reader.
pub const INLINE_GAP: f32 = 8.;
/// Pages drawn ahead of the view, beyond those in it.
const AHEAD: usize = 1;
/// Drawn pages kept beyond those wanted, on each side.
const KEEP: usize = 2;

/// A document's pages inline at `width`, one under the other.
pub fn inline_column(sizes: &[PageSize], width: f32) -> Column {
    geometry::column(&inline_sizes(sizes, width), (width, 0.), 0., INLINE_GAP)
}

/// The height of a document's pages inline at `width`.
pub fn inline_height(sizes: &[PageSize], width: f32) -> f32 {
    inline_column(sizes, width).size.1
}

/// Each page's size inline: as wide as the frame.
fn inline_sizes(sizes: &[PageSize], width: f32) -> Vec<PageSize> {
    sizes
        .iter()
        .map(|natural| (width, width * natural.1 / natural.0.max(1.)))
        .collect()
}

/// A page's picture and the pixel size it was drawn at.
struct Drawn {
    pixels: (u32, u32),
    image: Arc<RenderImage>,
}

pub struct PagedViewer {
    kind: RendererKind,
    /// In the collection reader: inline, no scrolling and no footer.
    inline: bool,
    worker: Option<PageWorker>,
    /// Every page's natural size, once the document has opened.
    sizes: Vec<PageSize>,
    mode: ScaleMode,
    /// The item's thumbnail, shown on the first page until it is drawn, and
    /// whether its load has settled (a thumbnail that failed counts).
    thumbnail: Option<Arc<RenderImage>>,
    thumbnail_settled: bool,
    drawn: HashMap<usize, Drawn>,
    /// Pages asked of the document's thread, at what size. Dropping a
    /// request's task gives the page up.
    pending: HashMap<usize, ((u32, u32), Task<()>)>,
    /// The size each wanted page is drawn at, as the last frame laid out.
    wanted: HashMap<usize, (u32, u32)>,
    /// Pages that failed at a size, so they are not asked for again.
    failed: HashMap<usize, (u32, u32)>,
    error: Option<SharedString>,
    /// The whole view's size, footer included, as last laid out.
    frame: PageSize,
    /// Inline: the part of the column in view, from the reader's clip.
    in_view: Option<(f32, f32)>,
    scale_factor: f32,
    scroll: ScrollHandle,
    /// The zoom the toolbar was last told.
    reported: Option<(bool, f32)>,
    presented: bool,
    first_drawn: bool,
    _open_task: Option<Task<()>>,
}

impl EventEmitter<ZoomChanged> for PagedViewer {}
impl EventEmitter<Progressed> for PagedViewer {}

impl PagedViewer {
    pub fn new(kind: RendererKind, path: PathBuf, inline: bool, cx: &mut Context<Self>) -> Self {
        let opened = PageWorker::open(kind, path);
        let open_task = cx.spawn(async move |this, cx| {
            let result = opened.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok((worker, sizes))) => {
                        this.worker = Some(worker);
                        this.sizes = sizes;
                    }
                    Ok(Err(error)) => this.error = Some(error.to_string().into()),
                    Err(_) => this.error = Some(open_failure(kind).into()),
                }
                cx.notify();
            });
        });
        Self {
            kind,
            inline,
            worker: None,
            sizes: Vec::new(),
            mode: ScaleMode::PageFit,
            thumbnail: None,
            thumbnail_settled: false,
            drawn: HashMap::new(),
            pending: HashMap::new(),
            wanted: HashMap::new(),
            failed: HashMap::new(),
            error: None,
            frame: (0., 0.),
            in_view: None,
            scale_factor: 1.,
            scroll: ScrollHandle::new(),
            reported: None,
            presented: false,
            first_drawn: false,
            _open_task: Some(open_task),
        }
    }

    /// The item's thumbnail once its load has settled, `None` if it failed.
    pub fn set_thumbnail(&mut self, thumbnail: Option<Arc<RenderImage>>, cx: &mut Context<Self>) {
        if self.thumbnail_settled {
            return;
        }
        self.thumbnail = thumbnail;
        self.thumbnail_settled = true;
        cx.notify();
    }

    /// Whether the view has something to show without a gap: the column
    /// measured and laid out, with the first page's thumbnail or drawing;
    /// or the reason it cannot show the document.
    pub fn presentable(&self) -> bool {
        self.error.is_some()
            || (!self.sizes.is_empty()
                && self.frame.0 > 0.
                && (self.thumbnail_settled || self.drawn.contains_key(&0)))
    }

    /// Whether the first page in view has been drawn (or cannot be).
    pub fn settled(&self) -> bool {
        self.error.is_some() || self.first_drawn
    }

    /// Only PDF pages take the toolbar's zoom controls.
    pub fn zoomable(&self) -> bool {
        self.kind == RendererKind::Pdf
    }

    /// The scale of the page in the middle of the view, in percent, as the
    /// toolbar shows it.
    pub fn percent(&self) -> f32 {
        (self.scale() * 100.).round()
    }

    fn scale(&self) -> f32 {
        let natural = self
            .sizes
            .get(self.current_page())
            .copied()
            .unwrap_or((1., 1.));
        self.mode.column_scale(self.available_width(), natural)
    }

    /// The scrolling viewport: the view less its footer.
    fn viewport(&self) -> PageSize {
        if self.inline {
            (self.frame.0, 0.)
        } else {
            (
                self.frame.0,
                (self.frame.1 - theme::TITLEBAR_HEIGHT).max(0.),
            )
        }
    }

    /// The viewport's content box width (inside its padding).
    fn available_width(&self) -> f32 {
        (self.viewport().0 - 2. * PADDING).max(1.)
    }

    /// Each page's size on screen.
    fn page_sizes(&self, mode: ScaleMode) -> Vec<PageSize> {
        if self.inline {
            return inline_sizes(&self.sizes, self.frame.0);
        }
        let available = self.available_width();
        self.sizes
            .iter()
            .map(|natural| {
                let scale = mode.column_scale(available, *natural);
                (natural.0 * scale, natural.1 * scale)
            })
            .collect()
    }

    fn column_for(&self, mode: ScaleMode) -> Column {
        let sizes = self.page_sizes(mode);
        if self.inline {
            inline_column(&self.sizes, self.frame.0)
        } else {
            geometry::column(&sizes, self.viewport(), PADDING, GAP)
        }
    }

    fn column(&self) -> Column {
        self.column_for(self.mode)
    }

    /// The part of the column in view, top and bottom.
    fn view_range(&self) -> (f32, f32) {
        if self.inline {
            return self.in_view.unwrap_or((0., 0.));
        }
        let top = -f32::from(self.scroll.offset().y);
        (top, top + self.viewport().1)
    }

    /// The page the view is on, 0-based: the one in its middle; the first
    /// while the whole column fits, and the last once scrolled to the end.
    fn current_page(&self) -> usize {
        if self.sizes.is_empty() {
            return 0;
        }
        let column = self.column();
        let (top, bottom) = self.view_range();
        if self.inline || column.size.1 <= bottom - top + 1. {
            return 0;
        }
        if bottom >= column.size.1 - 1. {
            return column.pages.len() - 1;
        }
        column.page_at((top + bottom) / 2.)
    }

    fn set_mode(&mut self, mode: ScaleMode, cx: &mut Context<Self>) {
        if !self.zoomable() || self.inline {
            return;
        }
        // The point at the top of the view stays there, and the middle of
        // the view across.
        if !self.sizes.is_empty() && self.frame.0 > 0. {
            let before = self.column();
            let offset = self.scroll.offset();
            let (top, left) = (-f32::from(offset.y), -f32::from(offset.x));
            let across = (left + self.viewport().0 / 2.) / before.size.0.max(1.);
            if let Some(anchor) = before.anchor(top) {
                let after = self.column_for(mode);
                let y = after.resolve(anchor).unwrap_or(0.);
                let x = across * after.size.0 - self.viewport().0 / 2.;
                self.scroll_to(&after, x, y);
            }
        }
        self.mode = mode;
        cx.emit(ZoomChanged);
        cx.notify();
    }

    /// Scrolls so `(x, y)` of the column is at the view's top left, within
    /// what the column allows.
    fn scroll_to(&self, column: &Column, x: f32, y: f32) {
        let viewport = self.viewport();
        let x = x.clamp(0., (column.size.0 - viewport.0).max(0.));
        let y = y.clamp(0., (column.size.1 - viewport.1).max(0.));
        self.scroll.set_offset(point(px(-x), px(-y)));
    }

    fn scroll_vertically(
        &mut self,
        y: impl FnOnce(&Column, f32, f32) -> f32,
        cx: &mut Context<Self>,
    ) {
        if self.inline || self.sizes.is_empty() {
            return;
        }
        let column = self.column();
        let offset = self.scroll.offset();
        let top = -f32::from(offset.y);
        let target = y(&column, top, self.viewport().1);
        self.scroll_to(&column, -f32::from(offset.x), target);
        cx.notify();
    }

    /// `fitToWindow`
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::PageFit, cx);
    }

    /// `fitActual`
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::Actual, cx);
    }

    /// `setZoomScale`, from the slider and the zoom menu.
    pub fn set_percent(&mut self, percent: f32, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::custom(percent / 100.), cx);
    }

    /// `zoomIn` / `zoomOut`: one 1.2× step from the scale on screen.
    pub fn step_zoom(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        let factor = if zoom_in {
            geometry::STEP
        } else {
            1. / geometry::STEP
        };
        self.set_mode(ScaleMode::custom(self.scale() * factor), cx);
    }

    /// J: to the top of the page at the top of the view, or, already there,
    /// of the one before it.
    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        self.scroll_vertically(
            |column, top, _| {
                let current = column.page_at(top + PADDING + 1.);
                let start = column.pages[current].1 - PADDING;
                if top > start + 1. || current == 0 {
                    start
                } else {
                    column.pages[current - 1].1 - PADDING
                }
            },
            cx,
        );
    }

    /// L: to the top of the next page.
    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        self.scroll_vertically(
            |column, top, _| {
                let current = column.page_at(top + PADDING + 1.);
                match column.pages.get(current + 1) {
                    Some(next) => next.1 - PADDING,
                    None => top,
                }
            },
            cx,
        );
    }

    /// Home and End.
    pub fn scroll_to_end(&mut self, end: bool, cx: &mut Context<Self>) {
        self.scroll_vertically(|column, _, _| if end { column.size.1 } else { 0. }, cx);
    }

    /// Page Up and Page Down, and Space: a view's height less a little, as
    /// a browser scrolls a page.
    pub fn scroll_page(&mut self, down: bool, cx: &mut Context<Self>) {
        self.scroll_vertically(
            |_, top, height| {
                let step = (height * 0.875).max(40.);
                if down { top + step } else { top - step }
            },
            cx,
        );
    }

    /// The pages to have drawn: those in view and one more each way, the
    /// one in the middle first.
    fn wanted_pages(&self, column: &Column) -> Vec<usize> {
        let (top, bottom) = self.view_range();
        if bottom <= top {
            return Vec::new();
        }
        let range: Range<usize> = column.between(top, bottom);
        if range.is_empty() {
            return Vec::new();
        }
        let start = range.start.saturating_sub(AHEAD);
        let end = (range.end + AHEAD).min(column.pages.len());
        let middle = column.page_at((top + bottom) / 2.).clamp(start, end - 1);
        let mut pages: Vec<usize> = (start..end).collect();
        pages.sort_by_key(|page| page.abs_diff(middle));
        pages
    }

    /// Asks for the pages in view at their size on screen, gives up those
    /// that left it, and lets go of drawings far from it.
    fn request(&mut self, column: &Column, cx: &mut Context<Self>) {
        let Some(worker) = &self.worker else {
            return;
        };
        let wanted: HashMap<usize, (u32, u32)> = self
            .wanted_pages(column)
            .into_iter()
            .filter_map(|index| {
                let page = column.pages.get(index)?;
                Some((
                    index,
                    geometry::page_pixels((page.2, page.3), self.scale_factor)?,
                ))
            })
            .collect();
        let mut order: Vec<usize> = wanted.keys().copied().collect();
        let middle = self.current_page();
        order.sort_by_key(|page| page.abs_diff(middle));
        self.pending
            .retain(|index, (pixels, _)| wanted.get(index) == Some(pixels));
        for index in order {
            let pixels = wanted[&index];
            let done = self.drawn.get(&index).is_some_and(|d| d.pixels == pixels);
            let failed = self.failed.get(&index) == Some(&pixels);
            if done || failed || self.pending.contains_key(&index) {
                continue;
            }
            let answer = worker.render(index, pixels);
            let task = cx.spawn(async move |this, cx| {
                let result = answer.await;
                let _ = this.update(cx, |this, cx| this.arrived(index, pixels, result, cx));
            });
            self.pending.insert(index, (pixels, task));
        }
        // Drawings beyond the kept margin go.
        if let (Some(first), Some(last)) = (wanted.keys().min(), wanted.keys().max()) {
            let keep = first.saturating_sub(KEEP)..last + KEEP + 1;
            let far: Vec<usize> = self
                .drawn
                .keys()
                .copied()
                .filter(|index| !keep.contains(index))
                .collect();
            for index in far {
                if let Some(drawn) = self.drawn.remove(&index) {
                    cx.drop_image(drawn.image, None);
                }
            }
        }
        self.wanted = wanted;
    }

    fn arrived(
        &mut self,
        index: usize,
        pixels: (u32, u32),
        result: Result<anyhow::Result<RenderedPage>, futures::channel::oneshot::Canceled>,
        cx: &mut Context<Self>,
    ) {
        // This answer's own request is done; its task is the one running
        // now, so it is let go of rather than cancelled.
        if self
            .pending
            .get(&index)
            .is_some_and(|(asked, _)| *asked == pixels)
            && let Some((_, task)) = self.pending.remove(&index)
        {
            task.detach();
        }
        match result {
            Ok(Ok(page)) => {
                let held = self.drawn.get(&index).map(|drawn| drawn.pixels);
                if !geometry::keep_drawn(page.pixels, self.wanted.get(&index).copied(), held) {
                    return;
                }
                let image = super::render_image(page.image);
                if let Some(old) = self.drawn.insert(index, Drawn { pixels, image }) {
                    cx.drop_image(old.image, None);
                }
                if !self.first_drawn {
                    self.first_drawn = true;
                    cx.emit(Progressed);
                }
            }
            Ok(Err(error)) => {
                eprintln!("picto: page {} of a document: {error}", index + 1);
                self.failed.insert(index, pixels);
                // The first page failing still settles the view: its
                // thumbnail stays.
                if !self.first_drawn {
                    self.first_drawn = true;
                    cx.emit(Progressed);
                }
            }
            // The document's thread is gone (its decoder failed past
            // recovery); nothing more will be drawn.
            Err(_) => self.error = Some(render_failure(self.kind).into()),
        }
        cx.notify();
    }

    /// One page: its drawing, else (the first page) the thumbnail, else a
    /// blank sheet, at its place in the column.
    fn page(&self, index: usize, place: geometry::PageRect) -> Div {
        let shadow = if self.inline {
            Vec::new()
        } else if self.kind == RendererKind::Pdf {
            theme::page_shadow()
        } else {
            theme::sheet_shadow()
        };
        let picture = self
            .drawn
            .get(&index)
            .map(|drawn| drawn.image.clone())
            .or_else(|| self.thumbnail.clone().filter(|_| index == 0));
        div()
            .absolute()
            .left(px(place.0))
            .top(px(place.1))
            .w(px(place.2))
            .h(px(place.3))
            .bg(rgb(theme::DOCUMENT_PAPER))
            .shadow(shadow)
            .children(picture.map(|picture| img(picture).size_full().object_fit(ObjectFit::Fill)))
    }
}

fn open_failure(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Pdf => "Could not open this PDF.",
        RendererKind::Djvu => "Could not open this DjVu document.",
        _ => "Could not open this CBZ.",
    }
}

fn render_failure(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Pdf => "Could not render this PDF.",
        RendererKind::Djvu => "Could not render this DjVu document.",
        _ => "Could not render this CBZ.",
    }
}

impl Render for PagedViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scale_factor = window.scale_factor();
        let entity = cx.entity().downgrade();
        let inline = self.inline;
        // The view's size, and inline the part of it the reader shows.
        let measure = canvas(
            move |bounds, window, cx| {
                let frame = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let clip = window.content_mask().bounds.intersect(&bounds);
                let in_view = (
                    f32::from(clip.top() - bounds.top()),
                    f32::from(clip.bottom() - bounds.top()),
                );
                let _ = entity.update(cx, |this, cx| {
                    let moved = inline
                        && this.in_view.is_none_or(|(top, bottom)| {
                            (top - in_view.0).abs() >= 1. || (bottom - in_view.1).abs() >= 1.
                        });
                    if this.frame != frame || moved {
                        this.frame = frame;
                        this.in_view = Some(in_view);
                        cx.notify();
                        // Notified while drawing: ask for the frame too.
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let root = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(measure);

        let column = self.column();
        if self.error.is_none() && self.frame.0 > 0. {
            self.request(&column, cx);
        }
        if !self.presentable() {
            // Nothing until the column can be shown whole: the host keeps
            // what it showed before.
            return root;
        }
        if !self.presented {
            self.presented = true;
            cx.emit(Progressed);
        }
        let zoom = (self.zoomable(), self.percent());
        if self.reported != Some(zoom) {
            self.reported = Some(zoom);
            cx.emit(ZoomChanged);
        }

        let error = self.error.clone().map(|error| {
            // `.message`: 12px on a 1.5 line, centred.
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(theme::TEXT_SM * 1.5))
                .text_color(theme::theme(cx).text_secondary())
                .child(error)
        });
        let pages: Vec<Div> = column
            .pages
            .iter()
            .enumerate()
            .map(|(index, place)| self.page(index, *place))
            .collect();
        if self.inline {
            return root.children(pages).children(error);
        }

        let viewport = div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(rgb(theme::DOCUMENT_BACKDROP))
            .child(
                div()
                    .id("document-pages")
                    .track_scroll(&self.scroll)
                    .overflow_scroll()
                    .size_full()
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w(px(column.size.0))
                            .h(px(column.size.1))
                            .children(if error.is_some() { Vec::new() } else { pages }),
                    ),
            )
            .children(error)
            .child(ui::scrollbar(
                "document-scrollbar",
                &self.scroll,
                window,
                cx,
            ));
        let status = if self.sizes.is_empty() {
            None
        } else {
            Some(shell::page_status_label(
                self.current_page() + 1,
                self.sizes.len(),
                cx,
            ))
        };
        let theme = theme::theme(cx);
        root.flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .child(viewport)
            .child(shell::footer(cx).children(status))
    }
}
