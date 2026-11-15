//! What the Subscriptions screen puts in the window's chrome, as the grid
//! and the other screens do: its actions at the end of the titlebar (the
//! count and Add, Add Gallery and Accounts over the cards; Run now, Hold and
//! the overflow menu on a subscription's page), and a subscription's
//! counters, schedule and destination in the inspector's rail.
use super::{SubscriptionsScreen, format, measure, place_of};
use crate::i18n::t;
use crate::theme;
use crate::ui::{self, action_button::Look};
use gpui::*;

/// The titlebar's buttons: the titlebar's control height, with the padding
/// a labelled button keeps there.
const BUTTON_HEIGHT: f32 = theme::CONTROL_MD;
const BUTTON_PADDING: f32 = 10.;
const BUTTON_GAP: f32 = 6.;

/// The labelled buttons of a subscription's page need this much of the
/// titlebar's room for controls, which starts after the history buttons and
/// shares the scope title's half; with less they show their glyphs alone,
/// their labels in their tooltips, as the grid's toolbar gives up its zoom
/// and search when narrow.
const PAGE_LABELLED: f32 = 360.;
const HOME_LABELLED: f32 = 420.;

/// The screen's titlebar controls; they follow the screen.
pub struct SubscriptionsToolbar {
    screen: WeakEntity<SubscriptionsScreen>,
    _observing: Subscription,
}

/// The count after the screen's title; it follows the screen.
pub struct SubscriptionsNote {
    screen: WeakEntity<SubscriptionsScreen>,
    _observing: Subscription,
}

/// A subscription's rail, in the inspector's place; it follows the screen.
pub struct SubscriptionRail {
    screen: WeakEntity<SubscriptionsScreen>,
    _observing: Subscription,
}

/// The screen's titlebar controls, for the toolbar.
pub fn controls(screen: &Entity<SubscriptionsScreen>, cx: &mut App) -> AnyView {
    cx.new(|cx| SubscriptionsToolbar {
        screen: screen.downgrade(),
        _observing: cx.observe(screen, |_, _, cx| cx.notify()),
    })
    .into()
}

/// The count after the screen's title, for the toolbar.
pub fn note(screen: &Entity<SubscriptionsScreen>, cx: &mut App) -> AnyView {
    cx.new(|cx| SubscriptionsNote {
        screen: screen.downgrade(),
        _observing: cx.observe(screen, |_, _, cx| cx.notify()),
    })
    .into()
}

/// A subscription's rail, for the shell to show in the inspector's place.
pub fn rail(screen: &Entity<SubscriptionsScreen>, cx: &mut App) -> AnyView {
    cx.new(|cx| SubscriptionRail {
        screen: screen.downgrade(),
        _observing: cx.observe(screen, |_, _, cx| cx.notify()),
    })
    .into()
}

impl Render for SubscriptionsToolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = div().flex_1().min_w_0().h_full().flex().items_center();
        let Some(screen) = self.screen.upgrade() else {
            return bar;
        };
        screen.update(cx, |screen, cx| screen.titlebar(bar, window, cx))
    }
}

impl Render for SubscriptionsNote {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self
            .screen
            .upgrade()
            .filter(|screen| screen.read(cx).list.is_some() && !screen.read(cx).showing_page())
            .map(|screen| screen.read(cx).count_note());
        div().whitespace_nowrap().children(text)
    }
}

impl Render for SubscriptionRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(screen) = self.screen.upgrade() else {
            return div().into_any_element();
        };
        screen.update(cx, |screen, cx| screen.rail(window, cx))
    }
}

impl SubscriptionsScreen {
    /// A titlebar button: labelled, or its glyph alone with the label as its
    /// tooltip.
    fn titlebar_button(
        id: &'static str,
        look: Look,
        icon: &'static str,
        label: &'static str,
        labelled: bool,
    ) -> ui::action_button::ActionButton {
        let button = ui::action_button(id, look)
            .icon(icon, 14.)
            .height(BUTTON_HEIGHT);
        if labelled {
            button.label(label).padding(BUTTON_PADDING).gap(BUTTON_GAP)
        } else {
            button.width(BUTTON_HEIGHT).padding(0.).tip(ui::tip(label))
        }
    }

    fn titlebar(&mut self, bar: Div, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let bar = bar
            .relative()
            .child(measure(self.titlebar_width.clone(), window));
        if self.list.is_none() {
            return bar;
        }
        match self.place_seen {
            None => self.home_titlebar(bar, cx),
            Some(_) => match self.opened().cloned() {
                Some(subscription) => self.page_titlebar(bar, &subscription, window, cx),
                None => bar,
            },
        }
    }

