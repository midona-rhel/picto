//! The Duplicates screen (`DuplicatesScreen.tsx`): the review queue of
//! perceptually similar pairs, one pair at a time, side by side with linked
//! zoom and pan, each file's facts beneath it, and the decisions in a footer.
//! Its counter, zoom and pair controls sit in the titlebar
//! ([`DuplicatesToolbar`]).
mod compare;
mod decisions;
mod difference;
mod view;

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::motion;
use crate::session::{Change, Session};
use crate::ui;
use compare::{Extent, LinkedZoom, PairKey, pair_key};
use gpui::*;
use picto_library::{
    DuplicateCandidate, DuplicateCandidateSide, DuplicateFile, DuplicateResolutionChoice, FileId,
    MediaRecord, RootDetails,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

// The review's actions. Their keys come from the shortcut registry
// (`crate::keymap`) and win over the grid's while the screen has the
// keyboard, as its `useShortcutScope` does.
actions!(
    duplicates,
    [
        PreviousPair,
        NextPair,
        FitPair,
        ActualPixels,
        KeepBoth,
        NotDuplicate,
        KeepLeft,
        KeepRight,
        SmartMerge
    ]
);

const CONTEXT: &str = "Duplicates";
/// `getDuplicatePairs`: the queue is read 500 pairs at a time.
const QUEUE_LIMIT: usize = 500;
/// `scanDuplicates(distanceThreshold = 16)`.
const SCAN_DISTANCE: u32 = 16;
/// `.mediaCard { grid-template-rows: 42px minmax(180px, 1fr) 222px }`.
const CARD_HEADER: f32 = 42.;
const PREVIEW_MIN: f32 = 180.;
const CARD_METADATA: f32 = 222.;
/// The container widths at which the layout tightens (`@container`).
const COMPACT_WIDTH: f32 = 900.;
const NARROW_WIDTH: f32 = 620.;

/// One side of the pair on screen.
struct Side {
    file: DuplicateFile,
    occurrences: usize,
    /// The item holding this file; `None` if its details could not be read.
    details: Option<RootDetails>,
    /// That item's media record for this file (`mediaForFile`).
    media: Option<MediaRecord>,
    thumbnail: Option<Arc<RenderImage>>,
    full: Option<Arc<RenderImage>>,
    /// When the original arrived, for its fade over the thumbnail.
    full_since: Option<Instant>,
    /// An original is on its way.
    full_pending: bool,
}

impl Side {
    fn new(side: &DuplicateCandidateSide, details: Option<RootDetails>) -> Self {
        let media = details.as_ref().and_then(|details| {
            details
                .media
                .iter()
                .find(|media| media.facts.content_hash == side.file.file_hash)
                .or(details.media.first())
                .cloned()
        });
        Self {
            file: side.file.clone(),
            occurrences: side.occurrences.len(),
            details,
            media,
            thumbnail: None,
            full: None,
            full_since: None,
            full_pending: false,
        }
    }

    /// The original to load: this very file, when it is a still image.
    fn original(&self) -> Option<PathBuf> {
        let media = self.media.as_ref()?;
        (self.file.mime_type.starts_with("image/")
            && media.facts.content_hash == self.file.file_hash)
            .then(|| PathBuf::from(&media.file_path))
    }

    fn image(&self) -> Extent {
        Extent::new(
            self.file.pixel_width.unwrap_or(0) as f32,
            self.file.pixel_height.unwrap_or(0) as f32,
        )
    }

    /// How far the original has faded in over the thumbnail.
    fn full_opacity(&self) -> f32 {
        self.full_since.map_or(0., |since| {
            motion::EASE_OUT.ease(motion::progress(since.elapsed(), motion::FULL_IMAGE_FADE))
        })
    }
}

/// The pair on screen.
struct Shown {
    key: PairKey,
    files: (FileId, FileId),
    distance: u32,
    sides: [Side; 2],
    /// The difference picture, once computed.
    difference: Option<Arc<RenderImage>>,
}

/// A decision on the pair on screen (`DuplicateAction`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Decision {
    /// "Not duplicates" and "Keep both": two different files; neither
    /// file is touched.
    KeepBoth,
    /// Keep this file and merge the other's item into it. The other file
    /// is freed and the application deletes it from the copy's blobs.
    Keep(FileId),
    /// Keep whichever file the library judges better, if it can tell.
    Smart,
}

