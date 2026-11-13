//! What is being browsed: the destination, its results, the selection and the
//! viewer. One model that every view reads, so a view repaints only for the
//! kind of change it shows and no view owns another's state.
use crate::application::{self, Application, Event};
use crate::backend::{self, Arrived, Fetch, Navigation, Reply, Request};
use crate::grid::selection::{End, Selection};
use crate::grid::{filters::Filters, nav, prefs::ViewPrefs, window};
use crate::motion;
use gpui::*;
use picto_library::predicate::FilterExpr;
use picto_library::query::{RootQuery, RootSummary, WindowRequest};
use picto_library::{RootDetails, RootId, TagRecord};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    time::{Duration, Instant},
};

/// Changing destination, as `WorkspaceSurface.tsx` stages it: the current grid
/// fades out, the surface stays hidden until the destination is ready, then
/// the destination fades in.
#[derive(Clone, Copy)]
pub enum Transition {
    Idle,
    FadingOut(Instant),
    Waiting,
    FadingIn(Instant),
}

impl Transition {
    /// Opacity of the grid surface and the scope title.
    pub fn opacity(&self) -> f32 {
        let eased = |started: &Instant| {
            motion::EASE_DEFAULT.ease(motion::progress(started.elapsed(), motion::SCOPE_FADE))
        };
        match self {
            Transition::Idle => 1.,
            Transition::FadingOut(started) => 1. - eased(started),
            Transition::Waiting => 0.,
            Transition::FadingIn(started) => eased(started),
        }
    }

    pub fn animating(&self) -> bool {
        matches!(self, Transition::FadingOut(_) | Transition::FadingIn(_))
    }
}

/// What changed, so each view can ignore what it does not show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// The destination list, the chosen destination, or its transition.
    Navigation,
    /// The items, their count, the view preferences or the zoom.
    Results,
    Selection,
    Details,
    /// An edit of the selected items finished: a summary of them is read
    /// again.
    Edited,
    /// The viewer opened, closed, or moved to another member.
    Viewer,
    /// Background work the sidebar shows: whether subscriptions are running
    /// or held globally.
    Activity,
}

/// Whether subscriptions are running, as the sidebar's Subscriptions row
/// shows it (`runningSubscriptionIds`, `globalPaused`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SubscriptionActivity {
    pub running: bool,
    pub paused: bool,
    /// A pause or resume of everything is on its way.
    pub busy: bool,
}

impl SubscriptionActivity {
    /// Reads `subscriptions.list` as `getSubscriptionOverview` does: running
    /// means an active run that is pending or running.
    pub fn from_list(list: &serde_json::Value) -> Self {
        let running = list
            .get("subscriptions")
            .and_then(|list| list.as_array())
            .is_some_and(|list| {
                list.iter().any(|subscription| {
                    !subscription["active_run_id"].is_null()
                        && matches!(subscription["status"].as_str(), Some("pending" | "running"))
                })
            });
        Self {
            running,
            paused: list["global_paused"].as_bool().unwrap_or(false),
            busy: false,
        }
    }

    /// The row shows its activity button.
    pub fn shown(&self) -> bool {
        self.running || self.paused
    }
}

/// `subscriptionsSettle.ts`: progress is read again this often while a run
/// is going.
pub const SUBSCRIPTION_POLL: Duration = Duration::from_millis(1500);
/// How often the session takes in what the library's thread has sent.
const RECEIVE_INTERVAL: Duration = Duration::from_millis(30);
/// `VIEW_PREFS_SAVE_DEBOUNCE_MS`: a view change is saved once it has rested
/// this long.
const VIEW_SAVE_DELAY: Duration = Duration::from_millis(500);
/// `gridController.scheduleRefresh`: a refresh after background changes
/// waits at least this long, and at most a second, three times as long as
/// the last refresh took, counted from when it finished.
const REFRESH_MIN: Duration = Duration::from_millis(40);
const REFRESH_MAX: Duration = Duration::from_millis(1000);
/// While the grid cannot be refreshed (a drag, a scope change, a page on
/// its way), a wanted refresh looks again after this long.
const REFRESH_RETRY: Duration = Duration::from_millis(100);

/// One step of Back and Forward.
#[derive(Clone, Copy, PartialEq)]
struct Visit {
    scope: usize,
    /// The viewer, opened on an item of this destination.
    viewing: bool,
    /// A place inside a screen, such as the subscription it has open.
    place: Option<i64>,
}

/// A destination's grid as it was left: its window, where it lay, and
/// what was selected.
struct SavedGrid {
    items: Vec<RootSummary>,
    start: usize,
    head: Vec<RootSummary>,
    total: u64,
    total_size_bytes: u128,
    prefs: ViewPrefs,
    tile: f32,
    selection: Selection,
    focus: Option<Focused>,
    anchor: Option<End>,
    ran: Option<RootQuery>,
    scroll: f32,
    density: Option<Density>,
    window_top: f32,
    /// The search and filters the results were for.
    search_text: String,
    filters: Filters,
}

/// The item the inspector and viewer show, and where it is in the
/// destination; it stays known when the grid's window moves away from it.
#[derive(Clone)]
struct Focused {
    ordinal: usize,
    item: RootSummary,
}

/// The height per item the grid measured for a layout geometry (width,
/// zoom, spacing, mode, text rows), which the scrollable height of a
/// destination is estimated from.
pub type Density = ((u32, u32, u32, crate::grid::layout::ViewMode, u32), f32);

pub struct Session {
    sender: Sender<Request>,
    receiver: Receiver<Reply>,
    /// The application layer: commands and its events.
    application: Arc<Application>,
    events: Receiver<Event>,
    pub subscriptions: SubscriptionActivity,
    /// The subscription list is being read for the sidebar's activity.
    activity_reading: bool,
    /// Reads progress again while a run is going.
    activity_poll: Option<Task<()>>,
    /// The sidebar's destinations are being read again; `true` inside when
    /// another change arrived meanwhile.
    navigation_reading: Option<bool>,
    pub nav: Vec<Navigation>,
    pub tags: HashMap<u32, TagRecord>,
    /// The destination chosen in the sidebar.
    pub scope: usize,
    /// The destination shown in the grid and title; trails `scope` while the
    /// previous one fades out.
    pub displayed: usize,
    /// Visited places, for Back and Forward: each destination's grid, and
    /// each item opened from one.
    history: Vec<Visit>,
    history_index: usize,
    /// What each destination's grid held when it was last left, so going
    /// back shows it where it was.
    saved: HashMap<usize, SavedGrid>,
    /// A saved grid to show once the surface has faded out.
    pending_restore: Option<SavedGrid>,
    /// How far the grid is scrolled, which the grid reports as it paints.
    pub grid_scroll: f32,
    /// Where the grid should scroll to when it lays out the restored results.
    pub restore_scroll: Option<f32>,
    pub transition: Transition,
    /// A destination's first window that arrived before the fade-out
    /// finished.
    pending_page: Option<Box<Arrived>>,
    /// The window of the results that is held: at most `window::WINDOW`
    /// items, the first at ordinal `start` of the destination's order.
    pub items: Vec<RootSummary>,
    pub start: usize,
    /// The destination's first items, wherever the window is, for the
    /// inspector's preview of it.
    pub head: Vec<RootSummary>,
    /// Counts each time the results are another destination's, or read
    /// afresh, rather than another window of the same.
    pub results_epoch: u64,
    /// Counts each time the window's items change: another window, or the
    /// same read again in place.
    pub window_epoch: u64,
    /// Every item of the results, not only the window's.
    pub total: u64,
    /// The size of every item in the results, not only the loaded ones.
    pub total_size_bytes: u128,
    /// The latest window asked for (`requestWindowAt`): its number, and
    /// the one on its way and the one waiting behind it, each with its
    /// start. Only the latest is shown when it arrives.
    window_request: u64,
    window_active: Option<(u64, usize)>,
    window_queued: Option<(u64, usize)>,
    /// An item moved to by the keyboard or a viewer outside the window: it
    /// is selected (with `true`, as a range) once its window arrives.
    pending_focus: Option<(usize, bool)>,
    /// An ordinal the grid should bring into view once it is laid out.
    pub reveal: Option<usize>,
    /// The height per item the grid measured, kept with the results so
    /// the scrollable height stays the same as the window moves.
    pub density: Option<Density>,
    /// Where the grid's window lay, as the grid last placed it, and where a
    /// restored window should lie again.
    pub window_top: f32,
    pub restore_window_top: Option<f32>,
    pub loading: bool,
    pub error: Option<String>,
    generation: u64,
    pub prefs: ViewPrefs,
    prefs_scope: Option<usize>,
    /// The zoom: the target tile size.
    pub tile: f32,
    /// The selected items, by id, or query-wide: everything, or spans of
    /// the order. Edits address query-wide selections through the query
    /// (`target_for`), as production's do.
    pub selection: Selection,
    /// The query the shown results were read with.
    ran: Option<RootQuery>,
    /// The selected item the inspector and viewer show: the one last chosen.
    focus: Option<Focused>,
    /// Where a shift-click or shift-move range starts.
    anchor: Option<End>,
    pub details: Option<RootDetails>,
    pub viewer: bool,
    /// Which member of a collection the viewer shows.
    pub member: usize,
    /// A bundled sample shown in the viewer instead of a library item.
    pub demo: Option<(&'static str, &'static str)>,
    search_text: String,
    pub filter: Option<FilterExpr>,
    /// The filter bar's filters, which `filter` is built from.
    pub filters: Filters,
    /// Whether the filter bar shows under the titlebar.
    pub filter_bar_open: bool,
    /// Whether the filters stay when another destination opens.
    pub filters_locked: bool,
    /// A view change, and the last query sent before it was saved: pages of
    /// that query or older bring the preferences from before it.
    view_patch: Option<(u64, serde_json::Value)>,
    blobs: PathBuf,
    /// The disposable copy the application opened.
    root: PathBuf,
    pub library_cover: Option<PathBuf>,
    /// A disposable folder that holds the demo media and is the scratch
    /// space the library cover's crops are written to.
    pub scratch: tempfile::TempDir,
    /// What the open place of a screen is called, for the scope title.
    pub place_label: Option<SharedString>,
    /// Folders items were added to, newest first, for the folder picker's
    /// Recent view and the grid menu's "Add to “…”" (`useRecentFolders`).
    /// Production keeps them per library in the renderer's localStorage;
    /// here they are kept in the app's state under the library the copy was
    /// made from.
    pub recent_folders: Vec<picto_library::FolderId>,
    /// Tags put on items, newest first, for the Add Tags dialog's Recent
    /// view (`picto-recent-tags`), kept in the app's state.
    pub recent_tags: Vec<String>,
    /// The recents changed since they were last kept. They are kept on the
    /// next poll, which has the app to write to.
    recents_changed: bool,
    /// A zoom change waiting to be saved to its destination's view: the
    /// destination's key, the tile size, and the timer.
    tile_save: Option<(String, f32, Task<()>)>,
    /// A refresh in place is on its way: what the grid shows stays until it
    /// arrives, an empty state included.
    pub refreshing: bool,
    /// The grid has the pointer for a gesture (a press, a rubber band, a
    /// drag) that a refresh would cut short. The grid says so as it paints.
    pub pointer_busy: bool,
    /// Background changes asked for the shown results to be read again
    /// (`requestReconcile`), and the timer that will.
    refresh_wanted: bool,
    refresh_timer: Option<Task<()>>,
    /// When the last refresh in place started and finished, for spacing the
    /// next one.
    refresh_started: Option<Instant>,
    refresh_took: Duration,
    refresh_finished: Option<Instant>,
}

