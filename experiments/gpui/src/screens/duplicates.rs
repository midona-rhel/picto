//! The Duplicates screen (`DuplicatesScreen.tsx`): the review queue of
//! perceptually similar pairs, one pair at a time, side by side with linked
//! zoom and pan, each file's facts beneath it, and the decisions in a footer.
//! Its counter, zoom and pair controls sit in the titlebar
//! ([`DuplicatesToolbar`]).
mod compare;
mod difference;

use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{motion, theme, ui};
use compare::{Extent, LinkedZoom, Mode, PairKey, pair_key};
use gpui::*;
use picto_library::{
    DuplicateCandidate, DuplicateCandidateSide, DuplicateFile, DuplicateResolutionChoice, FileId,
    MediaRecord, Rating, RootDetails, RootKind,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

actions!(
    duplicates,
    [
        PreviousPair,
        NextPair,
        FitPair,
        ActualPixels,
        KeepBoth,
        NotDuplicate
    ]
);

const CONTEXT: &str = "Duplicates";
/// `getDuplicatePairs`: the queue is read 500 pairs at a time.
const QUEUE_LIMIT: usize = 500;
/// `.mediaCard { grid-template-rows: 42px minmax(180px, 1fr) 222px }`.
const CARD_HEADER: f32 = 42.;
const PREVIEW_MIN: f32 = 180.;
const CARD_METADATA: f32 = 222.;
/// The container widths at which the layout tightens (`@container`).
const COMPACT_WIDTH: f32 = 900.;
const NARROW_WIDTH: f32 = 620.;

/// The keys of `useShortcutScope` in the Electron screen, which win over
/// the grid's while the screen has the keyboard.
fn bind(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", PreviousPair, context),
        KeyBinding::new("right", NextPair, context),
        KeyBinding::new("f", FitPair, context),
        KeyBinding::new("cmd-0", ActualPixels, context),
        KeyBinding::new("b", KeepBoth, context),
        KeyBinding::new("n", NotDuplicate, context),
        // TODO: Z keeps the left file, X the right one and S smart-merges,
        // once those decisions can be made (see the buttons).
    ]);
}

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

/// What a fetch is for.
#[derive(Clone, Copy)]
enum Fetch {
    /// Read the queue again; `reset` starts the progress count afresh.
    Queue { reset: bool },
    /// Settle a pair as two different files, then read the queue.
    Resolve(FileId, FileId),
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
    _session: Subscription,
}

