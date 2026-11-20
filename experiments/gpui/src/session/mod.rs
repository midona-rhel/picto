//! `Session` holds the state of browsing one open library: the list of
//! destinations in the sidebar and which one is open, the results the grid
//! shows for it, the selection, and whether the viewer is open. Every view
//! (the sidebar, the toolbar, the grid, the inspector, the viewer) reads this
//! one model. With every change the session emits a `Change` that names the
//! kind of state that changed, so each view redraws only for the kinds it
//! shows, and no view keeps state that belongs to another view.
mod edits;
mod events;
mod navigation;
mod results;
mod selection;
pub mod settle;

pub use edits::{Report, Reread, edit_then, edit_then_in, read_then, read_then_in};

use crate::application::{Application, Event};
use crate::backend::{self, Arrived, Navigation, Reply, Request};
use crate::grid::selection::{End, Selection};
use crate::grid::{filters::Filters, prefs::ViewPrefs};
use crate::motion;
use gpui::*;
use picto_library::predicate::FilterExpr;
use picto_library::query::{RootQuery, RootSummary};
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

/// The stage of the fade that plays when the grid's contents are replaced:
/// when another destination opens, or when a change of view preferences, such
/// as a new sort, reads the open destination again. The stages run in the
/// order `WorkspaceSurface.tsx` runs them. `FadingOut`: what is on screen
/// fades out over 170 ms (`motion::SCOPE_FADE`). `Waiting`: the grid stays
/// invisible until the new results have arrived. `FadingIn`: the new results
/// fade in over 170 ms. `Idle`: no fade is running. The two fading stages hold
/// the time at which the fade started.
#[derive(Clone, Copy)]
pub enum Transition {
    Idle,
    FadingOut(Instant),
    Waiting,
    FadingIn(Instant),
}

impl Transition {
    /// The opacity, from 0 to 1, that the grid and the scope title are drawn
    /// with at this stage of the fade.
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

/// The kind of state that changed. The session emits one of these with every
/// change, and each view redraws only for the kinds it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// The list of destinations, the open destination or the place open inside
    /// it, the Back and Forward history, the stage of the fade (`Transition`),
    /// or the library's cover changed.
    Navigation,
    /// The results changed: the items the grid holds, their total, whether
    /// they are loading, the view preferences, the zoom, the filter bar's
    /// settings, or the grayscale setting.
    Results,
    /// The width the grid lays its tiles out in changed. The same zoom may
    /// give a different number of columns at the new width, so the toolbar
    /// updates its zoom control.
    GridWidth,
    /// The selected items, the selected subfolder tiles, or the item the
    /// inspector shows changed.
    Selection,
    /// The details of the item the inspector shows arrived.
    Details,
    /// An edit of the selected items has finished. The inspector reads its
    /// summary of the selected items again.
    Edited,
    /// The viewer opened or closed, or it now shows another member of the
    /// collection it has open.
    Viewer,
    /// Whether any subscription is running, or whether all subscriptions are
    /// paused, changed. The sidebar's Subscriptions row shows this.
    Activity,
}

/// The state of subscriptions that the sidebar's Subscriptions row shows with
/// its pause and resume button. `running` is true while any subscription has a
/// run in progress (`runningSubscriptionIds`). `paused` is true while all
/// subscriptions are paused (`globalPaused`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SubscriptionActivity {
    pub running: bool,
    pub paused: bool,
    /// True from the click on the row's button until the
    /// `subscriptions.pause_all` command has answered. Clicks on the button
    /// are ignored during that time.
    pub busy: bool,
}

impl SubscriptionActivity {
    /// Builds the state from the answer of the `subscriptions.list` command,
    /// the way `getSubscriptionOverview` does. A subscription counts as
    /// running when it has an `active_run_id` and its `status` is `pending` or
    /// `running`. `paused` is the answer's `global_paused` field.
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

    /// Whether the Subscriptions row shows its pause and resume button. It
    /// does while a subscription is running or all subscriptions are paused.
    pub fn shown(&self) -> bool {
        self.running || self.paused
    }
}

