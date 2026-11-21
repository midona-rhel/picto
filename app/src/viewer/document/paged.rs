//! The viewer of the documents whose pages are drawn as pictures: PDF files,
//! DjVu files and comic archives (CBZ). It shows all the pages of a document
//! in one column, one under the other, and the column scrolls.
//!
//! The document is opened on a thread that is started for it
//! (`pages::PageWorker`), and that thread first measures every page. When the
//! sizes arrive, the viewer lays out the whole column, so the column has its
//! final height before any page has been drawn. A page that has not been
//! drawn yet is shown as a white rectangle of the page's size. The first page
//! shows the item's thumbnail until that page has been drawn.
//!
//! The same thread draws the pages. The viewer asks it only for the pages of
//! which a part is visible, and for one page before them and one page after
//! them. Each page is drawn at the size in device pixels that it has on
//! screen. The viewer keeps the drawings of those pages and of two more
//! pages on each side. It releases the drawings of all other pages, which
//! takes them out of GPUI's sprite atlas.
//!
//! The zoom controls of the viewer toolbar change the scale of the pages of
//! a PDF. The pages of a DjVu file or a comic archive cannot be zoomed: a
//! page that is too wide for the viewport is scaled down until it fits
//! between the viewport's padding, and any other page is shown at its
//! natural size.
//!
//! When the document is a member of a collection, the collection reader
//! (`viewer/group.rs`) shows it inline. The pages are then as wide as the
//! member's frame, one under the other with 8 px between them, and the
//! collection reader scrolls them. The viewer draws no backdrop, no
//! scrollbar and no footer in that case.
use super::geometry::{self, Column, PageSize, ScaleMode};
use super::kind::RendererKind;
use super::pages::{PageWorker, RenderedPage};
use super::shell;
use crate::i18n::t;
use crate::{theme, ui};
use gpui::*;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// The event the viewer emits when the zoom that the viewer toolbar shows
/// may have changed. It is emitted when a zoom control changes the scale
/// mode (`set_mode`). It is also emitted when `build` builds the first frame
/// in which `presentable` returns true, and when it builds a later frame in
/// which `zoomable` or `percent` returns another value than in the frame
/// before, for example after the view's width has changed. The media
/// surface then reads `zoomable` and `percent` again.
pub struct ZoomChanged;

/// The event the viewer emits when the opening of the document has got
/// further. It is emitted at most twice. The first time is in the first
/// frame in which `presentable` returns true. The second time is when
/// `settled` begins to return true because the first drawing of a page has
/// arrived or the drawing of a page has failed.
pub struct Progressed;

/// The padding in pixels between the edges of the scrolling viewport and
/// the column of pages. It is equal to `shell::VIEWPORT_PADDING`, the
/// padding around the page of the other document viewers.
pub const PADDING: f32 = 24.;
/// The space in pixels between two pages of the column.
pub const GAP: f32 = 16.;
/// The space in pixels between two pages of a document that the collection
/// reader shows inline.
pub const INLINE_GAP: f32 = 8.;
/// The number of pages before the first visible page, and the number of
/// pages after the last visible page, that are drawn as well as the visible
/// pages.
const AHEAD: usize = 1;
/// The number of pages before the first wanted page, and the number of
/// pages after the last wanted page, whose drawings are kept. The wanted
/// pages are the visible pages and the `AHEAD` pages on each side of them
/// (`wanted_pages`). `request` releases the drawings of all other pages.
const KEEP: usize = 2;

/// Lays out the pages of a document as the collection reader shows them
/// inline. Each page is `width` pixels wide and keeps its proportions. The
/// pages are one under the other from the top of the column, with 8 px
/// (`INLINE_GAP`) between two pages and no padding around them. `sizes`
/// holds the natural size of each page.
pub fn inline_column(sizes: &[PageSize], width: f32) -> Column {
    geometry::column(&inline_sizes(sizes, width), (width, 0.), 0., INLINE_GAP)
}

/// Returns the height in pixels of the column that `inline_column` lays
/// out: the heights of all the pages at `width`, plus the 8 px between each
/// two pages. The collection reader gives a member that is a document this
/// height.
pub fn inline_height(sizes: &[PageSize], width: f32) -> f32 {
    inline_column(sizes, width).size.1
}

