//! The grid of results. Its own view, so scrolling, thumbnail reveals and
//! fades repaint the grid and nothing else.
pub mod art;
pub mod drag;
pub mod empty;
pub mod filter_bar;
pub mod filter_editors;
pub mod filters;
pub mod format;
pub mod layout;
pub mod marquee;
pub mod menu;
pub mod nav;
pub mod originals;
pub mod prefs;
pub mod preview;
pub mod rendered;
pub mod reveal;
pub mod thumbnails;

use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder;
use gpui::*;
use layout::{Layout, ViewMode};
use picto_library::query::{ItemScope, RootSummary};
use prefs::ViewPrefs;
use std::path::PathBuf;
use thumbnails::Thumbnails;

/// A tile was right-clicked at this window position.
pub struct MenuRequested(pub Point<Pixels>);

/// Empty grid space was right-clicked at this window position.
pub struct EmptyMenuRequested(pub Point<Pixels>);

/// A tile was double-clicked (after selecting it) or middle-clicked; the
/// shell opens it as the Controls settings say.
pub struct Activated {
    pub index: usize,
    pub middle: bool,
}

pub struct GridView {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    originals: Entity<originals::Originals>,
    /// Keyboard focus returns here after a click on a tile.
    focus: FocusHandle,
    scroll: ScrollHandle,
    layout: Layout,
    layout_key: Option<(u32, u32, usize, u32, ViewMode, u32)>,
    reveal: reveal::RevealTracker,
    /// The steady clock the reveal timings are measured on.
    clock: std::time::Instant,
    /// Where the grid was last painted.
    bounds: Bounds<Pixels>,
    /// The session's result set this grid last laid out.
    epoch: u64,
    /// The tile whose image the pointer is over.
    hovered: Option<usize>,
    /// When the grid last scrolled; hover drawing waits 80 ms after it.
    scrolled: std::time::Instant,
    settle: Option<Task<()>>,
    /// The scroll position the last frame was painted at.
    last_top: f32,
    /// Where the pointer last was over the grid, in window coordinates.
    pointer: Option<Point<Pixels>>,
    /// The text rows' height under each image, from the view preferences.
    text_height: f32,
    /// A rubber band being dragged out.
    marquee: Option<marquee::Marquee>,
    /// The click that ends a rubber band selects nothing.
    swallow_click: bool,
    /// A press on a tile that becomes a drag after 5px: the tile, and where.
    press: Option<(usize, Point<Pixels>)>,
    /// The tile's selected items are being dragged.
    dragging: bool,
    /// The click that ends a drag selects nothing.
    swallow_tile_click: bool,
    preview: preview::HoverPreview,
    /// The broken-thumbnail and font artwork.
    art: Option<art::Art>,
    /// The items the layout's tiles are for, by id.
    layout_ids: Vec<u32>,
    /// Files whose missing thumbnail the application was asked to render
    /// (`scheduleThumbnailAfterMiss`), each once.
    rendering: std::collections::HashSet<String>,
    _session: Subscription,
    _thumbnails: Subscription,
}

/// "Return to Top" appears once the grid has scrolled this far.
const RETURN_TO_TOP_AFTER: f32 = 200.;

/// The canvas centres a tile's text rows on the middle of the font's em box
/// (`textBaseline = 'middle'`) at fractional positions; GPUI centres the
/// line's ascent and descent and lands half a point lower.
const CANVAS_TEXT_LIFT: f32 = -0.5;

/// A tile name's width in the grid's 13px UI font.
fn name_width(text: &str, window: &Window) -> f32 {
    let run = TextRun {
        len: text.len(),
        font: font(theme::FONT_UI),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(text.to_owned().into(), px(theme::TEXT_MD), &[run], None)
            .width,
    )
}

/// The item's aspect ratio when it is known. Audio without dimensions is
/// drawn two to one.
fn aspect(item: &RootSummary) -> Option<f64> {
    match (item.width, item.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(f64::from(width) / f64::from(height))
        }
        _ if item.mime.starts_with("audio/") => Some(2.),
        _ => None,
    }
}

impl EventEmitter<MenuRequested> for GridView {}
impl EventEmitter<EmptyMenuRequested> for GridView {}

impl EventEmitter<Activated> for GridView {}

/// Where a tile's thumbnail is.
#[derive(Clone, Copy, PartialEq)]
enum Thumb {
    Pending,
    Loaded,
    /// It could not be read or decoded: the tile shows the broken page.
    Failed,
}

/// What a tile draws, copied out of the session for the visible tiles only.
struct TileData {
    index: usize,
    tile: layout::Tile,
    item: RootSummary,
    thumbnail: PathBuf,
    selected: bool,
    thumb: Thumb,
    /// The original, decoded for this tile's size, once it is worth having.
    original: Option<std::sync::Arc<RenderImage>>,
}

/// Whether a tile letterboxes its media, and the media's rect inside the
/// tile's image area. Only the grid mode letterboxes by choice; waterfall and
/// justified tiles already have their item's shape. Video and audio always
/// do. An item of unknown shape takes the whole area.
fn media_rect(
    tile: &layout::Tile,
    item: &RootSummary,
    prefs: &ViewPrefs,
) -> (bool, (f32, f32, f32, f32)) {
    let image_height = tile.h - prefs.text_height();
    let timed = item.mime.starts_with("video/") || item.mime.starts_with("audio/");
    let contain = (prefs.mode == ViewMode::Grid && !prefs.fit_cover) || timed;
    let rect = match aspect(item).filter(|_| contain) {
        Some(aspect) => {
            let aspect = aspect as f32;
            let scale = (tile.w / aspect).min(image_height);
            let (w, h) = (aspect * scale, scale);
            ((tile.w - w) / 2., (image_height - h) / 2., w, h)
        }
        None => (0., 0., tile.w, image_height),
    };
    (contain, rect)
}