/// How often the session reads `subscriptions.list` again while a subscription
/// run is in progress: every 1.5 seconds, as `subscriptionsSettle.ts` does.
pub const SUBSCRIPTION_POLL: Duration = Duration::from_millis(1500);
/// The session checks for events from the application layer every 30 ms.
/// Replies from the library's thread do not wait for this: the thread wakes
/// the session each time it sends one.
const RECEIVE_INTERVAL: Duration = Duration::from_millis(30);
/// A zoom change is saved to the destination's view preferences 500 ms after
/// the last change (`VIEW_PREFS_SAVE_DEBOUNCE_MS`).
const VIEW_SAVE_DELAY: Duration = Duration::from_millis(500);
/// When the app quits while a zoom change is not saved yet, the main thread
/// waits at most 200 ms for the library's thread to write it.
const QUIT_WAIT: Duration = Duration::from_millis(200);
/// The shortest delay before the grid's results are read again after a
/// background change: 40 ms (`gridController.scheduleRefresh`). The delay is
/// three times as long as the previous refresh took, limited to the range from
/// `REFRESH_MIN` to `REFRESH_MAX`, and it is counted from the moment the
/// previous refresh finished (see `refresh_delay` in `results.rs`).
const REFRESH_MIN: Duration = Duration::from_millis(40);
/// The longest that delay can be: 1 second.
const REFRESH_MAX: Duration = Duration::from_millis(1000);
/// When a background refresh is due but cannot run yet, it is tried again
/// after 100 ms. It cannot run while the pointer is pressed, drawing a rubber
/// band or dragging in the grid, while results are loading, or while the fade
/// between destinations is running.
const REFRESH_RETRY: Duration = Duration::from_millis(100);

/// Keeps track of a read that may be requested again while it is still
/// running. `None` means no read is running. `Some(false)` means one is
/// running. `Some(true)` means one is running and at least one more was
/// requested meanwhile. A request made during a running read does not start a
/// second read at the same time. The caller runs the read once more when the
/// running one finishes, so the data read last is never older than the last
/// request.
#[derive(Debug, Default)]
pub struct Coalesced(Option<bool>);

impl Coalesced {
    /// Call this when a read is requested. Returns true when the caller should
    /// start the read now. Returns false when a read is already running; the
    /// request is remembered and `end` reports it.
    pub fn begin(&mut self) -> bool {
        match &mut self.0 {
            Some(again) => {
                *again = true;
                false
            }
            None => {
                self.0 = Some(false);
                true
            }
        }
    }

    /// Call this when the read finishes. Returns true when another read was
    /// requested while it ran, in which case the caller starts the read again.
    pub fn end(&mut self) -> bool {
        self.0.take() == Some(true)
    }
}

/// One entry in the Back and Forward history.
#[derive(Clone, Copy, PartialEq)]
struct Visit {
    scope: usize,
    /// True when this entry is the viewer open on an item of the destination.
    /// False when it is the destination's grid or screen.
    viewing: bool,
    /// The place open inside a screen, by id: for example the id of the
    /// subscription that the Subscriptions screen has open. `None` is the
    /// screen's own top level.
    place: Option<i64>,
    /// The filter bar's filters that the grid had at this entry, as an index
    /// into `Session::filter_steps`. Changing the filters adds a history entry
    /// of its own (`navigateWithGridFilters`), so Back returns to the previous
    /// filters.
    filters: usize,
}

/// The state of a destination's grid at the moment the user left it. It is
/// kept so that Back and Forward can show the grid again as it was, without
/// reading the library: the window of results, the scroll position, the view
/// preferences and zoom, and the selection. Most fields are copies of the
/// `Session` fields of the same name; `scroll` is a copy of
/// `Session::grid_scroll`.
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
    /// The search text and the filter bar's filters that these results were
    /// read with. `step_history` shows a saved grid again only when its search
    /// text is empty and its filters are the ones the history entry asks for.
    search_text: String,
    filters: Filters,
}

/// The item that the inspector and the viewer show, with its ordinal (its
/// position in the destination's order). The session keeps its own copy of the
/// item, so the item is still available after the grid has loaded a window of
/// results that does not contain it.
#[derive(Clone)]
struct Focused {
    ordinal: usize,
    item: RootSummary,
}

/// The path of the thumbnail of the file with content hash `hash`, inside a
/// library's `blobs` folder: `blobs/t/<ab>/<cd>/<hash>.jpg`. This is a
/// function outside `Session` so that code which has the `blobs` path but
/// cannot use the session can call it. The covers of the subfolder tiles are
/// read that way on the library's thread (`grid/subfolders.rs`).
pub fn thumbnail_in(blobs: &std::path::Path, hash: &str) -> PathBuf {
    blob_in(blobs, "t", hash, "jpg")
}

