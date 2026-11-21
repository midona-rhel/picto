//! What the Subscriptions screen draws outside its own area of the window, as
//! the grid and the other screens do. There are three parts. The first is the
//! screen's buttons at the right end of the titlebar: Add, Add Gallery and
//! Accounts while the screen shows its subscription cards, and Run now, Hold
//! and the "More actions" menu button while it shows one subscription's page.
//! The second is the count of subscriptions, which is shown after the screen's
//! title in the titlebar. The third is the rail of a subscription's page,
//! which is drawn where the inspector is for the grid and shows that
//! subscription's counters, schedule and destination.
use super::{SubscriptionsScreen, format, place_of};
use crate::i18n::t;
use crate::ui::{self, action_button::Look};
use gpui::*;

/// The width, in pixels, that the titlebar's space for the screen's controls
/// must have for the buttons to show their labels. `PAGE_LABELLED` is for the
/// buttons of a subscription's page and `HOME_LABELLED` is for the buttons
/// shown over the subscription cards. That space starts after the Back and
/// Forward buttons and runs to the right end of the toolbar. The scope title
/// covers the toolbar's left half, so it lies over the left part of the space.
/// When the space is narrower than this, each button shows only its icon and
/// puts its label in its tooltip. The grid's toolbar does something similar:
/// it hides its zoom control, and then its search field, when it is narrow.
const PAGE_LABELLED: f32 = 360.;
const HOME_LABELLED: f32 = 420.;

/// The view that draws the Subscriptions screen's buttons in the titlebar. It
/// observes the screen and is drawn again whenever the screen changes.
pub struct SubscriptionsToolbar {
    screen: WeakEntity<SubscriptionsScreen>,
    _observing: Subscription,
}

/// The view that draws the count of subscriptions after the screen's title in
/// the titlebar. It observes the screen and is drawn again whenever the screen
/// changes.
pub struct SubscriptionsNote {
    screen: WeakEntity<SubscriptionsScreen>,
    _observing: Subscription,
}

/// The view that draws a subscription's rail, which the shell shows where the
/// inspector normally is. It observes the screen and is drawn again whenever
/// the screen changes.
pub struct SubscriptionRail {
    screen: WeakEntity<SubscriptionsScreen>,
    _observing: Subscription,
}

/// Creates the view of the screen's titlebar buttons. The shell gives it to
/// the toolbar with `Toolbar::set_screen_controls`.
pub fn controls(screen: &Entity<SubscriptionsScreen>, cx: &mut App) -> AnyView {
    cx.new(|cx| SubscriptionsToolbar {
        screen: screen.downgrade(),
        _observing: cx.observe(screen, |_, _, cx| cx.notify()),
    })
    .into()
}

/// Creates the view of the count that is shown after the screen's title. The
/// shell gives it to the toolbar with `Toolbar::set_screen_note`.
pub fn note(screen: &Entity<SubscriptionsScreen>, cx: &mut App) -> AnyView {
    cx.new(|cx| SubscriptionsNote {
        screen: screen.downgrade(),
        _observing: cx.observe(screen, |_, _, cx| cx.notify()),
    })
    .into()
}

/// Creates a subscription's rail for the shell. The shell shows it where the
/// inspector normally is, for as long as the screen shows a subscription's
/// page (`showing_page`).
pub fn rail(screen: &Entity<SubscriptionsScreen>, cx: &mut App) -> crate::shell::ScreenRail {
    let view = cx
        .new(|cx| SubscriptionRail {
            screen: screen.downgrade(),
            _observing: cx.observe(screen, |_, _, cx| cx.notify()),
        })
        .into();
    let screen = screen.downgrade();
    crate::shell::ScreenRail {
        view,
        shows: Box::new(move |cx| {
            screen
                .upgrade()
                .is_some_and(|screen| screen.read(cx).showing_page())
        }),
    }
}

impl Render for SubscriptionsToolbar {
    /// The buttons show their labels only when the titlebar's space for
    /// them is wide enough. The buttons are therefore built for the width of
    /// that space in the frame that lays it out (`ui::sized_view`). The
    /// screen builds them, because they call the screen's methods.
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex_1().min_w_0().h_full().child(
            ui::sized_view(self.screen.clone(), |screen, bounds, window, cx| {
                screen
                    .titlebar(f32::from(bounds.size.width), window, cx)
                    .into_any_element()
            })
            .size_full(),
        )
    }
}

