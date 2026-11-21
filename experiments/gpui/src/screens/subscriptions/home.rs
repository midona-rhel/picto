//! The top level of the Subscriptions screen (production's
//! `SubscriptionsGrid.tsx`), which shows one tile for each subscription. It
//! is what the screen shows while no subscription's page is open.
//!
//! The tiles are positioned by the grid's layout (`grid::layout`) and are
//! drawn to look like the grid's tiles. A tile shows the subscription's
//! cover as its picture. Under the picture it has two lines: the
//! subscription's name, and then a details row with a status badge and the
//! numbers of items and sources.
//!
//! Gallery downloads are not shown as tiles. The ones that have run at
//! least once are listed in a bar above the tiles.
//!
//! The number of subscriptions and the buttons that add a subscription are
//! in the titlebar. `toolbar.rs` in this folder draws them.
use super::data::{self, Cover};
use super::{SubscriptionsScreen, format};
use crate::grid::layout::{self, Layout, Tile, ViewMode};
use crate::grid::prefs::ViewPrefs;
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::{prelude::FluentBuilder as _, *};

/// The height in pixels of the two lines of text under a tile's picture:
/// the name, and then the details row. Each line is
/// `layout::TEXT_ROW_HEIGHT` tall.
const TEXT_ROWS: f32 = 2. * layout::TEXT_ROW_HEIGHT;

/// Draws a subscription's cover in a square `size` pixels wide and high,
/// with corners rounded by `radius`. The image is inset by 1 px on every
/// side, which leaves room for the 1 px border that the caller draws over
/// the square's edge.
///
/// If the subscription has a cover image and the user has set a crop for
/// it, the image is scaled and positioned so that the cropped square of it
/// (`crate::cover::square`) fills the inset square. If the crop is the
/// default one, or the size of the source image is not known, the image is
/// scaled to cover the inset square around its centre.
///
/// If the subscription has no cover image, the square is filled with the
/// theme's `surface_2` colour. Over that it has a gradient that goes from
/// the theme's contrast colour at 5% opacity to fully transparent, and the
/// "download-thin" icon at `glyph` pixels in the square's centre.
pub(super) fn cover(
    id: impl Into<ElementId>,
    cover: Option<&Cover>,
    size: f32,
    radius: f32,
    glyph: f32,
    cache: &Entity<super::Thumbnails>,
    cx: &App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let inner = size - 2.;
    let mut frame = div()
        .id(id)
        .relative()
        .size(px(size))
        .flex_none()
        .overflow_hidden()
        .rounded(px(radius));
    match cover {
        Some(cover) => {
            let image = img(cover.thumbnail.clone())
                .image_cache(cache)
                .absolute()
                .object_fit(ObjectFit::Fill);
            let framed = cover
                .size
                .filter(|_| cover.crop != crate::cover::Crop::default())
                .map(|(width, height)| {
                    let (x, y, side) =
                        crate::cover::square(width as f32, height as f32, cover.crop);
                    let scale = inner / side.max(1.);
                    image
                        .left(px(1. - x * scale))
                        .top(px(1. - y * scale))
                        .w(px(width as f32 * scale))
                        .h(px(height as f32 * scale))
                });
            frame = frame.child(match framed {
                Some(image) => image,
                None => img(cover.thumbnail.clone())
                    .image_cache(cache)
                    .absolute()
                    .top(px(1.))
                    .left(px(1.))
                    .size(px(inner))
                    .rounded(px(radius - 1.))
                    .object_fit(ObjectFit::Cover),
            });
        }
        None => {
            let wash = theme.contrast(5.);
            frame = frame.bg(theme.surface_2()).child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(linear_gradient(
                        160.,
                        linear_color_stop(wash, 0.),
                        linear_color_stop(Rgba { a: 0., ..wash }, 1.),
                    ))
                    .child(ui::icon("download-thin", glyph, theme.text_tertiary())),
            );
        }
    }
    frame
}

/// Draws the panel that the screen shows when the open subscription is no
/// longer in the library, in the place where that subscription's page
/// would be. It is production's `EmptyState` with the Subscriptions
/// screen's welcome text: the heading "Subscribe to artists and tags" and
/// one sentence that says what a subscription does.
pub(super) fn empty_state(cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .p(px(16.))
        .rounded(px(theme::RADIUS_LG))
        .border_1()
        .border_color(theme.border_secondary())
        .bg(theme.rail())
        .text_color(theme.text_secondary())
        .child(
            div()
                .text(14.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text())
                .child(t("Subscribe to artists and tags")),
        )
        .child(div().text(theme::TEXT_SM).child(t(
            "Create a subscription and new posts will land in your library automatically.",
        )))
}