impl EventEmitter<Change> for Session {}
impl EventEmitter<Event> for Session {}

impl Session {
    pub fn new(
        root: PathBuf,
        application: Arc<Application>,
        events: Receiver<Event>,
        cx: &mut Context<Self>,
    ) -> Self {
        let blobs = root.join("blobs");
        let (recent_folders, recent_tags) = {
            let kept = crate::state::get(cx);
            let folders = kept
                .recent_folders
                .get(&crate::state::library_key(&root))
                .map(|ids| ids.iter().map(|id| picto_library::FolderId(*id)).collect())
                .unwrap_or_default();
            (folders, kept.recent_tags.clone())
        };
        let (sender, receiver) = backend::start(application.clone());
        cx.spawn(async move |session, cx| {
            loop {
                cx.background_executor().timer(RECEIVE_INTERVAL).await;
                if session
                    .update(cx, |this: &mut Self, cx| this.receive(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let scratch = crate::demo::fixtures().expect("create disposable media fixtures");
        let library_cover = blobs
            .parent()
            .and_then(|library| crate::cover::prepare(library, scratch.path()));
        Self {
            sender,
            receiver,
            application,
            events,
            subscriptions: SubscriptionActivity::default(),
            activity_reading: false,
            activity_poll: None,
            navigation_reading: None,
            nav: Vec::new(),
            tags: HashMap::new(),
            scope: 0,
            displayed: 0,
            history: vec![Visit {
                scope: 0,
                viewing: false,
                place: None,
            }],
            history_index: 0,
            saved: HashMap::new(),
            pending_restore: None,
            grid_scroll: 0.,
            restore_scroll: None,
            transition: Transition::Idle,
            pending_page: None,
            ran: None,
            items: Vec::new(),
            start: 0,
            head: Vec::new(),
            results_epoch: 0,
            window_epoch: 0,
            total: 0,
            total_size_bytes: 0,
            window_request: 0,
            window_active: None,
            window_queued: None,
            pending_focus: None,
            reveal: None,
            density: None,
            window_top: 0.,
            restore_window_top: None,
            loading: true,
            error: None,
            generation: 0,
            prefs: ViewPrefs::default(),
            prefs_scope: None,
            tile: 200.,
            selection: Selection::default(),
            focus: None,
            anchor: None,
            details: None,
            viewer: false,
            member: 0,
            demo: None,
            search_text: String::new(),
            filter: None,
            filters: Filters::default(),
            filter_bar_open: false,
            filters_locked: false,
            view_patch: None,
            blobs,
            root,
            library_cover,
            scratch,
            place_label: None,
            refreshing: false,
            pointer_busy: false,
            refresh_wanted: false,
            refresh_timer: None,
            refresh_started: None,
            refresh_took: Duration::ZERO,
            refresh_finished: None,
            recent_folders,
            recent_tags,
            recents_changed: false,
            tile_save: None,
        }
    }

    /// Whether the open library is still a marked disposable copy, which
    /// edits that cannot be undone require.
    pub fn disposable(&self) -> bool {
        crate::application::guard(&self.root).is_ok()
    }

    /// The library's blob store.
    pub fn blobs(&self) -> &std::path::Path {
        &self.blobs
    }

    /// The place open inside the current screen, if any.
    pub fn place(&self) -> Option<i64> {
        self.history
            .get(self.history_index)
            .filter(|visit| visit.scope == self.scope)
            .and_then(|visit| visit.place)
    }

    /// Opens a place inside the current screen, or the screen's own top
    /// with `None`, as a step of Back and Forward.
    pub fn open_place(&mut self, place: Option<i64>, cx: &mut Context<Self>) {
        if self.place() == place {
            return;
        }
        self.record(Visit {
            scope: self.scope,
            viewing: false,
            place,
        });
        cx.emit(Change::Navigation);
    }

    /// Names the open place in the scope title, or clears the name.
    pub fn set_place_label(&mut self, label: Option<SharedString>, cx: &mut Context<Self>) {
        if self.place_label != label {
            self.place_label = label;
            cx.emit(Change::Navigation);
        }
    }

    /// Where the library keeps an original: beside its thumbnail's folder,
    /// under its content hash and its type's extension.
    pub fn original(&self, hash: &str, mime: &str) -> PathBuf {
        if hash.len() < 4 {
            return PathBuf::new();
        }
        self.blobs
            .join("f")
            .join(&hash[..2])
            .join(&hash[2..4])
            .join(format!("{hash}.{}", crate::grid::format::extension(mime)))
    }

    pub fn thumbnail(&self, hash: &str) -> PathBuf {
        if hash.len() < 4 {
            return PathBuf::new();
        }
        self.blobs
            .join("t")
            .join(&hash[..2])
            .join(&hash[2..4])
            .join(format!("{hash}.jpg"))
    }

    pub fn selected_item(&self) -> Option<&RootSummary> {
        self.focus.as_ref().map(|focused| &focused.item)
    }

    /// Where the item shown in the inspector and viewer is in the
    /// destination's order.
    pub fn focus_ordinal(&self) -> Option<usize> {
        self.focus.as_ref().map(|focused| focused.ordinal)
    }

    /// The item at `ordinal` of the destination, if the window holds it.
    pub fn item_at(&self, ordinal: usize) -> Option<&RootSummary> {
        self.items.get(ordinal.checked_sub(self.start)?)
    }

    fn end_at(&self, ordinal: usize) -> Option<End> {
        self.item_at(ordinal).map(|item| End {
            id: item.root_id,
            ordinal,
        })
    }

    /// Whether the item at `ordinal`, held in the window, is selected.
    pub fn is_selected(&self, ordinal: usize) -> bool {
        self.item_at(ordinal)
            .is_some_and(|item| self.selection.contains(item.root_id, ordinal))
    }

    /// The selected items the window holds, in the grid's order.
    pub fn selected_loaded(&self) -> Vec<&RootSummary> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        self.items
            .iter()
            .enumerate()
            .filter(|(offset, item)| self.selection.contains(item.root_id, self.start + offset))
            .map(|(_, item)| item)
            .collect()
    }

    /// The selected items by id, in the grid's order: every item chosen one
    /// by one, loaded or not; of a query-wide selection, the ones loaded
    /// (an edit of them addresses the whole selection, see `target_for`).
    pub fn selected_ids(&self) -> Vec<RootId> {
        if self.selection.query_wide() {
            self.selected_loaded()
                .into_iter()
                .map(|item| item.root_id)
                .collect()
        } else {
            self.selection.chosen()
        }
    }

    /// Whether the destination has items after the window.
    pub fn has_following(&self) -> bool {
        self.start + self.items.len() < self.total as usize
    }

    /// A window of the results is on its way.
    pub fn window_loading(&self) -> bool {
        self.window_active.is_some() || self.window_queued.is_some()
    }

    /// The details of the selected item, once they have arrived. `details`
    /// itself keeps the previous item's until then, so the inspector never
    /// shows an empty frame between two items.
    pub fn selected_details(&self) -> Option<&RootDetails> {
        let id = self.selected_item()?.root_id;
        self.details
            .as_ref()
            .filter(|details| details.root.root_id == id)
    }

    /// The screen showing instead of the grid, if the destination is one.
    pub fn screen(&self) -> Option<backend::Screen> {
        self.nav.get(self.displayed)?.screen
    }

    /// Runs work on the library's thread and hands back its answer. This is
    /// how a screen reads and edits what only it needs; the library is never
    /// touched from the thread that draws.
    pub fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&picto_library::Library) -> T + Send + 'static,
    ) -> futures::channel::oneshot::Receiver<T> {
        let (answer, receiver) = futures::channel::oneshot::channel();
        let _ = self.sender.send(Request::Job(Box::new(move |library| {
            let _ = answer.send(work(library));
        })));
        receiver
    }

    /// Sends a command to the application layer, as the Electron renderer's
    /// `invoke(command, args)` does, and hands back its JSON answer. The
    /// command runs on the application's runtime, never on the thread that
    /// draws. The command names and their arguments are those of
    /// `core/src/ipc.rs`.
    pub fn dispatch(
        &self,
        command: impl Into<String>,
        args: serde_json::Value,
    ) -> futures::channel::oneshot::Receiver<Result<serde_json::Value, String>> {
        self.application.dispatch(command, args)
    }

    // ── The application's events ────────────────────────────────

    /// Takes what the application reported since the last look, merged into
    /// one invalidation, does what the Electron runtime stores do with it,
    /// and passes it on to the views that subscribe to [`Event`].
    fn receive_events(&mut self, cx: &mut Context<Self>) {
        let (changed, thumbnails) = application::coalesce(self.events.try_iter());
        for hash in thumbnails {
            cx.emit(Event::Thumbnail(hash));
        }
        let Some(changed) = changed else {
            return;
        };
        // `sidebarSettle.ts`: the counts and the destination list.
        if ["sidebar", "folders", "smart_folders", "tags", "duplicates"]
            .iter()
            .any(|resource| application::touches(&changed, resource))
        {
            self.refresh_navigation(cx);
        }
        if application::touches(&changed, "library") {
            // What other destinations held may no longer be true; going
            // back to one reads it again. The grid on screen is read again
            // in place (`gridSettle.ts`), keeping its scroll and selection.
            self.saved.clear();
            self.request_refresh(cx);
            let shown = self
                .selected_details()
                .map(|details| details.root.root_id.0);
            if shown.is_some_and(|id| changed.item_ids.contains(&id)) {
                self.refresh_details(cx);
            }
        }
        // `subscriptionsSettle.ts`: the running state follows runs.
        if application::touches(&changed, "subscriptions")
            || application::touches(&changed, "tasks")
        {
            self.refresh_activity(cx);
        }
        cx.emit(Event::Changed(changed));
    }

    /// Reads the sidebar's destinations and counts again. Changes that
    /// arrive while a read is under way are folded into one more read.
    pub fn refresh_navigation(&mut self, cx: &mut Context<Self>) {
        if let Some(again) = &mut self.navigation_reading {
            *again = true;
            return;
        }
        self.navigation_reading = Some(false);
        let answer = self.run(backend::navigation);
        cx.spawn(async move |session, cx| {
            let result = answer.await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                let again = this.navigation_reading.take() == Some(true);
                match result {
                    Ok(Ok((nav, tags))) => this.replace_navigation(nav, tags, cx),
                    Ok(Err(error)) => eprintln!("picto: {error}"),
                    Err(_) => {}
                }
                if again {
                    this.refresh_navigation(cx);
                }
            });
        })
        .detach();
    }