impl Render for SubscriptionsNote {
    /// Draws the count of subscriptions (`count_note`) on one line. It
    /// draws nothing until the list of subscriptions has been read for the
    /// first time. It also draws nothing while the screen shows one
    /// subscription's page. The toolbar goes on drawing this view after the
    /// scope title then, so the view itself has to be empty
    /// (`Toolbar::set_screen_note`).
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
    /// Draws the rail of the open subscription's page, which the screen
    /// builds (`SubscriptionsScreen::rail` in `detail.rs`). It draws
    /// nothing when the screen no longer exists.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(screen) = self.screen.upgrade() else {
            return div().into_any_element();
        };
        screen.update(cx, |screen, cx| screen.rail(window, cx))
    }
}

impl SubscriptionsScreen {
    /// Builds the screen's titlebar buttons for `width`, the width in pixels
    /// of the titlebar's space for them. Nothing is drawn until the list of
    /// subscriptions has been read for the first time.
    fn titlebar(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let bar = div().relative().size_full().flex().items_center();
        if self.list.is_none() {
            return bar;
        }
        match self.place_seen {
            None => self.home_titlebar(bar, width >= HOME_LABELLED, cx),
            Some(_) => match self.opened().cloned() {
                Some(subscription) => {
                    self.page_titlebar(bar, width >= PAGE_LABELLED, &subscription, window, cx)
                }
                None => bar,
            },
        }
    }

    /// The text of the count. It is "No subscriptions yet" when there are no
    /// subscription cards, the number selected out of the total (for example
    /// "2 of 5 selected") when more than one card is selected, and otherwise
    /// the number of subscription cards (for example "5 subscriptions").
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

    /// Builds the titlebar buttons shown over the subscription cards: Add, Add
    /// Gallery and Accounts. The count is not drawn here; the view that
    /// [`note`] creates draws it after the screen's title.
    fn home_titlebar(&mut self, bar: Div, labelled: bool, cx: &mut Context<Self>) -> Div {
        let gallery_running = self.gallery_running();
        bar.child(div().flex_1())
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(ui::titlebar_button::BETWEEN))
                .child(
                    ui::titlebar_button(
                        "subscription-add",
                        Look::Primary,
                        "plus",
                        t("Add"),
                        labelled,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_create(cx))),
                )
                // Only one gallery can download at a time, so the Add Gallery
                // button is disabled while a gallery download is running.
                .child(
                    ui::titlebar_button(
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
                    ui::titlebar_button(
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

    /// Builds the titlebar buttons of a subscription's page: the run button
    /// (Run now, Pause, Resume or Retry), the hold button (Hold, Release or
    /// Stop) and the "More actions" button, which opens the subscription's
    /// menu.
    fn page_titlebar(
        &mut self,
        bar: Div,
        labelled: bool,
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
        // The run button. It reads Pause while a run is under way, Resume
        // while the run is paused, Retry for any other active run, and Run now
        // when no run is active. A click sends `subscriptions.run`,
        // `subscriptions.run.pause` or `subscriptions.run.resume`.
        let acting = self.acting.contains(&id);
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
            ui::titlebar_button("subscription-run", look, icon, label, labelled).disabled(acting)
        } else {
            ui::titlebar_button(
                "subscription-run",
                Look::Primary,
                "player-play",
                t("Run now"),
                labelled,
            )
            .disabled(acting || subscription.paused || subscription.queries.is_empty())
        }
        .on_click(cx.listener(move |this, _, _, cx| this.run_control(id, cx)));
        // The hold button. While a run is active it reads Stop, and a click
        // cancels the run (`subscriptions.cancel`). Otherwise it reads Hold,
        // or Release when the subscription is already on hold, and a click
        // puts future runs on hold or allows them again
        // (`subscriptions.hold`).
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
                .gap(px(ui::titlebar_button::BETWEEN))
                .child(primary)
                .child(
                    ui::titlebar_button(
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
                // The "More actions" button. The element from `place_of`
                // stores the button's position and size in `overflow` each
                // time it is drawn. A click opens the subscription's menu
                // at the point 4 px below the button's bottom-left corner.
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