/// Places artwork of `size` (drawing units, `art::MARGIN` included) drawn at
/// `scale`, centred on the tile's image area, inside the media rect at
/// (`x`, `y`).
fn art_at(
    image: std::sync::Arc<Image>,
    size: (f32, f32),
    scale: f32,
    area: (f32, f32),
    media: (f32, f32),
) -> Img {
    let (width, height) = (size.0 * scale, size.1 * scale);
    img(image)
        .absolute()
        .left(px((area.0 - width) / 2. - media.0))
        .top(px((area.1 - height) / 2. - media.1))
        .w(px(width))
        .h(px(height))
}

impl GridView {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&session, |_, _, change: &Change, cx| {
            if !matches!(change, Change::Viewer) {
                cx.notify();
            }
        });
        // A thumbnail written since it was read is read again.
        let changed = cx.subscribe(
            &session,
            |this, session, event: &crate::application::Event, cx| {
                if let crate::application::Event::Thumbnail(hash) = event {
                    let jpeg = session.read(cx).thumbnail(hash);
                    this.thumbnails
                        .update(cx, |thumbnails, cx| thumbnails.forget(&jpeg, cx));
                    cx.notify();
                }
            },
        );
        let originals = cx.new(originals::Originals::new);
        // An original that finished decoding is drawn on the next frame.
        cx.observe(&originals, |_, _, cx| cx.notify()).detach();
        Self {
            session,
            thumbnails,
            originals,
            focus,
            scroll: ScrollHandle::new(),
            layout: Layout::default(),
            layout_key: None,
            reveal: reveal::RevealTracker::default(),
            clock: std::time::Instant::now(),
            bounds: Bounds::default(),
            epoch: 0,
            hovered: None,
            scrolled: std::time::Instant::now(),
            settle: None,
            last_top: 0.,
            pointer: None,
            text_height: 0.,
            marquee: None,
            swallow_click: false,
            press: None,
            dragging: false,
            swallow_tile_click: false,
            preview: preview::HoverPreview::default(),
            art: None,
            layout_ids: Vec::new(),
            rendering: Default::default(),
            _session: subscription,
            _thumbnails: changed,
        }
    }

    /// Asks the application to render the thumbnails of files that have
    /// none yet (`media.render_thumbnail_now`), as the media protocol does
    /// on a miss, and draws those it cannot make (Flash, PDF and the other
    /// paged documents) as the Electron main process does
    /// (`renderExternalThumbnail`); the thumbnail is read again once it has
    /// been written.
    fn render_missing(&mut self, missing: Vec<(String, String, PathBuf)>, cx: &mut Context<Self>) {
        for (hash, mime, jpeg) in missing {
            // Each file once; one whose thumbnail exists but did not decode
            // stays broken.
            if !self.rendering.insert(hash.clone())
                || jpeg.exists()
                || thumbnails::png_for(&jpeg).exists()
            {
                continue;
            }
            let answer = self.session.read(cx).dispatch(
                "media.render_thumbnail_now",
                serde_json::json!({ "file_hash": hash }),
            );
            let (original, disposable) = {
                let session = self.session.read(cx);
                (session.original(&hash, &mime), session.disposable())
            };
            cx.spawn(async move |this, cx| {
                let _ = answer.await;
                let png = thumbnails::png_for(&jpeg);
                // Only into a marked disposable copy, as every write here.
                if !jpeg.exists() && !png.exists() && disposable && rendered::draws(&mime) {
                    // A thread of its own: a movie plays for seconds first.
                    let (sent, drawn) = futures::channel::oneshot::channel();
                    let output = png.clone();
                    std::thread::spawn(move || {
                        let _ = sent.send(rendered::render(&mime, &original, &output));
                    });
                    if let Ok(Err(error)) = drawn.await {
                        eprintln!("picto: could not draw the thumbnail of {hash}: {error:#}");
                    }
                }
                if jpeg.exists() || png.exists() {
                    let _ = this.update(cx, |this, cx| {
                        this.thumbnails
                            .update(cx, |thumbnails, cx| thumbnails.forget(&jpeg, cx));
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    /// The width tiles lay out in, beside the scrollbar's gutter.
    fn grid_width(&self) -> f32 {
        (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.)
    }

    fn scroll_top(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    /// A window position relative to the grid's top-left corner.
    fn local(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.bounds.origin.x),
            f32::from(position.y - self.bounds.origin.y),
        )
    }

    /// The image under a window position, if any.
    fn image_at(&self, position: Point<Pixels>) -> Option<(usize, f32, f32)> {
        let (x, y) = self.local(position);
        let y = y + self.scroll_top();
        if x < 0. || x >= self.grid_width() {
            return None;
        }
        marquee::image_at(&self.layout, x, y, self.text_height).map(|index| (index, x, y))
    }

    /// Hover tracking: which image the pointer is over, and the hover
    /// preview's timers for its magnifier.
    fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        if self.marquee.is_some() {
            return;
        }
        let hit = self.image_at(position);
        let hovered = hit.map(|(index, _, _)| index);
        if hovered != self.hovered {
            self.hovered = hovered;
            cx.notify();
        }
        let magnifier = hit.filter(|&(index, x, y)| {
            marquee::on_magnifier(&self.layout.tiles[index], x, y, self.text_height)
        });
        if let Some((index, _, _)) = magnifier {
            self.preview.hide = None;
            if self.preview.show.is_some() {
                return;
            }
            // The item under the magnifier when the pointer arrived is the
            // one previewed, as the Electron grid's timer captures it.
            let target = self
                .session
                .read(cx)
                .items
                .get(index)
                .filter(|item| !item.mime.starts_with("video/"))
                .map(|item| (item.root_id, item.content_hash.clone()));
            if let Some((id, hash)) = target {
                self.preview.show = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(crate::motion::HOVER_PREVIEW_DELAY)
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.preview.show = None;
                        this.open_preview(id, hash, cx);
                    });
                }));
            }
        } else {
            self.preview.show = None;
            self.hide_preview_soon(cx);
        }
    }

    /// The pointer left the grid.
    fn pointer_left(&mut self, cx: &mut Context<Self>) {
        self.pointer = None;
        if self.marquee.is_none() && self.hovered.take().is_some() {
            cx.notify();
        }
        self.preview.show = None;
        self.preview.hide = None;
        self.hide_preview_soon(cx);
    }

    fn hide_preview_soon(&mut self, cx: &mut Context<Self>) {
        if self.preview.shown.is_none() || self.preview.hide.is_some() {
            return;
        }
        self.preview.hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::motion::HOVER_PREVIEW_HIDE)
                .await;
            let _ = this.update(cx, |this, cx| this.close_preview(cx));
        }));
    }

    fn close_preview(&mut self, cx: &mut Context<Self>) {
        let open = self.preview.shown.is_some();
        if let Some(image) = self.preview.clear() {
            cx.drop_image(image, None);
        }
        if open {
            cx.notify();
        }
    }

    /// Opens the preview of an item's original: its file's path from the
    /// library, then the decode, both off the drawing thread.
    fn open_preview(&mut self, id: picto_library::RootId, hash: String, cx: &mut Context<Self>) {
        if self
            .preview
            .shown
            .as_ref()
            .is_some_and(|shown| shown.hash == hash)
        {
            return;
        }
        if let Some(image) = self
            .preview
            .shown
            .take()
            .and_then(|mut shown| shown.take_image())
        {
            cx.drop_image(image, None);
        }
        let wanted = hash.clone();
        let path = self.session.read(cx).run(move |library| {
            let details = library.details(id).ok()?;
            // A collection shows the media its tile shows.
            details
                .media
                .iter()
                .find(|media| media.facts.content_hash == wanted)
                .or(details.media.first())
                .map(|media| PathBuf::from(&media.file_path))
        });
        let expected = hash.clone();
        let load = cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = path.await else {
                return;
            };
            let load = cx.update(|cx| {
                let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
                cx.background_executor().spawn(load)
            });
            let Ok(image) = load.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match &mut this.preview.shown {
                Some(shown) if shown.hash == expected => {
                    shown.decoded(image);
                    cx.notify();
                }
                _ => cx.drop_image(image, None),
            });
        });
        self.preview.shown = Some(preview::Shown::new(hash, load));
        cx.notify();
    }

    /// A left press on empty space starts a rubber band. Shift or Cmd held
    /// keeps what is selected.
    fn pressed(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.swallow_click = false;
        self.swallow_tile_click = false;
        let (x, y) = self.local(event.position);
        if x < 0. || x >= self.grid_width() {
            return;
        }
        // A press on an image may become a drag of the items.
        if let Some((index, _, _)) = self.image_at(event.position) {
            self.press = Some((index, event.position));
            cx.notify();
            return;
        }
        let modifiers = event.modifiers;
        let base = if modifiers.shift || modifiers.platform || modifiers.control {
            self.session.read(cx).selection.clone()
        } else {
            Default::default()
        };
        self.marquee = Some(marquee::Marquee::new(
            (x, y + self.scroll_top()),
            (x, y),
            base,
        ));
        cx.notify();
    }

    /// The pointer moved with the button held after a press on a tile:
    /// past 5px the selection, or that tile if it is not selected, is
    /// dragged (`CanvasGrid.tsx`), and the ghost follows the pointer.
    fn drag_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let drags = drag::model(cx);
        if self.dragging {
            drags.update(cx, |drags, cx| {
                if let Some(items) = &mut drags.items {
                    items.position = position;
                    cx.emit(drag::DragEvent::Moved);
                }
            });
            cx.notify();
            return;
        }
        let Some((index, from)) = self.press else {
            return;
        };
        if !drag::past_threshold(from, position) {
            return;
        }
        let items = {
            let session = self.session.read(cx);
            let theme = theme::theme(cx);
            let indices = if session.selection.contains(&index) {
                session.selection.iter().copied().collect::<Vec<_>>()
            } else {
                vec![index]
            };
            let chosen = indices
                .iter()
                .filter_map(|index| session.items.get(*index))
                .collect::<Vec<_>>();
            drag::ItemDrag {
                roots: chosen.iter().map(|item| item.root_id).collect(),
                source: session
                    .nav
                    .get(session.displayed)
                    .map(|entry| entry.scope.clone())
                    .unwrap_or(picto_library::query::ItemScope::All),
                covers: chosen
                    .iter()
                    .take(3)
                    .map(|item| drag::Cover {
                        thumbnail: session.thumbnail(&item.content_hash),
                        matte: item
                            .palette
                            .first()
                            .map(|color| rgb(format::lab_to_rgb(color.l, color.a, color.b)))
                            .unwrap_or(theme.background()),
                    })
                    .collect(),
                position,
            }
        };
        if items.roots.is_empty() {
            self.press = None;
            return;
        }
        for (holder, cover) in ["drag-ghost-0", "drag-ghost-1", "drag-ghost-2"]
            .into_iter()
            .zip(items.covers.iter().map(Some).chain(std::iter::repeat(None)))
        {
            let resource = cover.map(|cover| Resource::Path(cover.thumbnail.clone().into()));
            self.thumbnails
                .update(cx, |thumbnails, _| thumbnails.pin(holder, resource));
        }
        self.dragging = true;
        self.pointer_left(cx);
        drags.update(cx, |drags, cx| {
            drags.items = Some(items);
            cx.emit(drag::DragEvent::Moved);
        });
        cx.notify();
    }

    /// The button came up after a press on a tile: a drag hands its items
    /// to whatever is under the pointer, and the click it makes selects
    /// nothing.
    fn drag_released(&mut self, cx: &mut Context<Self>) {
        self.press = None;
        if !std::mem::take(&mut self.dragging) {
            return;
        }
        self.swallow_tile_click = true;
        for holder in ["drag-ghost-0", "drag-ghost-1", "drag-ghost-2"] {
            self.thumbnails
                .update(cx, |thumbnails, _| thumbnails.pin(holder, None));
        }
        drag::model(cx).update(cx, |drags, cx| {
            if let Some(items) = drags.items.take() {
                cx.emit(drag::DragEvent::Dropped(items));
            }
        });
        cx.notify();
    }

    /// The pointer moved while a rubber band is out, anywhere in the window.
    fn marquee_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let local = self.local(position);
        let top = self.scroll_top();
        let viewport = f32::from(self.bounds.size.height);
        if let Some(marquee) = &mut self.marquee {
            marquee.pointer = local;
            marquee.speed = marquee::auto_scroll_speed(local.1, viewport);
            marquee.track(top, &self.layout, self.text_height);
            cx.notify();
        }
    }

    /// The button came up: what the rectangle covers becomes the selection,
    /// and the click this release makes is swallowed.
    fn marquee_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.take() else {
            return;
        };
        if let Some(selection) = marquee.committed() {
            self.swallow_click = true;
            self.session
                .update(cx, |session, cx| session.set_selection(selection, cx));
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Scrolls down by nine tenths of the visible height, as Space does when
    /// the Controls setting has it scroll (`spaceKeyAction: 'scroll'`).
    pub fn scroll_by_page(&mut self, cx: &mut Context<Self>) {
        let height = f32::from(self.bounds.size.height);
        let bottom = -f32::from(self.scroll.max_offset().y);
        let top = (self.scroll_top() + height * 0.9).clamp(0., bottom.max(0.));
        self.scroll.set_offset(point(px(0.), px(-top)));
        cx.notify();
    }

    /// Moves the selection as the Electron grid's keyboard navigation does and
    /// scrolls the new selection into view.
    pub fn move_selection(&mut self, action: nav::Move, extend: bool, cx: &mut Context<Self>) {
        let (width, height) = (
            f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER,
            f32::from(self.bounds.size.height),
        );
        let (target, gap) = {
            let session = self.session.read(cx);
            (
                session.move_target(action, width, height),
                session.prefs.gap,
            )
        };
        let Some(target) = target else {
            return;
        };
        self.session.update(cx, |session, cx| {
            // Shift extends a range from where the selection started.
            if extend {
                session.select_to(target, false, cx)
            } else {
                session.select(target, cx)
            }
        });
        if let Some(tile) = self.layout.tiles.get(target) {
            let top = -f32::from(self.scroll.offset().y);
            if let Some(offset) = nav::scroll_into_view(tile, top, height, gap) {
                self.scroll.set_offset(point(px(0.), px(-offset)));
            }
        }
        cx.notify();
    }

    /// One grid tile, drawn to the rules of the Electron canvas grid
    /// (`drawBase.ts`): the media rect, its hairline ring, badges inside the
    /// rect, the selection stroke outside it, and optional text rows below.
    fn tile(
        &mut self,
        data: TileData,
        prefs: &ViewPrefs,
        now: f64,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let TileData {
            index,
            tile,
            item,
            thumbnail,
            selected,
            thumb,
            original,
        } = data;
        let image_height = tile.h - prefs.text_height();
        let audio = item.mime.starts_with("audio/");
        let font = item.mime.starts_with("font/");
        let timed = item.mime.starts_with("video/") || audio;
        // Letterboxed media keeps its own rect inside the tile; badges and the
        // selection stroke follow that rect, not the tile.
        let (contain, (x, y, w, h)) = media_rect(&tile, &item, prefs);
        // The image fades in over its dominant colour, then that colour fades
        // away beneath it, which matters for images with transparency.
        let placeholder = item
            .palette
            .first()
            .map(|color| rgb(format::lab_to_rgb(color.l, color.a, color.b)))
            .unwrap_or(theme.surface_2());
        let (image_alpha, placeholder_alpha) =
            reveal::alphas(self.reveal.progress(item.root_id.0, now));
        let mut cell = div()
            .absolute()
            .left(px(tile.x))
            .top(px(tile.y))
            .w(px(tile.w))
            .h(px(tile.h));
        if selected {
            cell = cell
                .child(
                    div()
                        .absolute()
                        .left(px(x - layout::SELECTION_EXTENT))
                        .top(px(y - layout::SELECTION_EXTENT))
                        .w(px(w + 2. * layout::SELECTION_EXTENT))
                        .h(px(h + 2. * layout::SELECTION_EXTENT))
                        .rounded(px(theme::TILE_RADIUS + layout::SELECTION_EXTENT))
                        .border_2()
                        .border_color(rgb(theme::GRID_SELECTION)),
                )
                // A hairline of the app background between stroke and image.
                .child(
                    div()
                        .absolute()
                        .left(px(x - 1.))
                        .top(px(y - 1.))
                        .w(px(w + 2.))
                        .h(px(h + 2.))
                        .rounded(px(theme::TILE_RADIUS + 1.))
                        .border_1()
                        .border_color(theme.background()),
                );
        }
        let layer = || div().absolute().inset_0().rounded(px(theme::TILE_RADIUS));
        let mut media = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .rounded(px(theme::TILE_RADIUS));
        let area = (tile.w, image_height);
        if font {
            // A font has no thumbnail: its card shows at once, centred on
            // the tile and at most 72% of it.
            let art = art::Art::for_color(&mut self.art, theme.surface_2());
            let mark = (tile.w * 0.72).min(image_height * 0.72).min(art::FONT_SIZE);
            let side = art::FONT_SIZE + 2. * art::MARGIN;
            media = media
                .overflow_hidden()
                .child(layer().bg(placeholder))
                .child(art_at(
                    art.font.clone(),
                    (side, side),
                    mark / art::FONT_SIZE,
                    area,
                    (x, y),
                ));
        } else if thumb == Thumb::Failed {
            // The torn page, at most 64% of the tile's height, on the plain
            // placeholder colour.
            let art = art::Art::for_color(&mut self.art, theme.surface_2());
            let (art_width, art_height) = art::BROKEN_SIZE;
            let mark = (image_height * 0.64).min(art_height);
            media = media.overflow_hidden().bg(theme.surface_2()).child(art_at(
                art.broken.clone(),
                (art_width + 2. * art::MARGIN, art_height + 2. * art::MARGIN),
                mark / art_height,
                area,
                (x, y),
            ));
        } else {
            media = media.child(layer().bg(placeholder).opacity(placeholder_alpha));
            // The original, where the tile outgrew its thumbnail, takes the
            // thumbnail's place with no second reveal.
            let image = match original {
                Some(original) => img(original),
                None => img(thumbnail).image_cache(&self.thumbnails),
            }
            .size_full()
            .rounded(px(theme::TILE_RADIUS))
            // A thumbnail of another shape than its item (cover art, a
            // rotated video) is fitted, not cropped.
            .object_fit(if contain {
                ObjectFit::Contain
            } else {
                ObjectFit::Cover
            });
            media = if audio {
                // Audio: a faint tint, and the cover art in grey at half
                // strength over it.
                media
                    .child(
                        layer()
                            .bg(rgba(theme::AUDIO_TINT))
                            .opacity(image_alpha)
                            .child(layer().bg(linear_gradient(
                                180.,
                                linear_color_stop(rgba(theme::AUDIO_TINT), 0.),
                                linear_color_stop(rgba(theme::AUDIO_TINT_FAR), 1.),
                            ))),
                    )
                    .child(
                        image
                            .absolute()
                            .inset_0()
                            .grayscale(true)
                            .opacity(image_alpha * 0.5),
                    )
            } else {
                media.child(image.opacity(image_alpha))
            };
        }
        cell = cell.child(media.child(layer().border_1().border_color(theme.border())));
        let collection = item.kind == picto_library::RootKind::Collection;
        if prefs.show_extension_label {
            let label = format::format_label(&item.mime);
            if collection {
                cell = cell.child(
                    ui::badge(0.)
                        .w(px(18.))
                        .justify_center()
                        .left(px(x + 5.))
                        .top(px(y + 5.))
                        .child(ui::icon("group", 14., rgba(theme::BADGE_TEXT))),
                );
            } else if !label.is_empty() {
                cell = cell.child(
                    ui::badge(4.)
                        .left(px(x + 5.))
                        .top(px(y + 5.))
                        .child(ui::badge_text(label)),
                );
            }
        }
        let animated = item.mime == "image/gif" && item.frame_count.unwrap_or(0) > 1;
        if let Some(duration) = item.duration_ms.filter(|ms| *ms > 0 && (timed || animated)) {
            cell = cell.child(
                ui::badge(2.)
                    .right(px(tile.w - x - w + 5.))
                    .top(px(y + 5.))
                    .child(ui::badge_text(format::format_duration(duration))),
            );
        }
        if prefs.show_item_count && collection {
            cell = cell.child(
                ui::badge(4.)
                    .left(px(x + 5.))
                    .top(px(y + h - 18. - 5.))
                    .child(ui::badge_text(item.media_count.to_string())),
            );
        }
        if prefs.show_name {
            cell = cell.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(image_height + 7.))
                    .h(px(15.))
                    .flex()
                    .justify_center()
                    .child(
                        // The name truncates to the tile less 8px as the
                        // canvas measures it; selected, it is white on a pill
                        // of the selection colour 3px wider either side.
                        div()
                            .px(px(3.))
                            .rounded(px(2.))
                            .when(selected, |pill| {
                                pill.bg(rgb(theme::GRID_SELECTION))
                                    .text_color(rgb(theme::BADGE_STRONG_TEXT))
                            })
                            .whitespace_nowrap()
                            .line_height(px(15.))
                            .child(div().relative().top(px(CANVAS_TEXT_LIFT)).child({
                                let name = if item.name.is_empty() {
                                    "Untitled".to_owned()
                                } else {
                                    item.name.clone()
                                };
                                // Show Extension adds `.EXT` after it.
                                let extension = format::label_extension(&item.mime);
                                let label = if prefs.show_extension && !extension.is_empty() {
                                    format!("{name}.{}", extension.to_uppercase())
                                } else {
                                    name
                                };
                                format::truncate_to(&label, tile.w - 8., |text| {
                                    name_width(text, window)
                                })
                            })),
                    ),
            );
        }
        if prefs.show_resolution
            && let (Some(width), Some(height)) = (item.width, item.height)
            && width > 0
            && height > 0
        {
            cell = cell.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(image_height
                        + prefs.show_name as u8 as f32 * layout::TEXT_ROW_HEIGHT
                        + 5.))
                    .h(px(15.))
                    .flex()
                    .justify_center()
                    .text(theme::TEXT_CAPTION)
                    .line_height(px(15.))
                    .text_color(theme.text_tertiary())
                    .child(
                        div()
                            .relative()
                            .top(px(CANVAS_TEXT_LIFT))
                            .child(format!("{width} × {height}")),
                    ),
            );
        }
        // The magnifier marks the hovered image, except on video and while
        // the grid is scrolling.
        if self.hovered == Some(index)
            && !item.mime.starts_with("video/")
            && self.scrolled.elapsed() >= crate::motion::MAGNIFIER_RESUME
        {
            cell = cell.child(
                div()
                    .absolute()
                    .left(px(tile.w - marquee::MAGNIFIER_WIDTH))
                    .top(px(image_height - marquee::MAGNIFIER_HEIGHT))
                    .w(px(marquee::MAGNIFIER_WIDTH))
                    .h(px(marquee::MAGNIFIER_HEIGHT))
                    .rounded_tl(px(10.))
                    .rounded_br(px(theme::TILE_RADIUS))
                    .bg(rgba(theme::GRID_MAGNIFIER_BACKGROUND))
                    .child(
                        svg()
                            .path("magnifier")
                            .size_full()
                            .text_color(rgba(theme::GRID_MAGNIFIER_GLYPH)),
                    ),
            );
        }
        // Only the image takes the pointer: the name and resolution rows
        // below it are empty space, as in the canvas grid's hit test.
        let in_selection = selected;
        cell.child(
            div()
                .id(index)
                .absolute()
                .left_0()
                .top_0()
                .w(px(tile.w))
                .h(px(image_height))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if std::mem::take(&mut this.swallow_tile_click) {
                        return;
                    }
                    let modifiers = event.modifiers();
                    let open = event.click_count() == 2;
                    this.session.update(cx, |session, cx| {
                        if modifiers.shift {
                            session.select_to(index, modifiers.platform, cx);
                        } else if modifiers.platform {
                            session.toggle(index, cx);
                        } else {
                            session.select(index, cx);
                        }
                    });
                    window.focus(&this.focus, cx);
                    if open && !modifiers.shift && !modifiers.platform {
                        cx.emit(Activated {
                            index,
                            middle: false,
                        });
                    }
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(Activated {
                            index,
                            middle: true,
                        });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        // The grid's own right-click is for empty space.
                        cx.stop_propagation();
                        // A right-click inside the selection keeps it.
                        if !in_selection {
                            this.session
                                .update(cx, |session, cx| session.select(index, cx));
                        }
                        cx.emit(MenuRequested(event.position));
                        window.focus(&this.focus, cx);
                    }),
                ),
        )
    }
}

