//! Notifications, as production's `notifications.ts` keeps them and its
//! `NotificationHost.tsx` draws them: one at a time, a new one taking the
//! place of the one shown, each leaving after four seconds or when its close
//! button is pressed. Call [`error`], [`warning`], [`info`] or [`success`]
//! from anywhere a `cx` is, an async result's included; the main window's
//! [`Host`] draws whatever is current above everything else.
use crate::i18n::t;
use crate::theme::{self, Text as _};
use crate::{motion, ui};
use gpui::accesskit::Live;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::rc::Rc;
use std::time::Duration;

/// `NotificationTone`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Error,
    Warning,
    Info,
    Success,
}

impl Tone {
    pub const ALL: [Tone; 4] = [Tone::Error, Tone::Warning, Tone::Info, Tone::Success];

    /// The name the settings store it under.
    pub fn name(self) -> &'static str {
        match self {
            Tone::Error => "error",
            Tone::Warning => "warning",
            Tone::Info => "info",
            Tone::Success => "success",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tone| tone.name() == name)
    }

    /// `toneIcons`.
    fn icon(self) -> &'static str {
        match self {
            Tone::Error => "notify-error",
            Tone::Warning => "notify-warning",
            Tone::Info => "notify-info",
            Tone::Success => "circle-check-filled",
        }
    }

    fn color(self, theme: &theme::Theme) -> Rgba {
        match self {
            Tone::Error => rgb(theme::NOTIFICATION_ERROR),
            Tone::Warning => rgb(theme::NOTIFICATION_WARNING),
            Tone::Info => theme.primary(),
            Tone::Success => rgb(theme::NOTIFICATION_SUCCESS),
        }
    }
}

/// A notification's button (`action`): pressing it runs it and dismisses
/// the notification.
#[derive(Clone)]
pub struct Action {
    pub label: SharedString,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

/// `AppNotification`.
#[derive(Clone)]
pub struct Notification {
    pub id: u64,
    pub tone: Tone,
    pub title: SharedString,
    pub message: SharedString,
    pub action: Option<Action>,
}

impl Notification {
    /// What the line reads: the title, then the message after a dash when
    /// there is one.
    pub fn text(&self) -> SharedString {
        line(&self.title, &self.message)
    }
}

fn line(title: &str, message: &str) -> SharedString {
    if message.is_empty() {
        title.to_owned().into()
    } else {
        format!("{title} — {message}").into()
    }
}

/// `ShowNotificationOptions`: what to show, for [`show`].
pub struct Options {
    title: SharedString,
    message: SharedString,
    duration: Duration,
    action: Option<Action>,
}

impl Options {
    pub fn new(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            duration: motion::NOTIFICATION_DURATION,
            action: None,
        }
    }

    /// A button after the text.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(Action {
            label: label.into(),
            run: Rc::new(run),
        });
        self
    }
}

/// `NotificationPopupPreferences`: whether pop-ups show at all, and which
/// tones do (Settings › Notifications).
#[derive(Clone, Debug, PartialEq)]
pub struct Popups {
    pub enabled: bool,
    pub tones: Vec<Tone>,
}

impl Default for Popups {
    fn default() -> Self {
        Self {
            enabled: true,
            tones: Tone::ALL.to_vec(),
        }
    }
}

impl Popups {
    fn shows(&self, tone: Tone) -> bool {
        self.enabled && self.tones.contains(&tone)
    }
}

/// The current notification and the pop-up preferences.
pub struct Notifications {
    current: Option<Notification>,
    next_id: u64,
    popups: Popups,
    /// Dismisses the current notification once its time is up.
    expiry: Option<Task<()>>,
}

