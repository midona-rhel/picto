//! The Subscriptions screen. At its top level the screen shows one tile for
//! each of the library's subscriptions (`home.rs`). When a tile is opened,
//! the screen shows that subscription's page (`detail.rs`), and the shell
//! draws the page's rail where the inspector normally is (`toolbar.rs`).
//!
//! The open subscription is a place inside the screen in the session's
//! navigation (`Session::open_place`). Opening a subscription therefore adds
//! an entry to the Back and Forward history, Back returns to the tiles, and
//! the scope title in the titlebar shows the subscription's name after the
//! screen's name.
//!
//! This file holds the screen's state, the commands that the run and hold
//! controls send, the selection of tiles, the context menus, the editing of
//! the folders and tags that a subscription gives its new files, and
//! `render`. `sync.rs` reads the subscriptions from the library and follows
//! the runs in progress. `dialogs.rs`, `cover.rs` and `accounts.rs` hold the
//! screen's dialogs.
mod accounts;
mod cover;
mod data;
mod detail;
mod dialogs;
mod format;
mod home;
pub mod requests;
mod sync;
mod toolbar;

pub use toolbar::{controls, note, rail};

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::t;
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

/// Binds the two keys that only this screen handles. Enter performs `Open`,
/// which opens the selected subscription, and Escape performs `Deselect`,
/// which clears the selection of tiles. Both are bound in the screen's
/// `Subscriptions` key context. That context is inside the shell's `Browser`
/// context, so these bindings take precedence over bindings for the same
/// keys in `Browser`. Like the bindings made from the shortcut registry
/// (`keymap.rs`), they do not run while the keyboard focus is in a text
/// field, a context menu, a popover or a dialog.
pub fn bind(cx: &mut App) {
    let context = Some("Subscriptions && !TextInput && !Menu && !Modal && !Popover");
    cx.bind_keys([
        KeyBinding::new("enter", Open, context),
        KeyBinding::new("escape", Deselect, context),
    ]);
}

/// How long the screen waits, after the application reports a change to the
/// subscriptions, before it reads them again. It is 250 ms. Each further
/// report within that time starts the wait again, so several changes close
/// together cause one read.
const INVALIDATION_DEBOUNCE: Duration = Duration::from_millis(250);
/// How long the screen goes on reading the subscriptions every 1.5 s after
/// it has sent a command that can start a run, even though no subscription
/// is reported as running yet. It is 5 s.
const RUN_GRACE: Duration = Duration::from_secs(5);

/// A command that one of a subscription's controls sends to the
/// application. The controls are the run button, the hold button, the
/// Schedule control and the "Posts per run" field. `Command::request` gives
/// the name the application knows each command by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Starts a run now (`subscriptions.run`).
    Run,
    /// Pauses the run that is in progress (`subscriptions.run.pause`).
    Pause,
    /// Continues a run that is paused, or tries again a run that stopped on
    /// a failure (`subscriptions.run.resume`).
    Resume,
    /// Cancels the active run (`subscriptions.cancel`). The Stop button's
    /// tooltip says that the posts the run has already added are kept.
    Stop,
    /// With `true`, puts the subscription on hold, so that its future runs
    /// do not start. With `false`, releases the hold (`subscriptions.hold`).
    Hold(bool),
    /// Sets when the subscription runs without being asked to. The value is
    /// `manual`, `daily`, `weekly` or `monthly`
    /// (`subscriptions.schedule`).
    Schedule(&'static str),
    /// Sets the subscription's "Posts per run" limit to this number
    /// (`subscriptions.posts_per_run`).
    PostsPerRun(i64),
}

impl Command {
    /// Returns the command that the run button sends. `active` says whether
    /// the subscription has an active run, and `running` says whether that
    /// run is in progress (`format::running`). The command is `Pause` when
    /// the run is in progress. It is `Resume` when the run is active but not
    /// in progress, which is when the run is paused or has stopped on a
    /// failure; the button then reads Resume or Retry. It is `Run` when no
    /// run is active.
    pub fn primary(active: bool, running: bool) -> Self {
        match (active, running) {
            (true, true) => Command::Pause,
            (true, false) => Command::Resume,
            (false, _) => Command::Run,
        }
    }

