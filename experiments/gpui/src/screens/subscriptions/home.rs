//! The grid of subscriptions (`SubscriptionsGrid.tsx`): a header with the
//! count, any gallery downloads and the add actions, over 180px cards of
//! cover, name, status and counts.
use super::data::{self, Cover};
use super::{SubscriptionsScreen, format, measure};
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use crate::{motion, theme};
use gpui::*;

/// `.followingGrid`: `repeat(auto-fill, minmax(150px, 180px))`, 16px apart.
/// Tracks take their 180px maximum whenever one fits.
const CARD: f32 = 180.;
const CARD_GAP: f32 = 16.;

/// `clamp(min, <percent>cqi, max)` against the grid's content width.
fn clamp_cqi(width: f32, min: f32, percent: f32, max: f32) -> f32 {
    (width * percent / 100.).clamp(min, max)
}

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
                .child("Subscribe to artists and tags"),
        )
        .child(
            div().text(theme::TEXT_SM).child(
                "Create a subscription and new posts will land in your library automatically.",
            ),
        )
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
        // `cqi` and the container queries measure the content box: the
        // measured width less the padding and the scrollbar's gutter.
        let width = (self.home_width.get() - 32. - ui::scrollbar::GUTTER).max(0.);
        let order = self.card_order();
        let cards: Vec<data::Subscription> = self.cards().into_iter().cloned().collect();
        let header = self.header(cards.len(), width, cx);
        let content = if cards.is_empty() {
            div()
                .py(px(4.))
                .text(theme::TEXT_MD)
                .text_color(theme::theme(cx).text_tertiary())
                .child("Create a subscription, then add the artists, tags, or accounts it should follow.")
                .into_any_element()
        } else {
            let order = std::rc::Rc::new(order);
            let mut grid = div()
                .id("subscription-cards")
                .flex()
                .flex_wrap()
                .gap(px(CARD_GAP))
                // A click between cards clears the selection.
                .on_click(cx.listener(|this, _, _, cx| {
                    this.selected.clear();
                    cx.notify();
                }));
            for (index, card) in cards.iter().enumerate() {
                grid = grid.child(self.card(index, card, order.clone(), window, cx));
            }
            grid.into_any_element()
        };
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
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .pt(px(12.))
                            .pb(px(24.))
                            .pl(px(16.))
                            // The scrollbar's gutter is kept, scrolling or not.
                            .pr(px(16. + ui::scrollbar::GUTTER))
                            // Out of the flow, so it adds no gap.
                            .child(measure(self.home_width.clone(), window))
                            .child(header)
                            .child(content),
                    ),
            )
            .child(ui::scrollbar(
                "subscriptions-home-scrollbar",
                &self.home_scroll,
                window,
                cx,
            ))
    }

    fn header(&mut self, count: usize, width: f32, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let selected = self.selected.len();
        let subtitle = if count == 0 {
            "No subscriptions yet".to_owned()
        } else if selected > 1 {
            format!("{selected} of {count} selected")
        } else {
            format!("{count} subscription{}", if count == 1 { "" } else { "s" })
        };
        let narrow = width <= 720.;
        let narrower = width <= 600.;
        let jobs: Vec<data::Subscription> = self
            .list
            .iter()
            .flatten()
            .filter(|subscription| {
                format::is_gallery_job(subscription) && subscription.run_status.is_some()
            })
            .cloned()
            .collect();
        let gallery_running = jobs.iter().any(|job| job.active_run_id.is_some());
        let (basis, min, max) = match (jobs.is_empty(), narrow, narrower) {
            (true, _, true) => (20., 12., 400.),
            (true, true, _) => (48., 20., 260.),
            (true, false, _) => (72., 32., 400.),
            (false, _, true) => (160., 88., 260.),
            (false, true, _) => (160., 104., 260.),
            (false, false, _) => (220., 120., 400.),
        };
        let mut strip = div()
            .flex_grow(1.)
            .flex_shrink(1.)
            .flex_basis(px(basis))
            .min_w(px(min))
            .max_w(px(max))
            .min_h(px(38.))
            .flex()
            .flex_col()
            .justify_center()
            .overflow_hidden();
        if !jobs.is_empty() {
            for job in &jobs {
                strip = strip.child(self.gallery_job(job, cx));
            }
        }
        let padding = clamp_cqi(width, 10., 1.8, 20.);
        let gap = clamp_cqi(width, 5., 0.7, 8.);
        div()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(if narrow { 8. } else { 12. }))
            .child(
                div()
                    .flex_grow_0()
                    .flex_shrink(1.)
                    .flex_basis(px(if narrower { 112. } else { 150. }))
                    .min_w(px(if narrower { 92. } else { 104. }))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    // `.heroTitle`: `letter-spacing: -0.01em`.
                    .child(div().overflow_hidden().child(ui::tracked(
                        "Subscriptions",
                        Font {
                            weight: FontWeight::SEMIBOLD,
                            ..font(theme::FONT_UI)
                        },
                        theme::TEXT_TITLE,
                        24.,
                        theme.text(),
                        theme::TEXT_TITLE * -0.01,
                    )))
                    .child(
                        div()
                            .text_size(px(theme::TEXT_SM))
                            .line_height(px(theme::TEXT_SM * theme::LINE_HEIGHT_BODY))
                            .text_color(theme.text_secondary())
                            .whitespace_nowrap()
                            .child(subtitle),
                    ),
            )
            .child(strip)
            .child(
                div()
                    .flex_grow_0()
                    .flex_shrink(1.)
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(clamp_cqi(width, 4., 0.8, 8.)))
                    .child(
                        ui::action_button("subscription-add", Look::Primary)
                            .label("Add")
                            .icon("plus", 14.)
                            .padding(padding)
                            .gap(gap)
                            .on_click(cx.listener(|this, _, _, cx| this.open_create(cx))),
                    )
                    // One gallery downloads at a time, so the button waits
                    // while one runs.
                    .child(
                        ui::action_button("subscription-add-gallery", Look::Standard)
                            .label("Add Gallery")
                            .icon("library-plus", 14.)
                            .padding(padding)
                            .gap(gap)
                            .disabled(gallery_running)
                            .on_click(cx.listener(|this, _, _, cx| this.open_gallery(cx))),
                    )
                    // TODO: the accounts window, for site logins.
                    .child(
                        ui::action_button("subscription-accounts", Look::Standard)
                            .label("Accounts")
                            .icon("shield-lock", 14.)
                            .padding(padding)
                            .gap(gap),
                    ),
            )
    }

    /// One gallery download in the header (`.galleryJob`): its name, how far
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
            (_, Some(total)) => format!(
                "{} / {} images downloaded",
                crate::grid::format::grouped(downloaded),
                crate::grid::format::grouped(total)
            ),
            (_, None) => format!(
                "{} images downloaded",
                crate::grid::format::grouped(downloaded)
            ),
        };
        let id = job.id;
        let acting = self.acting.contains(&id);
        let (icon, tip) = if running {
            ("player-pause", "Pause gallery download")
        } else if paused {
            ("player-play", "Resume gallery download")
        } else {
            ("refresh", "Retry gallery download")
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
                            .tip(ui::tip("Stop gallery download"))
                            .disabled(acting)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.perform(vec![id], |_| super::Command::Stop, cx)
                            })),
                    ),
            )
    }

    fn card(
        &self,
        index: usize,
        card: &data::Subscription,
        order: std::rc::Rc<Vec<i64>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let id = card.id;
        let selected = self.selected.contains(&id);
        let (hover, set_hover) = motion::hover_fade(
            ("subscription-card-hover", id as usize),
            motion::CARD_HOVER,
            motion::EASE,
            window,
            cx,
        );
        let (tone, label) = format::card_status(card);
        let accent = rgb(theme::ACCENT);
        let border = if selected {
            accent
        } else {
            theme::blend(theme.border_secondary(), theme.border(), hover)
        };
        let mut cover = cover(
            ("subscription-cover", id as usize),
            card.cover.as_ref(),
            CARD,
            theme::RADIUS_SM,
            28.,
            &self.covers,
            cx,
        )
        .child(
            // The border over the image, so a framed image keeps it.
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(border),
        );
        if card.cover.is_some() {
            cover = cover.bg(theme.surface_2());
        }
        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(if selected { accent } else { theme.text() })
            .child(card.name.clone());
        let (click_order, menu_order) = (order.clone(), order);
        div()
            .id(("subscription-card", id as usize))
            .w(px(CARD))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(4.))
            .cursor_pointer()
            .on_hover(set_hover)
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
            .child(div().relative().child(cover).children(selected.then(|| {
                // `outline: 2px` one pixel outside the border.
                div()
                    .absolute()
                    .top(px(-3.))
                    .left(px(-3.))
                    .right(px(-3.))
                    .bottom(px(-3.))
                    .rounded(px(theme::RADIUS_SM + 3.))
                    .border_2()
                    .border_color(accent)
            })))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(theme::TEXT_MD))
                    .line_height(px(theme::TEXT_MD * theme::LINE_HEIGHT_BODY))
                    .child(name)
                    .child(ui::status_badge(
                        ("subscription-card-status", id as usize),
                        tone,
                        label,
                    )),
            )
            .child(
                div()
                    .text_size(px(theme::TEXT_CAPTION))
                    .line_height(px(theme::TEXT_CAPTION * theme::LINE_HEIGHT_BODY))
                    .text_color(theme.text_tertiary())
                    .text_center()
                    .child(format::card_meta(card.items, card.queries.len())),
            )
    }
}