impl Notifications {
    fn show(&mut self, tone: Tone, options: Options, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let duration = options.duration;
        self.current = Some(Notification {
            id,
            tone,
            title: options.title,
            message: options.message,
            action: options.action,
        });
        // Its time starts now, whether or not the pop-up is shown.
        self.expiry = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| this.dismiss(id, cx));
        }));
        cx.notify();
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.id == id)
        {
            self.current = None;
            self.expiry = None;
            cx.notify();
        }
    }

    /// The current notification, if the preferences let it show.
    fn visible(&self) -> Option<&Notification> {
        self.current
            .as_ref()
            .filter(|current| self.popups.shows(current.tone))
    }
}

struct Global(Entity<Notifications>);

impl gpui::Global for Global {}

/// The app's notifications, made the first time they are needed.
fn notifications(cx: &mut App) -> Entity<Notifications> {
    if let Some(global) = cx.try_global::<Global>() {
        return global.0.clone();
    }
    let entity = cx.new(|_| Notifications {
        current: None,
        next_id: 1,
        popups: Popups::default(),
        expiry: None,
    });
    cx.set_global(Global(entity.clone()));
    entity
}

/// `showNotification`: shows a notification in place of the current one.
pub fn show(tone: Tone, options: Options, cx: &mut App) {
    notifications(cx).update(cx, |this, cx| this.show(tone, options, cx));
}

/// The text production's `showErrorNotification` shows instead of the
/// library's own when a folder would be nested too deep, and as a warning.
pub const FOLDER_DEPTH_LIMIT: &str = "folders may be nested at most 8 levels deep";

/// What an error shows as: a folder nested too deep is a warning with
/// production's own words.
fn error_options(title: SharedString, message: SharedString) -> (Tone, Options) {
    if message.contains(FOLDER_DEPTH_LIMIT) {
        return (
            Tone::Warning,
            Options::new(
                t("Folder depth limit"),
                crate::i18n::en(
                    "Folders can be nested up to 8 levels. Choose a higher destination or flatten the folder structure.",
                ),
            ),
        );
    }
    (Tone::Error, Options::new(title, message))
}

/// `showErrorNotification`.
pub fn error(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    let (tone, options) = error_options(title.into(), message.into());
    show(tone, options, cx);
}

/// `showWarningNotification`.
pub fn warning(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    show(Tone::Warning, Options::new(title, message), cx);
}

/// `showInfoNotification`.
pub fn info(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    show(Tone::Info, Options::new(title, message), cx);
}

/// `showSuccessNotification`.
pub fn success(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    show(Tone::Success, Options::new(title, message), cx);
}

/// `dismissNotification`: closes the notification `id` if it is the one
/// shown.
pub fn dismiss(id: u64, cx: &mut App) {
    notifications(cx).update(cx, |this, cx| this.dismiss(id, cx));
}

/// `configureNotificationPopups`: which tones pop up. Names that are not a
/// tone are ignored.
pub fn configure<'a>(enabled: bool, tones: impl IntoIterator<Item = &'a str>, cx: &mut App) {
    let popups = Popups {
        enabled,
        tones: tones.into_iter().filter_map(Tone::from_name).collect(),
    };
    notifications(cx).update(cx, |this, cx| {
        if this.popups != popups {
            this.popups = popups;
            cx.notify();
        }
    });
}

/// Whether Settings › Notifications lets a notification of `tone` pop up.
pub fn shows(tone: Tone, cx: &mut App) -> bool {
    notifications(cx).read(cx).popups.shows(tone)
}

/// Where a notification is drawn when it has got `shown` of the way in (1
/// in place, 0 gone, eased): how far above its place it is, and its
/// opacity. The curve overshoots, so a leaving notification rises a little
/// past 48 points before it is removed.
pub fn frame(shown: f32) -> (f32, f32) {
    (
        motion::NOTIFICATION_RISE * (1. - shown),
        shown.clamp(0., 1.),
    )
}

/// `NotificationHost`: draws the current notification centred under the
/// titlebar, above everything else in the window (`--layer-notification`).
/// It appears at once, and rises and fades out over 400 ms when it is
/// dismissed or its time is up.
pub struct Host {
    notifications: Entity<Notifications>,
    /// The notification on screen, which stays while it leaves.
    displayed: Option<Notification>,
    shown: motion::Fade,
    /// Removes the notification once it has left.
    removal: Option<Task<()>>,
    _observe: Subscription,
}