    /// How many subscriptions there are, or how many are selected.
    fn count_note(&self) -> String {
        let count = self.cards().len();
        let selected = self.selected.len();
        if count == 0 {
            t("No subscriptions yet").to_owned()
        } else if selected > 1 {
            crate::tf!("{value0} of {value1} selected", selected, count)
        } else {
            crate::tf!(
                "{value0} subscription{value1}",
                count,
                crate::i18n::plural_s(count as i128)
            )
        }
    }

    /// The add actions; the count follows the title (`note`).
    fn home_titlebar(&mut self, bar: Div, cx: &mut Context<Self>) -> Div {
        let gallery_running = self.gallery_running();
        let labelled = self.titlebar_width.get() >= HOME_LABELLED;
        bar.child(div().flex_1())
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    Self::titlebar_button(
                        "subscription-add",
                        Look::Primary,
                        "plus",
                        t("Add"),
                        labelled,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_create(cx))),
                )
                // One gallery downloads at a time, so the button waits
                // while one runs.
                .child(
                    Self::titlebar_button(
                        "subscription-add-gallery",
                        Look::Standard,
                        "library-plus",
                        t("Add Gallery"),
                        labelled,
                    )
                    .disabled(gallery_running)
                    .on_click(cx.listener(|this, _, _, cx| this.open_gallery(cx))),
                )
                .child(
                    Self::titlebar_button(
                        "subscription-accounts",
                        Look::Standard,
                        "shield-lock",
                        t("Accounts"),
                        labelled,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_accounts(None, cx))),
                ),
        )
    }

    /// Run or pause, hold or stop, and the overflow menu.
    fn page_titlebar(
        &mut self,
        bar: Div,
        subscription: &super::data::Subscription,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = subscription.id;
        let active = subscription.active_run_id.is_some();
        let running = format::running(subscription);
        let paused_run = subscription.run_status.as_deref() == Some("paused");
        let run_failed = active
            && !running
            && !matches!(
                subscription.run_status.as_deref(),
                Some("paused" | "inbox_full")
            );
        // Run Now, Pause, Resume and Retry (`subscriptions.run`,
        // `subscriptions.run.pause`, `subscriptions.run.resume`).
        let acting = self.acting.contains(&id);
        let labelled = self.titlebar_width.get() >= PAGE_LABELLED;
        let primary = if active {
            let (look, icon, label) = if running {
                (Look::Standard, "player-pause", t("Pause"))
            } else if run_failed {
                (Look::Primary, "refresh", t("Retry"))
            } else {
                (
                    Look::Primary,
                    "player-play",
                    if paused_run { t("Resume") } else { t("Retry") },
                )
            };
            Self::titlebar_button("subscription-run", look, icon, label, labelled).disabled(acting)
        } else {
            Self::titlebar_button(
                "subscription-run",
                Look::Primary,
                "player-play",
                t("Run now"),
                labelled,
            )
            .disabled(acting || subscription.paused || subscription.queries.is_empty())
        }
        .on_click(cx.listener(move |this, _, _, cx| this.run_control(id, cx)));
        // Stop the run (`subscriptions.cancel`), or hold and release future
        // runs (`subscriptions.hold`).
        let (hold_icon, hold_label, hold_tip) = if active {
            (
                "player-stop",
                t("Stop"),
                t("Stop this run and keep the posts already added"),
            )
        } else if subscription.paused {
            ("player-play", t("Release"), t("Allow future runs again"))
        } else {
            ("hand-stop", t("Hold"), t("Put future runs on hold"))
        };
        let bounds = self.overflow.clone();
        bar.child(div().flex_1()).child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(primary)
                .child(
                    Self::titlebar_button(
                        "subscription-hold",
                        Look::Standard,
                        hold_icon,
                        hold_label,
                        labelled,
                    )
                    .tip(ui::tip(hold_tip))
                    .disabled(acting)
                    .on_click(cx.listener(move |this, _, _, cx| this.hold_control(id, cx))),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .child(place_of(self.overflow.clone()))
                        .child(
                            ui::icon_button("subscription-more", "dots-vertical")
                                .tip(ui::tip(t("More actions")))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let at = bounds.get();
                                    let position = point(at.left(), at.bottom() + px(4.));
                                    this.open_menu(vec![id], position, window, cx);
                                })),
                        ),
                ),
        )
    }
}