    /// Returns the command that the hold button sends. It is `Stop` while
    /// the subscription has an active run. Otherwise it is `Hold` with the
    /// opposite of `held`: a subscription that is not on hold is put on
    /// hold, and one that is on hold is released.
    pub fn hold(active: bool, held: bool) -> Self {
        if active {
            Command::Stop
        } else {
            Command::Hold(!held)
        }
    }

    /// Returns the request that sends this command for the subscription
    /// `subscription_id`: the command's name and its arguments, as
    /// `core/src/ipc.rs` reads them.
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

    /// Returns true for `Run` and `Resume`, the two commands that can start
    /// a run. After `perform` has sent one of them, the screen reads the
    /// subscriptions every 1.5 s for 5 s (`RUN_GRACE`), even though no
    /// subscription is reported as running yet.
    fn starts(self) -> bool {
        matches!(self, Command::Run | Command::Resume)
    }
}

/// The schedules a subscription can have. Each pair holds the value that is
/// stored and sent with `subscriptions.schedule`, and the label that the
/// Schedule submenu of the context menu shows for it.
// i18n: translated where drawn (menu rows)
const SCHEDULES: [(&str, &str); 4] = [
    ("manual", "Manual"),
    ("daily", "Daily"),
    ("weekly", "Weekly"),
    ("monthly", "Monthly"),
];

/// Returns the stored value in `SCHEDULES` that is equal to `value`, or
/// `None` when there is none. The context menu reports the chosen row's
/// value as a string of its own, and `Command::Schedule` needs the
/// `&'static str` from `SCHEDULES`.
fn schedule_named(value: &str) -> Option<&'static str> {
    SCHEDULES
        .iter()
        .map(|(schedule, _)| *schedule)
        .find(|schedule| *schedule == value)
}

/// Builds the Schedule submenu of a subscription's context menu, with one
/// row for each schedule in `SCHEDULES`. The row of the schedule `current`
/// has a check mark and is disabled, so the schedule that is already set
/// cannot be chosen again. `current` is `None` in the menu for several
/// subscriptions that do not all have the same schedule, and then no row
/// has a check mark.
fn schedule_menu(current: Option<&str>) -> menu::Entry {
    menu::submenu(
        t("Schedule"),
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

/// The labels of the three tabs on a subscription's page, in the order they
/// are shown. Each tab shows one table: the subscription's sources, its
/// runs, or its open problems. The screen keeps the index of the tab that
/// is shown in `tab`.
// i18n: translated where drawn
const TABS: [&str; 3] = ["Sources", "History", "Problems"];
/// The index of the Sources tab in `TABS`.
const SOURCES: usize = 0;
/// The index of the History tab in `TABS`.
const HISTORY: usize = 1;

/// The progress of the run in progress of one gallery download, as the
/// application answers `subscriptions.progress.get`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GalleryProgress {
    /// The id of the run that the two numbers below belong to.
    run: i64,
    /// The number of images the run has downloaded so far.
    downloaded: i64,
    /// The number of images in the gallery, or `None` until the site has
    /// reported it.
    total: Option<i64>,
}

/// Says which list the open folder picker or tag picker edits: the folders
/// (`Folders`) or the tags (`Tags`) that the subscription with the given id
/// gives its new files.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Picking {
    Folders(i64),
    Tags(i64),
}

