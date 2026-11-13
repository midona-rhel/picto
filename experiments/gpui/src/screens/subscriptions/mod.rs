//! The Subscriptions screen (`SubscriptionsScreen.tsx`): a grid of the
//! library's subscriptions, and one subscription's workspace when a card is
//! opened. Opening a subscription is a place of the screen, so Back returns
//! to the grid and the scope title names it.
mod data;
mod detail;
mod format;
mod home;

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Session};
use crate::ui::menu::{self, Menu};
use crate::ui::text_input::{Committed, TextInput};
use gpui::*;
use std::{
    cell::Cell,
    collections::HashSet,
    rc::Rc,
    time::{Duration, Instant},
};

actions!(subscriptions, [Open, Deselect]);

/// The screen's own keys, over the browser's: Enter opens the selected
/// subscription and Escape clears the selection.
pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Open, Some("Subscriptions")),
        KeyBinding::new("escape", Deselect, Some("Subscriptions")),
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
    pub fn request(self, subscription_id: i64) -> (&'static str, serde_json::Value) {
        let id = serde_json::json!({ "subscription_id": subscription_id });
        match self {
            Command::Run => ("subscriptions.run", id),
            Command::Pause => ("subscriptions.run.pause", id),
            Command::Resume => ("subscriptions.run.resume", id),
            Command::Stop => ("subscriptions.cancel", id),
            Command::Hold(paused) => (
                "subscriptions.hold",
                serde_json::json!({ "subscription_id": subscription_id, "paused": paused }),
            ),
        }
    }

    /// Whether it may start a run, after which progress is followed at once.
    fn starts(self) -> bool {
        matches!(self, Command::Run | Command::Resume)
    }
}

/// The tabs of a subscription's data plane.
const TABS: [&str; 3] = ["Sources", "History", "Problems"];
const SOURCES: usize = 0;
const HISTORY: usize = 1;

