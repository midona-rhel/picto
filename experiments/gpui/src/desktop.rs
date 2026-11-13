//! What production's main process does for the desktop around the window:
//! the app's badge is the Inbox count (`inboxBadgeService.mjs`), and a
//! system notification says when a batch of subscription runs has finished
//! (`subscriptionNotificationService.mjs`). Both follow the library's change
//! events. Only the real app starts them, never the off-screen checks.
use crate::application::{self, Event};
use crate::session::Session;
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use std::collections::BTreeSet;
use std::time::Duration;

/// `CHANGE_DEBOUNCE_MS`: a burst of changes sets the badge once.
const BADGE_DEBOUNCE: Duration = Duration::from_millis(200);
/// `SETTLE_DEBOUNCE_MS`
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(250);
/// `MAX_BADGE`
const MAX_BADGE: u64 = 9_999;

/// Set by `main` so that the window's shell starts the desktop services.
struct Enabled;

impl Global for Enabled {}

/// Lets [`start`] run: only the app proper calls this.
pub fn enable(cx: &mut App) {
    cx.set_global(Enabled);
}

/// Starts the badge and the subscription notification for the session's
/// library, if the app enabled them.
pub fn start(session: &Entity<Session>, cx: &mut App) {
    if !cx.has_global::<Enabled>() {
        return;
    }
    let desktop = cx.new(|cx| Desktop::new(session.clone(), cx));
    cx.set_global(Running(desktop));
}

struct Running(#[allow(dead_code)] Entity<Desktop>);

impl Global for Running {}

struct Desktop {
    session: Entity<Session>,
    /// The count the badge shows.
    shown: Option<u64>,
    badge_timer: Option<Task<()>>,
    /// A badge read is under way; `true` inside when another is wanted.
    badge_reading: Option<bool>,
    /// The runs of the batch being watched, and whether one has been seen
    /// running.
    batch: BTreeSet<i64>,
    batch_active: bool,
    settle_timer: Option<Task<()>>,
    settle_reading: Option<bool>,
    _events: Subscription,
}

impl Desktop {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let events = cx.subscribe(&session, |this, _, event: &Event, cx| {
            if let Event::Changed(changed) = event {
                this.badge_changed(cx);
                if application::touches(changed, "subscriptions")
                    || application::touches(changed, "tasks")
                {
                    this.subscriptions_changed(cx);
                }
            }
        });
        let mut desktop = Self {
            session,
            shown: None,
            badge_timer: None,
            badge_reading: None,
            batch: BTreeSet::new(),
            batch_active: false,
            settle_timer: None,
            settle_reading: None,
            _events: events,
        };
        desktop.read_badge(cx);
        desktop.read_runs(cx);
        desktop
    }

    // ── The badge ───────────────────────────────────────────────

    fn badge_changed(&mut self, cx: &mut Context<Self>) {
        self.badge_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BADGE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.badge_timer = None;
                this.read_badge(cx);
            });
        }));
    }

    /// `library.inbox_count`, then the badge.
    fn read_badge(&mut self, cx: &mut Context<Self>) {
        if let Some(again) = &mut self.badge_reading {
            *again = true;
            return;
        }
        self.badge_reading = Some(false);
        let answer = self
            .session
            .read(cx)
            .dispatch("library.inbox_count", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                let again = this.badge_reading.take() == Some(true);
                match result {
                    Ok(Ok(count)) => this.show_badge(badge_count(&count)),
                    Ok(Err(error)) => eprintln!("picto: inbox badge refresh failed: {error}"),
                    Err(_) => {}
                }
                if again {
                    this.read_badge(cx);
                }
            });
        })
        .detach();
    }

    fn show_badge(&mut self, count: u64) {
        if self.shown == Some(count) {
            return;
        }
        self.shown = Some(count);
        platform::set_badge(count);
    }

    // ── Subscriptions finishing ─────────────────────────────────

    fn subscriptions_changed(&mut self, cx: &mut Context<Self>) {
        self.settle_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.settle_timer = None;
                this.read_runs(cx);
            });
        }));
    }

    /// `refreshOnce`: notes the runs going; once none is, reads what the
    /// batch added and says so.
    fn read_runs(&mut self, cx: &mut Context<Self>) {
        if let Some(again) = &mut self.settle_reading {
            *again = true;
            return;
        }
        self.settle_reading = Some(false);
        let session = self.session.clone();
        let list = session
            .read(cx)
            .dispatch("subscriptions.list", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let finished = match list.await {
                Ok(Ok(list)) => this
                    .update(cx, |this, _| this.observe(&list))
                    .ok()
                    .flatten(),
                Ok(Err(error)) => {
                    eprintln!("picto: subscription notification refresh failed: {error}");
                    None
                }
                Err(_) => None,
            };
            if let Some(runs) = finished {
                let mut activities = Vec::new();
                for run in &runs {
                    let answer = cx.update(|cx| {
                        session.read(cx).dispatch(
                            "subscriptions.runs.get",
                            serde_json::json!({ "run_id": run, "source_item_limit": 1 }),
                        )
                    });
                    activities.push(answer.await.ok().and_then(Result::ok));
                }
                let _ = this.update(cx, |this, cx| match new_items(&activities) {
                    Some(count) => {
                        this.batch.clear();
                        this.batch_active = false;
                        if notifications_allowed(cx) {
                            platform::notify("Subscriptions finished", &finished_body(count));
                        }
                    }
                    // An unavailable activity is not evidence that the run
                    // added nothing: the batch waits for a later change.
                    None => eprintln!(
                        "picto: subscription completion details are unavailable; notification is pending"
                    ),
                });
            }
            let _ = this.update(cx, |this, cx| {
                if this.settle_reading.take() == Some(true) {
                    this.read_runs(cx);
                }
            });
        })
        .detach();
    }

    /// Takes in `subscriptions.list`; the batch's runs when it has just
    /// finished.
    fn observe(&mut self, list: &serde_json::Value) -> Option<Vec<i64>> {
        let active = active_runs(list);
        self.batch.extend(active.iter().copied());
        if !active.is_empty() {
            self.batch_active = true;
            return None;
        }
        (self.batch_active && !self.batch.is_empty()).then(|| self.batch.iter().copied().collect())
    }
}

