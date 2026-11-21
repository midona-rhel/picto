//! The sync state of the open library: its configuration
//! (`cloud.configuration.get`) and its status (`cloud.status.get`). Both are
//! read when the library opens, when the core reports that `cloud` or `tasks`
//! changed, and on a timer. The timer fires every 250 ms while a sync runs,
//! because the core writes a sync's progress without reporting a change. It
//! fires every 15 s while sync is configured, and there is no timer while sync
//! is off. When the state changes, a notification is shown ([`settle`]).
use super::{Configuration, Status};
use crate::application::{self, Event};
use crate::i18n::{en, t};
use crate::session::Session;
use gpui::*;
use std::time::{Duration, Instant};

/// The time between two reads while a sync runs. `SLOW` is the time between
/// two reads while sync is configured and no sync runs.
const FAST: Duration = Duration::from_millis(250);
const SLOW: Duration = Duration::from_secs(15);

pub struct Cloud {
    session: Entity<Session>,
    pub configuration: Option<Configuration>,
    pub status: Option<Status>,
    /// The time at which the current sync was first seen running. The "Cloud
    /// update complete" notification is shown only for a sync that ran for at
    /// least five seconds.
    started: Option<Instant>,
    /// Whether a read is under way. `again` says whether another read was
    /// asked for meanwhile.
    reading: bool,
    again: bool,
    poll: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Cloud {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&session, |this, _, event: &Event, cx| {
            if let Event::Changed(changed) = event
                && (application::touches(changed, "cloud") || application::touches(changed, "tasks"))
            {
                this.refresh(cx);
            }
        })];
        let mut this = Self {
            session,
            configuration: None,
            status: None,
            started: None,
            reading: false,
            again: false,
            poll: None,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    /// Returns whether the library syncs to a provider's folder.
    pub fn configured(&self) -> bool {
        self.configuration
            .as_ref()
            .is_some_and(|configuration| configuration.root_path.is_some())
    }

    /// Reads the configuration and the status again. If a read is already
    /// under way, one more read follows it.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.reading {
            self.again = true;
            return;
        }
        self.reading = true;
        let session = self.session.read(cx);
        let configuration = session.dispatch("cloud.configuration.get", serde_json::json!({}));
        let status = session.dispatch("cloud.status.get", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let configuration = configuration.await.ok().and_then(Result::ok);
            let status = status.await.ok().and_then(Result::ok);
            let _ = this.update(cx, |this, cx| {
                this.reading = false;
                if let Some(configuration) =
                    configuration.and_then(|value| serde_json::from_value(value).ok())
                {
                    this.configuration = Some(configuration);
                }
                match status.and_then(|value| serde_json::from_value::<Status>(value).ok()) {
                    Some(status) => this.take(status, cx),
                    None => this.schedule(cx),
                }
                cx.notify();
                if std::mem::take(&mut this.again) {
                    this.refresh(cx);
                }
            });
        })
        .detach();
    }

    /// Stores a new status, and shows the notifications for what changed since
    /// the previous status ([`settle`]).
    fn take(&mut self, next: Status, cx: &mut Context<Self>) {
        if let Some(before) = self.status.take() {
            for popup in settle(&before, &next, &mut self.started, Instant::now()) {
                popup.show(cx);
            }
        }
        self.status = Some(next);
        self.schedule(cx);
    }

    /// Starts the timer for the next read: `FAST` while a sync runs, `SLOW`
    /// while sync is on. While sync is off, no timer is started.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let delay = match self.status.as_ref().map(|status| status.state.as_str()) {
            Some("reconciling") => Some(FAST),
            Some("disabled") | None => None,
            Some(_) => Some(SLOW),
        };
        self.poll = delay.map(|delay| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |this, cx| this.refresh(cx));
            })
        });
    }

    /// Sends a cloud command to the library's application. When the answer has
    /// arrived, it reads the state again and calls `done` with the answer.
    pub fn run(
        &mut self,
        command: &str,
        args: serde_json::Value,
        cx: &mut Context<Self>,
        done: impl FnOnce(Result<serde_json::Value, String>, &mut App) + 'static,
    ) {
        let answer = self.session.read(cx).call(command.to_owned(), args);
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| this.refresh(cx));
            cx.update(|cx| done(result, cx));
        })
        .detach();
    }
}

/// A notification about a change of the sync's state.
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    /// Changes from the cloud are being applied.
    Found,
    /// The sync failed. The text is the core's error message.
    Attention(String),
    /// A sync that ran for at least five seconds has finished.
    Complete,
}

impl Popup {
    fn show(self, cx: &mut App) {
        use crate::ui::notify::{self, Options, Tone};
        match self {
            Popup::Found => notify::show(
                Tone::Info,
                // The locale files have no key for the message, so it is shown
                // in English in every language.
                Options::new(t("Cloud changes found"), en("Picto is updating your library."))
                    .action(t("View progress"), |_, cx| view_progress(cx)),
                cx,
            ),
            Popup::Attention(message) => notify::show(
                Tone::Error,
                Options::new(t("Cloud sync needs attention"), message)
                    .action(t("View details"), |_, cx| view_progress(cx)),
                cx,
            ),
            Popup::Complete => notify::show(
                Tone::Success,
                Options::new(t("Cloud update complete"), en("Your library is up to date.")),
                cx,
            ),
        }
    }
}

/// Brings the app's window to the front and shows the diagnostics
/// (`Shell::show_diagnostics`). The View progress and View details buttons of
/// the notifications call it.
fn view_progress(cx: &mut App) {
    crate::app_menu::bring_forward(cx);
    crate::libraries::host::update_shell(cx, |shell, window, cx| {
        shell.show_diagnostics(window, cx)
    });
}

/// Returns the notifications for the change from `before` to `next`. A sync
/// that starts is announced, unless it is a blocking sync: that one restores
/// the media of a joined library and has its own dialog. An error is announced
/// when the state becomes `error` or the error message changes. A sync that
/// ran for at least five seconds is announced when it has finished.
pub fn settle(
    before: &Status,
    next: &Status,
    started: &mut Option<Instant>,
    now: Instant,
) -> Vec<Popup> {
    if next.state == "reconciling" && !next.blocking && before.state != "reconciling" {
        *started = Some(now);
        return vec![Popup::Found];
    }
    if next.state == "error" && (before.state != "error" || before.message != next.message) {
        return vec![Popup::Attention(next.message.clone())];
    }
    if before.state == "reconciling" && next.state == "idle" {
        let elapsed = started.take().map(|since| now.duration_since(since));
        if elapsed.is_some_and(|elapsed| elapsed >= Duration::from_secs(5)) {
            return vec![Popup::Complete];
        }
    }
    Vec::new()
}