    /// Reads whether subscriptions are running or held, and keeps reading
    /// while one runs.
    pub fn refresh_activity(&mut self, cx: &mut Context<Self>) {
        if self.activity_reading {
            return;
        }
        self.activity_reading = true;
        let answer = self.dispatch("subscriptions.list", serde_json::json!({}));
        cx.spawn(async move |session, cx| {
            let result = answer.await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                this.activity_reading = false;
                let Ok(Ok(list)) = result else {
                    return;
                };
                let activity = SubscriptionActivity {
                    busy: this.subscriptions.busy,
                    ..SubscriptionActivity::from_list(&list)
                };
                if activity != this.subscriptions {
                    this.subscriptions = activity;
                    cx.emit(Change::Activity);
                }
                this.poll_activity(cx);
            });
        })
        .detach();
    }

    /// Polls while a run is going, as the renderer does every 1.5 s.
    fn poll_activity(&mut self, cx: &mut Context<Self>) {
        if !self.subscriptions.running {
            self.activity_poll = None;
            return;
        }
        if self.activity_poll.is_some() {
            return;
        }
        self.activity_poll = Some(cx.spawn(async move |session, cx| {
            cx.background_executor().timer(SUBSCRIPTION_POLL).await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                this.activity_poll = None;
                this.refresh_activity(cx);
            });
        }));
    }

    /// The sidebar row's button: holds every subscription, or lets them run
    /// again (`subscriptions.pause_all`).
    pub fn toggle_all_subscriptions(&mut self, cx: &mut Context<Self>) {
        if self.subscriptions.busy {
            return;
        }
        let paused = !self.subscriptions.paused;
        self.subscriptions.busy = true;
        cx.emit(Change::Activity);
        let answer = self.dispatch(
            "subscriptions.pause_all",
            serde_json::json!({ "paused": paused }),
        );
        cx.spawn(async move |session, cx| {
            let result = answer.await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                this.subscriptions.busy = false;
                if let Ok(Err(error)) = result {
                    eprintln!("picto: {error}");
                }
                cx.emit(Change::Activity);
                this.refresh_activity(cx);
            });
        })
        .detach();
    }

    /// Fetches the current destination again, after an edit made elsewhere.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.saved.clear();
        self.query(cx);
    }

    /// `recordRecentFolderUse`: the folder moves to the front of the recent
    /// folders, each kept once, at most 30.
    pub fn record_folder_use(&mut self, folder: picto_library::FolderId) {
        record_recent(&mut self.recent_folders, folder);
        self.recents_changed = true;
    }

    /// `recordRecentItems('picto-recent-tags', …)`: the tags go to the front
    /// in their order, each kept once, at most 30.
    pub fn record_tags_use(&mut self, tags: &[String]) {
        record_recent_names(&mut self.recent_tags, tags);
        self.recents_changed = true;
    }

    /// Keeps the recents in the app's state once they changed.
    fn keep_recents(&mut self, cx: &mut App) {
        if !std::mem::take(&mut self.recents_changed) {
            return;
        }
        let library = crate::state::library_key(&self.root);
        let folders = self.recent_folders.iter().map(|folder| folder.0).collect();
        let tags = self.recent_tags.clone();
        crate::state::update(cx, |state| {
            state.recent_folders.insert(library, folders);
            state.recent_tags = tags;
        });
    }

    /// `requestReconcile`: background work (an import, a subscription
    /// run) changed the library, so the shown results are read again in
    /// place, soon after, coalesced with whatever else arrives meanwhile,
    /// and never under a gesture or a change of destination.
    fn request_refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_wanted = true;
        self.schedule_refresh(REFRESH_MIN, cx);
    }

    fn schedule_refresh(&mut self, earliest: Duration, cx: &mut Context<Self>) {
        if !self.refresh_wanted || self.refresh_timer.is_some() {
            return;
        }
        let since = self
            .refresh_finished
            .map_or(Duration::MAX, |finished| finished.elapsed());
        let delay = refresh_delay(self.refresh_took, since).max(earliest);
        self.refresh_timer = Some(cx.spawn(async move |session, cx| {
            cx.background_executor().timer(delay).await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                this.refresh_timer = None;
                this.run_wanted_refresh(cx);
            });
        }));
    }

    fn run_wanted_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.refresh_wanted {
            return;
        }
        let busy = self.pointer_busy
            || cx.has_active_drag()
            || self.loading
            || !matches!(self.transition, Transition::Idle)
            || self.scope != self.displayed;
        if busy {
            self.schedule_refresh(REFRESH_RETRY, cx);
            return;
        }
        self.refresh_wanted = false;
        self.refresh_started = Some(Instant::now());
        self.refresh_results(cx);
    }

    /// Fetches the current destination again after an edit that may change
    /// what it holds, keeping the selection (by item) and the scroll, as the
    /// Electron grid refreshes in place.
    pub fn refresh_results(&mut self, cx: &mut Context<Self>) {
        if self
            .nav
            .get(self.scope)
            .is_none_or(|entry| entry.screen.is_some())
        {
            return;
        }
        let Some(query) = self.ran.clone() else {
            self.query(cx);
            return;
        };
        self.saved.clear();
        self.refreshing = true;
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.window_active = None;
        self.window_queued = None;
        // Where the selection's items and the shown item are now is read
        // with the window.
        let mut tracked = self.selection.tracked();
        tracked.extend(self.selected_item().map(|item| item.root_id));
        tracked.extend(self.anchor.map(|anchor| anchor.id));
        tracked.sort_unstable();
        tracked.dedup();
        // The same window, as many items as it holds (`reconcileSnapshot`).
        let limit = self.items.len().clamp(window::PAGE, window::WINDOW);
        let _ = self.sender.send(Request::Window(
            self.generation,
            Box::new(query),
            WindowRequest::at(self.start, limit),
            Fetch::Refresh(tracked),
        ));
        cx.emit(Change::Results);
    }

    /// Puts a rebuilt destination list in place of the current one, after
    /// the sidebar edited the folders. Places are kept by what they show, not
    /// by their position. When the open destination is gone, its nearest
    /// surviving ancestor opens instead, or All, as the Electron app falls
    /// back after a delete.
    pub fn replace_navigation(
        &mut self,
        nav: Vec<Navigation>,
        tags: Vec<TagRecord>,
        cx: &mut Context<Self>,
    ) {
        let old = std::mem::replace(&mut self.nav, nav);
        let positions = self
            .nav
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id(), index))
            .collect::<HashMap<_, _>>();
        let map = old
            .iter()
            .map(|entry| positions.get(&entry.id()).copied())
            .collect::<Vec<_>>();
        let moved = |index: usize| map.get(index).copied().flatten();
        let survivor = |index: usize| {
            let mut at = Some(index);
            while let Some(index) = at {
                if let Some(now) = moved(index) {
                    return now;
                }
                at = old.get(index).and_then(|entry| entry.parent);
            }
            0
        };
        self.tags = tags.into_iter().map(|tag| (tag.tag_id.0, tag)).collect();
        let (history, history_index) = remap_history(&self.history, self.history_index, moved);
        self.history = history;
        self.history_index = history_index;
        self.saved = std::mem::take(&mut self.saved)
            .into_iter()
            .filter_map(|(index, grid)| Some((moved(index)?, grid)))
            .collect();
        self.prefs_scope = self.prefs_scope.and_then(moved);
        self.displayed = moved(self.displayed).unwrap_or_else(|| survivor(self.displayed));
        match moved(self.scope) {
            Some(scope) => self.scope = scope,
            None => {
                let fallback = survivor(self.scope);
                if self.history.is_empty() {
                    self.history_index = 0;
                } else {
                    self.history.truncate(self.history_index + 1);
                }
                self.history.push(Visit {
                    scope: fallback,
                    viewing: false,
                    place: None,
                });
                self.history_index = self.history.len() - 1;
                self.open(fallback, None, None, cx);
                // What `open` kept of the grid on screen was the deleted
                // destination's, not the fallback's.
                self.saved.remove(&fallback);
            }
        }
        cx.emit(Change::Navigation);
    }

    // ── Results ─────────────────────────────────────────────────

    /// Reads the destination afresh, from its first page (`loadFirstPage`).
    fn query(&mut self, cx: &mut Context<Self>) {
        self.refreshing = false;
        let Some(entry) = self.nav.get(self.scope) else {
            return;
        };
        if entry.screen.is_some() {
            return;
        }
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.window_active = None;
        self.window_queued = None;
        self.pending_focus = None;
        let _ = self.sender.send(Request::Query(
            self.generation,
            entry.scope.clone(),
            FilterExpr::All(vec![
                backend::text_filter(&self.search_text),
                self.filter.clone().unwrap_or_default(),
            ]),
            entry.random,
            WindowRequest::at(0, window::PAGE),
        ));
        cx.emit(Change::Results);
    }

    /// `requestWindowAt`: asks for the window that shows `ordinal`, unless
    /// it is held or already asked for. One read is on its way at a time;
    /// a newer ask waits behind it and replaces any that waited before.
    pub fn request_window_at(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let total = self.total as usize;
        if self.loading || total == 0 || self.ran.is_none() {
            return;
        }
        let start = window::start_at(ordinal, total);
        if window::holds(self.start, self.items.len(), start, total) {
            // What is on its way is no longer wanted.
            self.window_request += 1;
            self.window_queued = None;
            return;
        }
        let latest = self.window_request;
        let pending = self.window_queued.or(self.window_active);
        if pending == Some((latest, start)) {
            return;
        }
        self.window_request += 1;
        let request = (self.window_request, start);
        if self.window_active.is_some() {
            self.window_queued = Some(request);
        } else {
            self.send_window(request, cx);
        }
    }

    fn send_window(&mut self, (id, start): (u64, usize), cx: &mut Context<Self>) {
        let Some(query) = self.ran.clone() else {
            return;
        };
        self.window_active = Some((id, start));
        crate::stats::window_asked();
        let _ = self.sender.send(Request::Window(
            self.generation,
            Box::new(query),
            WindowRequest::at(start, window::WINDOW),
            Fetch::Move(id),
        ));
        cx.emit(Change::Results);
    }

    /// Asks for the window after the one held, as the Inbox review does
    /// before moving past the window's last item.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.has_following() {
            self.request_window_at(self.start + self.items.len(), cx);
        }
    }

    pub fn set_search(&mut self, text: String, cx: &mut Context<Self>) {
        if text != self.search_text {
            self.search_text = text;
            self.query(cx);
        }
    }

    /// Narrows the current destination, or clears the narrowing.
    pub fn set_filter(&mut self, filter: Option<FilterExpr>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.close_viewer(cx);
        self.query(cx);
    }

    /// Replaces the filter bar's filters and narrows the grid by them.
    pub fn set_filters(&mut self, filters: Filters, cx: &mut Context<Self>) {
        if filters == self.filters {
            return;
        }
        let filter = Self::filter_of(&filters);
        self.filters = filters;
        self.set_filter(filter, cx);
    }

    fn filter_of(filters: &Filters) -> Option<FilterExpr> {
        let expr = filters.to_expr();
        (expr != FilterExpr::default()).then_some(expr)
    }

    /// Shows or hides the filter bar.
    pub fn set_filter_bar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.filter_bar_open != open {
            self.filter_bar_open = open;
            cx.emit(Change::Results);
        }
    }

    /// Keeps the filters when another destination opens, or not.
    pub fn set_filters_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        if self.filters_locked != locked {
            self.filters_locked = locked;
            cx.emit(Change::Results);
        }
    }

    /// Opens a destination with these filters, as a step of Back and
    /// Forward (`navigateWithGridFilters`); on the destination already
    /// showing, only the filters change.
    pub fn navigate_with_filters(
        &mut self,
        index: usize,
        filters: Filters,
        cx: &mut Context<Self>,
    ) {
        if index == self.scope {
            self.set_filters(filters, cx);
            return;
        }
        self.record(Visit {
            scope: index,
            viewing: false,
            place: None,
        });
        self.open(index, None, Some(filters), cx);
    }

    /// Changes the current destination's view (`gridController.updateView`)
    /// and saves the change to its `view_pref` row (`saveViewPref`), as
    /// `settings.view.patch` does: out of undo. With `refetch` the results
    /// are fetched again behind the scope fade, as a new sort needs.
    pub fn update_view(&mut self, patch: serde_json::Value, refetch: bool, cx: &mut Context<Self>) {
        let Some(entry) = self.nav.get(self.scope) else {
            return;
        };
        if entry.screen.is_some() {
            return;
        }
        let scope = entry.scope.clone();
        self.prefs = self.prefs.patched(&scope, &patch);
        let mut merged = match self.view_patch.take() {
            Some((_, serde_json::Value::Object(earlier))) => earlier,
            _ => serde_json::Map::new(),
        };
        let fields = patch.as_object().cloned().unwrap_or_default();
        merged.extend(fields.clone());
        self.view_patch = Some((self.generation, serde_json::Value::Object(merged)));
        let key = crate::grid::prefs::scope_key(&scope);
        self.write_view(key, serde_json::Value::Object(fields));
        if refetch {
            self.close_viewer(cx);
            if matches!(self.transition, Transition::Idle) {
                self.transition = Transition::FadingOut(Instant::now());
            }
            self.query(cx);
            cx.emit(Change::Navigation);
        } else {
            cx.emit(Change::Results);
        }
    }

    /// Saves fields of a destination's view to its `view_pref` row, as
    /// `settings.view.patch` does: out of undo.
    fn write_view(&self, key: String, fields: serde_json::Value) {
        let fields = fields.as_object().cloned().unwrap_or_default();
        let _ = self.sender.send(Request::Job(Box::new(move |library| {
            let saved = library.update_auxiliary_json(
                "settings.view.patch",
                "Change view preferences",
                "view_pref",
                &key,
                false,
                move |stored| {
                    Ok(Some(
                        crate::settings::model::patched(stored, &fields).to_string(),
                    ))
                },
            );
            if let Err(error) = saved {
                eprintln!("picto: could not save the view of {key}: {error}");
            }
        })));
    }

    /// The preferences a page of `generation` brought, with any view change
    /// made after that query was sent.
    fn fresh_prefs(&mut self, generation: u64, prefs: ViewPrefs) -> ViewPrefs {
        match &self.view_patch {
            Some((sent, patch)) if generation <= *sent => {
                let scope = self.nav.get(self.scope).map(|entry| entry.scope.clone());
                scope.map_or(prefs.clone(), |scope| prefs.patched(&scope, patch))
            }
            Some(_) => {
                self.view_patch = None;
                prefs
            }
            None => prefs,
        }
    }

    /// The open library's folder, the one that holds its blobs.
    pub fn library_root(&self) -> Option<&std::path::Path> {
        self.blobs.parent()
    }

    /// What the search field holds, which the results are filtered by.
    pub fn search_text(&self) -> &str {
        &self.search_text
    }

    /// The library folder's name without its `.library` suffix.
    pub fn library_name(&self) -> Option<String> {
        let name = self.blobs.parent()?.file_name()?.to_string_lossy();
        Some(name.trim_end_matches(".library").to_owned())
    }

    /// Zooms the grid, and saves the zoom to the destination's view
    /// (`saveViewPref({ target_size })`) once it has rested for 500 ms
    /// (`VIEW_PREFS_SAVE_DEBOUNCE_MS`), so a slider drag writes once.
    pub fn set_tile(&mut self, tile: f32, cx: &mut Context<Self>) {
        self.tile = tile;
        let scope = self
            .nav
            .get(self.scope)
            .filter(|entry| entry.screen.is_none())
            .map(|entry| entry.scope.clone());
        if let Some(scope) = scope {
            let patch = serde_json::json!({ "target_size": tile });
            self.prefs = self.prefs.patched(&scope, &patch);
            // A page of an earlier query keeps the new zoom.
            let mut merged = match self.view_patch.take() {
                Some((_, serde_json::Value::Object(earlier))) => earlier,
                _ => serde_json::Map::new(),
            };
            merged.insert("target_size".into(), serde_json::json!(tile));
            self.view_patch = Some((self.generation, serde_json::Value::Object(merged)));
            let key = crate::grid::prefs::scope_key(&scope);
            // A zoom of another destination still waiting is saved now.
            if let Some((waiting, size, _)) = self.tile_save.take()
                && waiting != key
            {
                self.write_view(waiting, serde_json::json!({ "target_size": size }));
            }
            let timer = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(VIEW_SAVE_DELAY).await;
                let _ = this.update(cx, |this, _| {
                    if let Some((key, size, _)) = this.tile_save.take() {
                        this.write_view(key, serde_json::json!({ "target_size": size }));
                    }
                });
            });
            self.tile_save = Some((key, tile, timer));
        }
        cx.emit(Change::Results);
    }

    fn receive(&mut self, cx: &mut Context<Self>) {
        self.keep_recents(cx);
        self.receive_events(cx);
        while let Ok(reply) = self.receiver.try_recv() {
            match reply {
                Reply::Ready(nav, tags) => {
                    self.nav = nav;
                    self.tags = tags.into_iter().map(|tag| (tag.tag_id.0, tag)).collect();
                    cx.emit(Change::Navigation);
                    self.query(cx);
                    self.refresh_activity(cx);
                }
                Reply::Arrived(arrived) if arrived.generation == self.generation => {
                    match arrived.fetch {
                        Fetch::Open => {
                            if matches!(self.transition, Transition::FadingOut(_)) {
                                // The previous grid is still fading out. Hold
                                // the destination until it is gone.
                                self.pending_page = Some(arrived);
                            } else {
                                self.apply_open(arrived, cx);
                            }
                        }
                        Fetch::Move(id) => self.apply_move(id, arrived, cx),
                        Fetch::Refresh(_) => self.apply_refresh(arrived, cx),
                    }
                }
                Reply::Failed(generation, Fetch::Move(id), error)
                    if generation == self.generation =>
                {
                    eprintln!("picto: {error}");
                    if self.window_active.is_some_and(|(active, _)| active == id) {
                        self.window_active = None;
                    }
                    if let Some(queued) = self.window_queued.take() {
                        self.send_window(queued, cx);
                    }
                    cx.emit(Change::Results);
                }
                // A query that failed is the grid's error, shown in place of
                // the destination. A failed edit leaves the grid as it is.
                Reply::Failed(generation, _, error) if generation == self.generation => {
                    self.error = Some(error);
                    self.loading = false;
                    self.refreshing = false;
                    if matches!(self.transition, Transition::Waiting) {
                        self.show_error(cx);
                    }
                    cx.emit(Change::Results);
                }
                Reply::Error(error) => eprintln!("picto: {error}"),
                _ => {}
            }
        }
    }

    /// Puts a window's items in place of the held ones.
    fn take_window(&mut self, arrived: &mut Arrived) {
        let window = &mut arrived.window;
        self.items = std::mem::take(&mut window.page.items);
        self.start = window.start;
        self.head = std::mem::take(&mut window.head_items);
        self.total = window.page.total;
        self.total_size_bytes = window.page.total_size_bytes;
        self.window_epoch += 1;
    }

    /// A destination's first window, read afresh: it opens with nothing
    /// selected.
    fn apply_open(&mut self, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        let prefs = arrived.prefs.take().unwrap_or_else(|| self.prefs.clone());
        let prefs = self.fresh_prefs(arrived.generation, prefs);
        // A scope opens at its saved zoom. Searching inside it keeps
        // whatever the zoom control was moved to since.
        if self.prefs_scope != Some(self.scope) {
            self.prefs_scope = Some(self.scope);
            self.tile = prefs.target_size;
        }
        self.prefs = prefs;
        self.take_window(&mut arrived);
        self.ran = Some(arrived.query.clone());
        self.loading = false;
        self.refreshing = false;
        self.results_epoch += 1;
        self.density = None;
        self.restore_scroll = None;
        self.restore_window_top = None;
        self.forget_selection();
        crate::stats::results_opened(self.total);
        self.displayed = self.scope;
        if matches!(self.transition, Transition::Waiting) {
            self.transition = Transition::FadingIn(Instant::now());
        }
        cx.emit(Change::Navigation);
        cx.emit(Change::Selection);
        cx.emit(Change::Results);
    }

    /// Another window of the results arrived. Only the latest asked for is
    /// shown; the one waiting behind it is asked for next.
    fn apply_move(&mut self, id: u64, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        if self.window_active.is_some_and(|(active, _)| active == id) {
            self.window_active = None;
        }
        if id == self.window_request {
            crate::stats::window_arrived(arrived.window.start, arrived.window.page.items.len());
            self.take_window(&mut arrived);
            // A move to an item that was not held lands now, or, when a
            // later scroll asked for another window, not at all.
            if let Some((ordinal, extend)) = self.pending_focus.take() {
                if self.item_at(ordinal).is_none() {
                    if self.reveal == Some(ordinal) {
                        self.reveal = None;
                    }
                } else if extend {
                    self.select_to(ordinal, false, cx);
                } else {
                    self.select(ordinal, cx);
                }
            }
        }
        if let Some(queued) = self.window_queued.take() {
            self.send_window(queued, cx);
        }
        cx.emit(Change::Results);
    }

    /// The window read again in place: the same items stay selected, each
    /// followed to where it is now, and the grid keeps the topmost item it
    /// shows where it was.
    fn apply_refresh(&mut self, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        let Fetch::Refresh(tracked) = std::mem::replace(&mut arrived.fetch, Fetch::Open) else {
            return;
        };
        self.take_window(&mut arrived);
        crate::stats::window_refreshed(self.start, self.items.len());
        self.ran = Some(arrived.query.clone());
        self.loading = false;
        self.refreshing = false;
        if let Some(started) = self.refresh_started.take() {
            self.refresh_took = started.elapsed();
            self.refresh_finished = Some(Instant::now());
        }
        let places = tracked
            .into_iter()
            .zip(
                arrived
                    .places
                    .iter()
                    .map(|place| place.map(|ordinal| ordinal as usize)),
            )
            .collect::<HashMap<_, _>>();
        self.selection.reconcile(&places);
        let moved = |end: End| match places.get(&end.id) {
            Some(Some(ordinal)) => Some(End {
                id: end.id,
                ordinal: *ordinal,
            }),
            Some(None) => None,
            None => Some(end),
        };
        self.anchor = self.anchor.and_then(moved);
        // The shown item stays, where it is now; if it left, the last
        // selected item the window holds is shown.
        let shown = self.focus.as_ref().and_then(|focused| {
            moved(End {
                id: focused.item.root_id,
                ordinal: focused.ordinal,
            })
        });
        match shown {
            Some(end) => {
                if let Some(item) = self.item_at(end.ordinal).cloned() {
                    self.focus = Some(Focused {
                        ordinal: end.ordinal,
                        item,
                    });
                } else if let Some(focused) = &mut self.focus {
                    focused.ordinal = end.ordinal;
                }
                cx.emit(Change::Selection);
            }
            None => {
                let last = self
                    .selected_loaded()
                    .last()
                    .and_then(|item| self.position_of(item.root_id));
                self.focus_on(last, cx);
            }
        }
        cx.emit(Change::Results);
    }

    /// The selected items the window holds, by ordinal.
    pub fn selected_ordinals(&self) -> Vec<usize> {
        (self.start..self.start + self.items.len())
            .filter(|ordinal| self.is_selected(*ordinal))
            .collect()
    }

    /// Where the window holds an item, as an ordinal.
    pub fn position_of(&self, id: RootId) -> Option<usize> {
        self.items
            .iter()
            .position(|item| item.root_id == id)
            .map(|offset| self.start + offset)
    }

    // ── Navigation ──────────────────────────────────────────────

    /// Opens a sidebar destination and records it for Back and Forward.
    /// Choosing the destination already showing does nothing, unless the
    /// viewer is over it, which this closes.
    pub fn navigate(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.scope {
            // Choosing a screen from inside one of its places goes to its top.
            if self.place().is_some() {
                self.open_place(None, cx);
            }
            self.close_viewer(cx);
            return;
        }
        self.record(Visit {
            scope: index,
            viewing: false,
            place: None,
        });
        // A destination chosen afresh opens at its top, without filters
        // unless they are locked.
        let filters = (!self.filters_locked).then(Filters::default);
        self.open(index, None, filters, cx);
    }

    /// Adds a place after the current one, dropping any Forward steps.
    fn record(&mut self, visit: Visit) {
        self.history.truncate(self.history_index + 1);
        self.history.push(visit);
        self.history_index = self.history.len() - 1;
    }

    pub fn can_step_history(&self, delta: isize) -> bool {
        self.history_index
            .checked_add_signed(delta)
            .is_some_and(|target| target < self.history.len())
    }

    pub fn step_history(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(target) = self.history_index.checked_add_signed(delta) else {
            return;
        };
        let Some(&visit) = self.history.get(target) else {
            return;
        };
        self.history_index = target;
        if visit.scope == self.scope {
            // The same destination: only the viewer opens or closes, and the
            // grid stays where it was.
            let viewing = visit.viewing && self.focus.is_some();
            if self.viewer != viewing {
                self.viewer = viewing;
                cx.emit(Change::Viewer);
            }
            cx.emit(Change::Navigation);
            return;
        }
        // Another destination comes back with the filters it had, unless
        // they are locked, and as it was left when nothing has changed what
        // it would show.
        let filters = if self.filters_locked {
            self.filters.clone()
        } else {
            self.saved
                .get(&visit.scope)
                .map(|saved| saved.filters.clone())
                .unwrap_or_default()
        };
        let saved = self
            .saved
            .remove(&visit.scope)
            .filter(|saved| saved.search_text == self.search_text && saved.filters == filters);
        self.open(visit.scope, saved, Some(filters), cx);
    }

    /// What the current grid holds, to return to.
    fn save_grid(&mut self) {
        if self
            .nav
            .get(self.displayed)
            .is_none_or(|entry| entry.screen.is_some())
            || self.items.is_empty()
        {
            return;
        }
        self.saved.insert(
            self.displayed,
            SavedGrid {
                items: self.items.clone(),
                start: self.start,
                head: self.head.clone(),
                total: self.total,
                total_size_bytes: self.total_size_bytes,
                prefs: self.prefs.clone(),
                tile: self.tile,
                selection: self.selection.clone(),
                focus: self.focus.clone(),
                anchor: self.anchor,
                ran: self.ran.clone(),
                scroll: self.grid_scroll,
                density: self.density,
                window_top: self.window_top,
                search_text: self.search_text.clone(),
                filters: self.filters.clone(),
            },
        );
    }

    /// Puts a saved grid on the hidden surface and fades it in.
    fn restore_grid(&mut self, saved: SavedGrid, cx: &mut Context<Self>) {
        self.items = saved.items;
        self.start = saved.start;
        self.head = saved.head;
        self.total = saved.total;
        self.total_size_bytes = saved.total_size_bytes;
        self.prefs = saved.prefs;
        self.prefs_scope = Some(self.scope);
        self.tile = saved.tile;
        self.ran = saved.ran;
        self.loading = false;
        self.window_active = None;
        self.window_queued = None;
        self.pending_focus = None;
        self.results_epoch += 1;
        self.window_epoch += 1;
        // The window lies where it lay, in the same estimated height, so
        // the scroll offset shows what it showed.
        self.density = saved.density;
        self.restore_window_top = Some(saved.window_top);
        self.restore_scroll = Some(saved.scroll);
        self.selection = saved.selection;
        self.anchor = saved.anchor;
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
        let showing = self.selected_details().map(|details| details.root.root_id);
        self.focus = saved.focus;
        self.demo = None;
        match self.selected_item().map(|item| item.root_id) {
            Some(id) if showing != Some(id) => {
                self.member = 0;
                self.read_details(id, cx);
            }
            Some(_) => {}
            None => {
                self.member = 0;
                self.details = None;
            }
        }
        cx.emit(Change::Selection);
        // An entry that was the viewer opens it again on the same item.
        let viewing = self.history[self.history_index].viewing && self.focus.is_some();
        if self.viewer != viewing {
            self.viewer = viewing;
            cx.emit(Change::Viewer);
        }
        cx.emit(Change::Navigation);
        cx.emit(Change::Results);
    }

    /// Opens a destination: a saved grid, or its results fetched with
    /// `filters` in place of the current ones when given.
    fn open(
        &mut self,
        index: usize,
        saved: Option<SavedGrid>,
        filters: Option<Filters>,
        cx: &mut Context<Self>,
    ) {
        self.save_grid();
        if let Some(filters) = filters {
            self.filter = Self::filter_of(&filters);
            self.filters = filters;
        }
        self.scope = index;
        self.pending_page = None;
        self.pending_restore = None;
        if self.viewer {
            self.viewer = false;
            cx.emit(Change::Viewer);
        }
        let screen = self
            .nav
            .get(index)
            .is_some_and(|entry| entry.screen.is_some());
        self.transition = match self.transition {
            // Further clicks while the surface is fading out or already hidden
            // only retarget where it lands.
            Transition::FadingOut(_) | Transition::Waiting => self.transition,
            // Turning around mid fade-in continues from the current opacity.
            Transition::FadingIn(started) => {
                let shown = motion::progress(started.elapsed(), motion::SCOPE_FADE);
                Transition::FadingOut(Instant::now() - motion::SCOPE_FADE.mul_f32(1. - shown))
            }
            Transition::Idle => Transition::FadingOut(Instant::now()),
        };
        if screen {
            // Not a grid: nothing to fetch, and any reply still on its way for
            // the previous scope no longer applies.
            self.generation += 1;
            self.loading = false;
            if matches!(self.transition, Transition::Waiting) {
                self.commit_screen();
            }
        } else if let Some(saved) = saved {
            // Nothing to fetch: the saved grid goes on once the surface is
            // hidden, and a reply still on its way no longer applies.
            self.generation += 1;
            self.loading = false;
            if matches!(self.transition, Transition::Waiting) {
                self.restore_grid(saved, cx);
            } else {
                self.pending_restore = Some(saved);
            }
        } else {
            self.query(cx);
        }
        cx.emit(Change::Navigation);
    }

    /// Shows a destination that is a screen, not a grid, on the hidden surface.
    fn commit_screen(&mut self) {
        self.clear_results();
        self.total_size_bytes = 0;
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
    }

    /// Fades the destination in on its error, which takes the grid's place.
    /// The previous destination's items go, so a retry never shows them.
    fn show_error(&mut self, cx: &mut Context<Self>) {
        self.clear_results();
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
        cx.emit(Change::Navigation);
        cx.emit(Change::Selection);
    }

    /// No results: an empty window over nothing.
    fn clear_results(&mut self) {
        self.items.clear();
        self.head.clear();
        self.start = 0;
        self.total = 0;
        self.results_epoch += 1;
        self.window_epoch += 1;
        self.window_active = None;
        self.window_queued = None;
        self.pending_focus = None;
        self.forget_selection();
    }

    /// Moves a scope change along once its current stage has run its time.
    /// The views that fade call this as they paint.
    pub fn advance_transition(&mut self, cx: &mut Context<Self>) {
        match self.transition {
            Transition::FadingOut(started) if started.elapsed() >= motion::SCOPE_FADE => {
                self.transition = Transition::Waiting;
                if self
                    .nav
                    .get(self.scope)
                    .is_some_and(|entry| entry.screen.is_some())
                {
                    self.commit_screen();
                    cx.emit(Change::Results);
                    cx.emit(Change::Selection);
                } else if let Some(saved) = self.pending_restore.take() {
                    self.restore_grid(saved, cx);
                } else if let Some(arrived) = self.pending_page.take() {
                    self.apply_open(arrived, cx);
                } else if self.error.is_some() && !self.loading {
                    self.show_error(cx);
                }
                cx.emit(Change::Navigation);
            }
            Transition::FadingIn(started) if started.elapsed() >= motion::SCOPE_FADE => {
                self.transition = Transition::Idle;
                cx.emit(Change::Navigation);
            }
            _ => {}
        }
    }

    // ── Selection ───────────────────────────────────────────────

    fn forget_selection(&mut self) {
        self.selection = Selection::default();
        self.focus = None;
        self.anchor = None;
        self.member = 0;
        self.details = None;
    }

    /// Shows the item at `ordinal` in the inspector, or nothing. The
    /// previous item's details stay up until this one's arrive, and
    /// choosing the item already shown asks for nothing.
    fn focus_on(&mut self, ordinal: Option<usize>, cx: &mut Context<Self>) {
        self.demo = None;
        let showing = self.selected_details().map(|details| details.root.root_id);
        self.focus = ordinal.and_then(|ordinal| {
            Some(Focused {
                ordinal,
                item: self.item_at(ordinal)?.clone(),
            })
        });
        match self.selected_item().map(|item| item.root_id) {
            Some(id) if showing == Some(id) => {}
            Some(id) => {
                self.member = 0;
                self.read_details(id, cx);
            }
            None => {
                self.member = 0;
                self.details = None;
            }
        }
        cx.emit(Change::Selection);
    }

    /// Selects one item alone, as a plain click does.
    pub fn select(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(end) = self.end_at(ordinal) else {
            return;
        };
        self.selection = Selection::only(end);
        self.anchor = Some(end);
        self.focus_on(Some(ordinal), cx);
    }

    /// Adds or removes one item, as a command-click does.
    pub fn toggle(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(end) = self.end_at(ordinal) else {
            return;
        };
        self.selection.toggle(end, self.total);
        if self.selection.contains(end.id, ordinal) {
            self.anchor = Some(end);
            self.focus_on(Some(ordinal), cx);
        } else {
            let last = self
                .selected_loaded()
                .last()
                .and_then(|item| self.position_of(item.root_id));
            self.focus_on(last, cx);
        }
    }

    /// Selects from the anchor to `ordinal`, as a shift-click or shift-move
    /// does; `keep` adds the range to what is selected already. A range
    /// from an anchor outside the window is a span of the order, which the
    /// library resolves.
    pub fn select_to(&mut self, ordinal: usize, keep: bool, cx: &mut Context<Self>) {
        let Some(focus) = self.end_at(ordinal) else {
            return;
        };
        let anchor = *self.anchor.get_or_insert(focus);
        let (start, items) = (self.start, &self.items);
        self.selection.extend(anchor, focus, keep, |at| {
            items.get(at.checked_sub(start)?).map(|item| item.root_id)
        });
        self.focus_on(Some(ordinal), cx);
    }

    /// Everything the destination holds, loaded or not.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.total == 0 {
            return;
        }
        self.selection = Selection::whole();
        let focus = self
            .focus_ordinal()
            .filter(|ordinal| self.item_at(*ordinal).is_some())
            .unwrap_or(self.start);
        if self.anchor.is_none() {
            self.anchor = self.end_at(focus);
        }
        self.focus_on(Some(focus), cx);
    }

    /// How many items are selected, loaded or not.
    pub fn selection_count(&self) -> u64 {
        self.selection.count(self.total)
    }

    /// What an edit of the grid's selection addresses.
    pub fn selection_target(&self) -> Option<picto_library::selection::SelectionTarget> {
        if self.selection.is_empty() {
            return None;
        }
        self.selection.target(self.ran.as_ref())
    }

    /// What an edit of `ids` addresses. When they are the grid's
    /// selection, as [`Session::selected_ids`] or the window gives it, the
    /// edit addresses all of it: the items chosen that are not loaded, or
    /// the destination's query for a query-wide selection, so that it
    /// reaches every item. Other ids (the viewer's item, a tile dragged
    /// alone) are addressed as they are.
    pub fn target_for(&self, ids: Vec<RootId>) -> picto_library::selection::SelectionTarget {
        use picto_library::selection::SelectionTarget;
        let loaded = self
            .selected_loaded()
            .into_iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>();
        let whole = same_items(&ids, &loaded) || same_items(&ids, &self.selected_ids());
        match self.selection_target().filter(|_| whole) {
            Some(target) => target,
            None => SelectionTarget::Explicit { root_ids: ids },
        }
    }

    /// Replaces the selection with the items at these ordinals, as a
    /// finished rubber band does; `keep` adds them to the items chosen. The
    /// item shown stays if it is still selected; otherwise the last one is
    /// shown. No range anchor remains.
    pub fn set_selection(
        &mut self,
        ordinals: std::collections::BTreeSet<usize>,
        keep: bool,
        cx: &mut Context<Self>,
    ) {
        let ends = ordinals
            .iter()
            .filter_map(|ordinal| self.end_at(*ordinal))
            .collect::<Vec<_>>();
        self.selection = if keep && !self.selection.query_wide() {
            let mut kept = self.selection.clone();
            for end in &ends {
                if !kept.contains(end.id, end.ordinal) {
                    kept.toggle(*end, self.total);
                }
            }
            kept
        } else {
            Selection::of(ends.iter().copied())
        };
        self.anchor = None;
        let focus = self
            .focus_ordinal()
            .filter(|ordinal| self.is_selected(*ordinal))
            .or_else(|| ends.last().map(|end| end.ordinal));
        self.focus_on(focus, cx);
    }

    /// Whether the search field narrows the results (three characters or more).
    pub fn searching(&self) -> bool {
        backend::text_filter(&self.search_text) != FilterExpr::default()
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() || self.focus.is_some() {
            self.selection = Selection::default();
            self.anchor = None;
            self.focus_on(None, cx);
        }
    }

    /// The ordinal a keyboard move lands on, given the grid's current
    /// geometry, in the whole destination.
    pub fn move_target(&self, action: nav::Move, width: f32, height: f32) -> Option<usize> {
        let action = match action {
            // With the viewer open only the item changes, one at a time.
            nav::Move::Left | nav::Move::Right => action,
            _ if self.viewer => return None,
            _ => action,
        };
        // With nothing selected, any move selects the first item.
        let Some(focus) = self.focus_ordinal() else {
            return (self.total > 0).then_some(0);
        };
        nav::target(
            action,
            focus,
            self.total as usize,
            nav::column_count(width, self.tile, self.prefs.gap),
            nav::visible_rows(height, self.tile, self.prefs.gap),
        )
    }

    /// Moves to the item at `ordinal` (a keyboard move, the viewer's next
    /// and previous), selecting it alone or, with `extend`, as a range,
    /// and asks the grid to bring it into view. An item outside the window
    /// is selected once its window arrives; until then what is shown stays.
    pub fn move_to(&mut self, ordinal: usize, extend: bool, cx: &mut Context<Self>) {
        if ordinal >= self.total as usize {
            return;
        }
        if self.item_at(ordinal).is_some() {
            self.reveal = Some(ordinal);
            self.pending_focus = None;
            if extend {
                self.select_to(ordinal, false, cx);
            } else {
                self.select(ordinal, cx);
            }
        } else if !self.loading {
            // While the destination is read again there is no window to
            // move; the key does nothing.
            self.reveal = Some(ordinal);
            self.pending_focus = Some((ordinal, extend));
            self.request_window_at(ordinal, cx);
            cx.emit(Change::Results);
        }
    }

    // ── Viewer ──────────────────────────────────────────────────

    /// Opens the selected item. It is a place of its own: Back returns to
    /// the grid it was opened from, where that was scrolled to.
    pub fn open_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer && self.focus.is_some() {
            self.viewer = true;
            self.record(Visit {
                scope: self.scope,
                viewing: true,
                place: None,
            });
            cx.emit(Change::Viewer);
            cx.emit(Change::Navigation);
        }
    }

    /// Returns to the grid. Closing an item opened from this grid is the
    /// same as going back from it.
    pub fn close_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer {
            return;
        }
        self.viewer = false;
        let grid = Visit {
            scope: self.scope,
            viewing: false,
            place: None,
        };
        if self.history_index > 0 && self.history[self.history_index - 1] == grid {
            self.history_index -= 1;
        } else if self.history[self.history_index] != grid {
            self.record(grid);
        }
        cx.emit(Change::Viewer);
        cx.emit(Change::Navigation);
    }

    pub fn step_member(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self
            .details
            .as_ref()
            .map_or(1, |details| details.media.len());
        self.member = self
            .member
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        cx.emit(Change::Viewer);
    }

    pub fn show_demo(&mut self, name: &'static str, mime: &'static str, cx: &mut Context<Self>) {
        self.demo = Some((name, mime));
        self.viewer = true;
        cx.emit(Change::Selection);
        cx.emit(Change::Viewer);
    }

    // ── Edits, each through the library's own owner ─────────────

    /// Runs an edit on the library's thread, reports a failure on the
    /// console, and reads again what `reread` names, whether or not the edit
    /// went through. Hands back the edit's answer once the reads are asked
    /// for, so a view's own follow-up comes after them. What was shown stays
    /// until each read arrives. The sidebar's destinations and counts follow
    /// the library's change events by themselves.
    pub fn edit<T: Send + 'static, E: std::fmt::Display + Send + 'static>(
        &mut self,
        work: impl FnOnce(&picto_library::Library) -> Result<T, E> + Send + 'static,
        reread: Reread,
        cx: &mut Context<Self>,
    ) -> futures::channel::oneshot::Receiver<Result<T, E>> {
        let (answer, receiver) = futures::channel::oneshot::channel();
        let job = self.run(move |library| {
            let result = work(library);
            let navigation = reread
                .navigation
                .then(|| backend::navigation(library).ok())
                .flatten();
            (result, navigation)
        });
        cx.spawn(async move |session, cx| {
            let Ok((result, navigation)) = job.await else {
                return;
            };
            let _ = session.update(cx, |this, cx| {
                if let Some((nav, tags)) = navigation {
                    this.replace_navigation(nav, tags, cx);
                }
                // TODO: show a failure as a notification, as production's
                // `showErrorNotification` does.
                if let Err(error) = &result {
                    eprintln!("picto: {error}");
                }
                this.reread(reread, cx);
            });
            let _ = answer.send(result);
        })
        .detach();
        receiver
    }

    /// Reads again what an edit may have changed, keeping what is shown
    /// until each answer arrives.
    pub fn reread(&mut self, reread: Reread, cx: &mut Context<Self>) {
        if reread.details {
            self.refresh_details(cx);
        }
        if reread.summary {
            cx.emit(Change::Edited);
        }
        if reread.results {
            self.refresh_results(cx);
        }
    }

    /// Moves the selected items to the trash, or back out of it, and reads
    /// the destination again in place.
    pub fn move_selection_to(
        &mut self,
        lifecycle: picto_library::Lifecycle,
        cx: &mut Context<Self>,
    ) {
        let ids = self.selected_ids();
        if !ids.is_empty() {
            self.close_viewer(cx);
            let target = self.target_for(ids);
            let _ = self.set_lifecycle(target, lifecycle, cx);
        }
    }

    /// Moves `target` to the trash, the Inbox or the library, and reads the
    /// destination again in place. The answer says whether it went through.
    pub fn set_lifecycle(
        &mut self,
        target: picto_library::selection::SelectionTarget,
        lifecycle: picto_library::Lifecycle,
        cx: &mut Context<Self>,
    ) -> futures::channel::oneshot::Receiver<Result<(), picto_library::LibraryError>> {
        self.edit(
            move |library| library.set_lifecycle(&target, lifecycle).map(|_| ()),
            Reread::RESULTS,
            cx,
        )
    }

    /// Fetches the selected item's details again, after an edit changed
    /// them.
    pub fn refresh_details(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_item().map(|item| item.root_id) {
            self.read_details(id, cx);
        }
    }

    /// Reads an item's details, which are shown if it is still the one
    /// selected when they arrive.
    fn read_details(&mut self, id: RootId, cx: &mut Context<Self>) {
        let answer = self.run(move |library| library.details(id));
        cx.spawn(async move |session, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = session.update(cx, |this, cx| match result {
                Ok(details) if this.selected_item().is_some_and(|item| item.root_id == id) => {
                    let name = details.root.name.clone();
                    if let Some(focused) = &mut this.focus {
                        focused.item.name = name.clone();
                    }
                    if let Some(item) = this.items.iter_mut().find(|item| item.root_id == id) {
                        item.name = name;
                    }
                    this.details = Some(details);
                    cx.emit(Change::Details);
                }
                Ok(_) => {}
                Err(error) => eprintln!("picto: {error}"),
            });
        })
        .detach();
    }

    /// Undoes the library's last edit, while an item is selected, and shows
    /// that item as it is now.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.selected_item().is_some() {
            let _ = self.edit(|library| library.undo(), Reread::DETAILS, cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.selected_item().is_some() {
            let _ = self.edit(|library| library.redo(), Reread::DETAILS, cx);
        }
    }
}