/// The space in pixels between each side of a tile and the tile's details
/// row.
const DETAILS_INSET: f32 = 4.;
/// The space in pixels between the status badge in a details row and the
/// text after it.
const DETAILS_GAP: f32 = 6.;

/// Returns the width in pixels that a status badge with the text `label`
/// is drawn at: the width of the text at the caption size, plus 12 px for
/// the badge's 5 px of padding and 1 px border on each side.
pub(super) fn badge_width(label: &str, window: &Window) -> f32 {
    f32::from(
        ui::shape_line(
            ui::status_badge::sentence(label),
            font(theme::FONT_UI),
            theme::TEXT_CAPTION,
            black(),
            window,
        )
        .width,
    ) + 12.
}

/// Returns the width in pixels that the tile of `card` needs for its
/// details row to show all of its text. That is the width of the status
/// badge, the 6 px gap, the width of the text with the numbers of items
/// and sources, and the 4 px inset on each side, rounded up to a whole
/// pixel.
fn details_width(card: &data::Subscription, window: &Window) -> f32 {
    let (_, label) = format::status(card);
    let badge = badge_width(label, window);
    let meta = f32::from(
        ui::shape_line(
            format::card_meta(card.items, card.queries.len()),
            font(theme::FONT_UI),
            theme::TEXT_CAPTION,
            black(),
            window,
        )
        .width,
    );
    (badge + DETAILS_GAP + meta + 2. * DETAILS_INSET).ceil()
}

impl SubscriptionsScreen {
    /// Returns the subscriptions that are shown as tiles, in the order
    /// their tiles are shown: sorted by name (`format::card_order`).
    /// Gallery downloads are left out, because they are listed in the bar
    /// above the tiles.
    pub(super) fn cards(&self) -> Vec<&data::Subscription> {
        let mut cards: Vec<&data::Subscription> = self
            .list
            .iter()
            .flatten()
            .filter(|subscription| !format::is_gallery_job(subscription))
            .collect();
        format::card_order(&mut cards);
        cards
    }

    /// Returns the ids of the subscriptions that are shown as tiles, in the
    /// order their tiles are shown (`cards`).
    pub(super) fn card_order(&self) -> Vec<i64> {
        self.cards().iter().map(|card| card.id).collect()
    }