/// Returns the width and height of each page when the page is `width`
/// pixels wide and keeps the proportions of its natural size in `sizes`. A
/// natural width of less than 1 is counted as 1, so that the height is
/// never divided by 0.
fn inline_sizes(sizes: &[PageSize], width: f32) -> Vec<PageSize> {
    sizes
        .iter()
        .map(|natural| (width, width * natural.1 / natural.0.max(1.)))
        .collect()
}

/// The drawing of one page that the viewer holds.
struct Drawn {
    /// The width and height in device pixels that the viewer asked the
    /// document's thread to draw the page at. `request` compares it with
    /// the size the page needs in the current frame. The picture in `image`
    /// is smaller than this when a side of this size is longer than 8192
    /// pixels (`pages::MAX_RENDER_SIDE`), because the thread draws no
    /// picture larger than that.
    pixels: (u32, u32),
    /// The picture of the page.
    image: Arc<RenderImage>,
}

/// The view that shows one PDF file, DjVu file or comic archive. The
/// documentation at the top of this file describes what it does.
pub struct PagedViewer {
    /// Which of the three kinds of document this is. Only a PDF can be
    /// zoomed, and a PDF's pages have a different shadow from the pages of
    /// the other two kinds.
    kind: RendererKind,
    /// True when the document is a member of a collection and the
    /// collection reader shows it inline. The pages are then as wide as the
    /// view, and the viewer draws no backdrop, no page shadows, no
    /// scrollbar and no footer. The viewer does not scroll, and its zoom
    /// controls and page keys do nothing.
    inline: bool,
    /// The handle of the document's thread, through which the viewer asks
    /// for drawings of pages. It is `None` until the thread has opened the
    /// document, and it stays `None` when the document cannot be opened.
    worker: Option<PageWorker>,
    /// The natural width and height of every page: for a PDF in points,
    /// each of which is drawn as one logical pixel at a scale of 100%, and
    /// for a DjVu file or a comic archive in pixels. It is empty until the
    /// document has opened.
    sizes: Vec<PageSize>,
    /// How the pages are scaled. It starts as `ScaleMode::PageFit`, and
    /// only the zoom controls of a PDF that is not inline change it. An
    /// inline document does not use it for the size of its pages.
    mode: ScaleMode,
    /// The item's thumbnail, which is drawn on the first page until the
    /// first page has a drawing. It is `None` until `set_thumbnail` is
    /// called, and after that when the thumbnail could not be loaded.
    thumbnail: Option<Arc<RenderImage>>,
    /// True once `set_thumbnail` has been called, which means that the
    /// loading of the thumbnail has ended, whether it succeeded or failed.
    thumbnail_settled: bool,
    /// The drawings the viewer holds, by the index of their page, counted
    /// from 0.
    drawn: HashMap<usize, Drawn>,
    /// The requests for drawings that the document's thread has not
    /// answered yet, by the index of their page. Each entry holds the
    /// width and height in device pixels that was asked for, and the task
    /// that waits for the answer. Removing an entry drops its task. The
    /// thread then finds that nothing waits for the answer any more and
    /// does not draw the page.
    pending: HashMap<usize, ((u32, u32), Task<()>)>,
    /// The pages that the last call of `request` wanted to have drawings
    /// of, by their index, each with the width and height in device pixels
    /// that its drawing should have. `arrived` reads it to decide whether a
    /// drawing that arrives is kept.
    wanted: HashMap<usize, (u32, u32)>,
    /// The pages whose drawing failed, by their index, each with the width
    /// and height in device pixels that was asked for. `request` does not
    /// ask again for a page at the size at which it failed. It does ask
    /// again when the page is wanted at another size.
    failed: HashMap<usize, (u32, u32)>,
    /// The message that is shown in the centre of the view when the
    /// document cannot be shown. It is set when the document cannot be
    /// opened, and when the document's thread ends while the viewer waits
    /// for a drawing. A document that is not inline shows it in place of
    /// the pages. An inline document shows it over the pages.
    error: Option<SharedString>,
    /// The width and height of the whole view, footer included, in the
    /// frame that was built last. `build` sets it at its start.
    frame: PageSize,
    /// For a document that is a member of a collection: the top and the
    /// bottom of the part of the view that the collection reader shows,
    /// in pixels from the view's top. `build` sets it at its start. It is
    /// `None` before the first frame.
    in_view: Option<(f32, f32)>,
    /// The number of device pixels in one logical pixel of the window in
    /// the frame that was built last. `build` sets it at its start. A page
    /// is drawn at its size on screen multiplied by this number.
    scale_factor: f32,
    /// The scroll position of the viewport. An inline document does not
    /// use it.
    scroll: ScrollHandle,
    /// What `zoomable` and `percent` returned when `build` last emitted
    /// `ZoomChanged`. `build` emits the event again when they return
    /// another pair of values. It is `None` until the first frame in which
    /// `presentable` returned true.
    reported: Option<(bool, f32)>,
    /// True once `build` has emitted `Progressed` for the first frame in
    /// which `presentable` returned true.
    presented: bool,
    /// True once the document's thread has answered a request for a page
    /// with a drawing that the viewer kept, or with an error. The page is
    /// the first one whose answer arrived, which is not always the
    /// document's first page. `settled` returns true from then on.
    first_drawn: bool,
    /// The task that waits for the document's thread to open the document
    /// and measure its pages, and then stores the result in the viewer. It
    /// is kept here because dropping a task cancels it.
    _open_task: Option<Task<()>>,
}