impl DuplicatesScreen {
    pub fn new(
        session: Entity<Session>,
        _thumbnails: Entity<Thumbnails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        bind(cx);
        let screen = cx.weak_entity();
        let controls = cx.new(|_| DuplicatesToolbar { screen });
        let subscription =
            cx.subscribe_in(&session, window, |this, _, change: &Change, window, cx| {
                if *change == Change::Navigation {
                    this.navigated(window, cx);
                }
            });
        Self {
            session,
            focus: cx.focus_handle(),
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
            _session: subscription,
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
            };
            let pair = match fetch {
                Fetch::Pair(_) => target,
                Fetch::Queue { .. } | Fetch::Resolve(..) => {
                    // A settled pair is gone from the queue: the next one
                    // after it takes its place.
                    let mut active = shown;
                    if let Fetch::Resolve(a, b) = fetch {
                        library
                            .resolve_duplicate(a, b, DuplicateResolutionChoice::KeepBoth, now_ms())
                            .map_err(error)?;
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
            Ok(fetched) => fetched,
            Err(error) => {
                // TODO: report it as a toast, as the Electron screen does,
                // once the prototype has toasts; for now only the empty
                // state shows it.
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

    // ── Decisions and movement ──────────────────────────────────

    /// `requestIndex`: another pair of the queue, one at a time.
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.navigating || self.resolving {
            return;
        }
        let Some(index) = self.index() else {
            return;
        };
        let target = index
            .saturating_add_signed(delta)
            .min(self.pairs.len().saturating_sub(1));
        if target == index {
            return;
        }
        let pair = &self.pairs[target];
        self.fetch(Fetch::Pair(pair_key(pair.file_id_a, pair.file_id_b)), cx);
    }

    /// Settles the pair on screen as two different files. "Not duplicates"
    /// and "Keep both" both record this; neither touches a file.
    fn keep_both(&mut self, cx: &mut Context<Self>) {
        if self.resolving || self.navigating {
            return;
        }
        let Some((a, b)) = self.shown.as_ref().map(|shown| shown.files) else {
            return;
        };
        self.difference_hovered = false;
        self.fetch(Fetch::Resolve(a, b), cx);
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        self.zoom.fit();
        self.changed(cx);
    }

    fn actual(&mut self, cx: &mut Context<Self>) {
        self.zoom.actual();
        self.changed(cx);
    }

    fn index(&self) -> Option<usize> {
        let key = self.shown.as_ref()?.key;
        self.pairs
            .iter()
            .position(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
    }

    fn toolbar_model(&self) -> Option<ToolbarModel> {
        if self.loading {
            return None;
        }
        let index = self.index()?;
        Some(ToolbarModel {
            index,
            total: self.total,
            can_previous: index > 0,
            can_next: index + 1 < self.pairs.len(),
            disabled: self.resolving,
            fit: self.zoom.mode == Mode::Fit,
            actual: self.zoom.mode == Mode::Actual,
        })
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// The rating as the Electron screen prints it: the stored name.
fn rating_name(rating: Rating) -> &'static str {
    match rating {
        Rating::Unrated => "unrated",
        Rating::One => "one",
        Rating::Two => "two",
        Rating::Three => "three",
        Rating::Four => "four",
        Rating::Five => "five",
    }
}

// ── Drawing ─────────────────────────────────────────────────────

impl DuplicatesScreen {
    /// `.centerState`: the queue's first read, with its message after 200 ms.
    fn loading_state(&self, cx: &App) -> Div {
        let theme = theme::theme(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .p(px(24.))
            .text_color(theme.text_secondary())
            .children(
                self.loading_message
                    .then_some("Loading duplicate review queue..."),
            )
    }

    fn empty_state(&self) -> AnyElement {
        let resolved = self.initial_total.saturating_sub(self.total);
        let (title, description) = match &self.error {
            Some(error) => ("Unable to load duplicate review".to_owned(), error.clone()),
            None if resolved > 0 => (
                "Review complete".to_owned(),
                format!("{resolved} decisions saved."),
            ),
            None => (
                "No duplicate pairs".to_owned(),
                "Scan the library to find similar images.".to_owned(),
            ),
        };
        // TODO: scan the library for duplicates and read the queue again.
        // The scanner lives in the application core, which this prototype
        // does not link; Retry would scan as well.
        let action = ui::action_button("duplicates-scan", Look::EmptyState)
            .icon("action-refresh", 14.)
            .label(if self.error.is_some() {
                "Retry"
            } else {
                "Scan library"
            })
            .disabled(self.resolving);
        ui::empty_state("empty-copy", 28., title, description)
            .action(action)
            .into_any_element()
    }

    /// One candidate (`MediaCard`): its header with Keep, its preview, and
    /// its facts.
    fn card(
        &self,
        index: usize,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let side = &shown.sides[index];
        let (label, keep) = if index == 0 {
            ("Left candidate", "Keep left")
        } else {
            ("Right candidate", "Keep right")
        };
        // TODO: keep this file and merge the other into it. That frees the
        // other file for deletion from disk, which this prototype must never
        // cause, so the button is shown and not offered (Z and X likewise).
        let mut keep_button = ui::action_button(("duplicate-keep", index), Look::Standard)
            .icon("action-check", 15.)
            .height(theme::CONTROL_MD)
            .text_size(theme::TEXT_SM)
            .tip(ui::tip(keep).keys(&[if index == 0 { "Z" } else { "X" }]))
            .disabled(true);
        keep_button = if compact {
            keep_button.width(theme::CONTROL_MD).padding(0.)
        } else {
            keep_button.padding(10.).label(keep)
        };
        let header = div()
            .h(px(CARD_HEADER))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(8.))
            .child(
                div()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_SM)
                    .child(label),
            )
            .child(keep_button);
        div()
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_MD))
            .child(header)
            .child(self.preview(index, window, cx))
            .child(self.metadata(side, cx))
    }

    /// The preview pane: the image in its frame, zoomed and panned with the
    /// other, or the difference picture while Show Difference is held.
    fn preview(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let side = &shown.sides[index];
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                Self::measured(&entity, window, cx, |this| {
                    std::mem::replace(&mut this.panes[index], bounds) != bounds
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        // The frame: where the image sits in the pane. Until the pane has
        // been measured, the image fills it, fitted, which is the same.
        let (pane, image) = (self.pane(index), side.image());
        let frame = |layer: Div| {
            if pane.width > 0. && pane.height > 0. && image.width > 0. && image.height > 0. {
                let frame = self.zoom.frame(pane, image);
                layer
                    .absolute()
                    .left(px(frame.left))
                    .top(px(frame.top))
                    .w(px(frame.width))
                    .h(px(frame.height))
            } else {
                layer.absolute().inset_0()
            }
        };
        let fill = |image: Arc<RenderImage>, fit: ObjectFit| {
            img(image).absolute().inset_0().size_full().object_fit(fit)
        };
        let mut inside = div()
            .absolute()
            .inset(px(1.))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .child(measure)
            .children(side.details.is_none().then(|| {
                div()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_SM)
                    .child("Media details unavailable")
            }));
        if self.difference_hovered && !self.resolving {
            // `.differenceComposite`: on black.
            inside = inside.child(
                frame(div().bg(black())).children(
                    shown
                        .difference
                        .clone()
                        .map(|image| fill(image, ObjectFit::Fill)),
                ),
            );
        } else {
            let opacity = side.full_opacity();
            // Painting continues only while an arrived original fades in.
            if side.full_since.is_some() && opacity < 1. {
                window.request_animation_frame();
            }
            let mut layers = frame(div());
            if opacity < 1.
                && let Some(thumbnail) = side.thumbnail.clone()
            {
                layers = layers.child(fill(thumbnail, ObjectFit::Contain));
            }
            if let Some(full) = side.full.clone() {
                layers = layers.child(fill(full, ObjectFit::Contain).opacity(opacity));
            }
            inside = inside.child(layers);
        }
        let dragging = self.drag.as_ref().is_some_and(|drag| drag.side == index);
        div()
            .id(("duplicate-preview", index))
            .relative()
            .flex_1()
            .min_h(px(PREVIEW_MIN))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.black(30.))
            .cursor(if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .child(inside)
            // The border over the image, which may cover the pane's edge.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border_secondary()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if event.click_count == 2 {
                        this.drag = None;
                        this.fit(cx);
                    } else {
                        this.drag = Some(Drag {
                            side: index,
                            from: event.position,
                            center: this.zoom.center(),
                        });
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                this.wheel(index, event, window, cx);
                cx.stop_propagation();
            }))
    }

    /// The file's facts (`.metadata`).
    fn metadata(&self, side: &Side, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let name = side.media.as_ref().map_or_else(
            || side.file.file_hash.clone(),
            |media| media.media_name.clone(),
        );
        let details = side.details.as_ref();
        let row = |label: &'static str, value: String| {
            ui::property_row(label, value).mono(theme::FONT_MONO)
        };
        let mut rows = vec![
            row(
                "Resolution",
                compare::dimensions(side.file.pixel_width, side.file.pixel_height),
            ),
            row("Size", compare::format_bytes(side.file.size_bytes)),
            row("Format", side.file.mime_type.clone()),
            row(
                "Rating",
                details
                    .map_or("Unrated", |details| rating_name(details.rating))
                    .into(),
            ),
            row(
                "Tags",
                details
                    .map_or(0, |details| details.tag_ids.len())
                    .to_string(),
            ),
            row(
                "Created",
                details
                    .and_then(|details| details.root.captured_at_ms)
                    .filter(|at| *at != 0)
                    .map_or_else(|| "Unknown".into(), compare::short_date),
            ),
            row(
                "Added",
                details.map_or_else(
                    || "Unknown".into(),
                    |details| compare::short_date(details.root.imported_at_ms),
                ),
            ),
        ];
        if let Some(root) = details
            .map(|details| &details.root)
            .filter(|root| root.kind == RootKind::Collection)
        {
            let group = if root.name.is_empty() {
                format!("Group {}", root.root_id.0)
            } else {
                root.name.clone()
            };
            rows.push(row("Group", group));
        }
        rows.push(row("Occurrences", side.occurrences.to_string()));
        div()
            .h(px(CARD_METADATA))
            .flex_none()
            .pt(px(10.))
            .px(px(4.))
            .pb(px(12.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(4.))
                    .text_color(theme.text())
                    .text_size(px(theme::TEXT_MD))
                    .line_height(px(16.))
                    .truncate()
                    .child(name),
            )
            .children(rows)
    }