    /// Builds the screen's top level. From top to bottom it has the bar
    /// that lists the gallery downloads, when there are any, and the area
    /// that shows the tiles, which scrolls vertically and has a scrollbar.
    /// When there is no subscription to show as a tile, that area shows one
    /// sentence that says how to start.
    ///
    /// The tiles are laid out for `self.width`, the width of the screen's
    /// area in the frame that is being built.
    pub(super) fn home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let order = std::rc::Rc::new(self.card_order());
        let cards: Vec<data::Subscription> = self.cards().into_iter().cloned().collect();
        // The tiles are laid out with the grid's layout in grid mode, at the
        // default tile size and spacing. If the widest details line under a
        // tile does not fit the tile's width, one column is removed with a
        // zoom step and the layout is computed again. This repeats until
        // the line fits or one column is left.
        let prefs = ViewPrefs::default();
        let needed = cards
            .iter()
            .map(|card| details_width(card, window))
            .fold(0., f32::max);
        let width = self.width;
        let mut target = prefs.target_size;
        let layout = loop {
            let layout = Layout::compute(
                &vec![1.; cards.len()],
                width,
                target,
                prefs.gap,
                ViewMode::Grid,
                TEXT_ROWS,
            );
            let side = layout.tiles.first().map_or(f32::MAX, |tile| tile.w);
            let larger = layout::zoom_step(width, target, prefs.gap, ViewMode::Grid, true);
            match larger {
                Some(larger) if side < needed => target = larger,
                _ => break layout,
            }
        };
        let mut grid = div()
            .id("subscription-cards")
            .a11y_presentational()
            .relative()
            .w_full()
            .h(px(layout.total_height))
            // A tile stops a click on it from reaching this handler. A
            // click that arrives here was therefore on the empty space
            // between tiles, and it clears the selection.
            .on_click(cx.listener(|this, _, _, cx| {
                this.selected.clear();
                cx.notify();
            }));
        if cards.is_empty() {
            grid = grid.child(
                div()
                    .p(px(16.))
                    .text_color(theme::theme(cx).text_tertiary())
                    .child(t("Create a subscription, then add the artists, tags, or accounts it should follow.")),
            );
        }
        for (index, (card, tile)) in cards.iter().zip(&layout.tiles).enumerate() {
            grid = grid.child(self.card(index, card, *tile, order.clone(), window, cx));
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .children(self.gallery_jobs(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("subscriptions-home")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.home_scroll)
                            .child(grid),
                    )
                    .child(ui::scrollbar(
                        "subscriptions-home-scrollbar",
                        &self.home_scroll,
                        window,
                        cx,
                    )),
            )
    }

    /// Draws the bar that lists the gallery downloads, with one row for
    /// each. Only gallery downloads that have run at least once are listed.
    /// Returns `None` when there are none, and then no bar is shown. The
    /// bar is above the tiles, in the position the filter bar has above
    /// the grid.
    fn gallery_jobs(&self, cx: &mut Context<Self>) -> Option<Div> {
        let jobs: Vec<data::Subscription> = self
            .list
            .iter()
            .flatten()
            .filter(|subscription| {
                format::is_gallery_job(subscription) && subscription.run_status.is_some()
            })
            .cloned()
            .collect();
        if jobs.is_empty() {
            return None;
        }
        let theme = theme::theme(cx);
        let mut bar = div()
            .flex_none()
            .px(px(16.))
            .py(px(4.))
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(theme.border_secondary());
        for job in &jobs {
            bar = bar.child(self.gallery_job(job, cx));
        }
        Some(bar)
    }

    /// Draws the row of one gallery download (production's `.galleryJob`).
    /// From left to right the row has:
    ///
    /// - A download icon.
    /// - The download's name with a status text to its right, and a 2 px
    ///   tall progress bar under both. If the download is not running and
    ///   a failure message is recorded for it, the status text describes
    ///   that failure. Otherwise it is "N / M images downloaded", or
    ///   "N images downloaded" while the total is not known.
    /// - Two buttons. The first pauses, resumes or retries the download,
    ///   and the second stops it.
    fn gallery_job(&self, job: &data::Subscription, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let running = format::running(job);
        let paused = job.run_status.as_deref() == Some("paused");
        let failed = job
            .queries
            .iter()
            .find(|query| query.last_failure_message.is_some());
        // The progress that the run in progress has reported for this
        // download, if it has reported any: the number of images
        // downloaded and, once the site has given it, the total number.
        // Production keeps this in `progressBySubscriptionId`.
        let progress = self.gallery_progress.get(&job.id).copied();
        let downloaded = progress.map_or(0, |progress| progress.downloaded);
        let total = progress.and_then(|progress| progress.total);
        let status = match (failed, total) {
            (Some(query), _) if !running => format::describe_gallery_failure(
                query.last_failure_kind.as_deref(),
                query.last_failure_message.as_deref(),
            )
            .to_owned(),
            (_, Some(total)) => crate::tf!(
                "{value0} / {value1} images downloaded",
                crate::i18n::format::grouped(downloaded),
                crate::i18n::format::grouped(total)
            ),
            (_, None) => crate::tf!(
                "{value0} images downloaded",
                crate::i18n::format::grouped(downloaded)
            ),
        };
        let id = job.id;
        let acting = self.acting.contains(&id);
        let (icon, tip) = if running {
            ("player-pause", t("Pause gallery download"))
        } else if paused {
            ("player-play", t("Resume gallery download"))
        } else {
            ("refresh", t("Retry gallery download"))
        };
        div()
            .min_h(px(32.))
            .flex()
            .items_center()
            .gap(px(9.))
            .child(ui::icon("download-gallery", 15., theme.text()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.))
                            .text(12.)
                            .child(div().flex_1().min_w_0().truncate().child(job.name.clone()))
                            .child(
                                div()
                                    .flex_shrink(1.)
                                    .min_w_0()
                                    .max_w(relative(0.55))
                                    .truncate()
                                    .text_color(theme.text_tertiary())
                                    .font_features(theme::tabular_figures())
                                    .child(status),
                            ),
                    )
                    // Once the site has reported how many images the
                    // gallery has, the bar is filled in proportion to the
                    // number downloaded. Until then the bar shows the
                    // moving segment of a progress bar whose total is not
                    // known.
                    .child(match total {
                        Some(total) => ui::progress_bar().height(2.).filled(
                            ("gallery-progress", job.id as usize),
                            ui::progress_bar::share(downloaded, total),
                        ),
                        None => ui::progress_bar().height(2.),
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    // The first button pauses a running download, resumes a
                    // paused one, and retries any other. The second button
                    // stops the download. They send the same commands as
                    // the run controls of a subscription.
                    .child(
                        ui::small_button(("gallery-toggle", job.id as usize), icon)
                            .size(26.)
                            .tip(ui::tip(tip))
                            .disabled(acting)
                            .on_click(cx.listener(move |this, _, _, cx| this.run_control(id, cx))),
                    )
                    .child(
                        ui::small_button(("gallery-stop", job.id as usize), "player-stop")
                            .size(26.)
                            .tip(ui::tip(t("Stop gallery download")))
                            .disabled(acting)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.perform(vec![id], |_| super::Command::Stop, cx)
                            })),
                    ),
            )
    }

    /// Draws the tile of one subscription at the position and size that
    /// `tile` gives. The cover fills the square picture area at the top of
    /// the tile and has a 1 px border over its edge. Under the picture are
    /// the name, centred, and then the details row with the status badge
    /// and the numbers of items and sources.
    ///
    /// A selected tile is marked the way a selected grid tile is: an
    /// outline in the selection colour around the picture, and the
    /// selection colour behind the name. A click on the tile selects it, a
    /// double-click opens the subscription, and a right-click opens the
    /// tile's menu.
    fn card(
        &self,
        index: usize,
        card: &data::Subscription,
        tile: Tile,
        order: std::rc::Rc<Vec<i64>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let id = card.id;
        let selected = self.selected.contains(&id);
        let side = tile.w;
        let (tone, label) = format::status(card);
        let extent = layout::SELECTION_EXTENT;
        let picture = cover(
            ("subscription-cover", id as usize),
            card.cover.as_ref(),
            side,
            theme::TILE_RADIUS,
            28.,
            &self.covers,
            cx,
        )
        .absolute()
        .top_0()
        .left_0()
        .when(card.cover.is_some(), |picture| {
            picture.bg(theme.surface_2())
        })
        // A 1 px border in the theme's border colour is drawn over the
        // edge of the picture, as a grid tile has.
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::TILE_RADIUS))
                .border_1()
                .border_color(theme.border()),
        );
        let (click_order, menu_order) = (order.clone(), order);
        // To assistive technology a tile is a button whose label is the
        // subscription's name. When assistive technology presses the
        // button, the subscription is opened, as a double-click opens it.
        div()
            .id(("subscription-card", id as usize))
            .role(Role::Button)
            .aria_label(card.name.clone())
            .aria_selected(selected)
            .on_a11y_action(AccessibleAction::Click, {
                let view = cx.entity().downgrade();
                move |_, _, cx| {
                    let _ = view.update(cx, |this, cx| this.open(id, cx));
                }
            })
            .absolute()
            .left(px(tile.x))
            .top(px(tile.y))
            .w(px(tile.w))
            .h(px(tile.h))
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if event.click_count() >= 2 {
                    this.open(id, cx);
                } else {
                    this.click_card(index, &click_order, event.modifiers(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.card_menu(index, &menu_order, event.position, window, cx);
                }),
            )
            .when(selected, |cell| {
                // A selected tile has the grid's selection outline: a 2 px
                // border in the selection colour whose outer edge is 3 px
                // (`layout::SELECTION_EXTENT`) outside the picture on every
                // side. Between that border and the picture there is a
                // 1 px border in the background colour, which keeps the
                // outline visually apart from the picture.
                cell.child(
                    div()
                        .absolute()
                        .left(px(-extent))
                        .top(px(-extent))
                        .w(px(side + 2. * extent))
                        .h(px(side + 2. * extent))
                        .rounded(px(theme::TILE_RADIUS + extent))
                        .border_2()
                        .border_color(rgb(theme::GRID_SELECTION)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(-1.))
                        .top(px(-1.))
                        .w(px(side + 2.))
                        .h(px(side + 2.))
                        .rounded(px(theme::TILE_RADIUS + 1.))
                        .border_1()
                        .border_color(theme.background()),
                )
            })
            .child(picture)
            .child(
                // The name is drawn as a grid tile's name is. It is centred
                // under the picture. Its text can be as wide as the tile
                // less 8 px, and a longer name is cut off with an ellipsis.
                // On a selected tile the name is white on a rounded
                // rectangle of the selection colour.
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(side + 7.))
                    .h(px(15.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .max_w(px(side - 2.))
                            .px(px(3.))
                            .rounded(px(2.))
                            .when(selected, |pill| {
                                pill.bg(rgb(theme::GRID_SELECTION))
                                    .text_color(rgb(theme::BADGE_STRONG_TEXT))
                            })
                            .line_height(px(15.))
                            .truncate()
                            .child(card.name.clone()),
                    ),
            )
            .child(
                // The details row is the second line under the picture. It
                // shows the status badge and then the numbers of items and
                // sources, centred as a group.
                div()
                    .absolute()
                    .left(px(DETAILS_INSET))
                    .right(px(DETAILS_INSET))
                    .top(px(side + layout::TEXT_ROW_HEIGHT + 3.))
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(DETAILS_GAP))
                    .child(ui::status_badge(
                        ("subscription-card-status", id as usize),
                        tone,
                        label,
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(format::card_meta(card.items, card.queries.len())),
                    ),
            )
    }
}