/// What an edit may have changed, and so what [`Session::edit`] reads again
/// once it is done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reread {
    /// The selected item's details.
    pub details: bool,
    /// The summary the inspector shows of several selected items
    /// ([`Change::Edited`]).
    pub summary: bool,
    /// The grid's results, read in place: the same items stay selected and
    /// the grid where it was scrolled ([`Session::refresh_results`]).
    pub results: bool,
    /// The sidebar's destinations, read with the edit and put in place
    /// before its answer is handed back, for a follow-up that needs them at
    /// once, such as a new folder renamed in place.
    pub navigation: bool,
}

impl Reread {
    pub const NOTHING: Self = Self {
        details: false,
        summary: false,
        results: false,
        navigation: false,
    };
    pub const DETAILS: Self = Self {
        details: true,
        ..Self::NOTHING
    };
    /// The selected items: the one shown and the summary of them all.
    pub const SELECTION: Self = Self {
        details: true,
        summary: true,
        ..Self::NOTHING
    };
    /// The selected items, and the results they may have left or joined.
    pub const RESULTS: Self = Self {
        results: true,
        ..Self::SELECTION
    };
    /// The results alone, for an edit of what the destination holds rather
    /// than of its items.
    pub const RESULTS_ONLY: Self = Self {
        results: true,
        ..Self::NOTHING
    };
    pub const NAVIGATION: Self = Self {
        navigation: true,
        ..Self::NOTHING
    };
}