pub struct SubscriptionsScreen {
    session: Entity<Session>,
    /// The cache that the subscriptions' cover pictures are decoded into.
    /// It belongs to this screen and is separate from the grid's thumbnail
    /// cache. The grid's cache removes the pictures that were used longest
    /// ago when it is over its memory budget. Because the covers are not in
    /// that cache, it cannot remove a cover that this screen still shows.
    covers: Entity<Thumbnails>,
    /// The focus handle of the screen's root element. The screen's key
    /// bindings (`bind`) and the Delete shortcut work while the root
    /// element or an element inside it has the keyboard focus.
    focus: FocusHandle,
    /// Every subscription in the library, gallery downloads included, or
    /// `None` until the first read has arrived. A later read replaces the
    /// list only when its answer arrives, so the screen shows the earlier
    /// list until then.
    list: Option<Vec<data::Subscription>>,
    /// The number of the latest read of the list (`load`). The answer to a
    /// read is used only if no later read has been started since, so an
    /// answer that arrives late does not replace a newer one.
    generation: u64,
    /// The ids of the subscriptions whose tiles are selected.
    selected: HashSet<i64>,
    /// The index, in the order the tiles are shown, of the tile from which
    /// a Shift-click selects a range. A click without Shift sets it, and so
    /// does a right-click on a tile that is not selected. It is set back to
    /// `None` each time the screen is opened and each time a subscription's
    /// page is opened or closed.
    anchor: Option<usize>,
    /// The runs (History tab) and the open problems (Problems tab) that
    /// were last read for a subscription, with that subscription's id.
    /// After another subscription is opened, this still holds the earlier
    /// subscription's data until the read for the new one arrives. The page
    /// uses the data only when the id is the open subscription's.
    detail: Option<data::Detail>,
    /// The number of the latest read of `detail` (`load_detail`). It is
    /// used as `generation` is for the list.
    detail_generation: u64,
    /// True while the next page of problems is being read for the Problems
    /// tab. The tab's "Load more" button is disabled meanwhile.
    loading_more: bool,
    /// The index in `TABS` of the tab that a subscription's page shows.
    tab: usize,
    /// Holds the open context menu: the menu of one or several tiles, or
    /// the "More actions" menu of a subscription's page. The menu stays in
    /// it until it has faded out.
    menu: menu::Host,
    /// The ids of the subscriptions that the open context menu acts on.
    menu_targets: Vec<i64>,
    /// Holds the open dialog. The dialogs are "Add subscription", "Add
    /// gallery", "Rename Subscription", "Edit query", a source's details, a
    /// confirmation, the choice of a cover picture, and "Site accounts".
    dialog: modal::Host<dialogs::Dialog>,
    /// The picker that adds tags to, and removes them from, the tags that a
    /// subscription gives its new files. It is the tag picker that the
    /// inspector uses.
    tag_picker: Entity<TagPicker>,
    /// The picker that adds folders to, and removes them from, the folders
    /// that a subscription adds its new files to. It is the folder picker
    /// that the inspector uses.
    folder_picker: Entity<FolderPicker>,
    /// Which list of which subscription the picker that was opened last
    /// edits. It is set when a picker is opened and is not cleared when the
    /// picker closes.
    picking: Option<Picking>,
    /// The id of the site that is chosen in the row that adds a source,
    /// which is under the rows of the Sources table.
    add_site: &'static str,
    /// The text field of that row, in which the query is typed.
    add_query: Entity<TextInput>,
    /// The sites that have a saved login, as `auth.credentials.list`
    /// reports them. Together with `login_health` it decides whether a
    /// source's row shows a warning that the site needs a login before the
    /// source can run (`format::auth_warning`).
    logins: Vec<format::Login>,
    /// The result of the last check of each site's login, as
    /// `auth.health.list` reports it.
    login_health: Vec<format::Login>,
    /// The progress of each gallery download that is running, keyed by the
    /// download's subscription id. An entry is removed when its download is
    /// no longer running. The session, not this screen, shows the
    /// notification for a download that has ended and removes the download
    /// from the library (`session::settle`).
    gallery_progress: HashMap<i64, GalleryProgress>,
    /// The id of a subscription that the "Add subscription" dialog has just
    /// created. The screen opens its page when a read of the list contains
    /// it (`settled`).
    pending_open: Option<i64>,
    /// Whether the Subscriptions screen was the destination shown the last
    /// time the session reported a navigation. `navigated` compares it with
    /// the new state, and reads the list again each time the screen is
    /// opened.
    shown: bool,
    /// The id of the subscription whose page the screen shows, or `None`
    /// when the screen shows the tiles. `navigated` copies it from the
    /// session's place (`Session::place`) while the Subscriptions screen is
    /// the destination shown. When the user goes to another destination it
    /// keeps its value, so the screen goes on drawing the same thing while
    /// the shell fades it out.
    place_seen: Option<i64>,
    /// The width of the screen's area in pixels, in the frame that is
    /// being built. `render` sets it before it builds the tiles or the
    /// page. The tiles' layout and the tables' columns are computed for it.
    width: f32,
    /// The position and size, in window coordinates, of the "More actions"
    /// button in the titlebar of a subscription's page. A click on the
    /// button opens its menu at the point 4 px below the button's
    /// bottom-left corner. This field and the three after it are each
    /// written by an element from `place_of` every time the button, the
    /// rail or the control they describe is drawn. They are read when a
    /// click opens the menu or a picker.
    overflow: Rc<Cell<Bounds<Pixels>>>,
    /// The position and size of the rail of a subscription's page. The
    /// folder picker and the tag picker open to the left of the rail's left
    /// edge.
    rail_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The position and size of the control in the rail that adds a folder.
    /// The folder picker opens with its top at this control's top.
    add_folder: Rc<Cell<Bounds<Pixels>>>,
    /// The position and size of the control in the rail that adds a tag.
    /// The tag picker opens with its top at this control's top.
    add_tag: Rc<Cell<Bounds<Pixels>>>,
    /// The scroll position of the area that shows the tiles.
    home_scroll: ScrollHandle,
    /// The scroll position of the rail of a subscription's page.
    rail_scroll: ScrollHandle,
    /// The scroll position of the table on a subscription's page. The three
    /// tabs share it, and it is set back to the top when another tab is
    /// chosen.
    table_scroll: ScrollHandle,
    /// The titles of the rail's sections that the user has folded. A folded
    /// section shows only its title.
    collapsed: HashSet<&'static str>,
    /// The ids of the subscriptions for which `perform` has sent a command
    /// that has not been answered yet. The run button and the hold button
    /// of such a subscription are disabled.
    acting: HashSet<i64>,
    /// The number of calls of `act` whose requests have not all been
    /// answered yet. While it is not 0, `busy()` returns true, and the
    /// buttons of the Sources and Problems rows, the dialogs' buttons and
    /// the Add button of the row that adds a source are disabled.
    busy: usize,
    /// Where the screen's commands are sent (`requests::Outbox`). Normally
    /// they go to the application. When the environment variable
    /// `PICTO_GPUI_HOLD_RUNS` is set, every command is also written to a
    /// log, and a command that would start work on the network is answered
    /// at once and not sent.
    outbox: Outbox,
    /// The time until which the screen goes on reading the subscriptions
    /// every 1.5 s although none is reported as running. It is set to 5 s
    /// from now (`RUN_GRACE`) when a command that can start a run is sent.
    grace_until: Option<Instant>,
    /// The timer that `follow_runs` starts while a run is in progress or
    /// `grace_until` has not passed. When it ends, after 1.5 s, the screen
    /// reads the list and the open subscription's runs and problems again.
    poll: Option<Task<()>>,
    /// The timer that waits 250 ms (`INVALIDATION_DEBOUNCE`) after the
    /// application reports a change to the subscriptions. When it ends, the
    /// screen reads them again. A new report replaces the timer, which
    /// cancels the one before it.
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
            // The application reports which parts of the library have
            // changed. While the screen is shown, a report that names
            // `subscriptions` makes the screen read the list and the open
            // subscription's runs and problems again, 250 ms after the last
            // such report (`INVALIDATION_DEBOUNCE`). A report that names
            // `tasks` and not `subscriptions` makes the screen read the list
            // again at once.
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
            // The Add button of the row that adds a source is enabled only
            // while the row's text field holds text. The screen is
            // therefore drawn again whenever the field changes.
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
            // The context menu is drawn over the whole window, on the layer
            // that context menus use (`layer::MENU`). When the menu has
            // closed, the keyboard focus goes back to the screen, unless
            // the chosen action moved the focus somewhere else, for example
            // into a dialog's text field.
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
            pending_open: None,
            shown: false,
            place_seen: None,
            width: 0.,
            overflow: Rc::new(Cell::new(Bounds::default())),
            rail_bounds: Rc::new(Cell::new(Bounds::default())),
            add_folder: Rc::new(Cell::new(Bounds::default())),
            add_tag: Rc::new(Cell::new(Bounds::default())),
            home_scroll: ScrollHandle::new(),
            rail_scroll: ScrollHandle::new(),
            table_scroll: ScrollHandle::new(),
            collapsed: HashSet::new(),
            acting: HashSet::new(),
            busy: 0,
            outbox: Outbox::from_environment(),
            grace_until: None,
            poll: None,
            invalidation: None,
            _subscriptions: subscriptions,
        };
        // The list is read as soon as the screen is created, and not only
        // when the screen is first opened, so that the list has usually
        // arrived by the time the user opens the screen.
        screen.load(cx);
        screen
    }

    /// Sends `request` through the screen's outbox (`requests::Outbox`) and
    /// returns the receiver of the answer. Unless the outbox holds the
    /// request back, the request goes to the application through
    /// `Session::dispatch`.
    fn send(
        &self,
        request: Request,
        cx: &App,
    ) -> futures::channel::oneshot::Receiver<Result<Value, String>> {
        let session = self.session.read(cx);
        self.outbox
            .send(request, |name, args| session.dispatch(name, args))
    }

    /// Returns true while a call of `act` or a command sent by `perform` is
    /// still waiting for its answer. The controls that change a
    /// subscription or one of its sources are disabled while it is true.
    fn busy(&self) -> bool {
        self.busy > 0 || !self.acting.is_empty()
    }

    /// Sends `requests` to the application one after another. It waits for
    /// the answer to each request before it sends the next, and it sends no
    /// more after the first one that fails. A failure is shown as the error
    /// notification "Subscription action failed" with the application's
    /// message.
    ///
    /// When the requests are done, `then` is called with the answer to the
    /// last request that was sent, which is the error if one failed. After
    /// that the screen reads the subscriptions again (`reload`). The field
    /// `busy` is one higher from the call until the requests are done.
    ///
    /// If one of the requests starts work on the network
    /// (`requests::starts_network`), the screen also reads the subscriptions
    /// every 1.5 s for the next 5 s (`RUN_GRACE`).
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
                outcome = crate::application::answer(answer).await;
                if outcome.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.busy -= 1;
                if let Err(error) = &outcome {
                    crate::ui::notify::error(t("Subscription action failed"), error.clone(), cx);
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

    /// Returns the subscription whose page the screen shows (`place_seen`).
    /// It returns `None` when the screen shows the tiles, and when the list
    /// does not hold that subscription, either because the list has not
    /// been read yet or because the subscription has been deleted.
    fn opened(&self) -> Option<&data::Subscription> {
        let place = self.place_seen?;
        self.list.as_ref()?.iter().find(|entry| entry.id == place)
    }

    /// Returns true while the screen shows a subscription's page and not
    /// the tiles. The shell draws the page's rail only while this is true
    /// (`toolbar::rail`). `place_seen` keeps its value after the user goes
    /// to another destination, so this stays true while the shell fades the
    /// screen out.
    pub fn showing_page(&self) -> bool {
        self.place_seen.is_some()
    }

    /// Sets the name that the scope title in the titlebar shows for the
    /// open place (`Session::set_place_label`). The name is the open
    /// subscription's name. It is removed when the screen shows the tiles
    /// or the open subscription is not in the list.
    fn name_place(&self, cx: &mut Context<Self>) {
        let label = self
            .opened()
            .map(|subscription| SharedString::from(subscription.name.clone()));
        self.session
            .update(cx, |session, cx| session.set_place_label(label, cx));
    }

    /// Opens the page of the subscription `id`. It closes the open context
    /// menu and asks the session to open the subscription as a place inside
    /// the screen (`Session::open_place`), which adds an entry to the Back
    /// and Forward history. The session then reports a navigation, and
    /// `navigated` makes the screen show the page.
    fn open(&mut self, id: i64, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.session
            .update(cx, |session, cx| session.open_place(Some(id), cx));
    }

    // The functions below change and read the selection of tiles.

    /// Changes the selection for a click on the tile at `index` of `order`.
    /// `order` holds the ids of the subscriptions in the order their tiles
    /// are shown, and `modifiers` holds the keys held during the click.
    ///
    /// The key that adds to a selection is Command on macOS and Control on
    /// Windows and Linux, the same key as in the grid
    /// ([`crate::grid::selection::Press`]).
    ///
    /// - With Shift, when there is an anchor: every tile from the anchor to
    ///   the clicked tile is selected. The tiles that were selected before
    ///   are deselected first, unless that key is also held. The anchor
    ///   does not change.
    /// - With that key, when the first case does not apply: the clicked
    ///   tile is selected if it was not selected and deselected if it was,
    ///   and it becomes the anchor.
    /// - In every other case the clicked tile becomes the only selected
    ///   tile and the anchor.
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
        let adds = if cfg!(target_os = "macos") {
            modifiers.platform
        } else {
            modifiers.control
        };
        if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let range = order[anchor.min(index)..=anchor.max(index).min(order.len() - 1)]
                .iter()
                .copied();
            if !adds {
                self.selected.clear();
            }
            self.selected.extend(range);
        } else if adds {
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

    /// Returns the ids of the selected subscriptions, in the order their
    /// tiles are shown.
    fn chosen(&self) -> Vec<i64> {
        self.card_order()
            .into_iter()
            .filter(|id| self.selected.contains(id))
            .collect()
    }

    // The functions below open and close the context menu.

    /// Opens a context menu at `position` that acts on the subscriptions
    /// `targets`. No menu opens when the list has not been read or holds
    /// none of `targets`.
    ///
    /// The menu for one subscription has these rows:
    ///
    /// - The run command, which reads Run Now, Pause, Resume or Retry. It
    ///   is disabled while a command for the subscription is waiting for
    ///   its answer. Run Now is also disabled while the subscription is on
    ///   hold or has no sources.
    /// - The hold command, which reads Hold, Release Hold or Stop. It is
    ///   disabled while a command for the subscription is waiting for its
    ///   answer.
    /// - Rename, Set Cover Photo… and the Schedule submenu.
    /// - Reset Sync History…, which is disabled while the subscription has
    ///   an active run.
    /// - Delete Subscription….
    ///
    /// The menu for several subscriptions has "Run N Now", "Hold N" and
    /// "Release Hold for N", where N is the number of subscriptions, then
    /// the Schedule submenu and "Delete N…". The first three are disabled
    /// while any of the subscriptions has an active run.
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
                    (true, true) => (t("Pause"), "menu-player-pause"),
                    (true, false) if paused => (t("Resume"), "menu-player-play"),
                    (true, false) => (t("Retry"), "menu-refresh"),
                    (false, _) => (t("Run Now"), "menu-player-play"),
                };
                let (hold_label, hold_icon) = if active {
                    (t("Stop"), "menu-player-stop")
                } else if subscription.paused {
                    (t("Release Hold"), "menu-player-play")
                } else {
                    (t("Hold"), "menu-hand-stop")
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
                // A row of the Schedule submenu has a check mark only when
                // every one of the subscriptions has that schedule.
                let first = several[0].schedule.as_str();
                let shared = several
                    .iter()
                    .all(|subscription| subscription.schedule == first)
                    .then_some(first);
                vec![
                    menu::item("run-all", crate::tf!("Run {value0} Now", total))
                        .icon("menu-player-play")
                        .disabled(any_active)
                        .into(),
                    menu::item("hold-all", crate::tf!("Hold {value0}", total))
                        .icon("menu-hand-stop")
                        .disabled(any_active)
                        .into(),
                    menu::item(
                        "release-all",
                        crate::tf!("Release Hold for {value0}", total),
                    )
                    .icon("menu-player-play")
                    .disabled(any_active)
                    .into(),
                    schedule_menu(shared),
                    menu::Entry::Separator,
                    menu::item("delete-all", crate::tf!("Delete {value0}…", total))
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
                    // The Run, Hold and Release Hold rows of the menu for
                    // several subscriptions send one command for each
                    // subscription.
                    ("run-all", _) => this.perform(targets, |_| Command::Run, cx),
                    ("hold-all", _) => this.perform(targets, |_| Command::Hold(true), cx),
                    ("release-all", _) => this.perform(targets, |_| Command::Hold(false), cx),
                    ("delete-all", _) => this.confirm_delete_all(targets, cx),
                    _ => {}
                }
            });
    }

    /// Opens the context menu for a right-click at `position` on the tile
    /// at `index` of `order`. When the tile is selected, the menu acts on
    /// every selected subscription. When it is not, the tile first becomes
    /// the only selected tile and the anchor, and the menu acts on it
    /// alone.
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

    // The functions below edit a subscription's folders and tags.

    /// Saves the destination of the subscription `id`
    /// (`subscriptions.destination`): `folders` are the folders that its
    /// new files are added to, and `tags` are the tags that its new files
    /// are given. The subscription's entry in `list` is changed before the
    /// request is sent, so the rail shows the new folders and tags without
    /// waiting for the application's answer.
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

    /// Removes `folder` from the folders that the subscription `id` adds
    /// its new files to, and saves the destination. The remove button of a
    /// folder's chip in the rail calls it.
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

    /// Removes `tag` from the tags that the subscription `id` gives its new
    /// files, and saves the destination. The remove button of a tag's chip
    /// in the rail calls it.
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

    /// Returns the point that a picker is opened at for the rail's control
    /// whose position and size are `control`. The point's x is the rail's
    /// left edge and its y is the control's top. The picker opens with its
    /// right edge 12 px to the left of that x and its top at that y, in the
    /// way the inspector's pickers open to the left of the inspector. It is
    /// moved from there only as far as is needed to keep it 12 px inside
    /// the window.
    fn beside_rail(&self, control: Bounds<Pixels>) -> Point<f32> {
        point(
            f32::from(self.rail_bounds.get().left()),
            f32::from(control.top()),
        )
    }

    /// Opens the folder picker to the left of the rail, with its top at the
    /// top of the rail's control that adds a folder. The folders that the
    /// subscription `id` already adds its new files to are checked in the
    /// picker.
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
        let anchor = self.beside_rail(self.add_folder.get());
        self.picking = Some(Picking::Folders(id));
        self.folder_picker.update(cx, |picker, cx| {
            picker.open(Some(anchor), folders, window, cx)
        });
    }

    /// Opens the tag picker to the left of the rail, with its top at the
    /// top of the rail's control that adds a tag. The tags that the
    /// subscription `id` already gives its new files are checked in the
    /// picker.
    fn pick_tags(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.subscription(id) else {
            return;
        };
        let records = entry
            .automatic_tags
            .iter()
            .map(|tag| crate::tags::record(tag))
            .collect();
        let anchor = self.beside_rail(self.add_tag.get());
        self.picking = Some(Picking::Tags(id));
        self.tag_picker.update(cx, |picker, cx| {
            picker.open(Some(anchor), records, window, cx)
        });
    }

    /// Called when a folder is checked (`add` is true) or unchecked in the
    /// folder picker. The folder is added to the end of, or removed from,
    /// the folders that the subscription the picker was opened for adds its
    /// new files to, and the destination is saved.
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

    /// Called when a tag is checked (`add` is true) or unchecked in the tag
    /// picker. The tag is added to the end of, or removed from, the tags
    /// that the subscription the picker was opened for gives its new files,
    /// and the destination is saved.
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

    // The functions below add a source and change one source.

    /// Sets the site that is chosen in the row that adds a source, and
    /// changes the placeholder of the row's text field to that site's
    /// example query (`placeholder`).
    fn choose_add_site(&mut self, site: &'static str, cx: &mut Context<Self>) {
        self.add_site = site;
        self.add_query.update(cx, |input, cx| {
            input.set_placeholder(placeholder(site));
            cx.notify();
        });
        cx.notify();
    }

    /// Adds the query typed in the row that adds a source to the open
    /// subscription, as a source of the chosen site. The Add button and
    /// Enter in the text field call it. It does nothing when the field is
    /// empty or holds only spaces, when no site is chosen, when no
    /// subscription's page is shown, or while `busy()` is true.
    ///
    /// Whether the new source groups the media of one post is taken from
    /// the setting `subscription_default_group_posts`. The field is emptied
    /// when the source has been added. When the application answers with an
    /// error, the field keeps its text, so that the user can correct it and
    /// try again.
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
        self.act(vec![requests::add_query(id, &source)], cx, |this, answer, cx| {
            if answer.is_ok() {
                this.add_query.update(cx, |input, cx| input.clear(cx));
            }
        });
    }

    /// Starts a run of the source `query` alone
    /// (`subscriptions.queries.run`). The run downloads files.
    fn run_query(&mut self, query: i64, cx: &mut Context<Self>) {
        self.act(vec![requests::run_query(query)], cx, |_, _, _| {});
    }

    /// With `paused` true, puts the source `query` on hold, so that future
    /// runs of its subscription leave it out. With `paused` false, lets the
    /// source run again (`subscriptions.queries.pause`).
    fn pause_query(&mut self, query: i64, paused: bool, cx: &mut Context<Self>) {
        self.act(vec![requests::pause_query(query, paused)], cx, |_, _, _| {});
    }

    /// Sets whether the media of one post from the source `query` become
    /// one group in the library (`subscriptions.queries.grouping`).
    fn group_query(&mut self, query: i64, group_posts: bool, cx: &mut Context<Self>) {
        self.act(
            vec![requests::group_query(query, group_posts)],
            cx,
            |_, _, _| {},
        );
    }

    // The function below deletes the selected subscriptions.

    /// Opens the dialog that asks whether to delete the selected
    /// subscriptions. They are deleted when the user confirms. The Delete
    /// shortcut (`file.delete` in the shortcut registry, Mod+Backspace by
    /// default) calls it. It does nothing while a subscription's page is
    /// shown or no tile is selected.
    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let chosen = self.chosen();
        if self.place_seen.is_some() || chosen.is_empty() {
            return;
        }
        self.confirm_delete_all(chosen, cx);
    }
}