/// The layer notifications are drawn on, above every other deferred layer
/// (menus, pickers, tooltips, dialogs).
const LAYER: usize = super::layer::NOTIFICATION;

impl Host {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let notifications = notifications(cx);
        let observe = cx.observe(&notifications, |this, _, cx| this.follow(cx));
        let mut host = Self {
            notifications,
            displayed: None,
            shown: motion::Fade::at(false),
            removal: None,
            _observe: observe,
        };
        host.follow(cx);
        host
    }

    /// Follows the current notification, as the host's effects do.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let visible = self.notifications.read(cx).visible().cloned();
        match visible {
            Some(notification) => {
                // A notification is drawn in place at once, a first one and
                // one that arrives while the last is leaving alike, as
                // production's host shows them.
                self.removal = None;
                self.shown = motion::Fade::at(true);
                self.displayed = Some(notification);
            }
            None => {
                if self.displayed.is_none() {
                    return;
                }
                self.shown.set(false, motion::NOTIFICATION_EXIT);
                self.removal = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(motion::NOTIFICATION_EXIT)
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.displayed = None;
                        this.removal = None;
                        cx.notify();
                    });
                }));
            }
        }
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `NotificationHost`'s layer is a live region, there whether or not
        // a notification shows, so assistive technology announces each one
        // as it arrives: at once for an error or warning, politely
        // otherwise.
        let region = |urgent: bool| {
            div()
                .id("notification-region")
                .role(Role::Group)
                .aria_live(if urgent {
                    Live::Assertive
                } else {
                    Live::Polite
                })
        };
        let Some(notification) = self.displayed.clone() else {
            return region(false).into_any_element();
        };
        let urgent = matches!(notification.tone, Tone::Error | Tone::Warning);
        if self.shown.animating() {
            window.request_animation_frame();
        }
        let theme = theme::theme(cx);
        let (rise, opacity) = frame(self.shown.value_with(motion::NOTIFICATION_EASE));
        // `max-width: min(640px, calc(100vw - 24px))`.
        let width = window.viewport_size().width;
        let max_width = px(640.).min(width - px(24.));
        let id = notification.id;
        let leaving = !self.shown.on();
        let action = notification.action.clone().map(|action| {
            div()
                .id("notification-action")
                .role(Role::Button)
                .aria_label(action.label.clone())
                .flex_none()
                .ml(px(12.))
                .underline()
                .cursor_pointer()
                .child(action.label.clone())
                .on_click(move |_, window, cx| {
                    (action.run)(window, cx);
                    dismiss(id, cx);
                })
        });
        let close = div()
            .id("notification-close")
            .role(Role::Button)
            .aria_label(crate::i18n::t("Dismiss notification"))
            .aria_disabled(leaving)
            .relative()
            .flex_none()
            .size(px(24.))
            .ml(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .opacity(0.7)
            .when(!leaving, |close| {
                close
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.hover_overlay()).opacity(1.))
                    .on_click(move |_, _, cx| dismiss(id, cx))
            })
            // `.close::before`: the 16px rule 12px before the button.
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(px(-13.))
                    .w(px(1.))
                    .h(px(16.))
                    .bg(theme.border_secondary()),
            )
            .child(ui::icon("notify-close", 14., theme.text()));
        let panel = ui::glass_panel("notification", theme::RADIUS_LG, cx)
            .role(if urgent { Role::Alert } else { Role::Status })
            .aria_label(notification.text())
            .flex()
            .items_center()
            .min_w(px(100.))
            .max_w(max_width)
            .h(px(theme::NOTIFICATION_HEIGHT))
            // `padding: 0 8px` inside the 1px glass edge, which CSS lays
            // out as a border and GPUI draws over the contents.
            .px(px(9.))
            .overflow_hidden()
            .opacity(opacity)
            // `pointer-events: none` while it leaves.
            .when(!leaving, |panel| panel.occlude())
            .child(
                ui::icon(
                    notification.tone.icon(),
                    24.,
                    notification.tone.color(&theme),
                )
                .flex_none()
                .mr(px(4.)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .font_family(theme::FONT_UI)
                    .text(theme::TEXT_MD)
                    .line_height(px(theme::NOTIFICATION_HEIGHT))
                    .text_color(theme.text())
                    .child(div().min_w_0().truncate().child(notification.text()))
                    .children(action),
            )
            .child(close);
        deferred(
            region(urgent)
                .absolute()
                .top(px(theme::NOTIFICATION_TOP - rise))
                .left_0()
                .w(width)
                .flex()
                .justify_center()
                .child(panel),
        )
        .with_priority(LAYER)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Popups, Tone, error, error_options, frame, info, line, notifications};
    use std::time::Duration;

    fn current(cx: &mut gpui::TestAppContext) -> Option<String> {
        cx.update(|cx| {
            notifications(cx)
                .read(cx)
                .current
                .as_ref()
                .map(|current| current.text().to_string())
        })
    }

    #[test]
    fn a_notification_replaces_the_last_and_leaves_after_four_seconds() {
        let mut cx = gpui::TestAppContext::single();
        cx.update(|cx| error("First", "Old failure", cx));
        cx.executor().advance_clock(Duration::from_millis(2000));
        cx.update(|cx| info("Second", "New failure", cx));
        cx.run_until_parked();
        assert_eq!(current(&mut cx).as_deref(), Some("Second — New failure"));
        // The first one's time no longer counts.
        cx.executor().advance_clock(Duration::from_millis(3999));
        cx.run_until_parked();
        assert_eq!(current(&mut cx).as_deref(), Some("Second — New failure"));
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(current(&mut cx), None);
    }

    #[test]
    fn the_line_puts_the_message_after_a_dash() {
        assert_eq!(
            line("Second", "New failure").as_ref(),
            "Second — New failure"
        );
        assert_eq!(line("Undone", "").as_ref(), "Undone");
    }

    #[test]
    fn a_folder_nested_too_deep_is_a_warning_in_production_words() {
        let (tone, options) = error_options(
            "Could not move folder".into(),
            "folders may be nested at most 8 levels deep".into(),
        );
        assert_eq!(tone, Tone::Warning);
        assert_eq!(options.title.as_ref(), "Folder depth limit");
        assert!(
            options
                .message
                .starts_with("Folders can be nested up to 8 levels.")
        );
        let (tone, options) = error_options("Could not move folder".into(), "busy".into());
        assert_eq!(tone, Tone::Error);
        assert_eq!(options.title.as_ref(), "Could not move folder");
    }

    #[test]
    fn disabled_pop_ups_and_tones_are_not_shown() {
        let popups = Popups {
            enabled: true,
            tones: vec![Tone::Error],
        };
        assert!(popups.shows(Tone::Error));
        assert!(!popups.shows(Tone::Info));
        let off = Popups {
            enabled: false,
            ..Popups::default()
        };
        assert!(!off.shows(Tone::Error));
        assert!(Popups::default().shows(Tone::Success));
    }

    #[test]
    fn a_leaving_notification_rises_48_points_and_fades() {
        assert_eq!(frame(1.), (0., 1.));
        assert_eq!(frame(0.), (48., 0.));
        // The curve's overshoot carries it past its end, not below zero.
        let (rise, opacity) = frame(-0.03);
        assert!(rise > 48.);
        assert_eq!(opacity, 0.);
    }

    #[test]
    fn tones_are_read_by_their_settings_names() {
        for tone in Tone::ALL {
            assert_eq!(Tone::from_name(tone.name()), Some(tone));
        }
        assert_eq!(Tone::from_name("loud"), None);
    }
}
