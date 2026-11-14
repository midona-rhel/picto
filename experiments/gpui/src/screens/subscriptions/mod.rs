//! The Subscriptions screen (`SubscriptionsScreen.tsx`): a grid of the
//! library's subscriptions, and one subscription's workspace when a card is
//! opened. Opening a subscription is a place of the screen, so Back returns
//! to the grid and the scope title names it.
pub mod cover_dialog;
mod data;
mod detail;
mod dialogs;
mod format;
mod home;
mod requests;

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::inspector::{FolderPicker, TagPicker, folder_picker, tag_picker};
use crate::session::{Change, Session};
use crate::ui;
use crate::ui::menu::{self, Menu};
use crate::ui::modal;
use crate::ui::text_input::{Kind, Submitted, TextInput};
use gpui::*;
use requests::{Outbox, Request};
use serde_json::Value;
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::{Duration, Instant},
};

actions!(subscriptions, [Open, Deselect]);

/// The screen's own keys, over the browser's: Enter opens the selected
/// subscription and Escape clears the selection. Like the registry's keys,
/// they give way to a text field, a menu, a popover or a dialog.
pub fn bind(cx: &mut App) {
    let context = Some("Subscriptions && !TextInput && !Menu && !Modal && !Popover");
    cx.bind_keys([
        KeyBinding::new("enter", Open, context),
        KeyBinding::new("escape", Deselect, context),
    ]);
}

/// `subscriptionsSettle.ts`: a change to the subscriptions is read after
/// 250 ms of quiet, and progress for 5 s after a run is started even before
/// it shows as running.
const INVALIDATION_DEBOUNCE: Duration = Duration::from_millis(250);
const RUN_GRACE: Duration = Duration::from_secs(5);

/// What a run or hold control asks the application to do
/// (`subscriptionsController`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Run Now.
    Run,
    /// Pause the run in progress.
    Pause,
    /// Resume a paused run, or retry a failed one.
    Resume,
    /// Stop the run, keeping the posts already added.
    Stop,
    /// Put future runs on hold, or release them.
    Hold(bool),
    /// When the subscription runs by itself: `manual`, `daily`, `weekly` or
    /// `monthly`.
    Schedule(&'static str),
    /// How many posts one run reads at most.
    PostsPerRun(i64),
}

impl Command {
    /// The run control: Pause while running, Resume or Retry while a run
    /// waits, else Run Now.
    pub fn primary(active: bool, running: bool) -> Self {
        match (active, running) {
            (true, true) => Command::Pause,
            (true, false) => Command::Resume,
            (false, _) => Command::Run,
        }
    }

    /// The hold control: Stop while a run is active, else hold or release.
    pub fn hold(active: bool, held: bool) -> Self {
        if active {
            Command::Stop
        } else {
            Command::Hold(!held)
        }
    }

    /// The command name and arguments of `core/src/ipc.rs`.
    pub fn request(self, subscription_id: i64) -> Request {
        let id = serde_json::json!({ "subscription_id": subscription_id });
        let (name, args) = match self {
            Command::Run => ("subscriptions.run", id),
            Command::Pause => ("subscriptions.run.pause", id),
            Command::Resume => ("subscriptions.run.resume", id),
            Command::Stop => ("subscriptions.cancel", id),
            Command::Hold(paused) => (
                "subscriptions.hold",
                serde_json::json!({ "subscription_id": subscription_id, "paused": paused }),
            ),
            Command::Schedule(schedule) => (
                "subscriptions.schedule",
                serde_json::json!({ "subscription_id": subscription_id, "schedule": schedule }),
            ),
            Command::PostsPerRun(posts_per_run) => (
                "subscriptions.posts_per_run",
                serde_json::json!({
                    "subscription_id": subscription_id,
                    "posts_per_run": posts_per_run,
                }),
            ),
        };
        Request { name, args }
    }

    /// Whether it may start a run, after which progress is followed at once.
    fn starts(self) -> bool {
        matches!(self, Command::Run | Command::Resume)
    }
}

/// `SCHEDULES`: the stored value and its label.
const SCHEDULES: [(&str, &str); 4] = [
    ("manual", "Manual"),
    ("daily", "Daily"),
    ("weekly", "Weekly"),
    ("monthly", "Monthly"),
];

/// A schedule's stored value, as the menu hands it back.
fn schedule_named(value: &str) -> Option<&'static str> {
    SCHEDULES
        .iter()
        .map(|(schedule, _)| *schedule)
        .find(|schedule| *schedule == value)
}

/// The Schedule submenu: the current schedule ticked and not offered again.
fn schedule_menu(current: Option<&str>) -> menu::Entry {
    menu::submenu(
        "Schedule",
        SCHEDULES
            .iter()
            .map(|(value, label)| {
                let chosen = current == Some(*value);
                let item = menu::item("schedule", *label)
                    .value(*value)
                    .disabled(chosen);
                if chosen {
                    item.icon("menu-check")
                } else {
                    item
                }
                .into()
            })
            .collect(),
    )
    .icon("menu-clock")
    .into()
}

/// How a notification reads (`showErrorNotification`,
/// `showSuccessNotification`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Error,
    Success,
}

/// Shows a notification as production's toasts do, through the window's
/// notification host.
pub fn show_notification(tone: Tone, title: &str, message: &str, cx: &mut App) {
    let (title, message) = (title.to_owned(), message.to_owned());
    match tone {
        Tone::Error => crate::ui::notify::error(title, message, cx),
        Tone::Success => crate::ui::notify::success(title, message, cx),
    }
}