    /// The decisions (`.footer`).
    fn footer(
        &self,
        compact: bool,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let button = |id: &'static str, look: Look| {
            ui::action_button(id, look)
                .height(36.)
                .disabled(self.resolving)
        };
        let not_duplicates = button("duplicates-not", Look::Standard)
            .icon("action-x", 15.)
            .label("Not duplicates")
            .tip(ui::tip("These are different media").keys(&["N"]))
            .on_click(cx.listener(|this, _, _, cx| this.keep_both(cx)));
        let keep_both = button("duplicates-keep-both", Look::Standard)
            .icon("action-copy", 15.)
            .label("Keep both")
            .tip(ui::tip("Keep both files").keys(&["B"]))
            .on_click(cx.listener(|this, _, _, cx| this.keep_both(cx)));
        // TODO: keep the better file of the two and merge the other into it,
        // previewing the survivor with a green ring while hovered. The
        // merged-away file is freed for deletion from disk, which this
        // prototype must never cause, so the button is shown and not offered.
        let smart_merge = button("duplicates-smart-merge", Look::Primary)
            .icon("action-arrows-join", 16.)
            .label("Smart merge")
            .tip(ui::tip("Keep the stronger file and preserve item metadata").keys(&["S"]))
            .disabled(true);
        let entity = cx.weak_entity();
        let difference = button("duplicates-difference", Look::Standard)
            .icon("toolbar-difference", 16.)
            .label("Show Difference")
            .gap(6.)
            .padding(9.)
            .text_size(theme::TEXT_SM)
            .weight(FontWeight::NORMAL)
            .tip(ui::tip("Show differences while held"))
            .on_hover(move |hovered, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.difference_hovered != hovered {
                        this.difference_hovered = hovered;
                        cx.notify();
                    }
                });
            });
        let similarity = ui::tooltip::attach(
            div()
                .id("duplicates-similarity")
                .ml(px(8.))
                .text_color(theme.text_secondary())
                .text(theme::TEXT_SM)
                .whitespace_nowrap()
                .child(compare::similarity_label(shown.distance)),
            ("duplicates-similarity", 1usize),
            ui::tip("Perceptual similarity is not pixel equality"),
            theme::TEXT_SM + 2.,
            window,
            cx,
        );
        let mut actions = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(not_duplicates)
            .child(keep_both)
            .child(smart_merge)
            .child(difference)
            .child(similarity);
        if compact {
            actions = actions.flex_wrap();
        }
        div()
            .min_h(px(58.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .py(px(8.))
            .px(px(if narrow { 8. } else { 18. }))
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.background())
            .child(actions)
    }

    /// The two cards: side by side, or stacked in a scrolling column when
    /// the screen is narrow.
    fn comparison(
        &self,
        compact: bool,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                Self::measured(&entity, window, cx, |this| {
                    let height = f32::from(bounds.size.height);
                    std::mem::replace(&mut this.comparison_height, height) != height
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let (left, right) = (
            self.card(0, compact, window, cx),
            self.card(1, compact, window, cx),
        );
        if narrow {
            // One column, each card at least as tall as its rows allow and
            // sharing any height left over (`grid-template-columns: 1fr`).
            let gap = 10.;
            let card = ((self.comparison_height - 2. * 10. - gap) / 2.)
                .max(CARD_HEADER + PREVIEW_MIN + CARD_METADATA);
            return div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(measure)
                .child(
                    div()
                        .id("duplicates-comparison")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(gap))
                                .p(px(10.))
                                // `scrollbar-gutter: stable`
                                .pr(px(10. + ui::scrollbar::GUTTER))
                                .child(left.h(px(card)))
                                .child(right.h(px(card))),
                        ),
                )
                .child(ui::scrollbar(
                    "duplicates-scrollbar",
                    &self.scroll,
                    window,
                    cx,
                ))
                .into_any_element();
        }
        let (gap, x, y) = if compact {
            (8., 10., 10.)
        } else {
            (12., 18., 14.)
        };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(gap))
            .px(px(x))
            .py(px(y))
            .child(measure)
            .child(left.flex_1())
            .child(right.flex_1())
            .into_any_element()
    }
}