/// What fills the grid's place.
enum Surface {
    Tiles,
    /// The first page is on its way: nothing yet.
    Loading,
    Error(String),
    Empty(empty::EmptyCopy),
}

impl Render for GridView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Grid);
        let build_started = std::time::Instant::now();
        // A rubber band near the top or bottom edge scrolls the grid each
        // frame; the rectangle follows below.
        if let Some(speed) = self
            .marquee
            .as_ref()
            .map(|marquee| marquee.speed)
            .filter(|speed| *speed != 0.)
        {
            let current = self.scroll_top();
            let limit = f32::from(self.scroll.max_offset().y).max(0.);
            let next = (current + speed).clamp(0., limit);
            if next != current {
                self.scroll.set_offset(point(px(0.), px(-next)));
            }
            window.request_animation_frame();
        }
        // The session moves its scope change along, hears how far this grid
        // is scrolled, and says where a restored grid should open.
        let scrolled = self.scroll_top();
        let epoch = self.epoch;
        // A press, rubber band or drag in progress holds back a refresh of
        // the results, which would move what it is on.
        let busy = self.marquee.is_some() || self.dragging || self.press.is_some();
        let restore = self.session.update(cx, |session, cx| {
            session.advance_transition(cx);
            session.pointer_busy = busy;
            if session.results_epoch == epoch {
                session.grid_scroll = scrolled;
                None
            } else {
                Some((
                    session.restore_scroll.take().unwrap_or(0.),
                    std::mem::take(&mut session.refreshed_in_place),
                ))
            }
        });
        let mut follow = false;
        match restore {
            // Read again in place: the items that stay keep their place on
            // screen, laid out below.
            Some((_, true)) => follow = true,
            Some((offset, false)) => self.scroll.set_offset(point(px(0.), px(-offset))),
            None => {}
        }
        let theme = theme::theme(cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        // The scrollbar's gutter is not grid: tiles lay out beside it.
        let width = self.grid_width();
        let viewport = f32::from(self.bounds.size.height);
        let mut top = self.scroll_top();
        // Any scroll ends the hover and closes the preview at once; the
        // hover comes back 80 ms after the last one, where the pointer is.
        if top != self.last_top {
            self.last_top = top;
            self.hovered = None;
            if let Some(image) = self.preview.clear() {
                cx.drop_image(image, Some(window));
            }
            self.scrolled = std::time::Instant::now();
            self.settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::motion::MAGNIFIER_RESUME)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(pointer) = this.pointer {
                        this.pointer_moved(pointer, cx);
                    }
                    cx.notify();
                });
            }));
        }

        // Everything the frame needs from the session, copied for the visible
        // tiles only, so the listeners below can borrow the context.
        let (fade, animating, prefs, surface, mut tiles) = {
            let session = self.session.read(cx);
            if session.results_epoch != self.epoch {
                self.epoch = session.results_epoch;
                self.layout_key = None;
                // Positions now mean other items.
                self.marquee = None;
                self.hovered = None;
            }
            let prefs = session.prefs.clone();
            let text_height = prefs.text_height();
            self.text_height = text_height;
            let key = Some((
                width as u32,
                session.tile as u32,
                session.items.len(),
                prefs.gap as u32,
                prefs.mode,
                text_height as u32,
            ));
            if key != self.layout_key {
                let aspects = session
                    .items
                    .iter()
                    // `gridLayoutModel.ts`: an unknown shape lays out as 3:2.
                    .map(|item| aspect(item).unwrap_or(1.5))
                    .collect::<Vec<_>>();
                let previous = std::mem::replace(
                    &mut self.layout,
                    Layout::compute(
                        &aspects,
                        width,
                        session.tile,
                        prefs.gap,
                        prefs.mode,
                        text_height,
                    ),
                );
                // The same items laid out anew (zoom, spacing, layout mode,
                // labels, width), or the results read again in place: the
                // topmost visible tile, followed by its item, stays where it
                // was on screen, as `CanvasGrid` keeps it.
                let ids = session
                    .items
                    .iter()
                    .map(|item| item.root_id.0)
                    .collect::<Vec<_>>();
                if self.layout_key.is_some() || follow {
                    let now = ids
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (*id, index))
                        .collect::<std::collections::HashMap<_, _>>();
                    let before = self
                        .layout_ids
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (*id, index))
                        .collect::<std::collections::HashMap<_, _>>();
                    let selected = session
                        .selection
                        .iter()
                        .filter_map(|index| ids.get(*index))
                        .filter_map(|id| before.get(id).copied())
                        .collect();
                    let moved = |index: usize| {
                        self.layout_ids
                            .get(index)
                            .and_then(|id| now.get(id).copied())
                    };
                    if let Some(anchored) = layout::scroll_anchor(
                        &previous.tiles,
                        &self.layout.tiles,
                        &selected,
                        top,
                        viewport,
                        moved,
                    ) && anchored != top
                    {
                        top = anchored;
                        self.scroll.set_offset(point(px(0.), px(-anchored)));
                    }
                }
                self.layout_ids = ids;
                self.layout_key = key;
            }
            if let Some(marquee) = &mut self.marquee {
                marquee.track(top, &self.layout, text_height);
            }
            // `GridScreen.tsx`: an error first, then the tiles, and only a
            // scope known to be empty says so; a loading one shows nothing.
            let surface = if let Some(error) = &session.error {
                Surface::Error(error.clone())
            } else if !session.items.is_empty() {
                Surface::Tiles
            } else if session.loading && !session.refreshing {
                Surface::Loading
            } else {
                let scope = session
                    .nav
                    .get(session.displayed)
                    .map_or(ItemScope::All, |entry| entry.scope.clone());
                Surface::Empty(empty::copy(
                    &scope,
                    session.searching(),
                    session.filter.is_some(),
                ))
            };
            let tiles = self
                .layout
                .visible(top - reveal::MARGIN, top + viewport + reveal::MARGIN)
                .into_iter()
                .filter_map(|index| {
                    let item = session.items.get(index)?;
                    let selected = match &self.marquee {
                        Some(marquee) => marquee.shows_selected(index, &session.selection),
                        None => session.selection.contains(&index),
                    };
                    Some(TileData {
                        index,
                        tile: self.layout.tiles[index],
                        item: item.clone(),
                        thumbnail: session.thumbnail(&item.content_hash),
                        selected,
                        thumb: Thumb::Pending,
                        original: None,
                    })
                })
                .collect::<Vec<_>>();
            (
                session.transition.opacity(),
                session.transition.animating(),
                prefs,
                surface,
                tiles,
            )
        };
        // `CanvasGrid`: fetch the next page within three viewports of the end.
        if top + viewport * 4. >= self.layout.total_height && !tiles.is_empty() {
            self.session.update(cx, |session, cx| session.load_more(cx));
        }

        let tile_count = tiles.len();
        // Thumbnails load from the middle of the viewport outward.
        let spans = tiles
            .iter()
            .map(|data| (data.tile.y, data.tile.h))
            .collect::<Vec<_>>();
        let wanted = thumbnails::load_order(&spans, top, viewport)
            .into_iter()
            .map(|at| Resource::Path(tiles[at].thumbnail.clone().into()))
            .collect();
        self.thumbnails
            .update(cx, |thumbnails, cx| thumbnails.plan(wanted, window, cx));
        let mut missing = Vec::new();
        let decoded = {
            let thumbnails = self.thumbnails.read(cx);
            tiles
                .iter_mut()
                .map(|data| {
                    // A font draws its card and has nothing to load.
                    if data.item.mime.starts_with("font/") {
                        data.thumb = Thumb::Loaded;
                    } else {
                        let resource = Resource::Path(data.thumbnail.clone().into());
                        data.thumb = if thumbnails.is_loaded(&resource) {
                            Thumb::Loaded
                        } else if thumbnails.is_failed(&resource) {
                            missing.push((
                                data.item.content_hash.clone(),
                                data.item.mime.clone(),
                                data.thumbnail.clone(),
                            ));
                            Thumb::Failed
                        } else {
                            Thumb::Pending
                        };
                    }
                    (data.item.root_id.0, data.thumb == Thumb::Loaded)
                })
                .collect::<Vec<_>>()
        };
        self.render_missing(missing, cx);
        // Tiles in the viewport that draw more pixels than their thumbnail
        // holds ask for their original, which arrives after a short dwell.
        let scale = window.scale_factor();
        let wants = {
            let session = self.session.read(cx);
            tiles
                .iter()
                .filter(|data| {
                    data.thumb == Thumb::Loaded
                        && data.tile.y + data.tile.h > top
                        && data.tile.y < top + viewport
                })
                .filter_map(|data| {
                    let (contain, (_, _, w, h)) = media_rect(&data.tile, &data.item, &prefs);
                    let source = data.item.width.zip(data.item.height);
                    originals::wants_original(&data.item.mime, (w, h), source, contain, scale).then(
                        || originals::Want {
                            hash: data.item.content_hash.clone(),
                            path: session.original(&data.item.content_hash, &data.item.mime),
                            size: originals::decode_size((w, h), source, contain, scale),
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        self.originals
            .update(cx, |originals, cx| originals.plan(wants, window, cx));
        {
            let originals = self.originals.read(cx);
            for data in &mut tiles {
                data.original = originals.get(&data.item.content_hash);
            }
        }
        self.reveal.update(&decoded, now);
        // Keep painting while a fade or a thumbnail reveal is in motion.
        if animating || self.reveal.active(now) {
            window.request_animation_frame();
        }

        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.bounds != bounds {
                        this.bounds = bounds;
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        // Only the tiles scroll: an empty or failed grid has no scrollbar,
        // whatever the last results left in the scroll handle.
        let scrolls = matches!(surface, Surface::Tiles);
        let body = match surface {
            Surface::Error(error) => div()
                .id("grid-error")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .opacity(fade)
                .text(theme::TEXT_MD)
                .text_color(theme.negative())
                .child(error)
                .child(
                    div()
                        .id("grid-retry")
                        .px(px(12.))
                        .py(px(4.))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme.border())
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_secondary())
                        .cursor_pointer()
                        .hover(move |style| {
                            style.text_color(theme.text()).bg(theme.surface_hover())
                        })
                        .child("Retry")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.update(cx, |session, cx| session.reload(cx));
                        })),
                ),
            Surface::Loading => div().id("grid-loading").size_full(),
            Surface::Empty(copy) => {
                let actions = if copy.import {
                    // `chooseAndImportFiles` / `chooseAndImportFolder` into
                    // this destination, held while an import runs.
                    let destination = {
                        let session = self.session.read(cx);
                        session
                            .nav
                            .get(session.displayed)
                            .map(|entry| crate::imports::plan::Destination::for_scope(&entry.scope))
                            .unwrap_or_default()
                    };
                    let blocked = crate::imports::blocked(cx);
                    let start = move |folder: bool| {
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if let Some(imports) = crate::imports::get(cx) {
                                imports.update(cx, |imports, cx| {
                                    if folder {
                                        imports.choose_folder(destination, window, cx)
                                    } else {
                                        imports.choose_files(destination, window, cx)
                                    }
                                });
                            }
                        }
                    };
                    vec![
                        ui::empty_state_action("grid-import-files", "upload", "Import Files")
                            .disabled(blocked.is_some())
                            .on_click(start(false))
                            .into_any_element(),
                        ui::empty_state_action(
                            "grid-import-folder",
                            "folder-plus",
                            "Import Folder",
                        )
                        .disabled(blocked.is_some())
                        .on_click(start(true))
                        .into_any_element(),
                    ]
                } else {
                    Vec::new()
                };
                div()
                    .id("grid-empty")
                    .size_full()
                    .opacity(fade)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, event: &MouseDownEvent, _, cx| {
                            cx.emit(EmptyMenuRequested(event.position))
                        }),
                    )
                    .child(ui::empty_state(
                        "empty-photo",
                        copy.title,
                        copy.description,
                        actions,
                        cx,
                    ))
            }
            Surface::Tiles => {
                let mut content = div().relative().w_full().h(px(self.layout.total_height));
                for data in tiles {
                    content = content.child(self.tile(data, &prefs, now, window, cx));
                }
                // `.marquee`: square corners, over the tiles.
                if let Some(rect) = self.marquee.as_ref().and_then(|marquee| marquee.rect) {
                    content = content.child(
                        div()
                            .absolute()
                            .left(px(rect.left))
                            .top(px(rect.top))
                            .w(px(rect.width))
                            .h(px(rect.height))
                            .bg(rgba(theme::MARQUEE_FILL))
                            .border_1()
                            .border_color(rgba(theme::MARQUEE_BORDER)),
                    );
                }
                div()
                    .id("media-grid")
                    .size_full()
                    .opacity(fade)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // The frame that follows a scroll notices the new position.
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.pointer_moved(event.position, cx)
                    }))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.pointer_left(cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.pressed(event, cx)),
                    )
                    // A right-click no image took is on empty space.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            cx.emit(EmptyMenuRequested(event.position));
                            window.focus(&this.focus, cx);
                        }),
                    )
                    // A click that no image took is a click on empty space,
                    // unless it ends a rubber band.
                    .on_click(cx.listener(|this, _, window, cx| {
                        if std::mem::take(&mut this.swallow_click) {
                            return;
                        }
                        this.session
                            .update(cx, |session, cx| session.clear_selection(cx));
                        window.focus(&this.focus, cx);
                    }))
                    .child(content)
            }
        };
        crate::stats::grid_built(build_started, tile_count);
        let mut root = div()
            .relative()
            .size_full()
            .child(measure)
            .child(body)
            .when(scrolls, |root| {
                root.child(ui::scrollbar("grid-scrollbar", &self.scroll, window, cx))
            })
            .children(ui::scroll_to_top(
                "grid-return-to-top",
                &self.scroll,
                RETURN_TO_TOP_AFTER,
                12.,
                window,
                cx,
            ));
        // While a rubber band is out, the pointer is followed and the release
        // heard anywhere in the window, as pointer capture does. Capture runs
        // before the click the release makes, so that click can be swallowed.
        if self.marquee.is_some() {
            let entity = cx.entity().downgrade();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moved = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let _ = moved.update(cx, |this, cx| {
                                // A release the window never heard ends it too.
                                if event.pressed_button == Some(MouseButton::Left) {
                                    this.marquee_moved(event.position, cx);
                                } else {
                                    this.marquee_released(window, cx);
                                }
                            });
                        });
                        let released = entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                let _ = released
                                    .update(cx, |this, cx| this.marquee_released(window, cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        // A press on a tile is followed anywhere in the window until the
        // button comes up, as pointer capture does, so it can become a drag.
        if self.press.is_some() || self.dragging {
            let entity = cx.entity().downgrade();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moved = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let _ = moved.update(cx, |this, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    this.drag_moved(event.position, cx);
                                } else {
                                    this.drag_released(cx);
                                }
                            });
                        });
                        let released = entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                let _ = released.update(cx, |this, cx| this.drag_released(cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        if self.dragging
            && let Some(items) = drag::model(cx).read(cx).items.clone()
        {
            root = root.child(drag::ghost(&items, &self.thumbnails, cx));
        }
        if let Some(shown) = &self.preview.shown {
            let (overlay, fading) = preview::overlay(shown, window);
            if fading {
                window.request_animation_frame();
            }
            root = root.children(overlay);
        }
        root
    }
}