/// The tabs of a subscription's data plane.
const TABS: [&str; 3] = ["Sources", "History", "Problems"];
const SOURCES: usize = 0;
const HISTORY: usize = 1;

/// A gallery download's run as `subscriptions.progress.get` reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GalleryProgress {
    run: i64,
    downloaded: i64,
    /// The gallery's size once the site has reported it.
    total: Option<i64>,
}

/// Which destination list an open picker edits.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Picking {
    Folders(i64),
    Tags(i64),
}

pub struct SubscriptionsScreen {
    session: Entity<Session>,
    /// Covers decode into a cache of their own, so scrolling the grid never
    /// evicts an image this screen still shows.
    covers: Entity<Thumbnails>,
    focus: FocusHandle,
    /// Every subscription, once the first read has arrived. A newer read
    /// replaces it only when it arrives, so nothing blanks in between.
    list: Option<Vec<data::Subscription>>,
    generation: u64,
    /// Selected cards, by subscription.
    selected: HashSet<i64>,
    /// The card a shift-click selects from, by position in the grid.
    anchor: Option<usize>,
    /// The open subscription's History and Problems, and which one they are
    /// for; another subscription's stay until the new ones arrive.
    detail: Option<data::Detail>,
    detail_generation: u64,
    loading_more: bool,
    tab: usize,
    /// The open context menu, which stays while it fades out.
    menu: menu::Host,
    /// The subscriptions the open menu acts on.
    menu_targets: Vec<i64>,
    /// The open dialog: new subscription, gallery, rename, a source's edit
    /// or details, a confirmation, or the cover.
    dialog: modal::Host<dialogs::Dialog>,
    /// The pickers that add folders and tags to a subscription's
    /// destination, and which list the open one edits.
    tag_picker: Entity<TagPicker>,
    folder_picker: Entity<FolderPicker>,
    picking: Option<Picking>,
    /// The add-query bar's source and query.
    add_site: &'static str,
    add_query: Entity<TextInput>,
    /// The sites' saved logins and their last checks, for the chips of
    /// sources that cannot run without one.
    logins: Vec<format::Login>,
    login_health: Vec<format::Login>,
    /// Gallery downloads' progress, and the galleries being cleaned up.
    gallery_progress: HashMap<i64, GalleryProgress>,
    settling: HashSet<i64>,
    /// A subscription just created, opened once the list has it.
    pending_open: Option<i64>,
    /// Whether the screen showed at the last navigation, to read the list
    /// again each time it is opened.
    shown: bool,
    /// The place showing at the last navigation.
    place_seen: Option<i64>,
    /// The grid's content width and the data plane's width, which the
    /// container-relative CSS sizes are measured against.
    home_width: Rc<Cell<f32>>,
    plane_width: Rc<Cell<f32>>,
    /// Where the rail's "More actions" button is, to open its menu below it,
    /// and the destination's add buttons, to open the pickers above them.
    overflow: Rc<Cell<Bounds<Pixels>>>,
    add_folder: Rc<Cell<Bounds<Pixels>>>,
    add_tag: Rc<Cell<Bounds<Pixels>>>,
    home_scroll: ScrollHandle,
    rail_scroll: ScrollHandle,
    table_scroll: ScrollHandle,
    /// Subscriptions with a command on its way; their controls wait.
    acting: HashSet<i64>,
    /// Edits on their way (`busyKey`): rows, dialogs and the add bar wait.
    busy: usize,
    /// Where commands go: the application, or a log when runs are held.
    outbox: Outbox,
    /// Until when progress is read although nothing shows as running yet.
    grace_until: Option<Instant>,
    /// The next progress read while a run is going.
    poll: Option<Task<()>>,
    /// The read after a burst of changes has settled.
    invalidation: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl SubscriptionsScreen {
    pub fn new(
        session: Entity<Session>,
        _thumbnails: Entity<Thumbnails>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tag_picker = cx.new(|cx| TagPicker::new(session.clone(), cx));
        let folder_picker = cx.new(|cx| FolderPicker::new(session.clone(), cx));
        let add_query = cx.new(|cx| TextInput::new(Kind::Form, cx).submits());
        let add_site = format::query_sites().first().map_or("", |site| site.id);
        add_query.update(cx, |input, _| {
            input.set_placeholder(placeholder(add_site));
        });
        let subscriptions = vec![
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if *change == Change::Navigation {
                    this.navigated(cx);
                }
            }),
            // What the runners and other edits change, read again as the
            // renderer's settle store does: the list after a quiet 250 ms,
            // progress at once.
            cx.subscribe(&session, |this, _, event: &BackendEvent, cx| {
                let BackendEvent::Changed(changed) = event else {
                    return;
                };
                if !this.shown {
                    return;
                }
                if crate::application::touches(changed, "subscriptions") {
                    this.invalidation = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(INVALIDATION_DEBOUNCE).await;
                        let _ = this.update(cx, |this, cx| this.reload(cx));
                    }));
                } else if crate::application::touches(changed, "tasks") {
                    this.load(cx);
                }
            }),
            cx.subscribe(&tag_picker, |this, _, event: &tag_picker::Event, cx| {
                if let tag_picker::Event::Toggle { name, add, .. } = event {
                    this.tag_picked(name, *add, cx);
                }
            }),
            cx.subscribe(
                &folder_picker,
                |this, _, event: &folder_picker::Event, cx| {
                    if let folder_picker::Event::Toggle { folder, add } = event {
                        this.folder_picked(i64::from(folder.0), *add, cx);
                    }
                },
            ),
            cx.subscribe(&add_query, |this, _, _: &Submitted, cx| {
                this.submit_query(cx)
            }),
            // The Add button follows what is typed.
            cx.observe(&add_query, |_, _, cx| cx.notify()),
        ];
        let focus = cx.focus_handle();
        let mut screen = Self {
            session,
            covers: cx.new(Thumbnails::new),
            focus: focus.clone(),
            list: None,
            generation: 0,
            selected: HashSet::new(),
            anchor: None,
            detail: None,
            detail_generation: 0,
            loading_more: false,
            tab: SOURCES,
            // Over the window as the shell lays the grid's. The keyboard goes
            // back to the screen unless the action moved it.
            menu: menu::Host::deferred(crate::ui::layer::MENU, menu::Return::To(focus)),
            menu_targets: Vec::new(),
            dialog: modal::Host::default(),
            tag_picker,
            folder_picker,
            picking: None,
            add_site,
            add_query,
            logins: Vec::new(),
            login_health: Vec::new(),
            gallery_progress: HashMap::new(),
            settling: HashSet::new(),
            pending_open: None,
            shown: false,
            place_seen: None,
            home_width: Rc::new(Cell::new(0.)),
            plane_width: Rc::new(Cell::new(0.)),
            overflow: Rc::new(Cell::new(Bounds::default())),
            add_folder: Rc::new(Cell::new(Bounds::default())),
            add_tag: Rc::new(Cell::new(Bounds::default())),
            home_scroll: ScrollHandle::new(),
            rail_scroll: ScrollHandle::new(),
            table_scroll: ScrollHandle::new(),
            acting: HashSet::new(),
            busy: 0,
            outbox: Outbox::from_environment(),
            grace_until: None,
            poll: None,
            invalidation: None,
            _subscriptions: subscriptions,
        };
        // Read the list at once, so it is there before the screen is opened.
        screen.load(cx);
        screen
    }

    /// Sends a command to the application through the screen's outbox.
    fn send(
        &self,
        request: Request,
        cx: &App,
    ) -> futures::channel::oneshot::Receiver<Result<Value, String>> {
        let session = self.session.read(cx);
        self.outbox
            .send(request, |name, args| session.dispatch(name, args))
    }

    /// Whether an edit is on its way (`busy`).
    fn busy(&self) -> bool {
        self.busy > 0 || !self.acting.is_empty()
    }

    /// `act`: sends `requests` one after another, stopping at the first
    /// that fails, which is shown as production's error toast; then reads
    /// the subscriptions again and hands `then` whether all succeeded and
    /// the last answer.
    fn act(
        &mut self,
        requests: Vec<Request>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<Value, String>, &mut Context<Self>) + 'static,
    ) {
        if requests
            .iter()
            .any(|request| requests::starts_network(request.name))
        {
            self.grace_until = Some(Instant::now() + RUN_GRACE);
        }
        self.busy += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut outcome = Ok(Value::Null);
            for request in requests {
                let Ok(answer) = this.update(cx, |this, cx| this.send(request, cx)) else {
                    return;
                };
                outcome = answer
                    .await
                    .unwrap_or_else(|_| Err("The application has stopped".to_owned()));
                if outcome.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.busy -= 1;
                if let Err(error) = &outcome {
                    show_notification(Tone::Error, "Subscription action failed", error, cx);
                }
                then(this, outcome, cx);
                this.session
                    .update(cx, |session, cx| session.refresh_activity(cx));
                this.reload(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads every subscription again, and the sites' logins. What shows
    /// stays until the answer arrives.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let blobs = self.session.read(cx).blobs().to_path_buf();
        let answer = self.session.read(cx).run(move |library| {
            data::subscriptions(library, &blobs).map_err(|error| error.to_string())
        });
        let logins = self.send(requests::credentials(), cx);
        let health = self.send(requests::credential_health(), cx);
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let logins = logins.await.ok().and_then(Result::ok);
            let health = health.await.ok().and_then(Result::ok);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                if let Some(logins) = logins {
                    this.logins = read_logins(&logins);
                }
                if let Some(health) = health {
                    this.login_health = read_logins(&health);
                }
                match result {
                    Ok(list) => {
                        // A subscription that is gone is no longer selected.
                        this.selected
                            .retain(|id| list.iter().any(|entry| entry.id == *id));
                        this.list = Some(list);
                    }
                    Err(error) => {
                        show_notification(Tone::Error, "Subscriptions unavailable", &error, cx)
                    }
                }
                this.settled(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// What follows a new read of the list: a subscription just created
    /// opens, a deleted one that was open gives way to the grid, and runs
    /// and gallery downloads are followed.
    fn settled(&mut self, cx: &mut Context<Self>) {
        let exists = |id: i64| {
            self.list
                .iter()
                .flatten()
                .any(|entry| entry.id == id && !format::is_gallery_job(entry))
        };
        if let Some(id) = self.pending_open.filter(|id| exists(*id)) {
            self.pending_open = None;
            self.open(id, cx);
        } else if self.shown && self.list.is_some() && self.place_seen.is_some_and(|id| !exists(id))
        {
            // `refreshSubscriptionsWorkspace`: the workspace of a
            // subscription that is gone closes.
            self.session
                .update(cx, |session, cx| session.open_place(None, cx));
        }
        self.name_place(cx);
        self.follow_runs(cx);
        self.follow_galleries(cx);
    }

    /// Reads the list again, and the open subscription's runs and issues.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
        if let Some(id) = self.place_seen {
            self.load_detail(id, cx);
        }
    }

    /// Reads progress again in 1.5 s while a run is going (or was just
    /// started) and the screen shows.
    fn follow_runs(&mut self, cx: &mut Context<Self>) {
        let running = self
            .list
            .iter()
            .flatten()
            .any(|subscription| format::running(subscription));
        let starting = self.grace_until.is_some_and(|until| Instant::now() < until);
        if !self.shown || !(running || starting) {
            self.poll = None;
            return;
        }
        if self.poll.is_none() {
            self.poll = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::session::SUBSCRIPTION_POLL)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.poll = None;
                    this.reload(cx);
                });
            }));
        }
    }

    /// Gallery downloads: a running one's progress is read, with its total
    /// kept once known (`retainGalleryProgressTotals`); one whose run has
    /// settled is announced and removed (`settleFinishedGalleryImports`).
    fn follow_galleries(&mut self, cx: &mut Context<Self>) {
        let jobs: Vec<(i64, bool)> = self
            .list
            .iter()
            .flatten()
            .filter(|subscription| format::is_gallery_job(subscription))
            .map(|job| (job.id, format::running(job)))
            .collect();
        self.gallery_progress
            .retain(|id, _| jobs.iter().any(|(job, running)| job == id && *running));
        for (id, running) in jobs {
            if running {
                let answer = self.send(requests::progress(id), cx);
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(progress)) = answer.await else {
                        return;
                    };
                    let Ok(Some(progress)) = serde_json::from_value::<
                        Option<picto_core::subscription_activity::CurrentSubscriptionProgress>,
                    >(progress) else {
                        return;
                    };
                    let _ = this.update(cx, |this, cx| {
                        let known = this
                            .gallery_progress
                            .get(&id)
                            .filter(|known| known.run == progress.run_id)
                            .and_then(|known| known.total);
                        let total = match (known, progress.gallery_total_items) {
                            (Some(known), Some(reported)) => Some(known.max(reported)),
                            (known, reported) => known.or(reported),
                        };
                        this.gallery_progress.insert(
                            id,
                            GalleryProgress {
                                run: progress.run_id,
                                downloaded: progress.counts.downloaded,
                                total,
                            },
                        );
                        cx.notify();
                    });
                })
                .detach();
            } else if self.settling.insert(id) {
                self.settle_gallery(id, cx);
            }
        }
    }

    /// A gallery download that is not running: once its latest run has
    /// ended, it is announced and the transient subscription removed.
    fn settle_gallery(&mut self, id: i64, cx: &mut Context<Self>) {
        let runs = self.send(requests::runs(id), cx);
        cx.spawn(async move |this, cx| {
            let latest = runs
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|runs| {
                    serde_json::from_value::<
                            picto_core::subscription_activity::SubscriptionRunList,
                        >(runs)
                        .ok()
                })
                .and_then(|list| list.runs.into_iter().next());
            let Some(latest) =
                latest.filter(|run| !matches!(run.status.as_str(), "pending" | "running"))
            else {
                // Not started yet, or still going: looked at again later.
                let _ = this.update(cx, |this, _| this.settling.remove(&id));
                return;
            };
            let Ok(cleanup) =
                this.update(cx, |this, cx| this.send(requests::clean_up_gallery(id), cx))
            else {
                return;
            };
            let cleaned = cleanup.await.ok().and_then(Result::ok);
            let _ = this.update(cx, |this, cx| {
                let name = this
                    .subscription(id)
                    .map(|job| job.name.clone())
                    .unwrap_or_default();
                let title = cleaned
                    .as_ref()
                    .and_then(|value| value.get("title"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or(name);
                let exists = cleaned
                    .as_ref()
                    .and_then(|value| value.get("already_exists"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                match latest.status.as_str() {
                    "succeeded" if exists => show_notification(
                        Tone::Success,
                        "Gallery already exists",
                        &format!("{title} is already in the library."),
                        cx,
                    ),
                    "succeeded" => show_notification(
                        Tone::Success,
                        "Gallery downloaded",
                        &format!("{title} has been downloaded."),
                        cx,
                    ),
                    "cancelled" => {}
                    status => show_notification(
                        Tone::Error,
                        "Gallery import failed",
                        &latest.error_message.clone().unwrap_or_else(|| {
                            format!(
                                "Gallery worker failed ({}).",
                                latest.failure_kind.as_deref().unwrap_or(status)
                            )
                        }),
                        cx,
                    ),
                }
                if let Some(list) = &mut this.list {
                    list.retain(|entry| entry.id != id);
                }
                this.settling.remove(&id);
                cx.notify();
            });
        })
        .detach();
    }

    /// Sends `command` for each of `ids`, then reads what changed. The
    /// controls of a subscription wait while its command is on its way.
    fn perform(&mut self, ids: Vec<i64>, command: impl Fn(i64) -> Command, cx: &mut Context<Self>) {
        let mut answers = Vec::new();
        let mut starts = false;
        for id in ids {
            if !self.acting.insert(id) {
                continue;
            }
            let command = command(id);
            starts |= command.starts();
            answers.push((id, self.send(command.request(id), cx)));
        }
        if answers.is_empty() {
            return;
        }
        if starts {
            self.grace_until = Some(Instant::now() + RUN_GRACE);
        }
        cx.spawn(async move |this, cx| {
            let mut settled = Vec::new();
            for (id, answer) in answers {
                settled.push((id, answer.await));
            }
            let _ = this.update(cx, |this, cx| {
                for (id, result) in settled {
                    this.acting.remove(&id);
                    if let Ok(Err(error)) = result {
                        show_notification(Tone::Error, "Subscription action failed", &error, cx);
                    }
                }
                this.session
                    .update(cx, |session, cx| session.refresh_activity(cx));
                this.reload(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The run control of one subscription.
    fn run_control(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(command) = self.subscription(id).map(|subscription| {
            Command::primary(
                subscription.active_run_id.is_some(),
                format::running(subscription),
            )
        }) else {
            return;
        };
        self.perform(vec![id], |_| command, cx);
    }

    /// The hold or stop control of one subscription.
    fn hold_control(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(command) = self.subscription(id).map(|subscription| {
            Command::hold(subscription.active_run_id.is_some(), subscription.paused)
        }) else {
            return;
        };
        self.perform(vec![id], |_| command, cx);
    }

    fn subscription(&self, id: i64) -> Option<&data::Subscription> {
        self.list.as_ref()?.iter().find(|entry| entry.id == id)
    }

    fn subscription_mut(&mut self, id: i64) -> Option<&mut data::Subscription> {
        self.list.as_mut()?.iter_mut().find(|entry| entry.id == id)
    }

    /// Reads the open subscription's runs and issues.
    fn load_detail(&mut self, id: i64, cx: &mut Context<Self>) {
        self.detail_generation += 1;
        let generation = self.detail_generation;
        self.loading_more = false;
        let answer = self
            .session
            .read(cx)
            .run(move |library| data::detail(library, id).map_err(|error| error.to_string()));
        cx.spawn(async move |this, cx| {
            let detail = match answer.await {
                Ok(Ok(detail)) => detail,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.detail_generation == generation {
                            show_notification(
                                Tone::Error,
                                "Subscription details unavailable",
                                &error,
                                cx,
                            );
                        }
                    });
                    return;
                }
                Err(_) => return,
            };
            // `refreshDetail`: looking at a subscription acknowledges its
            // open issues, so its badge stops warning; the Problems tab
            // still lists them until they are resolved.
            let Ok(acknowledged) = this.update(cx, |this, cx| {
                this.send(
                    Request {
                        name: "subscriptions.issues.acknowledge",
                        args: serde_json::json!({ "subscription_id": id }),
                    },
                    cx,
                )
            }) else {
                return;
            };
            let acknowledged = matches!(acknowledged.await, Ok(Ok(_)));
            let _ = this.update(cx, |this, cx| {
                if this.detail_generation == generation {
                    this.detail = Some(detail);
                    if acknowledged && let Some(entry) = this.subscription_mut(id) {
                        entry.open_issues = 0;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Adds the next page of open issues to the Problems tab.
    fn load_more_issues(&mut self, cx: &mut Context<Self>) {
        let Some((id, after)) = self
            .detail
            .as_ref()
            .and_then(|detail| Some((detail.subscription, detail.issues.next.clone()?)))
        else {
            return;
        };
        if self.loading_more {
            return;
        }
        self.loading_more = true;
        let generation = self.detail_generation;
        let answer = self.session.read(cx).run(move |library| {
            data::more_issues(library, id, after).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
                if this.detail_generation != generation {
                    return;
                }
                match (result, this.detail.as_mut()) {
                    (Ok(Ok(page)), Some(detail)) => {
                        detail.issues.items.extend(page.items);
                        detail.issues.next = page.next;
                        detail.issues.total = page.total;
                    }
                    // `loadMoreHealth` runs through `act`.
                    (Ok(Err(error)), _) => {
                        ui::notify::error("Subscription action failed", error, cx)
                    }
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Follows the session's navigation: reads the list each time the
    /// screen is opened, and the detail when a subscription is.
    fn navigated(&mut self, cx: &mut Context<Self>) {
        let (showing, place) = {
            let session = self.session.read(cx);
            let showing = session
                .nav
                .get(session.scope)
                .is_some_and(|entry| entry.screen == Some(Screen::Subscriptions));
            (showing, session.place())
        };
        let entering = showing && !self.shown;
        if entering {
            self.load(cx);
        }
        if !showing {
            self.close_menu(cx);
        }
        self.shown = showing;
        if showing {
            if entering || place != self.place_seen {
                // A menu belongs to the surface it was opened on.
                self.close_menu(cx);
                // The grid opens with nothing selected, as it does each time
                // the Electron grid mounts.
                self.selected.clear();
                self.anchor = None;
                if let Some(id) = place {
                    if self.detail.as_ref().map(|detail| detail.subscription) != Some(id) {
                        self.tab = SOURCES;
                    }
                    self.load_detail(id, cx);
                }
            }
            self.place_seen = place;
            self.name_place(cx);
        }
        cx.notify();
    }

    /// The opened subscription, when it is one the library has. The screen
    /// keeps showing the place it had while it fades out.
    fn opened(&self) -> Option<&data::Subscription> {
        let place = self.place_seen?;
        self.list.as_ref()?.iter().find(|entry| entry.id == place)
    }

    /// Gives the scope title the open subscription's name.
    fn name_place(&self, cx: &mut Context<Self>) {
        let label = self
            .opened()
            .map(|subscription| SharedString::from(subscription.name.clone()));
        self.session
            .update(cx, |session, cx| session.set_place_label(label, cx));
    }

    /// Opens a subscription's workspace, as a step of Back and Forward.
    fn open(&mut self, id: i64, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.session
            .update(cx, |session, cx| session.open_place(Some(id), cx));
    }

    // ── Selection ───────────────────────────────────────────────

    /// Click, Cmd-click and Shift-click on the card at `index` of the grid's
    /// order, as `SubscriptionsGrid` handles them.
    fn click_card(
        &mut self,
        index: usize,
        order: &[i64],
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(&id) = order.get(index) else {
            return;
        };
        if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let range = order[anchor.min(index)..=anchor.max(index).min(order.len() - 1)]
                .iter()
                .copied();
            if !modifiers.platform {
                self.selected.clear();
            }
            self.selected.extend(range);
        } else if modifiers.platform {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.anchor = Some(index);
        } else {
            self.selected = [id].into();
            self.anchor = Some(index);
        }
        cx.notify();
    }

    /// The selected cards, in the grid's order.
    fn chosen(&self) -> Vec<i64> {
        self.card_order()
            .into_iter()
            .filter(|id| self.selected.contains(id))
            .collect()
    }

    // ── Context menu ────────────────────────────────────────────

    /// Opens the menu for `targets` at `position`: the one subscription's
    /// menu, or the bulk menu for several (`subscriptionsContextMenu.tsx`).
    fn open_menu(
        &mut self,
        targets: Vec<i64>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(list) = self.list.as_ref() else {
            return;
        };
        let chosen: Vec<&data::Subscription> = targets
            .iter()
            .filter_map(|id| list.iter().find(|entry| entry.id == *id))
            .collect();
        let entries: Vec<menu::Entry> = match chosen.as_slice() {
            [] => return,
            [subscription] => {
                let active = subscription.active_run_id.is_some();
                let running = format::running(subscription);
                let paused = subscription.run_status.as_deref() == Some("paused");
                let acting = self.acting.contains(&subscription.id);
                let (run_label, run_icon) = match (active, running) {
                    (true, true) => ("Pause", "menu-player-pause"),
                    (true, false) if paused => ("Resume", "menu-player-play"),
                    (true, false) => ("Retry", "menu-refresh"),
                    (false, _) => ("Run Now", "menu-player-play"),
                };
                let (hold_label, hold_icon) = if active {
                    ("Stop", "menu-player-stop")
                } else if subscription.paused {
                    ("Release Hold", "menu-player-play")
                } else {
                    ("Hold", "menu-hand-stop")
                };
                vec![
                    menu::item("run", run_label)
                        .icon(run_icon)
                        .disabled(
                            acting
                                || (!active
                                    && (subscription.paused || subscription.queries.is_empty())),
                        )
                        .into(),
                    menu::item("hold", hold_label)
                        .icon(hold_icon)
                        .disabled(acting)
                        .into(),
                    menu::Entry::Separator,
                    menu::item("rename", "Rename").icon("menu-rename").into(),
                    menu::item("cover", "Set Cover Photo…")
                        .icon("menu-photo-edit")
                        .into(),
                    schedule_menu(Some(subscription.schedule.as_str())),
                    menu::Entry::Separator,
                    menu::item("reset", "Reset Sync History…")
                        .icon("menu-refresh")
                        .disabled(active)
                        .into(),
                    menu::Entry::Separator,
                    menu::item("delete", "Delete Subscription…")
                        .icon("menu-trash")
                        .danger(true)
                        .into(),
                ]
            }
            several => {
                let total = several.len();
                let any_active = several
                    .iter()
                    .any(|subscription| subscription.active_run_id.is_some());
                // One schedule is ticked only when they all share it.
                let first = several[0].schedule.as_str();
                let shared = several
                    .iter()
                    .all(|subscription| subscription.schedule == first)
                    .then_some(first);
                vec![
                    menu::item("run-all", format!("Run {total} Now"))
                        .icon("menu-player-play")
                        .disabled(any_active)
                        .into(),
                    menu::item("hold-all", format!("Hold {total}"))
                        .icon("menu-hand-stop")
                        .disabled(any_active)
                        .into(),
                    menu::item("release-all", format!("Release Hold for {total}"))
                        .icon("menu-player-play")
                        .disabled(any_active)
                        .into(),
                    schedule_menu(shared),
                    menu::Entry::Separator,
                    menu::item("delete-all", format!("Delete {total}…"))
                        .icon("menu-trash")
                        .danger(true)
                        .into(),
                ]
            }
        };
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self.menu_targets = targets;
        self.menu
            .open(menu, window, cx, |this, menu, action, window, cx| {
                let targets = this.menu_targets.clone();
                if action == "schedule"
                    && let Some(schedule) = menu
                        .read(cx)
                        .chosen_value()
                        .and_then(|value| schedule_named(&value))
                {
                    this.perform(targets.clone(), |_| Command::Schedule(schedule), cx);
                }
                match (action, targets.as_slice()) {
                    ("run", [id]) => this.run_control(*id, cx),
                    ("hold", [id]) => this.hold_control(*id, cx),
                    ("rename", [id]) => this.open_rename(*id, window, cx),
                    ("cover", [id]) => this.open_cover(*id, cx),
                    ("reset", [id]) => this.confirm_reset(*id, cx),
                    ("delete", [id]) => this.confirm_delete(*id, cx),
                    // `handleBulk*`: each subscription on its own.
                    ("run-all", _) => this.perform(targets, |_| Command::Run, cx),
                    ("hold-all", _) => this.perform(targets, |_| Command::Hold(true), cx),
                    ("release-all", _) => this.perform(targets, |_| Command::Hold(false), cx),
                    ("delete-all", _) => this.confirm_delete_all(targets, cx),
                    _ => {}
                }
            });
    }

    /// The menu for a right-clicked card: the whole selection when the card
    /// is part of it, else the card alone, which becomes the selection.
    fn card_menu(
        &mut self,
        index: usize,
        order: &[i64],
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(&id) = order.get(index) else {
            return;
        };
        if !self.selected.contains(&id) {
            self.selected = [id].into();
            self.anchor = Some(index);
        }
        let targets = self.chosen();
        self.open_menu(targets, position, window, cx);
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu.close(cx);
    }

    // ── Destination ─────────────────────────────────────────────

    /// Saves where a subscription's new files go, showing it at once.
    fn set_destination(
        &mut self,
        id: i64,
        folders: Vec<i64>,
        tags: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let request = requests::destination(id, &folders, &tags);
        if let Some(entry) = self.subscription_mut(id) {
            entry.folder_ids = folders;
            entry.automatic_tags = tags;
        }
        self.act(vec![request], cx, |_, _, _| {});
    }

    fn remove_folder(&mut self, id: i64, folder: i64, cx: &mut Context<Self>) {
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let folders = entry
            .folder_ids
            .iter()
            .copied()
            .filter(|current| *current != folder)
            .collect();
        let tags = entry.automatic_tags.clone();
        self.set_destination(id, folders, tags, cx);
    }

    fn remove_tag(&mut self, id: i64, tag: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let folders = entry.folder_ids.clone();
        let tags = entry
            .automatic_tags
            .iter()
            .filter(|current| *current != tag)
            .cloned()
            .collect();
        self.set_destination(id, folders, tags, cx);
    }

    /// The folder picker, above the add button, on the folders new files go
    /// to.
    fn pick_folders(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let folders = entry
            .folder_ids
            .iter()
            .filter_map(|folder| u32::try_from(*folder).ok())
            .map(picto_library::FolderId)
            .collect();
        let anchor = self.add_folder.get().origin;
        self.picking = Some(Picking::Folders(id));
        self.folder_picker.update(cx, |picker, cx| {
            picker.open_above(
                point(f32::from(anchor.x), f32::from(anchor.y)),
                folders,
                window,
                cx,
            )
        });
    }

    /// The tag picker, above the add button, on the tags new files get.
    fn pick_tags(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let records = entry
            .automatic_tags
            .iter()
            .map(|tag| crate::tags::record(tag))
            .collect();
        let anchor = self.add_tag.get().origin;
        self.picking = Some(Picking::Tags(id));
        self.tag_picker.update(cx, |picker, cx| {
            picker.open_above(
                point(f32::from(anchor.x), f32::from(anchor.y)),
                records,
                window,
                cx,
            )
        });
    }

    /// A folder checked or unchecked in the picker (`onApplyFolders`).
    fn folder_picked(&mut self, folder: i64, add: bool, cx: &mut Context<Self>) {
        let Some(Picking::Folders(id)) = self.picking else {
            return;
        };
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let mut folders = entry.folder_ids.clone();
        folders.retain(|current| *current != folder);
        if add {
            folders.push(folder);
        }
        let tags = entry.automatic_tags.clone();
        self.set_destination(id, folders, tags, cx);
    }

    /// A tag checked or unchecked in the picker (`onApplyTags`).
    fn tag_picked(&mut self, name: &str, add: bool, cx: &mut Context<Self>) {
        let Some(Picking::Tags(id)) = self.picking else {
            return;
        };
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let mut tags = entry.automatic_tags.clone();
        tags.retain(|current| current != name);
        if add {
            tags.push(name.to_owned());
        }
        let folders = entry.folder_ids.clone();
        self.set_destination(id, folders, tags, cx);
    }

    // ── Sources ─────────────────────────────────────────────────

    /// The add-query bar's source.
    fn choose_add_site(&mut self, site: &'static str, cx: &mut Context<Self>) {
        self.add_site = site;
        self.add_query.update(cx, |input, cx| {
            input.set_placeholder(placeholder(site));
            cx.notify();
        });
        cx.notify();
    }

    /// `AddQueryBar`'s submit: the typed query is added to the open
    /// subscription, and the field empties once it is.
    fn submit_query(&mut self, cx: &mut Context<Self>) {
        let text = self.add_query.read(cx).value().trim().to_owned();
        let Some(id) = self.place_seen else {
            return;
        };
        if self.add_site.is_empty() || text.is_empty() || self.busy() {
            return;
        }
        let source = requests::Source {
            site: self.add_site.to_owned(),
            text,
            display_name: None,
            notes: None,
            group_posts: crate::settings::current(cx).subscription_default_group_posts,
        };
        self.act(vec![requests::add_query(id, &source)], cx, |this, _, cx| {
            this.add_query.update(cx, |input, cx| input.clear(cx));
        });
    }

    /// A source's own run (`runQuery`), which downloads.
    fn run_query(&mut self, query: i64, cx: &mut Context<Self>) {
        self.act(vec![requests::run_query(query)], cx, |_, _, _| {});
    }

    fn pause_query(&mut self, query: i64, paused: bool, cx: &mut Context<Self>) {
        self.act(vec![requests::pause_query(query, paused)], cx, |_, _, _| {});
    }

    fn group_query(&mut self, query: i64, group_posts: bool, cx: &mut Context<Self>) {
        self.act(
            vec![requests::group_query(query, group_posts)],
            cx,
            |_, _, _| {},
        );
    }

    // ── Deleting ────────────────────────────────────────────────

    /// ⌘⌫ on the grid: the selected subscriptions, after asking.
    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let chosen = self.chosen();
        if self.place_seen.is_some() || chosen.is_empty() {
            return;
        }
        self.confirm_delete_all(chosen, cx);
    }
}

/// `e.g. <the site's example>`, or `query` while there is no site.
fn placeholder(site: &str) -> SharedString {
    match format::site(site) {
        Some(site) => format!("e.g. {}", site.example_query).into(),
        None => "query".into(),
    }
}

/// The sites of `auth.credentials.list` or `auth.health.list`.
fn read_logins(answer: &Value) -> Vec<format::Login> {
    let text =
        |record: &Value, key: &str| record.get(key).and_then(Value::as_str).map(str::to_owned);
    answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| {
            Some(format::Login {
                site: text(record, "site_id")?,
                health: text(record, "status"),
                last_error: text(record, "last_error"),
            })
        })
        .collect()
}

impl Render for SubscriptionsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::theme(cx);
        let place = self.place_seen;
        let body = match (&self.list, place) {
            // Nothing is drawn until the first read arrives.
            (None, _) => div().into_any_element(),
            (Some(_), None) => self.home(window, cx).into_any_element(),
            (Some(_), Some(_)) => match self.opened().cloned() {
                Some(subscription) => self.workspace(&subscription, window, cx).into_any_element(),
                None => home::empty_state(cx).into_any_element(),
            },
        };
        let menu = self.menu.layer(window, cx);
        let dialog = self.dialog_layer(window, cx);
        div()
            .id("subscriptions")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .text_color(theme.text())
            .track_focus(&self.focus)
            .key_context("Subscriptions")
            // A press takes the keyboard to the screen.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    if !this.focus.contains_focused(window, cx) {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Open, _, cx| {
                let chosen = this.chosen();
                if this.place_seen.is_none()
                    && let [id] = chosen.as_slice()
                {
                    let id = *id;
                    this.open(id, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Deselect, _, cx| {
                if this.selected.is_empty() || this.place_seen.is_some() {
                    cx.propagate();
                } else {
                    this.selected.clear();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::keymap::DeleteItems, _, cx| {
                if this.place_seen.is_none() && !this.selected.is_empty() {
                    this.delete_selected(cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(body)
            .children(menu)
            .children(dialog)
            .child(self.tag_picker.clone())
            .child(self.folder_picker.clone())
    }
}

/// Keeps a measured width in `cell` and repaints `view` when it changes.
/// The repaint is asked for with a frame, since a notification made while
/// a frame is drawn schedules none.
fn measure(cell: Rc<Cell<f32>>, window: &Window) -> impl IntoElement {
    let view = window.current_view();
    canvas(
        move |bounds, window, cx| {
            let width = f32::from(bounds.size.width);
            if (cell.replace(width) - width).abs() > 0.5 {
                cx.notify(view);
                // A notification made while a frame is being drawn does not
                // by itself bring another frame.
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Keeps where an element is in `cell`.
fn place_of(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

#[cfg(test)]
mod tests {
    use super::{Command, read_logins};
    use picto_core::ipc;

    #[test]
    fn controls_send_the_commands_the_electron_screen_sends() {
        assert_eq!(Command::primary(false, false), Command::Run);
        assert_eq!(Command::primary(true, true), Command::Pause);
        assert_eq!(Command::primary(true, false), Command::Resume);
        assert_eq!(Command::hold(true, false), Command::Stop);
        assert_eq!(Command::hold(false, false), Command::Hold(true));
        assert_eq!(Command::hold(false, true), Command::Hold(false));
        let run = Command::Run.request(4);
        assert_eq!(
            (run.name, run.args),
            (
                "subscriptions.run",
                serde_json::json!({ "subscription_id": 4 })
            )
        );
        assert_eq!(Command::Pause.request(4).name, "subscriptions.run.pause");
        assert_eq!(Command::Resume.request(4).name, "subscriptions.run.resume");
        assert_eq!(Command::Stop.request(4).name, "subscriptions.cancel");
        assert_eq!(
            Command::Hold(false).request(4).args,
            serde_json::json!({ "subscription_id": 4, "paused": false })
        );
        assert_eq!(
            Command::Schedule("weekly").request(4).args,
            serde_json::json!({ "subscription_id": 4, "schedule": "weekly" })
        );
        assert_eq!(
            Command::PostsPerRun(250).request(4).args,
            serde_json::json!({ "subscription_id": 4, "posts_per_run": 250 })
        );
        assert!(!Command::Schedule("daily").starts() && !Command::PostsPerRun(1).starts());
        // Each reads as the application's own input.
        for command in [Command::Run, Command::Pause, Command::Resume, Command::Stop] {
            serde_json::from_value::<ipc::SubscriptionInput>(command.request(4).args).unwrap();
        }
        serde_json::from_value::<ipc::PauseSubscriptionInput>(Command::Hold(true).request(4).args)
            .unwrap();
        serde_json::from_value::<ipc::ScheduleSubscriptionInput>(
            Command::Schedule("manual").request(4).args,
        )
        .unwrap();
        serde_json::from_value::<ipc::SubscriptionPostsPerRunInput>(
            Command::PostsPerRun(9).request(4).args,
        )
        .unwrap();
    }

    #[test]
    fn logins_are_read_by_site() {
        let logins = read_logins(&serde_json::json!([
            { "site_id": "e621", "status": "healthy", "checked_at": null, "last_error": null },
            { "site_id": "newgrounds", "status": "invalid", "last_error": "403" },
            { "status": "orphan" },
        ]));
        assert_eq!(logins.len(), 2);
        assert_eq!(logins[1].site, "newgrounds");
        assert_eq!(logins[1].last_error.as_deref(), Some("403"));
    }
}
