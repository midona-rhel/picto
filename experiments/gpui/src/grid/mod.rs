//! The grid of results. Its own view, so scrolling, thumbnail reveals and
//! fades repaint the grid and nothing else.
pub mod format;
pub mod layout;
pub mod nav;
pub mod prefs;
pub mod reveal;
pub mod thumbnails;

use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder;
use gpui::*;
use layout::{Layout, ViewMode};
use picto_library::query::RootSummary;
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

/// What a tile draws, copied out of the session for the visible tiles only.
struct TileData {
    index: usize,
    tile: layout::Tile,
    item: RootSummary,
    thumbnail: PathBuf,
    selected: bool,
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
            _session: subscription,
        }
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
    fn tile(&self, data: TileData, prefs: &ViewPrefs, now: f64, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let TileData {
            index,
            tile,
            item,
            thumbnail,
            selected,
        } = data;
        let image_height = tile.h - prefs.text_height();
        let timed = item.mime.starts_with("video/") || item.mime.starts_with("audio/");
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
        cell = cell.child(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .rounded(px(theme::TILE_RADIUS))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(theme::TILE_RADIUS))
                        .bg(placeholder)
                        .opacity(placeholder_alpha),
                )
                .child(
                    img(thumbnail)
                        .image_cache(&self.thumbnails)
                        .size_full()
                        .rounded(px(theme::TILE_RADIUS))
                        // A thumbnail of another shape than its item (cover
                        // art, a rotated video) is fitted, not cropped.
                        .object_fit(if contain {
                            ObjectFit::Contain
                        } else {
                            ObjectFit::Cover
                        })
                        .opacity(image_alpha),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(theme::TILE_RADIUS))
                        .border_1()
                        .border_color(theme.border()),
                ),
        );
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
                    .left(px(tile.w - 28.))
                    .top(px(image_height - 26.))
                    .w(px(28.))
                    .h(px(26.))
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
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.hovered = Some(index);
                    } else if this.hovered == Some(index) {
                        this.hovered = None;
                    }
                    cx.notify();
                }))
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

impl Render for GridView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Grid);
        let build_started = std::time::Instant::now();
        self.session
            .update(cx, |session, cx| session.advance_transition(cx));
        let theme = theme::theme(cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        // The scrollbar's gutter is not grid: tiles lay out beside it.
        let width = (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.);
        let viewport = f32::from(self.bounds.size.height);
        let top = -f32::from(self.scroll.offset().y);

        // Everything the frame needs from the session, copied for the visible
        // tiles only, so the listeners below can borrow the context.
        let (fade, animating, prefs, message, tiles) = {
            let session = self.session.read(cx);
            if session.results_epoch != self.epoch {
                self.epoch = session.results_epoch;
                self.scroll.set_offset(point(px(0.), px(0.)));
                self.layout_key = None;
            }
            let prefs = session.prefs.clone();
            let text_height = prefs.text_height();
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
            let message = if !session.items.is_empty() {
                None
            } else if let Some(error) = &session.error {
                Some(error.clone())
            } else {
                Some(match session.nav.get(session.displayed) {
                    Some(entry) if entry.screen => {
                        format!("{} is not part of this prototype yet", entry.label)
                    }
                    _ if session.loading => "Opening library...".to_owned(),
                    _ => "No items match this filter".to_owned(),
                })
            };
            let tiles = self
                .layout
                .visible(top - reveal::MARGIN, top + viewport + reveal::MARGIN)
                .into_iter()
                .filter_map(|index| {
                    let item = session.items.get(index)?;
                    Some(TileData {
                        index,
                        tile: self.layout.tiles[index],
                        item: item.clone(),
                        thumbnail: session.thumbnail(&item.content_hash),
                        selected: session.selection.contains(&index),
                    })
                })
                .collect::<Vec<_>>();
            (
                session.transition.opacity(),
                session.transition.animating(),
                prefs,
                message,
                tiles,
            )
        };
        // `CanvasGrid`: fetch the next page within three viewports of the end.
        if top + viewport * 4. >= self.layout.total_height && !tiles.is_empty() {
            self.session.update(cx, |session, cx| session.load_more(cx));
        }

        let tile_count = tiles.len();
        let decoded = {
            let thumbnails = self.thumbnails.read(cx);
            tiles
                .iter()
                .map(|data| {
                    (
                        data.item.root_id.0,
                        thumbnails.is_loaded(&Resource::Path(data.thumbnail.clone().into())),
                    )
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

        let body = if let Some(message) = message {
            div()
                .id("grid-message")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .opacity(fade)
                .text_color(theme.text_tertiary())
                .child(message)
        } else {
            let mut content = div().relative().w_full().h(px(self.layout.total_height));
            for data in tiles {
                content = content.child(self.tile(data, &prefs, now, cx));
            }
            div()
                .id("media-grid")
                .size_full()
                .opacity(fade)
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .on_scroll(cx.listener(|this, _, _, cx| {
                    // Hover drawing stops while scrolling and resumes 80 ms
                    // after the last scroll event.
                    this.scrolled = std::time::Instant::now();
                    this.settle = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(HOVER_RESUME).await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    }));
                    cx.notify();
                }))
                // A click that no image took is a click on empty space.
                .on_click(cx.listener(|this, _, window, cx| {
                    this.session
                        .update(cx, |session, cx| session.clear_selection(cx));
                    window.focus(&this.focus, cx);
                }))
                .child(content)
        };
        crate::stats::grid_built(build_started, tile_count);
        div()
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
            ))
    }
}
