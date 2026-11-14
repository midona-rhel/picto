//! What is being browsed: the destination, its results, the selection and the
//! viewer. One model that every view reads, so a view repaints only for the
//! kind of change it shows and no view owns another's state.
mod edits;
mod events;
mod navigation;
mod results;
mod selection;

pub use edits::{Report, Reread, edit_then, edit_then_in};

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
/// How often the session looks for the application's events (replies from
/// the library's thread wake it at once).
const RECEIVE_INTERVAL: Duration = Duration::from_millis(30);
/// `VIEW_PREFS_SAVE_DEBOUNCE_MS`: a view change is saved once it has rested
/// this long.
const VIEW_SAVE_DELAY: Duration = Duration::from_millis(500);
/// How long quitting waits for the library's thread to save a view change.
const QUIT_WAIT: Duration = Duration::from_millis(200);
/// `gridController.scheduleRefresh`: a refresh after background changes
/// waits at least this long, and at most a second, three times as long as
/// the last refresh took, counted from when it finished.
const REFRESH_MIN: Duration = Duration::from_millis(40);
const REFRESH_MAX: Duration = Duration::from_millis(1000);
/// While the grid cannot be refreshed (a drag, a scope change, a page on
/// its way), a wanted refresh looks again after this long.
const REFRESH_RETRY: Duration = Duration::from_millis(100);

/// A read of something that can change while it is read. Asked for again
/// while one is under way, it runs once more when that one is done, rather
/// than twice at once or not at all.
#[derive(Debug, Default)]
pub struct Coalesced(Option<bool>);

impl Coalesced {
    /// Whether to start a read now; while one is under way, another is
    /// noted instead.
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

    /// The read is done: whether it was asked for again meanwhile.
    pub fn end(&mut self) -> bool {
        self.0.take() == Some(true)
    }
}

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

/// A file's thumbnail in a library's `blobs` folder, under its content
/// hash, for work that runs away from the session.
pub fn thumbnail_in(blobs: &std::path::Path, hash: &str) -> PathBuf {
    blob_in(blobs, "t", hash, "jpg")
}

/// `blobs/<kind>/<ab>/<cd>/<hash>.<extension>`, the layout core's blob
/// store keeps (`BlobStore::original_path_with_ext`, `thumbnail_path_with_ext`;
/// opening a `BlobStore` writes its staging folder, so the layout is
/// followed here). A hash the library hands back that is not hexadecimal,
/// and so could step out of the folder or split a character, names no file.
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
    activity_reading: Coalesced,
    /// Reads progress again while a run is going.
    activity_poll: Option<Task<()>>,
    /// The sidebar's destinations are being read again.
    navigation_reading: Coalesced,
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
    /// The subfolder tiles selected in the grid (`folderNodeIds`), by node
    /// id (`folder:5`). Selecting items clears them, and the other way round.
    pub folder_selection: Vec<String>,
    /// Items to select once the destination's results are read afresh: the
    /// items a reorder drag put back, as production keeps them selected.
    pub reselect: Option<Vec<RootId>>,
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
    /// The library the copy was made from, which the recents are kept under.
    library_key: String,
    /// Whether `root` passed the guard when the session began. The root
    /// never changes, so the guard's file checks are not run again.
    disposable: bool,
    pub library_cover: Option<PathBuf>,
    /// The grid draws its thumbnails without colour.
    pub grayscale: bool,
    /// A folder of the session's own, in the copy's temporary folder, that
    /// the library cover's crops are written to.
    scratch: Option<tempfile::TempDir>,
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
                // A reply is taken in as soon as it is sent; the application's
                // events at the next look.
                let tick = std::pin::pin!(cx.background_executor().timer(RECEIVE_INTERVAL));
                match &mut woken {
                    Some(wake) => {
                        let first = futures::future::select(tick, wake.next());
                        // The library's thread has stopped: looks alone.
                        if let Either::Right((None, _)) = first.await {
                            woken = None;
                        }
                        // One look takes every reply sent so far, so the
                        // wakes sent with them are spent too.
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
        // In the copy's own temporary folder, beside it, else the system's.
        let scratch = root
            .parent()
            .and_then(|holder| {
                tempfile::Builder::new()
                    .prefix("scratch.")
                    .tempdir_in(holder)
                    .ok()
            })
            .or_else(|| tempfile::tempdir().ok());
        // The library's cover is cropped off the thread that draws, and
        // shown once it is ready.
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
        // A zoom change still waiting to be saved is saved as the app quits,
        // before the application closes the library.
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
            navigation_reading: Coalesced::default(),
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

    /// Whether the open library is a marked disposable copy, which edits
    /// that cannot be undone require.
    pub fn disposable(&self) -> bool {
        self.disposable
    }

    /// The disposable copy's canonical root.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The session's scratch folder, for the library cover's crops: its own
    /// temporary folder, or, when none could be made, the folder that holds
    /// the copy.
    pub fn scratch(&self) -> &std::path::Path {
        match &self.scratch {
            Some(scratch) => scratch.path(),
            None => self.root.parent().unwrap_or(&self.root),
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
        blob_in(
            &self.blobs,
            "f",
            hash,
            picto_core::blob_store::mime_to_extension(mime),
        )
    }

    pub fn thumbnail(&self, hash: &str) -> PathBuf {
        thumbnail_in(&self.blobs, hash)
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
}

#[cfg(test)]
mod tests {
    use super::{Coalesced, SubscriptionActivity, thumbnail_in};

    #[test]
    fn a_read_asked_for_while_one_runs_runs_once_more() {
        let mut reading = Coalesced::default();
        assert!(reading.begin());
        // Two more asks while it runs: one more read, not two.
        assert!(!reading.begin());
        assert!(!reading.begin());
        assert!(reading.end());
        assert!(reading.begin());
        assert!(!reading.end());
        // Nothing under way: the next ask starts at once.
        assert!(reading.begin());
    }
    use std::path::{Path, PathBuf};

    /// The blob layout core keeps; a hash that is not hexadecimal names no
    /// file, so it can neither leave the folder nor split a character.
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