/// `recordRecentFolderUse`: `folder` first, each folder once, at most 30.
fn record_recent(recent: &mut Vec<picto_library::FolderId>, folder: picto_library::FolderId) {
    recent.retain(|id| *id != folder);
    recent.insert(0, folder);
    recent.truncate(30);
}

/// `recordRecentItems`: `names` first, in their order, then the others,
/// each once, at most 30.
fn record_recent_names(recent: &mut Vec<String>, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let mut next = Vec::with_capacity(30);
    for name in names.iter().chain(recent.iter()) {
        if !next.contains(name) {
            next.push(name.clone());
        }
    }
    next.truncate(30);
    *recent = next;
}

/// Back and Forward without the places that no longer exist, each moved to
/// its destination's new position, and where the current place now is.
fn remap_history(
    history: &[Visit],
    index: usize,
    moved: impl Fn(usize) -> Option<usize>,
) -> (Vec<Visit>, usize) {
    let mut kept = Vec::with_capacity(history.len());
    let mut current = 0;
    for (at, visit) in history.iter().enumerate() {
        if let Some(scope) = moved(visit.scope) {
            // Two steps to the same place, once what was between them is gone.
            if kept.last() != Some(&Visit { scope, ..*visit }) {
                kept.push(Visit { scope, ..*visit });
            }
        }
        if at == index {
            current = kept.len().saturating_sub(1);
        }
    }
    (kept, current)
}

