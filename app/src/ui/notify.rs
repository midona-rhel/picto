//! Keeps and draws the app's notifications. One notification is shown at a
//! time, and a new one replaces the one that is shown. A notification is
//! removed after four seconds, or when its close button is pressed. Call
//! [`error`], [`warning`], [`info`] or [`success`] from any place that has a
//! `cx`, including the handling of an async task's result. The main window's
//! [`Host`] draws the current notification above everything else.
use crate::i18n::t;
use crate::theme::{self, Text as _};
use crate::{motion, ui};
use gpui::accesskit::Live;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::rc::Rc;
use std::time::Duration;

/// The kind of a notification. It sets the notification's icon and the icon's
/// colour, and whether assistive technology announces it at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Error,
    Warning,
    Info,
    Success,
}

impl Tone {
    pub const ALL: [Tone; 4] = [Tone::Error, Tone::Warning, Tone::Info, Tone::Success];

    /// Returns the name under which the settings store the tone.
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

    /// Returns the name of the tone's icon.
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

/// A notification's button. Pressing the button calls `run` and dismisses the
/// notification.
#[derive(Clone)]
pub struct Action {
    pub label: SharedString,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

/// One notification: its tone, its title, its message and, when it has one,
/// its button.
#[derive(Clone)]
pub struct Notification {
    pub id: u64,
    pub tone: Tone,
    pub title: SharedString,
    pub message: SharedString,
    pub action: Option<Action>,
}

impl Notification {
    /// Returns the text of the notification's line: the title and, when there
    /// is a message, a dash and the message.
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

/// What a notification shows and for how long. It is passed to [`show`].
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

    /// Sets how long the notification stays, in place of the usual four
    /// seconds.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Adds a button with the label `label` after the text. Pressing it calls
    /// `run`.
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

/// The user's choices in Settings › Notifications: whether notifications are
/// shown at all, and which tones are shown.
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

/// Holds the current notification and the user's choices in Settings ›
/// Notifications.
pub struct Notifications {
    current: Option<Notification>,
    next_id: u64,
    popups: Popups,
    /// The task that dismisses the current notification when its time is up.
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
        // The notification's time starts now, whether or not the settings let
        // it be shown.
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

    /// Returns the current notification if the settings let it be shown.
    fn visible(&self) -> Option<&Notification> {
        self.current
            .as_ref()
            .filter(|current| self.popups.shows(current.tone))
    }
}

struct Global(Entity<Notifications>);

impl gpui::Global for Global {}

/// Returns the app's notifications. They are created the first time this is
/// called.
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

/// Shows a notification in place of the current one.
pub fn show(tone: Tone, options: Options, cx: &mut App) {
    notifications(cx).update(cx, |this, cx| this.show(tone, options, cx));
}

/// The words in the library's error message when a folder would be nested too
/// deep. [`error`] shows an error with these words as a warning that has its
/// own title and text.
pub const FOLDER_DEPTH_LIMIT: &str = "folders may be nested at most 8 levels deep";

/// Returns the tone and the contents that an error is shown with. An error
/// about a folder that would be nested too deep is shown as a warning that
/// explains the limit. Every other error is shown with its own title and
/// message.
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

/// Shows an error with the title `title` and the message `message`.
pub fn error(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    let (tone, options) = error_options(title.into(), message.into());
    show(tone, options, cx);
}

/// Shows a warning with the title `title` and the message `message`.
pub fn warning(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    show(Tone::Warning, Options::new(title, message), cx);
}

/// Shows a piece of information with the title `title` and the message
/// `message`.
pub fn info(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    show(Tone::Info, Options::new(title, message), cx);
}

/// Shows a notification of success with the title `title` and the message
/// `message`.
pub fn success(title: impl Into<SharedString>, message: impl Into<SharedString>, cx: &mut App) {
    show(Tone::Success, Options::new(title, message), cx);
}

/// Closes the notification `id` if it is the one that is shown.
pub fn dismiss(id: u64, cx: &mut App) {
    notifications(cx).update(cx, |this, cx| this.dismiss(id, cx));
}

/// Sets the user's choices from Settings › Notifications: whether
/// notifications are shown, and which tones are shown. A name in `tones` that
/// is not a tone's name is ignored.
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

/// Returns where a notification is drawn when it is `shown` of the way in (1
/// is in place, 0 is gone; the caller has applied the easing curve): how many
/// points above its place it is, and its opacity. The curve overshoots, so a
/// notification that is being removed rises a little past 48 points before it
/// is removed.
pub fn frame(shown: f32) -> (f32, f32) {
    (
        motion::NOTIFICATION_RISE * (1. - shown),
        shown.clamp(0., 1.),
    )
}

/// Draws the current notification centred under the titlebar, above everything
/// else in the window (`layer::NOTIFICATION`). A notification appears at once.
/// When it is dismissed or its time is up, it rises and fades out over 400 ms.
pub struct Host {
    notifications: Entity<Notifications>,
    /// The notification that is drawn. It stays here while it fades out.
    displayed: Option<Notification>,
    shown: motion::Fade,
    /// The task that removes the notification when its fade-out has ended.
    removal: Option<Task<()>>,
    _observe: Subscription,
}

/// The drawing order of notifications, which is above every other deferred
/// layer (menus, pickers, tooltips, dialogs).
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

    /// Makes `displayed` match the current notification. A new notification is
    /// shown at once. When there is no current notification any more, the
    /// fade-out of the displayed one starts.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let visible = self.notifications.read(cx).visible().cloned();
        match visible {
            Some(notification) => {
                // A notification is drawn in place at once. This holds for the
                // first one, and for one that arrives while the last one is
                // fading out.
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
        // The region is a live region, and it is there whether or not a
        // notification is shown, so assistive technology announces each
        // notification as it arrives. An error or a warning is announced at
        // once. The other tones are announced when the assistive technology
        // has finished what it is saying.
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
        // The notification is at most 640px wide, and at most the window's
        // width less 24px.
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
            // A line 1px wide and 16px high, 12px to the left of the close
            // button.
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
            // The contents have 8px of padding on each side inside the 1px
            // glass edge. GPUI draws the edge over the contents, so the
            // padding here is 9px.
            .px(px(9.))
            .overflow_hidden()
            .opacity(opacity)
            // A notification that is fading out takes no clicks.
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
        // The time that had passed for the first notification does not count
        // for the second.
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
    fn a_folder_nested_too_deep_is_a_warning_in_the_earlier_versions_words() {
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
        // The curve's overshoot makes `shown` negative near the end. The
        // notification then rises past 48 points, and its opacity stays at
        // zero.
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