impl EventEmitter<ZoomChanged> for PagedViewer {}
impl EventEmitter<Progressed> for PagedViewer {}

impl PagedViewer {
    /// Makes the viewer of the file at `path`, which is a document of
    /// `kind`, and starts the document's thread. That thread opens the file
    /// and measures every page. When its answer arrives, the viewer stores
    /// the thread's handle and the page sizes and is drawn again. If the
    /// document cannot be opened, the viewer stores the reason as its error
    /// message instead. If the thread ends without an answer, the message
    /// is the one `open_failure` returns. `inline` is true when the
    /// collection reader shows the document as a member of a collection.
    pub fn new(kind: RendererKind, path: PathBuf, inline: bool, cx: &mut Context<Self>) -> Self {
        // When the viewer is released, its drawings and the item's
        // thumbnail are taken out of GPUI's sprite atlas, which otherwise
        // keeps every picture that was drawn. If the media surface still
        // draws the thumbnail after that, GPUI puts the thumbnail into the
        // atlas again.
        cx.on_release(|this, cx| {
            let pages = this.drawn.drain().map(|(_, drawn)| drawn.image);
            for image in pages.chain(this.thumbnail.take()) {
                crate::atlas::release(image, cx);
            }
        })
        .detach();
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

    /// Gives the viewer the item's thumbnail. The media surface calls this
    /// when the loading of the thumbnail has ended, and passes `None` when
    /// the thumbnail could not be loaded. The viewer draws the thumbnail on
    /// the first page until the first page has a drawing. Only the first
    /// call has an effect.
    pub fn set_thumbnail(&mut self, thumbnail: Option<Arc<RenderImage>>, cx: &mut Context<Self>) {
        if self.thumbnail_settled {
            return;
        }
        self.thumbnail = thumbnail;
        self.thumbnail_settled = true;
        cx.notify();
    }

    /// Returns true when the viewer has what it needs to draw the document
    /// in place of what its host showed before. That is the case when the
    /// viewer has an error message to show. Without an error message, it is
    /// the case when all three of these are true: the pages have been
    /// measured, a frame has been built for a view that is wider than 0,
    /// and either the loading of the thumbnail has ended or the first page
    /// has a drawing. Until then `build` draws nothing.
    pub fn presentable(&self) -> bool {
        self.error.is_some()
            || (!self.sizes.is_empty()
                && self.frame.0 > 0.
                && (self.thumbnail_settled || self.drawn.contains_key(&0)))
    }

    /// Returns true once the first drawing of a page has arrived, or the
    /// drawing of a page has failed, or the viewer has an error message.
    /// The collection reader draws a member's thumbnail over the member
    /// until this returns true, and then fades the thumbnail out.
    pub fn settled(&self) -> bool {
        self.error.is_some() || self.first_drawn
    }

    /// Returns true when the document is a PDF. The zoom controls of the
    /// viewer toolbar change the scale of a PDF's pages only.
    pub fn zoomable(&self) -> bool {
        self.kind == RendererKind::Pdf
    }

    /// Returns the scale of the current page (`current_page`) in percent,
    /// rounded to a whole number. At 100 the page has its natural size. The
    /// viewer toolbar shows this number.
    pub fn percent(&self) -> f32 {
        (self.scale() * 100.).round()
    }

    /// Returns the factor by which the scale mode in use multiplies the
    /// natural size of the current page (`ScaleMode::column_scale`). Before
    /// the pages have been measured, it is computed for a page that is 1
    /// wide and 1 high. An inline document does not size its pages by this
    /// factor (`page_sizes`).
    fn scale(&self) -> f32 {
        let natural = self
            .sizes
            .get(self.current_page())
            .copied()
            .unwrap_or((1., 1.));
        self.mode.column_scale(self.available_width(), natural)
    }

    /// Returns the width and height in pixels of the viewport, which is
    /// the part of the view in which the pages scroll: the view's width,
    /// and the view's height less the height of the footer
    /// (`theme::TITLEBAR_HEIGHT`). An inline document has no footer and
    /// does not scroll, and the height returned for it is 0.
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

    /// Returns the width in pixels that a page can have and still fit into
    /// the viewport with its padding: the viewport's width less 24 px
    /// (`PADDING`) on each side. The result is never less than 1.
    fn available_width(&self) -> f32 {
        (self.viewport().0 - 2. * PADDING).max(1.)
    }

    /// Returns the width and height in pixels that each page has on screen
    /// under `mode`. A page of an inline document is as wide as the view,
    /// whatever `mode` is. Any other page has its natural size multiplied
    /// by `mode.column_scale`. With `PageFit`, a page that is wider than
    /// `available_width` is scaled down to that width, and a page that is
    /// not wider keeps its natural size. With `Actual`, every page has its
    /// natural size. With `Custom`, every page is scaled by the custom
    /// factor.
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

    /// Lays the pages out under `mode` and returns the column: the position
    /// and size of every page, and the size of the whole content. An inline
    /// document is laid out by `inline_column` for the view's width. Any
    /// other document has 24 px (`PADDING`) around the pages and 16 px
    /// (`GAP`) between two pages. Each page is centred horizontally, and a
    /// column that is shorter than the viewport is centred vertically in
    /// it (`geometry::column`).
    fn column_for(&self, mode: ScaleMode) -> Column {
        let sizes = self.page_sizes(mode);
        if self.inline {
            inline_column(&self.sizes, self.frame.0)
        } else {
            geometry::column(&sizes, self.viewport(), PADDING, GAP)
        }
    }

    /// Returns the column under the scale mode in use.
    fn column(&self) -> Column {
        self.column_for(self.mode)
    }

    /// Returns the top and the bottom of the part of the column that is
    /// visible, in pixels from the column's top. For a document that
    /// scrolls, the top is how far the viewport is scrolled down, and the
    /// bottom is the top plus the viewport's height. For an inline
    /// document, they are the values in `in_view`, and both are 0 before
    /// the first frame.
    fn view_range(&self) -> (f32, f32) {
        if self.inline {
            return self.in_view.unwrap_or((0., 0.));
        }
        let top = -f32::from(self.scroll.offset().y);
        (top, top + self.viewport().1)
    }

    /// Returns the index, counted from 0, of the current page. The footer
    /// shows its number in "Page n of m", and the toolbar shows its scale.
    /// It is the page at the vertical middle of the visible part of the
    /// column, or the last page that starts above the middle when the
    /// middle is between two pages. There are two exceptions. It is the
    /// first page when the document is inline, when no page has been
    /// measured, and when the whole column is at most 1 px higher than the
    /// viewport. It is the last page when the viewport is scrolled to
    /// within 1 px of the column's end.
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

    /// Changes the scale mode to `mode`, scrolls so that the same part of
    /// the document stays in view, emits `ZoomChanged` and asks for the
    /// viewer to be drawn again. It does nothing for a document that is
    /// not a PDF and for an inline document.
    fn set_mode(&mut self, mode: ScaleMode, cx: &mut Context<Self>) {
        if !self.zoomable() || self.inline {
            return;
        }
        // Scroll the column as it is laid out under `mode`, so that two
        // points of the document stay where they are in the viewport.
        // Vertically, the point of the page that is at the viewport's top
        // edge stays at the top edge: `anchor` is that page and how far
        // down the page the point is, as a fraction of the page's height.
        // Horizontally, the point of the column that is in the middle of
        // the viewport stays in the middle: `across` is that point's
        // distance from the column's left edge, as a fraction of the
        // column's width.
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

    /// Scrolls the viewport so that the point (`x`, `y`) of `column`, in
    /// pixels from the column's top-left corner, is at the viewport's
    /// top-left corner. `x` and `y` are first limited to the positions at
    /// which the viewport does not show anything outside the column: each
    /// is at least 0 and at most the column's size less the viewport's
    /// size in that direction.
    fn scroll_to(&self, column: &Column, x: f32, y: f32) {
        let viewport = self.viewport();
        let x = x.clamp(0., (column.size.0 - viewport.0).max(0.));
        let y = y.clamp(0., (column.size.1 - viewport.1).max(0.));
        self.scroll.set_offset(point(px(-x), px(-y)));
    }

    /// Scrolls the viewport up or down to the position that `y` returns,
    /// and keeps its horizontal position. `y` is called with the column,
    /// with how far the viewport is scrolled down now in pixels, and with
    /// the viewport's height. It returns how far the viewport should be
    /// scrolled down, and `scroll_to` limits that to the column. This does
    /// nothing for an inline document and before the pages have been
    /// measured.
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

    /// Sets the scale mode to `PageFit`: a page that is wider than the
    /// viewport has room for is scaled down to fit, and no page is made
    /// larger than its natural size. The media surface calls this for the
    /// "Fit to window" control of the viewer toolbar and for its shortcut.
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::PageFit, cx);
    }