/// Whether `ids` are the same items as `selected`: the grid's selection,
/// rather than one item acted on by itself (the viewer's, a tile dragged
/// alone).
fn same_items(ids: &[RootId], selected: &[RootId]) -> bool {
    if ids.is_empty() || ids.len() != selected.len() {
        return false;
    }
    let mut ids = ids.to_vec();
    let mut selected = selected.to_vec();
    ids.sort_unstable();
    selected.sort_unstable();
    ids == selected
}

/// How long a background refresh waits, `since` the last one finished,
/// which `took` that long: three times as long, between 40 ms and a second,
/// less the time already passed, so that under a stream of changes about a
/// quarter of the time goes to refreshing.
fn refresh_delay(took: Duration, since: Duration) -> Duration {
    (took * 3)
        .clamp(REFRESH_MIN, REFRESH_MAX)
        .saturating_sub(since)
}

#[cfg(test)]
mod tests {
    use super::{
        SubscriptionActivity, Visit, record_recent, refresh_delay, remap_history, same_items,
    };
    use picto_library::RootId;
    use std::time::Duration;

    #[test]
    fn background_refreshes_are_spaced_by_how_long_the_last_took() {
        let ms = Duration::from_millis;
        // A quick refresh long ago: at once.
        assert_eq!(refresh_delay(ms(5), ms(5000)), Duration::ZERO);
        // A quick one just now: 40 ms after it.
        assert_eq!(refresh_delay(ms(5), ms(10)), ms(30));
        // A slow one just now: three times as long, at most a second.
        assert_eq!(refresh_delay(ms(200), Duration::ZERO), ms(600));
        assert_eq!(refresh_delay(ms(900), ms(100)), ms(900));
    }
    use picto_library::FolderId;