/// What "Duplicate scan complete" says after a scan that found `found` new
/// pairs.
fn scan_message(found: u64) -> String {
    if found > 0 {
        format!("Found {found} new review pairs")
    } else {
        "Scan complete - no new review pairs".into()
    }
}

/// What a fetch is for.
#[derive(Clone, Copy)]
enum Fetch {
    /// Read the queue again; `reset` starts the progress count afresh.
    Queue { reset: bool },
    /// Settle a pair, then read the queue.
    Resolve(FileId, FileId, Decision),
    /// Show another pair of the queue already read.
    Pair(PairKey),
}

/// What came back from the library's thread.
struct Fetched {
    /// The queue, when it was read.
    pairs: Option<Vec<DuplicateCandidate>>,
    /// The pair to show.
    key: Option<PairKey>,
    /// The two items' details, when the pair to show is not the one shown.
    details: Option<[Option<RootDetails>; 2]>,
    /// The sidebar's duplicate count.
    count: Option<u64>,
    /// Smart merge found no clear winner; the pair stays.
    ambiguous: bool,
}

/// A drag of one preview: where it started and the centre then.
struct Drag {
    side: usize,
    from: Point<Pixels>,
    center: (f32, f32),
}

/// The titlebar controls' model (`DuplicateToolbarModel`).
struct ToolbarModel {
    index: usize,
    total: usize,
    can_previous: bool,
    can_next: bool,
    disabled: bool,
    fit: bool,
    actual: bool,
}

pub struct DuplicatesScreen {
    session: Entity<Session>,
    focus: FocusHandle,
    /// Whatever had the keyboard before this screen took it.
    return_focus: Option<FocusHandle>,
    controls: Entity<DuplicatesToolbar>,
    /// The session's destination is this screen.
    visible: bool,
    /// The keyboard was taken for this visit.
    focused_once: bool,
    pairs: Vec<DuplicateCandidate>,
    total: usize,
    /// The queue's length when it was last read afresh, for the count of
    /// decisions made since.
    initial_total: usize,
    shown: Option<Shown>,
    /// The queue is being read for the first time.
    loading: bool,
    /// The loading message, which waits 200 ms.
    loading_message: bool,
    resolving: bool,
    /// Another pair is being prepared.
    navigating: bool,
    error: Option<String>,
    /// Counts fetches, so a late answer to an earlier one is dropped.
    generation: u64,
    zoom: LinkedZoom,
    drag: Option<Drag>,
    difference_hovered: bool,
    /// The two previews' insides and the screen's own bounds, as last laid out.
    panes: [Bounds<Pixels>; 2],
    width: f32,
    comparison_height: f32,
    scroll: ScrollHandle,
    fetch_task: Option<Task<()>>,
    original_tasks: [Option<Task<()>>; 2],
    difference_task: Option<Task<()>>,
    message_task: Option<Task<()>>,
    /// A scan of the library is running.
    scanning: bool,
    /// Its progress bar, which waits 200 ms (`useDelayedFlag`).
    scan_progress: bool,
    scan_task: Option<Task<()>>,
    /// The read of the queue that ends the scan.
    scan_fetch: Option<u64>,
    /// Smart merge is hovered, which rings the file it would keep.
    smart_hovered: bool,
    survivor: motion::Fade,
    /// A card's menu (`MediaCard`'s `contextMenu`).
    menu: ui::menu::Host,
    _session: Vec<Subscription>,
}

impl DuplicatesScreen {
    pub fn new(
        session: Entity<Session>,
        _thumbnails: Entity<Thumbnails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let screen = cx.weak_entity();
        let controls = cx.new(|_| DuplicatesToolbar { screen });
        let subscriptions = vec![
            cx.subscribe_in(&session, window, |this, _, change: &Change, window, cx| {
                if *change == Change::Navigation {
                    this.navigated(window, cx);
                }
            }),
            // A scan made elsewhere (the media worker scans new images by
            // itself) changes the queue; a decision of this screen reads the
            // queue itself and is not read twice.
            cx.subscribe(&session, |this, _, event: &BackendEvent, cx| {
                if let BackendEvent::Changed(changed) = event
                    && crate::application::touches(changed, "duplicates")
                    && this.visible
                    && !this.loading
                    && !this.resolving
                    && !this.scanning
                {
                    this.fetch(Fetch::Queue { reset: true }, cx);
                }
            }),
        ];
        let focus = cx.focus_handle();
        Self {
            session,
            menu: ui::menu::Host::deferred(
                crate::ui::layer::MENU,
                ui::menu::Return::To(focus.clone()),
            ),
            focus,
            return_focus: None,
            controls,
            visible: false,
            focused_once: false,
            pairs: Vec::new(),
            total: 0,
            initial_total: 0,
            shown: None,
            loading: true,
            loading_message: false,
            resolving: false,
            navigating: false,
            error: None,
            generation: 0,
            zoom: LinkedZoom::default(),
            drag: None,
            difference_hovered: false,
            panes: [Bounds::default(); 2],
            width: 0.,
            comparison_height: 0.,
            scroll: ScrollHandle::new(),
            fetch_task: None,
            original_tasks: [None, None],
            difference_task: None,
            message_task: None,
            scanning: false,
            scan_progress: false,
            scan_task: None,
            scan_fetch: None,
            smart_hovered: false,
            survivor: motion::Fade::default(),
            _session: subscriptions,
        }
    }