    /// Sets the scale mode to `Actual`: every page has its natural size,
    /// which the toolbar shows as 100%. The media surface calls this for
    /// the "Actual size" control of the viewer toolbar and for its
    /// shortcut.
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::Actual, cx);
    }

    /// Scales every page to `percent` percent of its natural size. A value
    /// below 25 is treated as 25, and a value above 800 as 800. The zoom
    /// slider and the zoom menu of the viewer toolbar call this.
    pub fn set_percent(&mut self, percent: f32, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::custom(percent / 100.), cx);
    }

    /// Zooms in or out by one step. The scale of the current page is
    /// multiplied by 1.2 (`geometry::STEP`) when `zoom_in` is true and
    /// divided by 1.2 when it is false. The result, kept between 25% and
    /// 800%, becomes the scale of every page. The Zoom In and Zoom Out
    /// shortcuts call this.
    pub fn step_zoom(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        let factor = if zoom_in {
            geometry::STEP
        } else {
            1. / geometry::STEP
        };
        self.set_mode(ScaleMode::custom(self.scale() * factor), cx);
    }

    /// Scrolls back to the start of a page. The Previous Page shortcut
    /// (`document.previousPage`, the J key by default) calls this. A page
    /// is at its start when its top edge is 24 px (`PADDING`) below the
    /// viewport's top edge. The page at the top of the viewport is taken
    /// to be the last page whose top edge is at most 25 px below the
    /// viewport's top edge. When the viewport is scrolled more than 1 px
    /// past the start of that page, this scrolls to the start of that
    /// page. Otherwise it scrolls to the start of the page before it. For
    /// the first page it always scrolls to the start of the first page.
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

    /// Scrolls to the start of the page after the page at the top of the
    /// viewport. The Next Page shortcut (`document.nextPage`, the L key by
    /// default) calls this. The documentation of `previous_page` says what
    /// the start of a page is and which page counts as the page at the top
    /// of the viewport. When that page is the last page, the viewport does
    /// not move.
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

    /// Scrolls to the bottom of the column when `end` is true, which is
    /// what the End key does, and to the top of the column when `end` is
    /// false, which is what the Home key does.
    pub fn scroll_to_end(&mut self, end: bool, cx: &mut Context<Self>) {
        self.scroll_vertically(|column, _, _| if end { column.size.1 } else { 0. }, cx);
    }

    /// Scrolls by 87.5% of the viewport's height, and by at least 40 px.
    /// It scrolls down when `down` is true, which is what the Page Down
    /// key and the Space key do. It scrolls up when `down` is false, which
    /// is what the Page Up key and Shift+Space do.
    pub fn scroll_page(&mut self, down: bool, cx: &mut Context<Self>) {
        self.scroll_vertically(
            |_, top, height| {
                let step = (height * 0.875).max(40.);
                if down { top + step } else { top - step }
            },
            cx,
        );
    }

    /// Returns the indexes of the pages that should have a drawing: every
    /// page of which a part is in the visible part of the column, the
    /// `AHEAD` (1) pages before the first of them and the `AHEAD` pages
    /// after the last of them. They are ordered by their distance from the
    /// page at the vertical middle of the visible part, nearest first. The
    /// result is empty when the visible part has no height and when no
    /// page is in it.
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

    /// Makes the requests for drawings match the pages that are wanted in
    /// this frame (`wanted_pages`). `build` calls it in every frame in
    /// which the viewer has no error message and the view is wider than 0.
    /// It does nothing until the document has opened. It does these five
    /// things in this order.
    ///
    /// It computes the size in device pixels of the drawing that each
    /// wanted page needs: the page's size in `column` multiplied by
    /// `scale_factor`, rounded up.
    ///
    /// It removes from `pending` each request whose page is not wanted any
    /// more, or is now wanted at another size. That cancels the request.
    ///
    /// It asks the document's thread for a drawing of each wanted page that
    /// has no drawing at the needed size, has not failed at that size and
    /// has no request in `pending`. The page nearest to the current page
    /// is asked for first, because the thread draws in the order asked.
    ///
    /// It releases the drawings of the pages that are more than `KEEP` (2)
    /// pages before the first wanted page or after the last wanted page.
    ///
    /// It stores the wanted pages and their sizes in `wanted`.
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
        // Take out of the sprite atlas, and forget, the drawing of every
        // page that is more than `KEEP` pages before the first wanted page
        // or more than `KEEP` pages after the last wanted page. When no
        // page is wanted, every drawing is kept.
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
                    crate::atlas::release(drawn.image, cx);
                }
            }
        }
        self.wanted = wanted;
    }

    /// Handles the answer of the document's thread to the request for a
    /// drawing of page `index` at `pixels`, the width and height in device
    /// pixels that was asked for. `result` is the drawing, or the error
    /// with which drawing the page failed, or `Canceled` when the thread
    /// dropped the request without an answer.
    ///
    /// A drawing is stored in `drawn` when `geometry::keep_drawn` says to
    /// keep it. It is kept when the page is still wanted at this size. A
    /// drawing of another size is kept only when the page is still wanted
    /// and has no drawing at all. Any other drawing is discarded. A
    /// drawing that the new one replaces is taken out of the sprite atlas.
    fn arrived(
        &mut self,
        index: usize,
        pixels: (u32, u32),
        result: Result<anyhow::Result<RenderedPage>, futures::channel::oneshot::Canceled>,
        cx: &mut Context<Self>,
    ) {
        // Remove the request that this answer belongs to from `pending`,
        // if `pending` still holds it for this page at this size. The
        // request's task is the task that is running this code. Dropping
        // a task cancels it, so the task is detached instead, which lets
        // it run to its end.
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
                    crate::atlas::release(old.image, cx);
                }
                if !self.first_drawn {
                    self.first_drawn = true;
                    cx.emit(Progressed);
                }
            }
            Ok(Err(error)) => {
                eprintln!("picto: page {} of a document: {error}", index + 1);
                self.failed.insert(index, pixels);
                // A page that could not be drawn counts as the first
                // answer too, so that `settled` returns true and the host
                // stops waiting for a first drawing. The page goes on
                // showing what it showed before: an earlier drawing if it
                // has one, otherwise the item's thumbnail if it is the
                // first page, otherwise a white rectangle.
                if !self.first_drawn {
                    self.first_drawn = true;
                    cx.emit(Progressed);
                }
            }
            // The request was dropped without an answer. That happens only
            // when the document's thread has ended, so no page can be
            // drawn any more. The viewer shows the message that
            // `render_failure` returns.
            Err(_) => self.error = Some(render_failure(self.kind).into()),
        }
        cx.notify();
    }

    /// Returns the element that draws page `index`. `place` is the page's
    /// left edge, top edge, width and height in the column, in pixels. The
    /// element is a white rectangle of that size at that position. The
    /// page's drawing is drawn on it, stretched to the rectangle. A page
    /// that has no drawing shows only the white rectangle, except the
    /// first page, which shows the item's thumbnail stretched to the
    /// rectangle when the viewer has the thumbnail. The rectangle of a PDF
    /// page has the shadow `theme::page_shadow`, and that of a DjVu or
    /// comic page has the shadow `theme::sheet_shadow`. The pages of an
    /// inline document have no shadow.
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