impl Render for DuplicatesScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                Self::measured(&entity, window, cx, |this| {
                    let width = f32::from(bounds.size.width);
                    std::mem::replace(&mut this.width, width) != width
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let mut root = div()
            .id("duplicates")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .relative()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .child(measure)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousPair, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NextPair, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &FitPair, _, cx| this.fit(cx)))
            .on_action(cx.listener(|this, _: &ActualPixels, _, cx| this.actual(cx)))
            .on_action(cx.listener(|this, _: &KeepBoth, _, cx| this.keep_both(cx)))
            .on_action(cx.listener(|this, _: &NotDuplicate, _, cx| this.keep_both(cx)));
        if self.shown.is_none() {
            return root.child(if self.loading {
                self.loading_state(cx).into_any_element()
            } else {
                self.empty_state()
            });
        }
        // A drag follows the pointer anywhere in the window until the
        // button is released, as pointer capture does.
        if self.drag.is_some() {
            let entity = cx.weak_entity();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moved = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ =
                                    moved.update(cx, |this, cx| this.dragged(event.position, cx));
                            }
                        });
                        let released = entity.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = released.update(cx, |this, cx| {
                                    this.drag = None;
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        let compact = self.width > 0. && self.width <= COMPACT_WIDTH;
        let narrow = self.width > 0. && self.width <= NARROW_WIDTH;
        let comparison = self.comparison(compact, narrow, window, cx);
        let footer = self.footer(compact, narrow, window, cx);
        root.child(comparison).child(footer)
    }
}

