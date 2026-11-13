//! The grid of results. Its own view, so scrolling, thumbnail reveals and
//! fades repaint the grid and nothing else.
pub mod art;
pub mod empty;
pub mod format;
pub mod layout;
pub mod marquee;
pub mod nav;
#[allow(dead_code)]
pub mod originals;
pub mod prefs;
pub mod preview;
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

pub struct GridView {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
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
    preview: preview::HoverPreview,
    /// The broken-thumbnail and font artwork.
    art: Option<art::Art>,
    _session: Subscription,
}

/// `HOVER_RESUME_DELAY_MS`: the magnifier comes back this long after scrolling.
const HOVER_RESUME: std::time::Duration = std::time::Duration::from_millis(80);
/// "Return to Top" appears once the grid has scrolled this far.
const RETURN_TO_TOP_AFTER: f32 = 200.;

/// The item's aspect ratio when it is known. Audio without dimensions is
/// drawn two to one.
fn aspect(item: &RootSummary) -> Option<f32> {
    match (item.width, item.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(width as f32 / height as f32)
        }
        _ if item.mime.starts_with("audio/") => Some(2.),
        _ => None,
    }
}

impl EventEmitter<MenuRequested> for GridView {}

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
        Self {
            session,
            thumbnails,
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
            preview: preview::HoverPreview::default(),
            art: None,
            _session: subscription,
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
                    cx.background_executor().timer(preview::SHOW_DELAY).await;
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
            cx.background_executor().timer(preview::HIDE_DELAY).await;
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
        let (x, y) = self.local(event.position);
        if x < 0. || x >= self.grid_width() || self.image_at(event.position).is_some() {
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
    fn tile(&mut self, data: TileData, prefs: &ViewPrefs, now: f64, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let TileData {
            index,
            tile,
            item,
            thumbnail,
            selected,
            thumb,
        } = data;
        let image_height = tile.h - prefs.text_height();
        let audio = item.mime.starts_with("audio/");
        let font = item.mime.starts_with("font/");
        let timed = item.mime.starts_with("video/") || audio;
        // Letterboxed media keeps its own rect inside the tile; badges and the
        // selection stroke follow that rect, not the tile.
        // Only the grid mode letterboxes by choice; waterfall and justified
        // tiles already have their item's shape. Video and audio always do.
        let contain = (prefs.mode == ViewMode::Grid && !prefs.fit_cover) || timed;
        let (x, y, w, h) = match aspect(&item).filter(|_| contain) {
            Some(aspect) => {
                let scale = (tile.w / aspect).min(image_height);
                let (w, h) = (aspect * scale, scale);
                ((tile.w - w) / 2., (image_height - h) / 2., w, h)
            }
            // An item of unknown shape takes the whole tile.
            None => (0., 0., tile.w, image_height),
        };
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
            let image = img(thumbnail)
                .image_cache(&self.thumbnails)
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
                cell = cell.child(ui::badge(4.).left(px(x + 5.)).top(px(y + 5.)).child(label));
            }
        }
        let animated = item.mime == "image/gif" && item.frame_count.unwrap_or(0) > 1;
        if let Some(duration) = item.duration_ms.filter(|ms| *ms > 0 && (timed || animated)) {
            cell = cell.child(
                ui::badge(2.)
                    .right(px(tile.w - x - w + 5.))
                    .top(px(y + 5.))
                    .child(format::format_duration(duration)),
            );
        }
        if prefs.show_item_count && collection {
            cell = cell.child(
                ui::badge(4.)
                    .left(px(x + 5.))
                    .top(px(y + h - 18. - 5.))
                    .child(item.media_count.to_string()),
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
                        // The name truncates to the tile less 8px; selected,
                        // it is white on a pill of the selection colour.
                        div()
                            .max_w(px(tile.w - 2.))
                            .px(px(3.))
                            .rounded(px(2.))
                            .when(selected, |pill| {
                                pill.bg(rgb(theme::GRID_SELECTION)).text_color(white())
                            })
                            .truncate()
                            .line_height(px(15.))
                            .child(if item.name.is_empty() {
                                "Untitled".to_owned()
                            } else {
                                item.name.clone()
                            }),
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
                    .child(format!("{width} × {height}")),
            );
        }
        // The magnifier marks the hovered image, except on video and while
        // the grid is scrolling.
        if self.hovered == Some(index)
            && !item.mime.starts_with("video/")
            && self.scrolled.elapsed() >= HOVER_RESUME
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
                    .bg(rgba(0x00000066))
                    .child(
                        svg()
                            .path("magnifier")
                            .size_full()
                            .text_color(rgba(0xffffffb3)),
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
                    let modifiers = event.modifiers();
                    let open = event.click_count() == 2;
                    this.session.update(cx, |session, cx| {
                        if modifiers.shift {
                            session.select_to(index, modifiers.platform, cx);
                        } else if modifiers.platform {
                            session.toggle(index, cx);
                        } else {
                            session.select(index, cx);
                            if open {
                                session.open_viewer(cx);
                            }
                        }
                    });
                    window.focus(&this.focus, cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
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
        let restore = self.session.update(cx, |session, cx| {
            session.advance_transition(cx);
            if session.results_epoch == epoch {
                session.grid_scroll = scrolled;
                None
            } else {
                Some(session.restore_scroll.take().unwrap_or(0.))
            }
        });
        if let Some(offset) = restore {
            self.scroll.set_offset(point(px(0.), px(-offset)));
        }
        let theme = theme::theme(cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        // The scrollbar's gutter is not grid: tiles lay out beside it.
        let width = self.grid_width();
        let viewport = f32::from(self.bounds.size.height);
        let top = self.scroll_top();
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
                cx.background_executor().timer(HOVER_RESUME).await;
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
                self.layout = Layout::compute(
                    &aspects,
                    width,
                    session.tile,
                    prefs.gap,
                    prefs.mode,
                    text_height,
                );
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
            } else if session.loading {
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
                            Thumb::Failed
                        } else {
                            Thumb::Pending
                        };
                    }
                    (data.item.root_id.0, data.thumb == Thumb::Loaded)
                })
                .collect::<Vec<_>>()
        };
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
                    vec![
                        ui::empty_state_action("grid-import-files", "upload", "Import Files")
                            // TODO: choose files and import them into this scope.
                            .on_click(|_, _, _| {})
                            .into_any_element(),
                        ui::empty_state_action(
                            "grid-import-folder",
                            "folder-plus",
                            "Import Folder",
                        )
                        // TODO: choose a folder and import it into this scope.
                        .on_click(|_, _, _| {})
                        .into_any_element(),
                    ]
                } else {
                    Vec::new()
                };
                div()
                    .id("grid-empty")
                    .size_full()
                    .opacity(fade)
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
                    content = content.child(self.tile(data, &prefs, now, cx));
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
            .child(ui::scrollbar("grid-scrollbar", &self.scroll, window, cx))
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