/// Returns the error message for a document of `kind` whose thread ended
/// before it answered whether the document could be opened. The thread
/// reports a document that it cannot open with a message of its own. So
/// this message is shown only when the thread could not be started, or
/// when it ended without an answer, for example because the code that
/// opens the document panicked.
fn open_failure(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Pdf => t("Could not open this PDF."),
        RendererKind::Djvu => t("Could not open this DjVu document."),
        _ => "Could not open this CBZ.",
    }
}

/// Returns the error message for a document of `kind` whose thread ended
/// while the viewer waited for the drawing of a page.
fn render_failure(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Pdf => "Could not render this PDF.",
        RendererKind::Djvu => "Could not render this DjVu document.",
        _ => "Could not render this CBZ.",
    }
}

impl Render for PagedViewer {
    /// The pages are scaled to the view's width, and only the pages in view
    /// are drawn, so `render` returns an element that fills the view's area
    /// and builds the pages for that element's bounds in the frame that
    /// lays it out (`ui::sized_view`).
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::sized_view(cx.weak_entity(), Self::build).size_full()
    }
}

impl PagedViewer {
    /// Builds one frame of the viewer for `bounds`, the view's position and
    /// size in window coordinates in this frame.
    fn build(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.scale_factor = window.scale_factor();
        self.frame = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        // A document that is a member of a collection is drawn at its full
        // height inside the collection reader's scrolling column. The part
        // of it that is in view is the part of `bounds` that the reader
        // does not clip, measured from the top of `bounds`.
        let clip = window.content_mask().bounds.intersect(&bounds);
        self.in_view = Some((
            f32::from(clip.top() - bounds.top()),
            f32::from(clip.bottom() - bounds.top()),
        ));
        let root = div().relative().size_full().overflow_hidden();

        let column = self.column();
        if self.error.is_none() && self.frame.0 > 0. {
            self.request(&column, cx);
        }
        if !self.presentable() {
            // The viewer draws nothing until `presentable` returns true.
            // Until then the media surface goes on drawing the item that
            // it showed before this document, for 1,500 ms at most
            // (`motion::MEDIA_HOLD`).
            return root.into_any_element();
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
            // The error message is drawn in the centre of the viewport, or
            // of the whole view for an inline document, in 12 px text with
            // a line height of 1.5 times the text size. A message that has
            // a translation is shown in the app's language, and any other
            // message is shown as it is.
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(theme::TEXT_SM * 1.5))
                .text_color(theme::theme(cx).text_secondary())
                .child(crate::i18n::t_shared(error))
        });
        let pages: Vec<Div> = column
            .pages
            .iter()
            .enumerate()
            .map(|(index, place)| self.page(index, *place))
            .collect();
        // An inline document is drawn as its pages only, each at its place
        // in the column, which starts at the view's top-left corner. The
        // error message is drawn over them when there is one. The
        // collection reader scrolls the view.
        if self.inline {
            return root.children(pages).children(error).into_any_element();
        }

        // The viewport fills the view above the footer and has the
        // document backdrop colour. The element inside it has the size of
        // the column, holds the pages and scrolls. Each scroll asks for the
        // viewer to be built again, because the wanted pages and the
        // current page depend on the scroll position. When there is an
        // error message, the pages are not drawn.
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
        // The footer shows "Page n of m" for the current page. It shows
        // nothing when no page has been measured, which is the case when
        // the document could not be opened.
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
            .into_any_element()
    }
}
