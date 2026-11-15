//! The subscriptions as grid tiles (`SubscriptionsGrid.tsx`), laid out and
//! drawn by the grid's own rules: the cover as the tile's picture, the name
//! under it, and the state and counts as its details row; any gallery
//! downloads in a bar above them. The count and the add actions are in the
//! titlebar (`toolbar.rs`).
use super::data::{self, Cover};
use super::{SubscriptionsScreen, format, measure};
use crate::grid::layout::{self, Layout, Tile, ViewMode};
use crate::grid::prefs::ViewPrefs;
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::{prelude::FluentBuilder as _, *};

/// A tile's two text rows under its picture: the name, then the details.
const TEXT_ROWS: f32 = 2. * layout::TEXT_ROW_HEIGHT;

/// A subscription's cover in a square box of `size` with a 1px border: the
/// framed image, or the download glyph over a faint gradient.
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

/// `EmptyState` with the screen's welcome: shown in place of a subscription
/// the library no longer has.
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

/// The space either side of a tile's details row.
const DETAILS_INSET: f32 = 4.;
const DETAILS_GAP: f32 = 6.;

/// How wide a status badge with `label` is drawn: its spaced mono
/// capitals, its padding and its border.
pub(super) fn badge_width(label: &str, window: &Window) -> f32 {
    let label = label.to_uppercase();
    f32::from(
        ui::shape_line(
            label.clone(),
            font(theme::FONT_MONO),
            theme::TEXT_XS,
            black(),
            window,
        )
        .width,
    ) + label.chars().count() as f32 * theme::TEXT_XS * 0.04
        + 12.
}

/// How wide a tile's details row is: the state badge (its spaced mono
/// capitals, padding and border), the gap, and the items and sources.
fn details_width(card: &data::Subscription, window: &Window) -> f32 {
    let (_, label) = format::card_status(card);
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
    /// The subscriptions the grid shows, gallery downloads aside, in its order.
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

    pub(super) fn card_order(&self) -> Vec<i64> {
        self.cards().iter().map(|card| card.id).collect()
    }

    pub(super) fn home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let order = std::rc::Rc::new(self.card_order());
        let cards: Vec<data::Subscription> = self.cards().into_iter().cloned().collect();
        // The grid's own layout at its default size and spacing, across the
        // width the tiles have, with the size setting raised a step at a
        // time, as the grid's slider raises it, until every tile's details
        // fit on their line.
        let prefs = ViewPrefs::default();
        let needed = cards
            .iter()
            .map(|card| details_width(card, window))
            .fold(0., f32::max);
        let width = self.home_width.get();
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
            if side >= needed || target >= width {
                break layout;
            }
            target += 50.;
        };
        let mut grid = div()
            .id("subscription-cards")
            .a11y_presentational()
            .relative()
            .w_full()
            .h(px(layout.total_height))
            .child(measure(self.home_width.clone(), window))
            // A click between tiles clears the selection.
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

    /// The gallery downloads that have run, as a bar over the tiles, as the
    /// filter bar sits over the grid.
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

    /// One gallery download (`.galleryJob`): its name, how far
    /// it has got or why it stopped, a 2px progress line, and its controls.
    fn gallery_job(&self, job: &data::Subscription, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let running = format::running(job);
        let paused = job.run_status.as_deref() == Some("paused");
        let failed = job
            .queries
            .iter()
            .find(|query| query.last_failure_message.is_some());
        // `progressBySubscriptionId`: what the run in progress reports.
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
                    // Filled to the share downloaded once the site has
                    // reported the gallery's size, swept until then.
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
                    // Pause, resume or retry the download, and stop it, as a
                    // subscription's run is.
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

    /// One tile: the cover in the tile's picture, ringed and selected as a
    /// grid tile is, the name centred under it, then the state and counts.
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
        let (tone, label) = format::card_status(card);
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
        // The hairline ring over the picture, as a grid tile has.
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::TILE_RADIUS))
                .border_1()
                .border_color(theme.border()),
        );
        let (click_order, menu_order) = (order.clone(), order);
        // A tile is a button named by its subscription; assistive
        // technology's press opens it, as a double-click does.
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
                // The grid's selection stroke outside the picture, with a
                // hairline of the background between it and the picture.
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
                // The name, as a grid tile's: centred, cut short to the tile
                // less 8px, white on a pill of the selection colour when
                // selected.
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
                // The details row: the state, then the items and sources.
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