    #[test]
    fn only_the_grid_selection_stands_for_the_whole_destination() {
        let ids = |range: std::ops::Range<u32>| range.map(RootId).collect::<Vec<_>>();
        // The loaded selection, acted on as one, in any order.
        let mut reversed = ids(0..500);
        reversed.reverse();
        assert!(same_items(&reversed, &ids(0..500)));
        // One item acted on by itself while everything is selected.
        assert!(!same_items(&ids(3..4), &ids(0..500)));
        assert!(!same_items(&[], &[]));
    }

    #[test]
    fn recent_tags_put_the_new_ones_first_in_their_order() {
        let mut recent = vec!["b".to_owned(), "c".to_owned()];
        super::record_recent_names(&mut recent, &["a".to_owned(), "c".to_owned()]);
        assert_eq!(recent, ["a", "c", "b"]);
        let many = (0..40).map(|n| n.to_string()).collect::<Vec<_>>();
        super::record_recent_names(&mut recent, &many);
        assert_eq!(recent.len(), 30);
        assert_eq!(recent[0], "0");
    }

    #[test]
    fn recent_folders_are_newest_first_each_once_and_at_most_thirty() {
        let mut recent = vec![FolderId(1), FolderId(2), FolderId(3)];
        record_recent(&mut recent, FolderId(3));
        assert_eq!(recent, [FolderId(3), FolderId(1), FolderId(2)]);
        record_recent(&mut recent, FolderId(9));
        assert_eq!(recent[0], FolderId(9));
        for id in 100..140 {
            record_recent(&mut recent, FolderId(id));
        }
        assert_eq!(recent.len(), 30);
        assert_eq!(recent[0], FolderId(139));
    }