    /// The titlebar controls this screen drives.
    pub fn toolbar(&self) -> Entity<DuplicatesToolbar> {
        self.controls.clone()
    }

    /// Repaints this screen and its titlebar controls.
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        self.controls.update(cx, |_, cx| cx.notify());
    }

    // ── Visits ──────────────────────────────────────────────────

    /// Follows the session: reads the queue when the screen is chosen,
    /// takes the keyboard once it shows, and gives both back when left.
    fn navigated(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (chosen, displayed) = {
            let session = self.session.read(cx);
            (
                session
                    .nav
                    .get(session.scope)
                    .and_then(|entry| entry.screen)
                    == Some(Screen::Duplicates),
                session.screen() == Some(Screen::Duplicates),
            )
        };
        if chosen != self.visible {
            self.visible = chosen;
            if chosen {
                self.opened(cx);
            } else {
                self.left(window, cx);
            }
        }
        if chosen && displayed && !self.focused_once {
            self.focused_once = true;
            self.return_focus = window.focused(cx);
            window.focus(&self.focus, cx);
        }
        if !chosen && !displayed {
            self.release_originals(cx);
        }
    }

    fn opened(&mut self, cx: &mut Context<Self>) {
        if self.shown.is_none() {
            self.loading = true;
            self.loading_message = false;
            self.message_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(motion::LOADING_MESSAGE_DELAY)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.loading {
                        this.loading_message = true;
                        cx.notify();
                    }
                });
            }));
        } else {
            // The pair left on screen shows at once; its originals, let go
            // once the screen had faded out, load again.
            self.load_originals(cx);
            if self
                .shown
                .as_ref()
                .is_some_and(|shown| shown.difference.is_none())
            {
                self.refresh_difference(cx);
            }
        }
        self.fetch(Fetch::Queue { reset: true }, cx);
    }

    fn left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Nothing still on its way applies any more. What is on screen
        // stays as it is while the screen fades out.
        self.generation += 1;
        self.fetch_task = None;
        self.resolving = false;
        self.navigating = false;
        self.drag = None;
        self.difference_hovered = false;
        self.focused_once = false;
        if self.focus.contains_focused(window, cx) {
            match self.return_focus.take() {
                Some(handle) => window.focus(&handle, cx),
                None => window.blur(cx),
            }
        }
        self.changed(cx);
    }

    /// Lets the originals and the difference picture go once the screen
    /// is no longer painted. They are the large part; the thumbnails stay,
    /// so the pair shows at once when the screen is chosen again.
    fn release_originals(&mut self, cx: &mut Context<Self>) {
        self.original_tasks = [None, None];
        self.difference_task = None;
        let Some(shown) = &mut self.shown else {
            return;
        };
        let mut released = shown.difference.take().into_iter().collect::<Vec<_>>();
        for side in &mut shown.sides {
            released.extend(side.full.take());
            side.full_since = None;
            side.full_pending = false;
        }
        for image in released {
            cx.drop_image(image, None);
        }
    }

    // ── Fetching ────────────────────────────────────────────────

    fn pair(&self, key: PairKey) -> Option<&DuplicateCandidate> {
        self.pairs
            .iter()
            .find(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
    }

    /// No fetch is under way any more.
    fn settle(&mut self) {
        self.loading = false;
        self.loading_message = false;
        self.message_task = None;
        self.resolving = false;
        self.navigating = false;
        if self.scan_fetch == Some(self.generation) {
            self.end_scan();
        }
    }

    // ── Scanning ────────────────────────────────────────────────

    /// "Scan library" and "Retry": the application compares every image's
    /// perceptual hash (`duplicates.scan`), then the queue is read again.
    /// The progress bar shows if it takes longer than 200 ms.
    fn scan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.error = None;
        let answer = self.session.read(cx).dispatch(
            "duplicates.scan",
            serde_json::json!({ "distance_threshold": SCAN_DISTANCE }),
        );
        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let delay = cx
                .background_executor()
                .timer(motion::LOADING_MESSAGE_DELAY);
            let mut answer = std::pin::pin!(answer);
            let delayed = futures::future::select(delay, answer.as_mut()).await;
            let result = match delayed {
                futures::future::Either::Left(((), answer)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.scanning {
                            this.scan_progress = true;
                            this.changed(cx);
                        }
                    });
                    answer.await
                }
                futures::future::Either::Right((result, _)) => result,
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(summary)) => {
                    let found = summary["candidate_count"].as_u64().unwrap_or(0);
                    ui::notify::info("Duplicate scan complete", scan_message(found), cx);
                    this.fetch(Fetch::Queue { reset: true }, cx);
                    this.scan_fetch = Some(this.generation);
                }
                Ok(Err(error)) => {
                    ui::notify::error("Unable to scan for duplicates", error.clone(), cx);
                    this.error = Some(error);
                    this.end_scan();
                    this.changed(cx);
                }
                Err(_) => {
                    this.end_scan();
                    this.changed(cx);
                }
            });
        }));
        self.changed(cx);
    }

    fn end_scan(&mut self) {
        self.scanning = false;
        self.scan_progress = false;
        self.scan_fetch = None;
    }

    /// Asks the library for what `fetch` needs, then prepares the pair to
    /// show. The pair on screen stays until the next one's details and both
    /// its thumbnails are ready, so the screen never shows a half-loaded pair.
    fn fetch(&mut self, fetch: Fetch, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.resolving = matches!(fetch, Fetch::Resolve(..));
        self.navigating = matches!(fetch, Fetch::Pair(_));
        let previous = self
            .pairs
            .iter()
            .map(|pair| pair_key(pair.file_id_a, pair.file_id_b))
            .collect::<Vec<_>>();
        let shown = self.shown.as_ref().map(|shown| shown.key);
        let target = match fetch {
            Fetch::Pair(key) => self.pair(key).cloned(),
            _ => None,
        };
        let receiver = self.session.read(cx).run(move |library| {
            let error = |error: picto_library::LibraryError| error.to_string();
            let mut fetched = Fetched {
                pairs: None,
                key: None,
                details: None,
                count: None,
                ambiguous: false,
            };
            let pair = match fetch {
                Fetch::Pair(_) => target,
                Fetch::Queue { .. } | Fetch::Resolve(..) => {
                    // A settled pair is gone from the queue: the next one
                    // after it takes its place.
                    let mut active = shown;
                    if let Fetch::Resolve(a, b, decision) = fetch {
                        let choice = match decision {
                            Decision::KeepBoth => Some(DuplicateResolutionChoice::KeepBoth),
                            Decision::Keep(winner) => Some(DuplicateResolutionChoice::KeepFile {
                                winner_file_id: winner,
                            }),
                            Decision::Smart => None,
                        };
                        match choice {
                            Some(choice) => {
                                library
                                    .resolve_duplicate(a, b, choice, crate::now_ms())
                                    .map_err(error)?;
                            }
                            None => {
                                let resolved = library
                                    .resolve_duplicate_automatically(a, b, crate::now_ms())
                                    .map_err(error)?;
                                if resolved.is_none() {
                                    fetched.ambiguous = true;
                                    return Ok(fetched);
                                }
                            }
                        }
                        active = Some(pair_key(a, b));
                    }
                    let pairs = library.duplicate_candidates(QUEUE_LIMIT).map_err(error)?;
                    let keys = pairs
                        .iter()
                        .map(|pair| pair_key(pair.file_id_a, pair.file_id_b))
                        .collect::<Vec<_>>();
                    let key = compare::select_stable(&previous, active, &keys);
                    let pair = key.and_then(|key| {
                        pairs
                            .iter()
                            .find(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
                            .cloned()
                    });
                    fetched.pairs = Some(pairs);
                    fetched.count = Some(library.sidebar_counts().map_err(error)?.duplicates);
                    pair
                }
            };
            if let Some(pair) = pair {
                let key = pair_key(pair.file_id_a, pair.file_id_b);
                fetched.key = Some(key);
                if Some(key) != shown {
                    let details = |side: &DuplicateCandidateSide| {
                        side.occurrences
                            .first()
                            .and_then(|occurrence| library.details(occurrence.root_item_id).ok())
                    };
                    fetched.details = Some([details(&pair.left), details(&pair.right)]);
                }
            }
            Ok::<_, String>(fetched)
        });
        self.fetch_task = Some(cx.spawn(async move |this, cx| {
            let Ok(reply) = receiver.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.received(generation, fetch, reply, cx));
        }));
        self.changed(cx);
    }

    fn received(
        &mut self,
        generation: u64,
        fetch: Fetch,
        reply: Result<Fetched, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        let fetched = match reply {
            Ok(fetched) if fetched.ambiguous => {
                // The pair stays.
                ui::notify::warning(
                    "Smart merge needs a choice",
                    "No clear quality winner. Choose left or right, or keep both.",
                    cx,
                );
                self.settle();
                self.changed(cx);
                return;
            }
            Ok(fetched) => fetched,
            Err(error) => {
                let title = match fetch {
                    Fetch::Resolve(..) => "Unable to resolve duplicate pair",
                    Fetch::Pair(_) => "Unable to load duplicate media",
                    Fetch::Queue { .. } => "Unable to load duplicate review",
                };
                ui::notify::error(title, error.clone(), cx);
                self.error = Some(error);
                self.settle();
                self.changed(cx);
                return;
            }
        };
        if let Some(count) = fetched.count {
            self.session.update(cx, |session, cx| {
                if let Some(entry) = session
                    .nav
                    .iter_mut()
                    .find(|entry| entry.screen == Some(Screen::Duplicates))
                    && entry.count != Some(count)
                {
                    entry.count = Some(count);
                    cx.emit(Change::Navigation);
                }
            });
        }
        let reset = matches!(fetch, Fetch::Queue { reset: true });
        let (Some(key), Some([left, right])) = (fetched.key, fetched.details) else {
            // The pair on screen stays, or the queue is empty.
            if let Some(pairs) = fetched.pairs {
                self.set_pairs(pairs, reset);
            }
            if fetched.key.is_none() {
                self.release(cx);
            }
            self.error = None;
            self.settle();
            self.changed(cx);
            return;
        };
        let Some(pair) = fetched
            .pairs
            .as_ref()
            .unwrap_or(&self.pairs)
            .iter()
            .find(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
            .cloned()
        else {
            self.settle();
            self.changed(cx);
            return;
        };
        let shown = Shown {
            key,
            files: (pair.file_id_a, pair.file_id_b),
            distance: pair.distance,
            sides: [Side::new(&pair.left, left), Side::new(&pair.right, right)],
            difference: None,
        };
        let thumbnails = {
            let session = self.session.read(cx);
            shown
                .sides
                .each_ref()
                .map(|side| session.thumbnail(&side.file.file_hash))
        };
        let [left, right] = thumbnails.map(|path| load_image(path, cx));
        let pairs = fetched.pairs;
        self.fetch_task = Some(cx.spawn(async move |this, cx| {
            let (left, right) = (left.await, right.await);
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    let mut shown = shown;
                    shown.sides[0].thumbnail = left;
                    shown.sides[1].thumbnail = right;
                    this.commit(pairs, reset, shown, cx);
                }
            });
        }));
    }

    fn set_pairs(&mut self, pairs: Vec<DuplicateCandidate>, reset: bool) {
        self.total = pairs.len();
        if reset {
            self.initial_total = self.total;
        }
        self.pairs = pairs;
    }

    /// Puts a prepared pair on screen, both thumbnails at once, and starts
    /// its originals.
    fn commit(
        &mut self,
        pairs: Option<Vec<DuplicateCandidate>>,
        reset: bool,
        shown: Shown,
        cx: &mut Context<Self>,
    ) {
        if let Some(pairs) = pairs {
            self.set_pairs(pairs, reset);
        }
        if self.shown.as_ref().map(|old| old.key) != Some(shown.key) {
            self.zoom.fit();
        }
        self.release(cx);
        self.shown = Some(shown);
        self.error = None;
        self.settle();
        self.load_originals(cx);
        self.refresh_difference(cx);
        self.changed(cx);
    }

    /// Takes the pair off screen and gives its images back.
    fn release(&mut self, cx: &mut Context<Self>) {
        self.original_tasks = [None, None];
        self.difference_task = None;
        self.drag = None;
        let Some(shown) = self.shown.take() else {
            return;
        };
        let images = shown.difference.into_iter().chain(
            shown
                .sides
                .into_iter()
                .flat_map(|side| side.thumbnail.into_iter().chain(side.full)),
        );
        for image in images {
            cx.drop_image(image, None);
        }
    }

    /// Loads both originals; each fades in over its thumbnail on arrival.
    fn load_originals(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = &mut self.shown else {
            return;
        };
        let key = shown.key;
        for (index, side) in shown.sides.iter_mut().enumerate() {
            let Some(path) = side.original().filter(|_| side.full.is_none()) else {
                continue;
            };
            side.full_pending = true;
            let load = load_image(path, cx);
            self.original_tasks[index] = Some(cx.spawn(async move |this, cx| {
                let image = load.await;
                let _ = this.update(cx, |this, cx| {
                    let Some(shown) = this.shown.as_mut().filter(|shown| shown.key == key) else {
                        return;
                    };
                    let side = &mut shown.sides[index];
                    side.full_pending = false;
                    if image.is_some() {
                        side.full = image;
                        side.full_since = Some(Instant::now());
                    }
                    let arrived = shown.sides.iter().any(|side| side.full.is_some());
                    if arrived && shown.sides.iter().all(|side| !side.full_pending) {
                        this.refresh_difference(cx);
                    }
                    cx.notify();
                });
            }));
        }
    }

    /// Computes the difference picture from the best images there are.
    fn refresh_difference(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = &self.shown else {
            return;
        };
        let best = |side: &Side| side.full.clone().or_else(|| side.thumbnail.clone());
        let (Some(left), Some(right)) = (best(&shown.sides[0]), best(&shown.sides[1])) else {
            return;
        };
        let key = shown.key;
        let work = cx
            .background_executor()
            .spawn(async move { difference::compute(&left, &right) });
        self.difference_task = Some(cx.spawn(async move |this, cx| {
            let image = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(shown) = this.shown.as_mut().filter(|shown| shown.key == key) {
                    if let Some(old) = std::mem::replace(&mut shown.difference, image) {
                        cx.drop_image(old, None);
                    }
                    if this.difference_hovered {
                        cx.notify();
                    }
                }
            });
        }));
    }

    // ── Pointer on the previews ─────────────────────────────────

    fn pane(&self, side: usize) -> Extent {
        let size = self.panes[side].size;
        Extent::new(f32::from(size.width), f32::from(size.height))
    }

    fn image(&self, side: usize) -> Extent {
        self.shown
            .as_ref()
            .map_or_else(Extent::default, |shown| shown.sides[side].image())
    }

    fn wheel(&mut self, side: usize, event: &ScrollEvent, window: &Window, cx: &mut Context<Self>) {
        let bounds = self.panes[side];
        let delta = event.delta.pixel_delta(window.line_height());
        let offset = event.position - bounds.center();
        // GPUI's delta is the content's movement; the DOM's is the wheel's.
        self.zoom.wheel(
            self.pane(side),
            self.image(side),
            (f32::from(offset.x), f32::from(offset.y)),
            -f32::from(delta.y),
        );
        self.changed(cx);
    }

    fn dragged(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else {
            return;
        };
        let moved = position - drag.from;
        let (side, center) = (drag.side, drag.center);
        self.zoom.pan(
            self.pane(side),
            self.image(side),
            center,
            (f32::from(moved.x), f32::from(moved.y)),
        );
        cx.notify();
    }

    /// Records a laid-out rectangle; a change paints once more with it.
    fn measured(
        entity: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
        update: impl FnOnce(&mut Self) -> bool,
    ) {
        let changed = entity.update(cx, |this, _| update(this)).unwrap_or(false);
        if changed {
            let entity = entity.clone();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |_, cx| cx.notify());
            });
        }
    }
}

/// Starts decoding an image file off the drawing thread.
fn load_image(path: PathBuf, cx: &mut App) -> Task<Option<Arc<RenderImage>>> {
    let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
    cx.background_executor()
        .spawn(async move { load.await.ok() })
}

/// The screen's titlebar controls (`DuplicatesToolbar`): the pair counter in
/// the middle, then fit, actual pixels, and previous and next pair.
pub struct DuplicatesToolbar {
    screen: WeakEntity<DuplicatesScreen>,
}

#[cfg(test)]
mod tests {
    use super::scan_message;

    #[test]
    fn the_scan_says_how_many_pairs_it_found() {
        assert_eq!(scan_message(3), "Found 3 new review pairs");
        assert_eq!(scan_message(0), "Scan complete - no new review pairs");
    }
}