/// The screen's titlebar controls (`DuplicatesToolbar`): the pair counter in
/// the middle, then fit, actual pixels, and previous and next pair.
pub struct DuplicatesToolbar {
    screen: WeakEntity<DuplicatesScreen>,
}

impl Render for DuplicatesToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let bar = div().flex_1().min_w_0().h_full().flex().items_center();
        let Some(model) = self
            .screen
            .upgrade()
            .and_then(|screen| screen.read(cx).toolbar_model())
        else {
            return bar;
        };
        // The buttons call the screen directly: the screen repaints these
        // controls itself, which it could not do while they were updating.
        let act = |action: fn(&mut DuplicatesScreen, &mut Context<DuplicatesScreen>)| {
            let screen = self.screen.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = screen.update(cx, action);
            }
        };
        bar.child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .ml(px(4.))
                        .text_color(theme.text_secondary())
                        .text(theme::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .whitespace_nowrap()
                        .child(format!("{} / {}", model.index + 1, model.total)),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    ui::icon_button("duplicates-fit", "toolbar-fit")
                        .toggled(model.fit)
                        .tip(ui::tip("Zoom to fit").keys(&["`"]))
                        .on_click(act(DuplicatesScreen::fit)),
                )
                .child(
                    ui::icon_button("duplicates-actual", "toolbar-actual-size")
                        .toggled(model.actual)
                        .tip(ui::tip("Actual pixels").keys(&["⌘", "0"]))
                        .on_click(act(DuplicatesScreen::actual)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            ui::icon_button("duplicates-previous", "toolbar-chevron-left")
                                .disabled(!model.can_previous || model.disabled)
                                .tip(ui::tip("Previous pair").keys(&["←"]))
                                .on_click(act(|screen, cx| screen.step(-1, cx))),
                        )
                        .child(
                            ui::icon_button("duplicates-next", "toolbar-chevron-right")
                                .disabled(!model.can_next || model.disabled)
                                .tip(ui::tip("Next pair").keys(&["→"]))
                                .on_click(act(|screen, cx| screen.step(1, cx))),
                        ),
                ),
        )
    }
}