/// Builds the path `blobs/<kind>/<ab>/<cd>/<hash>.<extension>`, where `<ab>`
/// and `<cd>` are the first two and the next two characters of the hash. This
/// is the layout that the blob store in the `picto_core` crate uses
/// (`BlobStore::original_path_with_ext`, `thumbnail_path_with_ext`). The path
/// is built here and not by a `BlobStore`, because opening a `BlobStore`
/// creates its staging folder on disk.
///
/// Returns an empty path when `hash` is shorter than four characters or
/// contains a character that is not a hexadecimal digit. Such a hash could
/// contain `..` or a path separator and lead outside the `blobs` folder. It
/// could also contain a character of several bytes, and cutting the hash in
/// the middle of such a character makes Rust panic.
fn blob_in(blobs: &std::path::Path, kind: &str, hash: &str, extension: &str) -> PathBuf {
    if hash.len() < 4 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return PathBuf::new();
    }
    blobs
        .join(kind)
        .join(&hash[..2])
        .join(&hash[2..4])
        .join(format!("{hash}.{extension}"))
}

/// The average height in pixels that one item takes in the grid's layout,
/// together with the layout geometry it was measured for: the grid's width,
/// the zoom's tile size (as the bits of the `f32`), the spacing, the layout
/// mode, and the height of the text rows under a tile. The grid measures it on
/// the first items it lays out and multiplies it by the destination's total to
/// estimate the scrollable height of the whole destination. A measurement is
/// reused only while the geometry is the same.
pub type Density = ((u32, u32, u32, crate::grid::layout::ViewMode, u32), f32);

