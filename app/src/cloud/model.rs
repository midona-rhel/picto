//! The open library's sync state: its configuration (`cloud.configuration.get`)
//! and status (`cloud.status.get`), read when the library opens, when the
//! core says `cloud` or `tasks` changed, and on a timer as production's
//! `CloudSyncBar` polls: every 250 ms while a sync runs (its progress is
//! written without publishing), every 15 s while sync is configured, not at
//! all while it is off. Changes of state are announced as `cloudSettle`
//! announces them.
use super::{Configuration, Status};
use crate::application::{self, Event};
use crate::i18n::{en, t};
use crate::session::Session;
use gpui::*;
use std::time::{Duration, Instant};

/// While a sync runs, and while one is configured.
const FAST: Duration = Duration::from_millis(250);
const SLOW: Duration = Duration::from_secs(15);

pub struct Cloud {
    session: Entity<Session>,
    pub configuration: Option<Configuration>,
    pub status: Option<Status>,
    /// When the current sync was first seen running, for the "update
    /// complete" announcement (`reconnectStartedAt`).
    started: Option<Instant>,
    /// A read under way, and whether another was asked for meanwhile.
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

    /// Whether the library syncs to a provider's folder.
    pub fn configured(&self) -> bool {
        self.configuration
            .as_ref()
            .is_some_and(|configuration| configuration.root_path.is_some())
    }

    /// Reads the configuration and status again.
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

    /// Takes a status in, announcing what changed as `cloudSettle` does.
    fn take(&mut self, next: Status, cx: &mut Context<Self>) {
        if let Some(before) = self.status.take() {
            for popup in settle(&before, &next, &mut self.started, Instant::now()) {
                popup.show(cx);
            }
        }
        self.status = Some(next);
        self.schedule(cx);
    }

    /// Reads again after `FAST` while a sync runs, after `SLOW` while sync
    /// is on, and not at all while it is off.
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

    /// Sends a cloud command to the library's application, reads the state
    /// again once it answered, and hands `done` the answer.
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

/// What `cloudSettle` announces.
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    /// Changes from the cloud are being applied.
    Found,
    /// Sync failed, with the core's words.
    Attention(String),
    /// A sync that took a while finished.
    Complete,
}

impl Popup {
    fn show(self, cx: &mut App) {
        use crate::ui::notify::{self, Options, Tone};
        match self {
            Popup::Found => notify::show(
                Tone::Info,
                // Production writes the message without a key.
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

/// View progress / View details: the log window (`picto:open-diagnostics`).
fn view_progress(cx: &mut App) {
    crate::app_menu::bring_forward(cx);
    crate::libraries::host::update_shell(cx, |shell, window, cx| {
        shell.show_diagnostics(window, cx)
    });
}

/// `cloudSettle`'s rules: a sync that starts by itself (not one restoring
/// a joined library's media, which shows its own dialog) is announced, an
/// error is announced once per message, and a sync that ran at least five
/// seconds says when it is done.
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
