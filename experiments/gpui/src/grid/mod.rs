//! The grid of results. Its own view, so scrolling, thumbnail reveals and
//! fades repaint the grid and nothing else.
pub mod format;
pub mod layout;
pub mod nav;
pub mod prefs;
pub mod reveal;
pub mod thumbnails;

use crate::session::{Change, Session};
use crate::{theme, ui};
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
    _session: Subscription,
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
            _session: subscription,
        }
    }

    /// Moves the selection as the Electron grid's keyboard navigation does and
    /// scrolls the new selection into view.
    pub fn move_selection(&mut self, action: nav::Move, cx: &mut Context<Self>) {
        let (width, height) = (
            f32::from(self.bounds.size.width),
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
        self.session
            .update(cx, |session, cx| session.select(target, cx));
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
        &self,
        data: TileData,
        prefs: &ViewPrefs,
        now: f64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
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
        let (x, y, w, h) = if !prefs.fit_cover || timed {
            let aspect = match (item.width, item.height) {
                (Some(w), Some(h)) if w > 0 && h > 0 => w as f32 / h as f32,
                _ => 1.,
            };
            let scale = (tile.w / aspect).min(image_height);
            let (w, h) = (aspect * scale, scale);
            ((tile.w - w) / 2., (image_height - h) / 2., w, h)
        } else {
            (0., 0., tile.w, image_height)
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
            .id(index)
            .absolute()
            .left(px(tile.x))
            .top(px(tile.y))
            .w(px(tile.w))
            .h(px(tile.h));
        if selected {
            cell = cell.child(
                div()
                    .absolute()
                    .left(px(x - layout::SELECTION_EXTENT))
                    .top(px(y - layout::SELECTION_EXTENT))
                    .w(px(w + 2. * layout::SELECTION_EXTENT))
                    .h(px(h + 2. * layout::SELECTION_EXTENT))
                    .rounded(px(theme::TILE_RADIUS + layout::SELECTION_EXTENT))
                    .border_2()
                    .border_color(theme.primary()),
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
                        .object_fit(ObjectFit::Cover)
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
                        .child(ui::icon("folders", 12., rgba(theme::BADGE_TEXT))),
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
                        div()
                            .max_w_full()
                            .px(px(3.))
                            .rounded(px(2.))
                            .bg(if selected {
                                theme.primary()
                            } else {
                                rgba(0x00000000)
                            })
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .line_height(px(15.))
                            .child(item.name.clone()),
                    ),
            );
        }
        if prefs.show_resolution
            && let (Some(width), Some(height)) = (item.width, item.height)
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
                    .text_size(px(theme::TEXT_CAPTION))
                    .line_height(px(15.))
                    .text_color(theme.text_tertiary())
                    .child(format!("{width} × {height}")),
            );
        }
        cell.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            let open = event.click_count() == 2;
            this.session.update(cx, |session, cx| {
                session.select(index, cx);
                if open {
                    session.open_viewer(cx);
                }
            });
            window.focus(&this.focus, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.session
                    .update(cx, |session, cx| session.select(index, cx));
                cx.emit(MenuRequested(event.position));
                window.focus(&this.focus, cx);
            }),
        )
    }
}

impl Render for GridView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.session
            .update(cx, |session, cx| session.advance_transition(cx));
        let theme = theme::theme(cx);
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        let width = f32::from(self.bounds.size.width);
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
                    .map(|i| i.width.unwrap_or(1) as f32 / i.height.unwrap_or(1).max(1) as f32)
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
                        selected: index == session.selected,
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
        if top + viewport + 1000. >= self.layout.total_height && !tiles.is_empty() {
            self.session.update(cx, |session, cx| session.load_more(cx));
        }

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
                .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                .child(content)
        };
        div().relative().size_full().child(measure).child(body)
    }
}