    #[test]
    fn subscription_activity_is_read_as_the_overview_reads_it() {
        let list = serde_json::json!({
            "global_paused": false,
            "subscriptions": [
                { "active_run_id": null, "status": "running" },
                { "active_run_id": 4, "status": "failed" },
            ],
        });
        let idle = SubscriptionActivity::from_list(&list);
        assert!(!idle.running && !idle.paused && !idle.shown());
        let list = serde_json::json!({
            "global_paused": true,
            "subscriptions": [{ "active_run_id": 9, "status": "pending" }],
        });
        let running = SubscriptionActivity::from_list(&list);
        assert!(running.running && running.paused && running.shown());
        assert!(!SubscriptionActivity::from_list(&serde_json::json!({})).shown());
    }

    #[test]
    fn history_drops_deleted_places_and_follows_moved_ones() {
        let visit = |scope| Visit {
            scope,
            viewing: false,
            place: None,
        };
        // Destinations 0..4; 2 was deleted and 3 moved up to 2.
        let moved = |index: usize| match index {
            2 => None,
            3 => Some(2),
            other => Some(other),
        };
        let history = [visit(0), visit(2), visit(3), visit(1)];
        let (kept, current) = remap_history(&history, 2, moved);
        assert!(kept == [visit(0), visit(2), visit(1)]);
        assert_eq!(current, 1);
        // The current place itself deleted: the one before it is current.
        let (kept, current) = remap_history(&history, 1, moved);
        assert_eq!(kept.len(), 3);
        assert_eq!(current, 0);
        // Steps that meet once the place between them is gone become one.
        let (kept, _) = remap_history(&[visit(1), visit(2), visit(1)], 2, moved);
        assert!(kept == [visit(1)]);
    }
}