pub struct Session {
    sender: Sender<Request>,
    receiver: Receiver<Reply>,
    /// The application layer of the open library. `dispatch` and `call` send
    /// commands to it, and its events arrive on `events`.
    application: Arc<Application>,
    events: Receiver<Event>,
    pub subscriptions: SubscriptionActivity,
    /// Tracks the read of `subscriptions.list` that `refresh_activity` makes
    /// for the sidebar's Subscriptions row, so that only one runs at a time.
    activity_reading: Coalesced,
    /// The timer that calls `refresh_activity` again after 1.5 seconds
    /// (`SUBSCRIPTION_POLL`). It is set only while a subscription is running.
    activity_poll: Option<Task<()>>,
    /// The model that decides which notifications to show about subscription
    /// runs and gallery downloads (see `settle.rs`). It remembers what it has
    /// already announced and which reads it is waiting for.
    settle: settle::Settle,
    /// Tracks the read of the sidebar's destinations and counts that
    /// `refresh_navigation` makes, so that only one runs at a time.
    navigation_reading: Coalesced,
    pub nav: Vec<Navigation>,
    pub tags: HashMap<u32, TagRecord>,
    /// The open destination, as an index into `nav`. It changes as soon as the
    /// user chooses a destination.
    pub scope: usize,
    /// The destination whose grid and title are on screen, as an index into
    /// `nav`. While the previous destination fades out and the new one's
    /// results have not arrived, this is still the previous destination. It
    /// becomes equal to `scope` when the new one is shown.
    pub displayed: usize,
    /// The Back and Forward history. Opening a destination, changing the
    /// filter bar's filters, opening a place inside a screen and opening the
    /// viewer each add an entry. `history_index` is the entry the user is at.
    history: Vec<Visit>,
    history_index: usize,
    /// The grid of each destination as the user last left it, keyed by the
    /// destination's index in `nav` and the index of its filters in
    /// `filter_steps`. Back and Forward show a saved grid at the scroll
    /// position it had, without reading the library. The map is emptied
    /// whenever the library changes or the results are read again after an
    /// edit, because the saved items may then be out of date.
    saved: HashMap<(usize, usize), SavedGrid>,
    /// Every different set of filter bar filters that a history entry has
    /// used. `Visit::filters` is an index into this list. Index 0 is the empty
    /// set, which filters nothing.
    filter_steps: Vec<Filters>,
    /// A saved grid that is shown when what is on screen has finished fading
    /// out.
    pending_restore: Option<SavedGrid>,
    /// The grid's scroll offset in pixels. The grid writes it each time it
    /// draws (`GridView::prepare` in `grid/frame.rs`).
    pub grid_scroll: f32,
    /// The scroll offset in pixels that the grid starts at the next time it
    /// draws new results. It is set when a saved grid is restored, to the
    /// offset that grid had. The grid takes the value, so it applies once.
    /// When it is `None`, new results start at the top.
    pub restore_scroll: Option<f32>,
    pub transition: Transition,
    /// The first window of results of the destination that is opening, when it
    /// arrived while the previous grid was still fading out. It is shown when
    /// the fade-out finishes.
    pending_page: Option<Box<Arrived>>,
    /// The window of results the session holds: at most `window::WINDOW`
    /// (1,500) items of the destination. `start` is the ordinal of `items[0]`,
    /// which is its position in the destination's order.
    pub items: Vec<RootSummary>,
    pub start: usize,
    /// The first items of the destination, whichever part of it the window
    /// holds. The inspector uses them for its preview of the destination
    /// (`inspector/scope.rs`).
    pub head: Vec<RootSummary>,
    /// A counter that goes up each time the results are replaced by a
    /// different set: the results of a read from the first page arrived, a
    /// saved grid was restored, or the results were emptied because a screen
    /// or an error is shown. It does not go up when another window of the same
    /// results arrives or when the results are refreshed in place. The grid
    /// compares it with the value it last drew, and when it differs the grid
    /// resets its scroll position and its layout.
    pub results_epoch: u64,
    /// A counter that goes up each time `items` is replaced: when another
    /// window arrives, when the window is refreshed in place, and every time
    /// `results_epoch` goes up. The grid lays its tiles out again when it
    /// changes.
    pub window_epoch: u64,
    /// The number of items in the whole destination with the current search
    /// and filters, not only in the window.
    pub total: u64,
    /// The combined file size in bytes of all those items, not only of the
    /// ones in the window.
    pub total_size_bytes: u128,
    /// The state of the requests for another window of results
    /// (`requestWindowAt`). `window_request` is the number given to the newest
    /// request. `window_active` is the request that was sent to the library's
    /// thread and has not been answered yet, as its number and the start
    /// ordinal of its window. `window_queued` is the one request that waits
    /// until the active one is answered. A window that arrives is shown only
    /// if its number equals `window_request`; an older one is dropped.
    window_request: u64,
    window_active: Option<(u64, usize)>,
    window_queued: Option<(u64, usize)>,
    /// The ordinal of an item that a keyboard move, or the viewer's next or
    /// previous, moved to while the window did not hold it. The item is
    /// selected when its window arrives. The `bool` is true when the move
    /// extends the selection as a range.
    pending_focus: Option<(usize, bool)>,
    /// An item that becomes the only selected item when the refresh in
    /// progress reports its ordinal. It is used for an item that an edit has
    /// just created, for example a new collection, which the window does not
    /// hold yet (production calls `replace_items` with the new collection).
    pending_select: Option<RootId>,
    /// The ordinal of an item that the grid scrolls into view the next time it
    /// lays out a window that holds it. The grid then clears it.
    pub reveal: Option<usize>,
    /// The average height per item that the grid measured (see `Density`). It
    /// is kept here and not in the grid so that it stays the same while other
    /// windows of the same results load. The estimated scrollable height then
    /// does not change as the user scrolls.
    pub density: Option<Density>,
    /// `window_top` is the distance in pixels from the top of the scrollable
    /// height to the top of the window's tiles, as the grid last placed them.
    /// The grid writes it each time it draws. `restore_window_top` is that
    /// distance for a saved grid that is being restored: the grid places the
    /// window there again, so that the saved scroll offset shows the same
    /// items.
    pub window_top: f32,
    pub restore_window_top: Option<f32>,
    pub loading: bool,
    pub error: Option<String>,
    /// The number of the newest read of results. It goes up each time the
    /// destination is read from its first page or refreshed in place, and when
    /// a destination opens that needs no read. A reply that carries another
    /// number is ignored.
    generation: u64,
    pub prefs: ViewPrefs,
    /// The destination, as an index into `nav`, that the zoom in `tile` was
    /// last taken from. `apply_open` takes the saved zoom from the view
    /// preferences only when the open destination is not this one.
    prefs_scope: Option<usize>,
    /// The zoom's tile size in pixels. The layout turns it into a number of
    /// columns for the grid's width (`grid::layout::columns`), or uses it as
    /// the row height in the justified layout.
    pub tile: f32,
    /// The width in pixels the grid lays its tiles out in. The grid sets it
    /// each time it draws. It is 0 until the grid has drawn once.
    pub grid_width: f32,
    /// The selected items. Either a set of item ids chosen one by one, or a
    /// query-wide selection: every item of the destination (Select All), or
    /// ranges of the destination's order (Shift-click), which can include
    /// items the window does not hold. An edit of a query-wide selection sends
    /// the query to the library, which finds the items (`selection_target`),
    /// as production does.
    pub selection: Selection,
    /// The subfolder tiles selected in the grid (`folderNodeIds`), as node ids
    /// such as `folder:5`. Items and subfolder tiles are never selected
    /// together: selecting an item empties this list, and selecting a
    /// subfolder tile empties `selection`.
    pub folder_selection: Vec<String>,
    /// Items to select when the destination's results, which are being read
    /// again from the first page, arrive. The grid sets it after a drag that
    /// reordered items in a folder, so that the dragged items are still
    /// selected afterwards, as in production.
    pub reselect: Option<Vec<RootId>>,
    /// The query that the results in `items` were read with. Requests for
    /// another window and refreshes in place send it again.
    ran: Option<RootQuery>,
    /// The one selected item that the inspector and the viewer show. It is the
    /// item that was selected most recently.
    focus: Option<Focused>,
    /// The item a range selection starts from. A Shift-click or a move with
    /// Shift held selects from this item to the item clicked or moved to.
    anchor: Option<End>,
    pub details: Option<RootDetails>,
    pub viewer: bool,
    /// The index of the member of a collection that the viewer shows. 0 is the
    /// first member.
    pub member: usize,
    search_text: String,
    pub filter: Option<FilterExpr>,
    /// The filter bar's filters, which `filter` is built from.
    pub filters: Filters,
    /// Whether the filter bar shows under the titlebar.
    pub filter_bar_open: bool,
    /// Whether the filter bar's filters are kept when another destination
    /// opens. When false, a destination chosen in the sidebar opens without
    /// filters.
    pub filters_locked: bool,
    /// View preference changes that results still on their way may not
    /// include. The `u64` is the value of `generation` when the user made the
    /// change, and the JSON object holds the changed fields. Results of a
    /// query with that number or a lower one may have been read before the
    /// change was saved, so `fresh_prefs` applies these fields on top of the
    /// preferences those results bring. The first results of a newer query
    /// clear it.
    view_patch: Option<(u64, serde_json::Value)>,
    blobs: PathBuf,
    /// The folder of the open library. It is either a disposable copy or a
    /// library the user keeps.
    root: PathBuf,
    /// The cloud sync state of the library. The shell creates it and sets this
    /// field.
    pub cloud: Option<Entity<crate::cloud::Cloud>>,
    /// The key under which this library's recent folders are stored in the
    /// app's state. For a disposable copy it is the path of the library the
    /// copy was made from, so the recent folders are still there when a new
    /// copy is made. For any other library it is the library's own path.
    library_key: String,
    /// Whether `root` is a marked disposable copy: the result of
    /// `application::guard(&root)` when the session was created. The result is
    /// stored because `root` never changes, so the guard's file checks do not
    /// have to run again.
    disposable: bool,
    pub library_cover: Option<PathBuf>,
    /// When true, the grid draws its thumbnails in grayscale.
    pub grayscale: bool,
    /// A temporary folder that belongs to this session. The cropped image of
    /// the library's cover is written to it. For a disposable copy the folder
    /// is created in the folder that holds the copy: a folder named
    /// `picto-gpui-…` in a temporary folder, made for that copy alone. For a
    /// library the user keeps, or if that fails, it is created in the system's
    /// temporary folder. `None` when no folder could be created.
    scratch: Option<tempfile::TempDir>,
    /// The name of the place open inside a screen, which the toolbar shows in
    /// the scope title. The screen sets it with `set_place_label`.
    pub place_label: Option<SharedString>,
    /// The folders that items were most recently added to, newest first, at
    /// most 30. The folder picker's Recent view and the grid menu's "Add to
    /// “…”" row use them (`useRecentFolders`). Production stores them per
    /// library in the renderer's localStorage. This app stores them in its
    /// state (`crate::state`) under `library_key`.
    pub recent_folders: Vec<picto_library::FolderId>,
    /// The tags most recently added to items, newest first, at most 30. The
    /// Add Tags dialog's Recent view shows them (`picto-recent-tags`). They
    /// are stored in the app's state.
    pub recent_tags: Vec<String>,
    /// True when `recent_folders` or `recent_tags` changed and has not been
    /// written to the app's state yet. `keep_recents` writes them the next
    /// time `receive` runs. The methods that record a use cannot write them,
    /// because they do not have the `App` context that writing needs.
    recents_changed: bool,
    /// A zoom change that has not been saved to its destination's view
    /// preferences yet: the destination's `view_pref` key, the tile size, and
    /// the timer that saves it after 500 ms (`VIEW_SAVE_DELAY`).
    tile_save: Option<(String, f32, Task<()>)>,
    /// True while a refresh in place is waiting for its answer. The grid keeps
    /// showing what it showed until the answer arrives, even if that is the
    /// empty state, and does not show the loading state.
    pub refreshing: bool,
    /// True while the pointer is pressed on the grid, drawing a rubber band,
    /// or dragging items. A background refresh waits until this is false,
    /// because replacing the items would interrupt the gesture. The grid
    /// writes it each time it draws.
    pub pointer_busy: bool,
    /// `refresh_wanted` is true when a change made in the background (an
    /// import, a subscription run) means the shown results must be read again
    /// (`requestReconcile`) and that read has not started yet. `refresh_timer`
    /// is the timer that starts the read.
    refresh_wanted: bool,
    refresh_timer: Option<Task<()>>,
    /// When the last background refresh started, how long it took, and when it
    /// finished. `refresh_started` is set only while one is running.
    /// `refresh_delay` uses the other two to decide how long the next
    /// background refresh waits.
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
        let disposable = crate::application::guard(&root).is_ok();
        let library_key = crate::state::library_key(&root);
        let (recent_folders, recent_tags) = {
            let kept = crate::state::get(cx);
            let folders = kept
                .recent_folders
                .get(&library_key)
                .map(|ids| ids.iter().map(|id| picto_library::FolderId(*id)).collect())
                .unwrap_or_default();
            (folders, kept.recent_tags.clone())
        };
        let (sender, receiver, woken) = backend::start(application.clone());
        cx.spawn(async move |session, cx| {
            use futures::{StreamExt as _, future::Either};
            let mut woken = Some(woken);
            loop {
                // Each turn of the loop waits for whichever comes first: the
                // 30 ms timer (`RECEIVE_INTERVAL`) or a wake from the
                // library's thread, which sends one with every reply. So a
                // reply is handled as soon as it is sent, and an event from
                // the application layer is handled at most 30 ms after it
                // arrives.
                let tick = std::pin::pin!(cx.background_executor().timer(RECEIVE_INTERVAL));
                match &mut woken {
                    Some(wake) => {
                        let first = futures::future::select(tick, wake.next());
                        // The wake channel has closed, which means the
                        // library's thread has stopped. From now on the loop
                        // only waits for the timer.
                        if let Either::Right((None, _)) = first.await {
                            woken = None;
                        }
                        // The call to `receive` below handles every reply sent
                        // so far. Remove the wakes already queued for those
                        // replies, so that they do not each cause another call
                        // that finds nothing.
                        while let Some(Ok(())) = woken.as_mut().map(|wake| wake.try_recv()) {}
                    }
                    None => tick.await,
                }
                if session
                    .update(cx, |this: &mut Self, cx| this.receive(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // Create the scratch folder. For a disposable copy it goes in the
        // folder that holds the copy: a folder named `picto-gpui-…` in a
        // temporary folder, made for that copy alone. For a library the user
        // keeps it goes in the system's temporary folder, because the folder
        // that holds such a library belongs to the user and this app must not
        // write to it. The system's temporary folder is also used if the first
        // attempt fails.
        let scratch = root
            .parent()
            .filter(|_| disposable)
            .and_then(|holder| {
                tempfile::Builder::new()
                    .prefix("scratch.")
                    .tempdir_in(holder)
                    .ok()
            })
            .or_else(|| tempfile::tempdir().ok());
        // Crop the library's cover on a background thread, so that the main
        // thread is not blocked, and show it when it is ready.
        if let Some(folder) = scratch.as_ref().map(|scratch| scratch.path().to_path_buf()) {
            let library = root.clone();
            let prepared = cx
                .background_executor()
                .spawn(async move { crate::cover::prepare(&library, &folder) });
            cx.spawn(async move |session, cx| {
                let cover = prepared.await;
                let _ = session.update(cx, |this: &mut Self, cx| {
                    if cover.is_some() && this.library_cover.is_none() {
                        this.library_cover = cover;
                        cx.emit(Change::Navigation);
                    }
                });
            })
            .detach();
        }
        // When the app quits, save a zoom change that is still waiting for its
        // 500 ms delay. The save has to reach the library's thread before the
        // quit handler in `main.rs` closes the library
        // (`libraries::host::close_for_quit`).
        cx.on_app_quit(|this: &mut Self, _| {
            this.save_view_now();
            async {}
        })
        .detach();
        Self {
            sender,
            receiver,
            application,
            events,
            subscriptions: SubscriptionActivity::default(),
            activity_reading: Coalesced::default(),
            activity_poll: None,
            settle: settle::Settle::default(),
            navigation_reading: Coalesced::default(),
            nav: Vec::new(),
            tags: HashMap::new(),
            scope: 0,
            displayed: 0,
            history: vec![Visit {
                scope: 0,
                viewing: false,
                place: None,
                filters: 0,
            }],
            history_index: 0,
            saved: HashMap::new(),
            filter_steps: vec![Filters::default()],
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
            pending_select: None,
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
            grid_width: 0.,
            selection: Selection::default(),
            folder_selection: Vec::new(),
            reselect: None,
            focus: None,
            anchor: None,
            details: None,
            viewer: false,
            member: 0,
            search_text: String::new(),
            filter: None,
            filters: Filters::default(),
            filter_bar_open: false,
            filters_locked: false,
            view_patch: None,
            blobs,
            root,
            cloud: None,
            disposable,
            library_key,
            library_cover: None,
            grayscale: false,
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

    /// Disconnects this session from the library before the library is closed
    /// (`libraries::host` calls this). The sender to the library's thread is
    /// replaced by one that leads nowhere, so later requests are lost, and the
    /// thread ends once it has answered the requests already sent. The
    /// application layer is told to refuse commands. After this, nothing this
    /// session does can reach the library that is opened next.
    pub fn shut(&mut self) {
        let (sender, _) = std::sync::mpsc::channel();
        self.sender = sender;
        self.application.retire();
    }

    /// Whether the open library is a marked disposable copy. The sidebar's
    /// library switcher shows this, so that the user does not mistake the copy
    /// for the library it was copied from. No feature is enabled or disabled
    /// by it. What may be written is decided by [`Session::writable_root`].
    pub fn disposable(&self) -> bool {
        self.disposable
    }

    /// The folder that edits which write files directly (regenerated
    /// thumbnails, Set as Thumbnail, the library's cover) may write to.
    /// Returns this session's library folder when it is the library open in
    /// the main window, whether it is a disposable copy or a library the user
    /// keeps, and `None` otherwise. Delete Permanently runs only when this
    /// returns a folder. See `libraries::writable`.
    pub fn writable_root(&self, cx: &App) -> Option<std::path::PathBuf> {
        crate::libraries::writable(&self.root, cx)
    }

    /// The canonical path of the open library's folder.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The folder that the cropped image of the library's cover is written to:
    /// this session's own temporary folder, or the system's temporary folder
    /// if the session's could not be created.
    pub fn scratch(&self) -> std::path::PathBuf {
        match &self.scratch {
            Some(scratch) => scratch.path().to_path_buf(),
            None => std::env::temp_dir(),
        }
    }

    /// The folder of the open library: the parent of its `blobs` folder.
    pub fn library_root(&self) -> Option<&std::path::Path> {
        self.blobs.parent()
    }

    /// The text in the search field. The results are filtered by it when it
    /// has three characters or more.
    pub fn search_text(&self) -> &str {
        &self.search_text
    }

    /// The library folder's name without its `.library` suffix.
    pub fn library_name(&self) -> Option<String> {
        let name = self.blobs.parent()?.file_name()?.to_string_lossy();
        Some(name.trim_end_matches(".library").to_owned())
    }

    /// The path of the library's `blobs` folder, which holds the original
    /// files and the thumbnails.
    pub fn blobs(&self) -> &std::path::Path {
        &self.blobs
    }

    /// The id of the place open inside the current screen, for example the
    /// subscription that the Subscriptions screen has open. `None` when the
    /// screen shows its top level, or when the current history entry is for
    /// another destination.
    pub fn place(&self) -> Option<i64> {
        self.history
            .get(self.history_index)
            .filter(|visit| visit.scope == self.scope)
            .and_then(|visit| visit.place)
    }

    /// Opens a place inside the current screen, or the screen's top level when
    /// `place` is `None`, and adds a Back and Forward history entry for it.
    /// Does nothing when that place is already open.
    pub fn open_place(&mut self, place: Option<i64>, cx: &mut Context<Self>) {
        if self.place() == place {
            return;
        }
        let filters = self.filters_step();
        self.record(Visit {
            scope: self.scope,
            viewing: false,
            place,
            filters,
        });
        cx.emit(Change::Navigation);
    }

    /// Sets the name of the open place, which the toolbar shows in the scope
    /// title. `None` removes the name.
    pub fn set_place_label(&mut self, label: Option<SharedString>, cx: &mut Context<Self>) {
        if self.place_label != label {
            self.place_label = label;
            cx.emit(Change::Navigation);
        }
    }

    /// The path of the original file with content hash `hash`:
    /// `blobs/f/<ab>/<cd>/<hash>.<extension>`. The extension is the one the
    /// blob store uses for the MIME type `mime`.
    pub fn original(&self, hash: &str, mime: &str) -> PathBuf {
        blob_in(
            &self.blobs,
            "f",
            hash,
            picto_core::blob_store::mime_to_extension(mime),
        )
    }

    /// The path of the thumbnail of the file with content hash `hash` in the
    /// open library.
    pub fn thumbnail(&self, hash: &str) -> PathBuf {
        thumbnail_in(&self.blobs, hash)
    }

    /// The item that the inspector and the viewer show, if there is one.
    pub fn selected_item(&self) -> Option<&RootSummary> {
        self.focus.as_ref().map(|focused| &focused.item)
    }

    /// The ordinal of the item that the inspector and the viewer show: its
    /// position in the destination's order.
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

    /// Whether the item at `ordinal` is selected. Returns false for an ordinal
    /// the window does not hold.
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

    /// The ids of the selected items, in the grid's order. When the items were
    /// chosen one by one, this is all of them, including items the window no
    /// longer holds. When the selection is query-wide (Select All or a range),
    /// this is only the selected items the window holds, so it can be
    /// incomplete. An edit does not use this list. It uses
    /// [`Session::selection_target`], which covers the whole selection.
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

    /// Whether a request for another window of results has not been answered
    /// yet.
    pub fn window_loading(&self) -> bool {
        self.window_active.is_some() || self.window_queued.is_some()
    }

    /// The details of the item the inspector and the viewer show, or `None`
    /// while they have not arrived. The `details` field still holds the
    /// previous item's details until the new ones arrive, so that the
    /// inspector can keep showing them and is not empty for a moment between
    /// two items. This method returns the details only when they belong to the
    /// current item.
    pub fn selected_details(&self) -> Option<&RootDetails> {
        let id = self.selected_item()?.root_id;
        self.details
            .as_ref()
            .filter(|details| details.root.root_id == id)
    }

    /// The screen that is shown in place of the grid, when the displayed
    /// destination is a screen (Tags, Subscriptions or Duplicates) and not a
    /// grid.
    pub fn screen(&self) -> Option<backend::Screen> {
        self.nav.get(self.displayed)?.screen
    }

    /// Runs `work` on the library's thread and returns a receiver for its
    /// result. Screens use this for the reads and edits that only they need.
    /// The library is never read or written on the main thread, which draws
    /// the interface.
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
    /// `invoke(command, args)` does, and returns a receiver for its JSON
    /// answer. The command runs on the application's runtime, never on the
    /// main thread. The command names and their arguments are those of
    /// `core/src/ipc.rs`.
    pub fn dispatch(
        &self,
        command: impl Into<String>,
        args: serde_json::Value,
    ) -> futures::channel::oneshot::Receiver<Result<serde_json::Value, String>> {
        self.application.dispatch(command, args)
    }

    /// Asks the application layer (`media.resolve_target_paths`) for the
    /// content hash and the path of every file in `target`, in the target's
    /// order. A collection contributes each of its members. The library
    /// resolves the whole target in one request, whatever the number of
    /// selected items.
    pub fn resolve_files(
        &self,
        target: &picto_library::selection::SelectionTarget,
    ) -> impl std::future::Future<Output = Result<Vec<(String, std::path::PathBuf)>, String>> + use<>
    {
        let answer = serde_json::to_value(target)
            .map_err(|error| error.to_string())
            .map(|target| {
                self.call(
                    "media.resolve_target_paths",
                    serde_json::json!({ "target": target }),
                )
            });
        async move {
            let found = answer?.await?;
            Ok(found
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|file| {
                    let hash = file.get("file_hash")?.as_str()?.to_owned();
                    let path = file.get("path")?.as_str()?;
                    Some((hash, std::path::PathBuf::from(path)))
                })
                .collect())
        }
    }

    /// The same as [`Session::dispatch`], but returns a future that resolves
    /// to the answer instead of a receiver.
    pub fn call<C: Into<String>>(
        &self,
        command: C,
        args: serde_json::Value,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + use<C> {
        self.application.call(command, args)
    }
}

#[cfg(test)]
mod tests {
    use super::{Coalesced, SubscriptionActivity, thumbnail_in};

    #[test]
    fn a_read_asked_for_while_one_runs_runs_once_more() {
        let mut reading = Coalesced::default();
        assert!(reading.begin());
        // Two more requests arrive while the read runs. Together they cause
        // one more read, not two.
        assert!(!reading.begin());
        assert!(!reading.begin());
        assert!(reading.end());
        assert!(reading.begin());
        assert!(!reading.end());
        // No read is running, so the next request starts one at once.
        assert!(reading.begin());
    }
    use std::path::{Path, PathBuf};

    /// A thumbnail's path follows the blob store's layout. A hash that is too
    /// short or is not hexadecimal gives an empty path, so it cannot lead
    /// outside the `blobs` folder or cut a character of several bytes in the
    /// middle.
    #[test]
    fn blobs_are_found_under_their_hash_and_only_a_hash() {
        let blobs = Path::new("lib/blobs");
        assert_eq!(
            thumbnail_in(blobs, "abcdef12"),
            Path::new("lib/blobs/t/ab/cd/abcdef12.jpg")
        );
        for hash in ["", "abc", "../../x", "ab/cdef", "é0123", "abcd\\x"] {
            assert_eq!(thumbnail_in(blobs, hash), PathBuf::new(), "{hash}");
        }
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
}