/// The run of each subscription that is pending or running.
fn active_runs(list: &serde_json::Value) -> Vec<i64> {
    list["subscriptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|subscription| {
            matches!(subscription["status"].as_str(), Some("pending" | "running"))
        })
        .filter_map(|subscription| subscription["active_run_id"].as_i64())
        .collect()
}

/// How many library items the batch's runs added, counting each once; none
/// while any run's activity could not be read.
fn new_items(activities: &[Option<serde_json::Value>]) -> Option<usize> {
    let mut keys = BTreeSet::new();
    for activity in activities {
        for key in activity.as_ref()?["new_library_item_keys"].as_array()? {
            keys.insert(key.as_str()?.to_owned());
        }
    }
    Some(keys.len())
}

/// The notification's text.
fn finished_body(count: usize) -> String {
    match count {
        0 => "All subscriptions finished. No new library items were added.".into(),
        1 => "1 new library item is ready to review.".into(),
        count => format!(
            "{} new library items are ready to review.",
            crate::grid::format::grouped(count as i128)
        ),
    }
}

/// The badge's number: the count, at most 9,999.
fn badge_count(count: &serde_json::Value) -> u64 {
    count
        .as_u64()
        .or_else(|| count.as_f64().map(|count| count.max(0.) as u64))
        .unwrap_or(0)
        .min(MAX_BADGE)
}

/// Production shows this notification whatever Settings › Notifications
/// says; here it follows the pop-up switch and the Information tone, as an
/// in-app notification of the same kind would.
fn notifications_allowed(cx: &mut App) -> bool {
    crate::ui::notify::shows(crate::ui::notify::Tone::Info, cx)
}

#[cfg(target_os = "macos")]
mod platform {
    //! The dock tile's badge and the user notification centre.
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::NSString;

    #[link(name = "UserNotifications", kind = "framework")]
    unsafe extern "C" {}

    /// `app.setBadgeCount`: the number on the dock icon, none for zero.
    pub fn set_badge(count: u64) {
        let Some(application) = AnyClass::get(c"NSApplication") else {
            return;
        };
        let label = (count > 0).then(|| NSString::from_str(&count.to_string()));
        // SAFETY: AppKit's documented selectors, sent on the main thread
        // (GPUI's foreground), with the argument types they take.
        unsafe {
            let shared: Option<Retained<AnyObject>> = msg_send![application, sharedApplication];
            let Some(shared) = shared else { return };
            let tile: Option<Retained<AnyObject>> = msg_send![&*shared, dockTile];
            let Some(tile) = tile else { return };
            let _: () = msg_send![&*tile, setBadgeLabel: label.as_deref()];
        }
    }