pub struct SubscriptionsScreen {
    session: Entity<Session>,
    /// Covers decode into a cache of their own, so scrolling the grid never
    /// evicts an image this screen still shows.
    covers: Entity<Thumbnails>,
    focus: FocusHandle,
    /// Every subscription, once the first read has arrived. A newer read
    /// replaces it only when it arrives, so nothing blanks in between.
    list: Option<Vec<data::Subscription>>,
    error: Option<String>,
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
    /// The name field of a rename in place.
    rename: Entity<TextInput>,
    /// The subscription whose name is being edited.
    renaming: Option<i64>,
    /// The open context menu, which stays while it fades out.
    menu: Option<Entity<Menu>>,
    _menu: Option<Subscription>,
    /// The subscriptions the open menu acts on.
    menu_targets: Vec<i64>,
    /// Whether the screen showed at the last navigation, to read the list
    /// again each time it is opened.
    shown: bool,
    /// The place showing at the last navigation.
    place_seen: Option<i64>,
    /// The grid's content width and the data plane's width, which the
    /// container-relative CSS sizes are measured against.
    home_width: Rc<Cell<f32>>,
    plane_width: Rc<Cell<f32>>,
    /// Where the rail's "More actions" button is, to open its menu below it.
    overflow: Rc<Cell<Bounds<Pixels>>>,
    /// Where the name field of a rename is; a press anywhere else ends it.
    rename_bounds: Rc<Cell<Bounds<Pixels>>>,
    home_scroll: ScrollHandle,
    rail_scroll: ScrollHandle,
    table_scroll: ScrollHandle,
    /// Subscriptions with a command on its way; their controls wait.
    acting: HashSet<i64>,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rename = cx.new(|cx| TextInput::rename(crate::theme::TEXT_MD, window, cx));
        let rename_focus = rename.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if *change == Change::Navigation {
                    this.navigated(cx);
                }
            }),
            // The field is keyed by the subscription it renames, so a commit
            // that arrives after the field closed still knows its target.
            cx.subscribe(&rename, |this, _, Committed(id, name), cx| {
                this.commit_rename(*id as i64, name.clone(), cx);
            }),
            cx.on_blur(&rename_focus, window, |this, window, cx| {
                this.renaming = None;
                window.focus(&this.focus, cx);
                cx.notify();
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
        ];
        let mut screen = Self {
            session,
            covers: cx.new(Thumbnails::new),
            focus: cx.focus_handle(),
            list: None,
            error: None,
            generation: 0,
            selected: HashSet::new(),
            anchor: None,
            detail: None,
            detail_generation: 0,
            loading_more: false,
            tab: SOURCES,
            rename,
            renaming: None,
            menu: None,
            _menu: None,
            menu_targets: Vec::new(),
            shown: false,
            place_seen: None,
            home_width: Rc::new(Cell::new(0.)),
            plane_width: Rc::new(Cell::new(0.)),
            overflow: Rc::new(Cell::new(Bounds::default())),
            rename_bounds: Rc::new(Cell::new(Bounds::default())),
            home_scroll: ScrollHandle::new(),
            rail_scroll: ScrollHandle::new(),
            table_scroll: ScrollHandle::new(),
            acting: HashSet::new(),
            grace_until: None,
            poll: None,
            invalidation: None,
            _subscriptions: subscriptions,
        };
        // Read the list at once, so it is there before the screen is opened.
        screen.load(cx);
        screen
    }

    /// Reads every subscription again. What shows stays until the answer
    /// arrives.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let blobs = self.session.read(cx).blobs().to_path_buf();
        let answer = self.session.read(cx).run(move |library| {
            data::subscriptions(library, &blobs).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(list) => {
                        this.error = None;
                        // A subscription that is gone is no longer selected.
                        this.selected
                            .retain(|id| list.iter().any(|entry| entry.id == *id));
                        this.list = Some(list);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.name_place(cx);
                this.follow_runs(cx);
                cx.notify();
            });
        })
        .detach();
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
            let (name, args) = command.request(id);
            answers.push((id, self.session.read(cx).dispatch(name, args)));
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
                        // TODO: show it as an error toast, once the
                        // prototype has toasts.
                        eprintln!("picto: subscription {id}: {error}");
                        this.error = Some(error);
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
            let Ok(Ok(detail)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.detail_generation == generation {
                    this.detail = Some(detail);
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
                if let (Ok(Ok(page)), Some(detail)) = (result, this.detail.as_mut()) {
                    detail.issues.items.extend(page.items);
                    detail.issues.next = page.next;
                    detail.issues.total = page.total;
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

    // ── Rename in place ─────────────────────────────────────────

    /// Edits a name in place, in a field the size of the name it replaces.
    fn start_rename(&mut self, id: i64, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .list
            .as_ref()
            .and_then(|list| list.iter().find(|entry| entry.id == id))
            .map(|entry| entry.name.clone())
        else {
            return;
        };
        self.renaming = Some(id);
        self.rename.update(cx, |input, cx| {
            input.set_rename_size(size, cx);
            input.set_field(id as u64, name, cx);
            input.select_everything(cx);
        });
        window.focus(&self.rename.focus_handle(cx), cx);
        cx.notify();
    }

    /// Shows the new name at once and writes it through the library.
    fn commit_rename(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        if let Some(entry) = self
            .list
            .as_mut()
            .and_then(|list| list.iter_mut().find(|entry| entry.id == id))
        {
            entry.name = name.clone();
        }
        self.name_place(cx);
        let answer = self.session.read(cx).run(move |library| {
            data::rename(library, id, &name).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Err(error)) = result {
                    this.error = Some(error);
                }
                // Read back what the library holds now.
                this.load(cx);
            });
        })
        .detach();
        cx.notify();
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

    // ── Context menu ────────────────────────────────────────────

    /// Opens the menu for `targets` at `position`: the one subscription's
    /// menu, or the bulk menu for several (`subscriptionsContextMenu.tsx`).
    /// Rows this prototype cannot perform are shown and not offered.
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
                    // TODO: the cover dialog, to frame a downloaded image.
                    menu::item("cover", "Set Cover Photo…")
                        .icon("menu-photo-edit")
                        .disabled(true)
                        .into(),
                    // TODO: a submenu of Manual, Daily, Weekly and Monthly.
                    menu::item("schedule", "Schedule")
                        .icon("menu-clock")
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    // TODO: confirm, then clear the sync history.
                    menu::item("reset", "Reset Sync History…")
                        .icon("menu-refresh")
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    // TODO: confirm, then delete the subscription; its files
                    // stay in the library.
                    menu::item("delete", "Delete Subscription…")
                        .icon("menu-trash")
                        .danger(true)
                        .disabled(true)
                        .into(),
                ]
            }
            several => {
                let total = several.len();
                // TODO: Schedule and Delete for several at once.
                vec![
                    menu::item("run-all", format!("Run {total} Now"))
                        .icon("menu-player-play")
                        .into(),
                    menu::item("hold-all", format!("Hold {total}"))
                        .icon("menu-hand-stop")
                        .into(),
                    menu::item("release-all", format!("Release Hold for {total}"))
                        .icon("menu-player-play")
                        .into(),
                    menu::item("schedule", "Schedule")
                        .icon("menu-clock")
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    menu::item("delete", format!("Delete {total}…"))
                        .icon("menu-trash")
                        .danger(true)
                        .disabled(true)
                        .into(),
                ]
            }
        };
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self._menu =
            Some(
                cx.subscribe_in(&menu, window, |this, _, event: &menu::Event, window, cx| {
                    let menu::Event::Chosen(action) = event;
                    let targets = this.menu_targets.clone();
                    match (*action, targets.as_slice()) {
                        ("run", [id]) => this.run_control(*id, cx),
                        ("hold", [id]) => this.hold_control(*id, cx),
                        // `handleBulk*`: each subscription on its own.
                        ("run-all", _) => this.perform(targets, |_| Command::Run, cx),
                        ("hold-all", _) => this.perform(targets, |_| Command::Hold(true), cx),
                        ("release-all", _) => this.perform(targets, |_| Command::Hold(false), cx),
                        _ => {}
                    }
                    if *action == "rename"
                        && let [id] = this.menu_targets.as_slice()
                    {
                        let id = *id;
                        // The rail shows the name larger than a card does.
                        let size = if this.place_seen.is_some() {
                            crate::theme::TEXT_XL
                        } else {
                            crate::theme::TEXT_MD
                        };
                        this.start_rename(id, size, window, cx);
                    }
                }),
            );
        self.menu_targets = targets;
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(menu);
        cx.notify();
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
        let targets = order
            .iter()
            .copied()
            .filter(|id| self.selected.contains(id))
            .collect();
        self.open_menu(targets, position, window, cx);
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = &self.menu {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
    }

    /// The open menu, laid over the whole window as the shell lays the
    /// grid's, until its fade-out is over.
    fn menu_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(menu) = self.menu.take_if(|menu| menu.read(cx).finished()) {
            self._menu = None;
            // Unless its action moved the keyboard, as Rename does. A menu
            // shown in pop-up windows can outlive this by a moment, still
            // holding the keyboard.
            if window.focused(cx).is_none() || menu.focus_handle(cx).is_focused(window) {
                window.focus(&self.focus, cx);
            }
        }
        let menu = self.menu.clone()?;
        let viewport = window.viewport_size();
        Some(
            deferred(
                anchored()
                    .position(point(px(0.), px(0.)))
                    .position_mode(AnchoredPositionMode::Window)
                    .child(div().w(viewport.width).h(viewport.height).child(menu)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
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
        let menu = self.menu_layer(window, cx);
        div()
            .id("subscriptions")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .text_color(theme.text())
            .track_focus(&self.focus)
            .key_context("Subscriptions")
            // A press takes the keyboard to the screen, which ends a rename
            // unless it lands in the name field.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if this.renaming.is_none()
                        || !this.rename_bounds.get().contains(&event.position)
                    {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Open, _, cx| {
                let order = this.card_order();
                let chosen = order
                    .iter()
                    .copied()
                    .filter(|id| this.selected.contains(id))
                    .collect::<Vec<_>>();
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
            .child(body)
            .children(menu)
    }
}

impl SubscriptionsScreen {
    /// The name field of a rename in place, which notes where it is.
    fn rename_field(&self, id: &'static str) -> Div {
        let bounds = self.rename_bounds.clone();
        div()
            .relative()
            .w_full()
            .child(crate::ui::field(
                id,
                &self.rename,
                crate::ui::field::Look::Rename,
            ))
            .child(
                canvas(move |at, _, _| bounds.set(at), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
    }
}

/// Keeps a measured width in `cell` and repaints `view` when it changes.
fn measure(cell: Rc<Cell<f32>>, window: &Window) -> impl IntoElement {
    let view = window.current_view();
    canvas(
        move |bounds, _, cx| {
            let width = f32::from(bounds.size.width);
            if (cell.replace(width) - width).abs() > 0.5 {
                cx.notify(view);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn controls_send_the_commands_the_electron_screen_sends() {
        assert_eq!(Command::primary(false, false), Command::Run);
        assert_eq!(Command::primary(true, true), Command::Pause);
        assert_eq!(Command::primary(true, false), Command::Resume);
        assert_eq!(Command::hold(true, false), Command::Stop);
        assert_eq!(Command::hold(false, false), Command::Hold(true));
        assert_eq!(Command::hold(false, true), Command::Hold(false));
        assert_eq!(
            Command::Run.request(4),
            (
                "subscriptions.run",
                serde_json::json!({ "subscription_id": 4 })
            )
        );
        assert_eq!(Command::Pause.request(4).0, "subscriptions.run.pause");
        assert_eq!(Command::Resume.request(4).0, "subscriptions.run.resume");
        assert_eq!(Command::Stop.request(4).0, "subscriptions.cancel");
        assert_eq!(
            Command::Hold(false).request(4),
            (
                "subscriptions.hold",
                serde_json::json!({ "subscription_id": 4, "paused": false })
            )
        );
    }
}