/// Returns the placeholder of the text field in the row that adds a source,
/// for the site with the id `site`: "e.g. " followed by the site's example
/// query. When there is no site with that id, which is the case when no
/// site is offered at all, the placeholder is "query".
fn placeholder(site: &str) -> SharedString {
    match format::site(site) {
        Some(site) => crate::tf!("e.g. {value0}", site.example_query).into(),
        None => t("query").into(),
    }
}

/// Reads the answer to `auth.credentials.list` or `auth.health.list`, which
/// is an array with one record for each site. From each record it takes
/// the site's id (`site_id`), the status of the last check of the site's
/// login (`status`) and the error of that check (`last_error`). A record
/// without a `site_id` is left out. An answer that is not an array gives
/// an empty list.
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
        // The tiles' layout and the columns of a page's tables depend on
        // the screen's width. The tiles or the page are therefore built
        // for the width of their own area in the frame that lays that area
        // out (`ui::sized_view`).
        let body = ui::sized_view(cx.weak_entity(), |this, bounds, window, cx| {
            this.width = f32::from(bounds.size.width);
            let body = match (&this.list, this.place_seen) {
                // Nothing is drawn until the list of subscriptions has
                // been read for the first time.
                (None, _) => div().into_any_element(),
                (Some(_), None) => this.home(window, cx).into_any_element(),
                (Some(_), Some(_)) => match this.opened().cloned() {
                    Some(subscription) => {
                        this.workspace(&subscription, window, cx).into_any_element()
                    }
                    // The open subscription is not in the list, which
                    // happens when it has been deleted. The empty-state
                    // panel is drawn where its page would be.
                    None => home::empty_state(cx).into_any_element(),
                },
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(body)
                .into_any_element()
        })
        .flex_1()
        .min_h_0()
        .w_full();
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
            .key_context(t("Subscriptions"))
            // A press of the left mouse button anywhere on the screen gives
            // the screen's root element the keyboard focus, unless the root
            // element or an element inside it already has the focus. The
            // screen's key bindings work only while the focus is there.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    if !this.focus.contains_focused(window, cx) {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            // Enter opens the selected subscription when the tiles are
            // shown and exactly one tile is selected. Otherwise the screen
            // does not handle the key, and the action is passed on to the
            // elements that contain the screen.
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
            // Escape clears the selection when the tiles are shown and at
            // least one tile is selected. Otherwise the action is passed
            // on.
            .on_action(cx.listener(|this, _: &Deselect, _, cx| {
                if this.selected.is_empty() || this.place_seen.is_some() {
                    cx.propagate();
                } else {
                    this.selected.clear();
                    cx.notify();
                }
            }))
            // The Delete shortcut asks whether to delete the selected
            // subscriptions when the tiles are shown and at least one tile
            // is selected. Otherwise the action is passed on.
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

/// Returns an element that draws nothing and records where its parent is.
/// The element covers its parent, which must be positioned (`relative`).
/// Each time a frame is drawn, the element stores its own position and
/// size, in window coordinates, in `cell`. A click handler reads `cell`
/// later, to open a menu or a picker next to the parent.
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
    fn controls_send_the_commands_the_earlier_versions_screen_sent() {
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
        // The arguments of each command can be read as the input type that
        // the application reads for that command in `core/src/ipc.rs`.
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