    /// `new Notification({ title, body }).show()`. The notification centre
    /// serves only an app with a bundle identifier; a bare binary (`cargo
    /// run`) has none, and the notification is written to standard error
    /// instead.
    pub fn notify(title: &str, body: &str) {
        let (Some(bundle), Some(center), Some(content), Some(request)) = (
            AnyClass::get(c"NSBundle"),
            AnyClass::get(c"UNUserNotificationCenter"),
            AnyClass::get(c"UNMutableNotificationContent"),
            AnyClass::get(c"UNNotificationRequest"),
        ) else {
            return;
        };
        // SAFETY: Foundation's and UserNotifications' documented selectors
        // with the argument types they take; the centre is asked for only
        // when the bundle has an identifier, without which it raises.
        unsafe {
            let main: Option<Retained<AnyObject>> = msg_send![bundle, mainBundle];
            let identifier: Option<Retained<NSString>> = match &main {
                Some(main) => msg_send![&**main, bundleIdentifier],
                None => None,
            };
            if identifier.is_none() {
                eprintln!("picto: {title}: {body} (no bundle, so no system notification)");
                return;
            }
            let center: Option<Retained<AnyObject>> = msg_send![center, currentNotificationCenter];
            let Some(center) = center else { return };
            // Alert and sound; asked once, then remembered by the system.
            let asked = block2::RcBlock::new(|_granted: Bool, _error: *mut AnyObject| {});
            let _: () = msg_send![
                &*center,
                requestAuthorizationWithOptions: 6usize,
                completionHandler: &*asked
            ];
            let content: Option<Retained<AnyObject>> = msg_send![content, new];
            let Some(content) = content else { return };
            let _: () = msg_send![&*content, setTitle: &*NSString::from_str(title)];
            let _: () = msg_send![&*content, setBody: &*NSString::from_str(body)];
            let identifier = NSString::from_str(&format!(
                "picto-subscriptions-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis())
            ));
            let request: Option<Retained<AnyObject>> = msg_send![
                request,
                requestWithIdentifier: &*identifier,
                content: &*content,
                trigger: Option::<&AnyObject>::None
            ];
            let Some(request) = request else { return };
            let done: Option<&block2::Block<dyn Fn(*mut AnyObject)>> = None;
            let _: () = msg_send![
                &*center,
                addNotificationRequest: &*request,
                withCompletionHandler: done
            ];
        }
    }
}

/// Windows would set the taskbar button's overlay icon
/// (`ITaskbarList3::SetOverlayIcon`) and show a toast through
/// `ToastNotificationManager` under the app's AppUserModelID; Linux would
/// send the Unity launcher's `com.canonical.Unity.LauncherEntry` `Update`
/// signal with the count and `org.freedesktop.Notifications.Notify` over
/// D-Bus. Neither is built: on those systems both do nothing.
#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn set_badge(_count: u64) {}

    pub fn notify(_title: &str, _body: &str) {}
}

#[cfg(test)]
mod tests {
    use super::{active_runs, badge_count, finished_body, new_items};
    use serde_json::json;

    #[test]
    fn running_and_pending_runs_make_the_batch() {
        let list = json!({ "subscriptions": [
            { "active_run_id": 4, "status": "running" },
            { "active_run_id": 5, "status": "pending" },
            { "active_run_id": 6, "status": "paused" },
            { "active_run_id": null, "status": "running" },
        ]});
        assert_eq!(active_runs(&list), vec![4, 5]);
    }

    #[test]
    fn new_items_are_counted_once_across_runs() {
        let runs = [
            Some(json!({ "new_library_item_keys": ["a", "b"] })),
            Some(json!({ "new_library_item_keys": ["b", "c"] })),
        ];
        assert_eq!(new_items(&runs), Some(3));
        // An unreadable run leaves the count unknown.
        assert_eq!(new_items(&[runs[0].clone(), None]), None);
        assert_eq!(
            new_items(&[Some(json!({ "new_library_item_keys": [1] }))]),
            None
        );
    }

    #[test]
    fn the_notification_reads_as_production_writes_it() {
        assert_eq!(
            finished_body(0),
            "All subscriptions finished. No new library items were added."
        );
        assert_eq!(finished_body(1), "1 new library item is ready to review.");
        assert_eq!(
            finished_body(1234),
            "1,234 new library items are ready to review."
        );
    }

    #[test]
    fn the_badge_is_capped() {
        assert_eq!(badge_count(&json!(12)), 12);
        assert_eq!(badge_count(&json!(120000)), 9_999);
        assert_eq!(badge_count(&json!(null)), 0);
    }
}
