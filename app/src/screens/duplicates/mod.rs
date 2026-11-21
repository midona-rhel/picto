//! The Duplicates screen. It is a port of the Electron app's
//! `DuplicatesScreen.tsx` and shows the library's queue of pairs of files
//! that a scan found to be perceptually similar, one pair at a time.
//!
//! The two pictures are side by side and are zoomed and panned together.
//! Above each picture is a row with the file's name and its Keep left or
//! Keep right button. Under each picture is a list of the file's facts.
//!
//! The titlebar holds the rest of the screen's controls
//! ([`DuplicatesToolbar`]): the position in the queue with the Previous pair
//! and Next pair buttons, one line of text with the similarity and the pixel
//! measurement, the Smart merge, Not duplicates, Keep both and Show
//! Difference buttons, and the Zoom to fit and Actual pixels buttons. The
//! Electron screen has those four buttons in a footer. This app puts every
//! main screen's buttons in the titlebar, so this screen does too.
//!
//! Three things here are not in the Electron screen. The pixel difference is
//! also shown while the D key is held down. The titlebar says how many of
//! the two originals' pixels differ. The pictures of the pair that follows
//! the pair on screen are decoded in advance, so that when a decision moves
//! to that pair its originals are drawn at once.
mod compare;
mod decisions;
mod difference;
mod view;

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::{en, t};
use crate::motion;
use crate::session::{Change, Session};
use crate::ui;
use compare::{Extent, LinkedZoom, PairKey, pair_key};
use gpui::*;
use picto_library::{
    DuplicateCandidate, DuplicateCandidateSide, DuplicateFile, FileId, MediaRecord, RootDetails,
};
use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc, time::Instant};

// The screen's actions. The shortcut registry (`crate::keymap`) binds their
// keys in the `Duplicates` key context, so they take priority over the
// grid's keys while this screen has keyboard focus. The Electron screen does
// the same with `useShortcutScope`.
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
        SmartMerge,
        ShowDifference
    ]
);

// i18n: a key context
const CONTEXT: &str = "Duplicates";
/// The number of pairs read from the library at a time. The Electron app's
/// `getDuplicatePairs` reads the same number.
const QUEUE_LIMIT: usize = 500;
/// The distance threshold passed to `duplicates.scan`. It is the default of
/// the Electron app's `scanDuplicates`.
const SCAN_DISTANCE: u32 = 16;

/// One of the two files of the pair on screen.
struct Side {
    file: DuplicateFile,
    /// The number of items in the library that hold this file.
    occurrences: usize,
    /// The details of the first item that holds this file. This is `None`
    /// when the details could not be read.
    details: Option<RootDetails>,
    /// That item's media record for this file. The Electron screen finds it
    /// with `mediaForFile`.
    media: Option<MediaRecord>,
    /// The time at which this file's original was decoded, if that happened
    /// while the file was on screen. The original then fades in over the
    /// thumbnail, starting at this time. This is `None` when the original
    /// was already decoded when the pair was put on screen, and the original
    /// is then drawn at full opacity at once.
    original_since: Option<Instant>,
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
            original_since: None,
        }
    }

    /// The path of the original file to decode. This is `None` when the file
    /// is not a still image, or when the item's media record is for a
    /// different file.
    fn original(&self) -> Option<PathBuf> {
        let media = self.media.as_ref()?;
        (self.file.mime_type.starts_with("image/")
            && media.facts.content_hash == self.file.file_hash)
            .then(|| PathBuf::from(&media.file_path))
    }

    /// The file's width and height in pixels. Both are zero when the library
    /// does not know them.
    fn image(&self) -> Extent {
        Extent::new(
            self.file.pixel_width.unwrap_or(0) as f32,
            self.file.pixel_height.unwrap_or(0) as f32,
        )
    }

    /// The opacity of the original over the thumbnail, from 0 to 1. It
    /// rises over [`motion::FULL_IMAGE_FADE`] from the time the original was
    /// decoded.
    fn original_opacity(&self) -> f32 {
        self.original_since.map_or(1., |since| {
            motion::EASE_OUT.ease(motion::progress(since.elapsed(), motion::FULL_IMAGE_FADE))
        })
    }
}

/// The decoded pictures of one file. The screen keeps them while the file
/// belongs to the pair on screen, to the pair that is about to be put on
/// screen, or to the pair that follows the pair on screen.
#[derive(Default)]
struct Pictures {
    thumbnail: Option<Arc<RenderImage>>,
    /// True once the thumbnail was decoded, or once decoding it failed.
    thumbnail_settled: bool,
    thumbnail_task: Option<Task<()>>,
    original: Option<Arc<RenderImage>>,
    /// True while the original is being decoded.
    original_pending: bool,
    original_task: Option<Task<()>>,
}

/// Whether the two originals of a pair have been compared pixel by pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Counted {
    /// The comparison has not finished yet.
    Waiting,
    /// The comparison finished, with this result.
    Done(difference::Measure),
    /// The pair cannot be compared: a file is not a still image, or an
    /// original could not be decoded.
    Impossible,
}

/// A pair of files with what the screen knows about it. The screen keeps
/// one for the pair on screen, one for the pair about to be put on screen
/// and one for the pair that follows the pair on screen.
struct Shown {
    key: PairKey,
    files: (FileId, FileId),
    distance: u32,
    sides: [Side; 2],
    /// The difference picture, once it was computed. The two booleans say,
    /// for the left and the right file, whether the picture was computed
    /// from the file's original (`true`) or from its thumbnail (`false`).
    difference: Option<(Arc<RenderImage>, [bool; 2])>,
    /// The result of comparing the two originals pixel by pixel. It is set
    /// once and then never changes, so the titlebar's line of text does not
    /// change while the pair is on screen.
    counted: Counted,
}

impl Shown {
    fn new(pair: &DuplicateCandidate, [left, right]: [Option<RootDetails>; 2]) -> Self {
        let sides = [Side::new(&pair.left, left), Side::new(&pair.right, right)];
        let comparable = sides.iter().all(|side| side.original().is_some());
        Self {
            key: pair_key(pair.file_id_a, pair.file_id_b),
            files: (pair.file_id_a, pair.file_id_b),
            distance: pair.distance,
            sides,
            difference: None,
            counted: if comparable {
                Counted::Waiting
            } else {
                Counted::Impossible
            },
        }
    }

    fn hashes(&self) -> impl Iterator<Item = &String> {
        self.sides.iter().map(|side| &side.file.file_hash)
    }
}

/// A decision on the pair on screen. The Electron app calls it a
/// `DuplicateAction`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Decision {
    /// Records that the two files are different. Neither file is changed.
    /// The Not duplicates button and the Keep both button both do this.
    KeepBoth,
    /// Keeps this file and merges the other file's item into its item. The
    /// application then deletes the other file from the library's blobs.
    Keep(FileId),
    /// Keeps the file that the library rates as better, if the library can
    /// rate one as better.
    Smart,
}

/// The message of the "Duplicate scan complete" notification after a scan
/// that found `found` new pairs.
fn scan_message(found: u64) -> String {
    if found > 0 {
        format!("Found {found} new review pairs")
    } else {
        en("Scan complete - no new review pairs").into()
    }
}

/// The reason for a read from the library.
#[derive(Clone, Copy)]
enum Fetch {
    /// Read the queue again. With `reset`, the count of decisions made since
    /// the queue was read starts again from zero.
    Queue { reset: bool },
    /// Read the queue after a decision on this pair. The pair is no longer
    /// in the queue, and the pair that followed it is shown.
    Settled(PairKey),
    /// Show this other pair of the queue that was already read.
    Pair(PairKey),
}

/// A pair together with the details of its two items.
type Detailed = (DuplicateCandidate, [Option<RootDetails>; 2]);

/// The answer to a read from the library.
struct Fetched {
    /// The queue, if this read was a read of the queue.
    pairs: Option<Vec<DuplicateCandidate>>,
    /// The key of the pair to show.
    key: Option<PairKey>,
    /// The details of the two items of the pair to show. This is `None`
    /// when the pair to show is the pair already on screen.
    details: Option<[Option<RootDetails>; 2]>,
    /// The pair that follows the pair to show. Its pictures are decoded in
    /// advance.
    ahead: Option<Detailed>,
    /// The number of duplicate pairs that the sidebar shows.
    count: Option<u64>,
}

/// A pair that was read and is waiting for its two thumbnails to be decoded.
/// It is put on screen only when both are, so the screen never shows one
/// picture of the new pair beside one picture of the old pair.
struct Pending {
    pairs: Option<Vec<DuplicateCandidate>>,
    reset: bool,
    shown: Shown,
    ahead: Option<Shown>,
}

/// A drag of one of the two pictures with the left mouse button.
struct Drag {
    /// 0 for the left picture, 1 for the right picture.
    side: usize,
    /// Where the pointer was when the drag started.
    from: Point<Pixels>,
    /// The zoom's centre when the drag started.
    center: (f32, f32),
}

/// What the titlebar controls show. The Electron app calls it a
/// `DuplicateToolbarModel`.
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
    /// The element that had keyboard focus before this screen took it. The
    /// screen gives focus back to it when the user leaves the screen.
    return_focus: Option<FocusHandle>,
    controls: Entity<DuplicatesToolbar>,
    /// True while this screen is the session's destination.
    visible: bool,
    /// True once the screen took keyboard focus during this visit.
    focused_once: bool,
    pairs: Vec<DuplicateCandidate>,
    total: usize,
    /// The length of the queue when it was last read with `reset`. The
    /// empty state subtracts `total` from it to say how many decisions were
    /// saved.
    initial_total: usize,
    /// The pair on screen.
    shown: Option<Shown>,
    /// The pair that is about to be put on screen.
    pending: Option<Pending>,
    /// The pair that follows the pair on screen. Its pictures are decoded
    /// and its originals compared in advance.
    ahead: Option<Shown>,
    /// The decoded pictures of the files of those three pairs, by file hash.
    pictures: HashMap<String, Pictures>,
    /// True while the queue is being read for the first time.
    loading: bool,
    /// True once the first read has taken longer than 200 ms. The loading
    /// message is shown only then.
    loading_message: bool,
    /// True while a decision is being written to the library.
    resolving: bool,
    /// True while another pair of the queue is being prepared.
    navigating: bool,
    error: Option<String>,
    /// A number that goes up with every read. An answer to an earlier read
    /// carries a lower number and is ignored.
    generation: u64,
    /// The library's revision after this screen's last decision. A change
    /// event with this revision or a lower one reports that decision, and
    /// the screen has already read the queue for it.
    own_revision: u64,
    zoom: LinkedZoom,
    drag: Option<Drag>,
    /// True while the pointer is on the Show Difference button.
    difference_hovered: bool,
    /// True while the Show Difference button has keyboard focus. On this
    /// screen the Tab key toggles the panels, as it does in production, so
    /// only assistive technology can give the button keyboard focus.
    difference_focused: bool,
    /// True after a click on the Show Difference button, until the next
    /// press of a mouse button anywhere in the window. In the Electron app a
    /// click gives the button keyboard focus, and the difference stays on
    /// screen until something else is clicked.
    difference_pressed: bool,
    /// True while the key bound to Show Difference (D) is held down.
    difference_held: bool,
    /// The bounds of the two picture panes in the last painted frame. The
    /// pointer handlers use them.
    panes: [Bounds<Pixels>; 2],
    fetch_task: Option<Task<()>>,
    /// The task that computes the difference picture of the pair on screen.
    difference_task: Option<Task<()>>,
    /// The task that compares the originals of the pair in `ahead`.
    ahead_task: Option<Task<()>>,
    message_task: Option<Task<()>>,
    /// True while a scan of the library is running.
    scanning: bool,
    /// True once the scan has taken longer than 200 ms. The scan's progress
    /// bar is shown only then, as the Electron screen's `useDelayedFlag`
    /// does.
    scan_progress: bool,
    scan_task: Option<Task<()>>,
    /// The number of the read of the queue that follows a scan. The scan is
    /// over when that read is answered.
    scan_fetch: Option<u64>,
    /// True while the pointer is on the Smart merge button.
    smart_hovered: bool,
    /// True while the Smart merge button has keyboard focus.
    smart_focused: bool,
    /// The context menu of a file. The Electron screen's `MediaCard` opens
    /// the same menu.
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
            // The queue changes when the media worker scans newly imported
            // images, and when a decision is undone. The screen then reads
            // the queue again. It does not do that for the change that its
            // own decision caused, because it reads the queue itself after a
            // decision.
            cx.subscribe(&session, |this, _, event: &BackendEvent, cx| {
                if let BackendEvent::Changed(changed) = event
                    && crate::application::touches(changed, "duplicates")
                    && changed.revision > this.own_revision
                    && this.visible
                    && !this.loading
                    && !this.resolving
                    && !this.scanning
                {
                    this.fetch(Fetch::Queue { reset: true }, cx);
                }
            }),
            // When the user switches to another application while holding
            // D, this window never receives the key-up event. The screen
            // therefore stops showing the difference when the window stops
            // being the active window.
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.hold_difference(false, cx);
                }
            }),
        ];
        let focus = cx.focus_handle();
        Self {
            session,
            menu: ui::menu::Host::deferred(ui::layer::MENU, ui::menu::Return::To(focus.clone())),
            focus,
            return_focus: None,
            controls,
            visible: false,
            focused_once: false,
            pairs: Vec::new(),
            total: 0,
            initial_total: 0,
            shown: None,
            pending: None,
            ahead: None,
            pictures: HashMap::new(),
            loading: true,
            loading_message: false,
            resolving: false,
            navigating: false,
            error: None,
            generation: 0,
            own_revision: 0,
            zoom: LinkedZoom::default(),
            drag: None,
            difference_hovered: false,
            difference_focused: false,
            difference_pressed: false,
            difference_held: false,
            panes: [Bounds::default(); 2],
            fetch_task: None,
            difference_task: None,
            ahead_task: None,
            message_task: None,
            scanning: false,
            scan_progress: false,
            scan_task: None,
            scan_fetch: None,
            smart_hovered: false,
            smart_focused: false,
            _session: subscriptions,
        }
    }

    /// The view that draws this screen's controls in the titlebar.
    pub fn toolbar(&self) -> Entity<DuplicatesToolbar> {
        self.controls.clone()
    }

    /// Asks for this screen and its titlebar controls to be drawn again.
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        self.controls.update(cx, |_, cx| cx.notify());
    }

    // The functions below run when the user enters or leaves the screen.

    /// Runs when the session's destination changes. It reads the queue when
    /// the user chooses this screen, takes keyboard focus once the screen is
    /// drawn, and gives focus back when the user leaves.
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
            // The pair from the last visit is still in `shown` and is drawn
            // at once from its thumbnails. Its originals were released when
            // the user left the screen, so they are decoded again.
            self.load_pictures(cx);
            self.refresh_difference(cx);
        }
        self.fetch(Fetch::Queue { reset: true }, cx);
    }

    fn left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Answers that are still on their way are ignored from here on. The
        // pair on screen is kept, because the screen is still drawn while it
        // fades out.
        self.generation += 1;
        self.fetch_task = None;
        self.pending = None;
        self.resolving = false;
        self.navigating = false;
        self.drag = None;
        self.difference_hovered = false;
        self.difference_focused = false;
        self.difference_pressed = false;
        self.difference_held = false;
        self.smart_hovered = false;
        self.smart_focused = false;
        self.focused_once = false;
        if self.focus.contains_focused(window, cx) {
            match self.return_focus.take() {
                Some(handle) => window.focus(&handle, cx),
                None => window.blur(cx),
            }
        }
        self.changed(cx);
    }

    /// Releases the decoded originals and the difference pictures once the
    /// screen is no longer drawn. They use the most memory. The thumbnails
    /// are kept, so the pair is drawn at once when the user comes back.
    fn release_originals(&mut self, cx: &mut Context<Self>) {
        self.difference_task = None;
        self.ahead_task = None;
        let mut released = Vec::new();
        for pair in [self.shown.as_mut(), self.ahead.as_mut()]
            .into_iter()
            .flatten()
        {
            released.extend(pair.difference.take().map(|(image, _)| image));
            for side in &mut pair.sides {
                side.original_since = None;
            }
        }
        for pictures in self.pictures.values_mut() {
            released.extend(pictures.original.take());
            pictures.original_pending = false;
            pictures.original_task = None;
        }
        for image in released {
            crate::atlas::release(image, cx);
        }
    }

    // The two functions below are used by the reads from the library.

    fn pair(&self, key: PairKey) -> Option<&DuplicateCandidate> {
        self.pairs
            .iter()
            .find(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
    }

    /// Records that no read and no decision is under way any more.
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

    // The two functions below scan the library for duplicates.

    /// Runs when the user clicks "Scan library" or "Retry" in the empty
    /// state. The application compares the perceptual hashes of all images
    /// (`duplicates.scan`), and then the screen reads the queue again. The
    /// progress bar is shown if the scan takes longer than 200 ms.
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
                    ui::notify::info(t("Duplicate scan complete"), scan_message(found), cx);
                    this.fetch(Fetch::Queue { reset: true }, cx);
                    this.scan_fetch = Some(this.generation);
                }
                Ok(Err(error)) => {
                    ui::notify::error(t("Unable to scan for duplicates"), error.clone(), cx);
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

    // The functions below read from the library and put a pair on screen.

    /// Reads from the library what `fetch` asks for, and then prepares the
    /// pair to show. The pair on screen stays on screen until the details
    /// and both thumbnails of the next pair are ready.
    fn fetch(&mut self, fetch: Fetch, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.pending = None;
        self.navigating = matches!(fetch, Fetch::Pair(_));
        let previous = self
            .pairs
            .iter()
            .map(|pair| pair_key(pair.file_id_a, pair.file_id_b))
            .collect::<Vec<_>>();
        let shown = self.shown.as_ref().map(|shown| shown.key);
        // For a move to another pair of the queue that is already in
        // memory: that pair, and the pair after it.
        let known = match fetch {
            Fetch::Pair(key) => previous
                .iter()
                .position(|each| *each == key)
                .map(|at| (self.pairs[at].clone(), self.pairs.get(at + 1).cloned())),
            _ => None,
        };
        let receiver = self.session.read(cx).run(move |library| {
            let error = |error: picto_library::LibraryError| error.to_string();
            let mut fetched = Fetched {
                pairs: None,
                key: None,
                details: None,
                ahead: None,
                count: None,
            };
            let (pair, after) = match fetch {
                Fetch::Pair(_) => known.map_or((None, None), |(pair, after)| (Some(pair), after)),
                Fetch::Queue { .. } | Fetch::Settled(_) => {
                    // After a decision the decided pair is no longer in the
                    // queue. `select_stable` then picks the pair that
                    // followed it in the old queue.
                    let active = match fetch {
                        Fetch::Settled(key) => Some(key),
                        _ => shown,
                    };
                    let pairs = library.duplicate_candidates(QUEUE_LIMIT).map_err(error)?;
                    let keys = pairs
                        .iter()
                        .map(|pair| pair_key(pair.file_id_a, pair.file_id_b))
                        .collect::<Vec<_>>();
                    let at = compare::select_stable(&previous, active, &keys)
                        .and_then(|key| keys.iter().position(|each| *each == key));
                    let found = (
                        at.map(|at| pairs[at].clone()),
                        at.and_then(|at| pairs.get(at + 1).cloned()),
                    );
                    fetched.pairs = Some(pairs);
                    fetched.count = Some(library.sidebar_counts().map_err(error)?.duplicates);
                    found
                }
            };
            let details = |pair: &DuplicateCandidate| {
                [&pair.left, &pair.right].map(|side| {
                    side.occurrences
                        .first()
                        .and_then(|occurrence| library.details(occurrence.root_item_id).ok())
                })
            };
            if let Some(pair) = pair {
                let key = pair_key(pair.file_id_a, pair.file_id_b);
                fetched.key = Some(key);
                if Some(key) != shown {
                    fetched.details = Some(details(&pair));
                }
                fetched.ahead = after.map(|after| {
                    let details = details(&after);
                    (after, details)
                });
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
                let title = match fetch {
                    Fetch::Pair(_) => t("Unable to load duplicate media"),
                    Fetch::Queue { .. } | Fetch::Settled(_) => t("Unable to load duplicate review"),
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
        let ahead = fetched
            .ahead
            .map(|(pair, details)| Shown::new(&pair, details));
        let pair = fetched.key.and_then(|key| {
            fetched
                .pairs
                .as_ref()
                .unwrap_or(&self.pairs)
                .iter()
                .find(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
                .cloned()
        });
        let (Some(pair), Some(details)) = (pair, fetched.details) else {
            // Either the pair to show is the pair already on screen, or the
            // queue is empty.
            if let Some(pairs) = fetched.pairs {
                self.set_pairs(pairs, reset);
            }
            if fetched.key.is_none() && !matches!(fetch, Fetch::Pair(_)) {
                self.release(cx);
            }
            self.set_ahead(ahead, cx);
            self.error = None;
            self.settle();
            self.changed(cx);
            return;
        };
        self.pending = Some(Pending {
            pairs: fetched.pairs,
            reset,
            shown: Shown::new(&pair, details),
            ahead,
        });
        self.load_pictures(cx);
        self.commit_when_ready(cx);
    }

    fn set_pairs(&mut self, pairs: Vec<DuplicateCandidate>, reset: bool) {
        self.total = pairs.len();
        if reset {
            self.initial_total = self.total;
        }
        self.pairs = pairs;
    }

    /// Puts the pair in `pending` on screen, if both of its thumbnails were
    /// decoded or could not be decoded. Otherwise it does nothing, and it
    /// is called again when a thumbnail finishes decoding.
    fn commit_when_ready(&mut self, cx: &mut Context<Self>) {
        let ready = self.pending.as_ref().is_some_and(|pending| {
            pending.shown.hashes().all(|hash| {
                self.pictures
                    .get(hash)
                    .is_some_and(|pictures| pictures.thumbnail_settled)
            })
        });
        if !ready {
            return;
        }
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if let Some(pairs) = pending.pairs {
            self.set_pairs(pairs, pending.reset);
        }
        if self.shown.as_ref().map(|old| old.key) != Some(pending.shown.key) {
            self.zoom.fit();
        }
        self.drag = None;
        self.difference_task = None;
        self.ahead_task = None;
        // If this pair was the pair in `ahead`, its originals may already
        // have been compared. The result is carried over, so the titlebar's
        // line of text is complete as soon as the pair is on screen.
        if let Some(ahead) = self.ahead.take_if(|ahead| ahead.key == pending.shown.key) {
            pending.shown.difference = ahead.difference;
            pending.shown.counted = ahead.counted;
        }
        if let Some((image, _)) = self.shown.take().and_then(|old| old.difference) {
            crate::atlas::release(image, cx);
        }
        self.shown = Some(pending.shown);
        self.error = None;
        self.settle();
        self.set_ahead(pending.ahead, cx);
        self.refresh_difference(cx);
        self.changed(cx);
    }

    /// Removes the pair from the screen and releases its pictures.
    fn release(&mut self, cx: &mut Context<Self>) {
        self.difference_task = None;
        self.drag = None;
        self.pending = None;
        if let Some((image, _)) = self.shown.take().and_then(|shown| shown.difference) {
            crate::atlas::release(image, cx);
        }
        self.keep_pictures(cx);
    }

    // The functions below decode, keep and release the pictures.

    /// Sets the pair whose pictures are decoded in advance. It is the pair
    /// that follows the pair on screen in the queue.
    fn set_ahead(&mut self, ahead: Option<Shown>, cx: &mut Context<Self>) {
        // If the pair is the same as before, what was already computed for
        // it is kept.
        let same = match (&self.ahead, &ahead) {
            (Some(old), Some(new)) => old.key == new.key,
            _ => false,
        };
        if !same {
            self.ahead_task = None;
            if let Some((image, _)) = self.ahead.take().and_then(|old| old.difference) {
                crate::atlas::release(image, cx);
            }
            self.ahead = ahead;
        }
        self.keep_pictures(cx);
        self.load_pictures(cx);
        self.compare_ahead(cx);
    }

    /// Releases the pictures of every file that belongs to none of the
    /// three pairs the screen keeps.
    fn keep_pictures(&mut self, cx: &mut Context<Self>) {
        let kept = [
            self.shown.as_ref(),
            self.pending.as_ref().map(|pending| &pending.shown),
            self.ahead.as_ref(),
        ]
        .into_iter()
        .flatten()
        .flat_map(Shown::hashes)
        .cloned()
        .collect::<std::collections::HashSet<_>>();
        let gone = self
            .pictures
            .keys()
            .filter(|hash| !kept.contains(*hash))
            .cloned()
            .collect::<Vec<_>>();
        for hash in gone {
            if let Some(pictures) = self.pictures.remove(&hash) {
                for image in pictures.thumbnail.into_iter().chain(pictures.original) {
                    crate::atlas::release(image, cx);
                }
            }
        }
    }

    /// Starts decoding every thumbnail and original that the three pairs
    /// need and that is neither decoded nor being decoded.
    fn load_pictures(&mut self, cx: &mut Context<Self>) {
        let wanted = [
            self.shown.as_ref(),
            self.pending.as_ref().map(|pending| &pending.shown),
            self.ahead.as_ref(),
        ]
        .into_iter()
        .flatten()
        .flat_map(|pair| &pair.sides)
        .map(|side| {
            (
                side.file.file_hash.clone(),
                side.file.mime_type.clone(),
                side.original(),
            )
        })
        .collect::<Vec<_>>();
        for (hash, mime, original) in wanted {
            let thumbnail = self.session.read(cx).thumbnail(&hash);
            let pictures = self.pictures.entry(hash.clone()).or_default();
            if !pictures.thumbnail_settled && pictures.thumbnail_task.is_none() {
                let load = load_image(thumbnail, "", cx);
                let hash = hash.clone();
                pictures.thumbnail_task = Some(cx.spawn(async move |this, cx| {
                    let image = load.await;
                    let _ = this.update(cx, |this, cx| {
                        match this.pictures.get_mut(&hash) {
                            Some(pictures) => {
                                pictures.thumbnail = image;
                                pictures.thumbnail_settled = true;
                            }
                            None => {
                                if let Some(image) = image {
                                    crate::atlas::release(image, cx);
                                }
                                return;
                            }
                        }
                        this.commit_when_ready(cx);
                    });
                }));
            }
            let pictures = self.pictures.entry(hash.clone()).or_default();
            if let Some(path) = original
                && pictures.original.is_none()
                && !pictures.original_pending
            {
                pictures.original_pending = true;
                let load = load_image(path, &mime, cx);
                pictures.original_task = Some(cx.spawn(async move |this, cx| {
                    let image = load.await;
                    let _ = this.update(cx, |this, cx| this.original_arrived(hash, image, cx));
                }));
            }
        }
    }

    /// Runs when an original finishes decoding. `image` is `None` when the
    /// file could not be decoded.
    fn original_arrived(
        &mut self,
        hash: String,
        image: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        let Some(pictures) = self.pictures.get_mut(&hash) else {
            if let Some(image) = image {
                crate::atlas::release(image, cx);
            }
            return;
        };
        pictures.original_pending = false;
        let decoded = image.is_some();
        if let Some(image) = image
            && let Some(old) = pictures.original.replace(image)
        {
            crate::atlas::release(old, cx);
        }
        for pair in [
            self.shown.as_mut(),
            self.pending.as_mut().map(|pending| &mut pending.shown),
            self.ahead.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            // Without this original the pair cannot be compared.
            if !decoded
                && pair.counted == Counted::Waiting
                && pair.hashes().any(|each| *each == hash)
            {
                pair.counted = Counted::Impossible;
            }
        }
        let mut on_screen = false;
        if let Some(shown) = &mut self.shown {
            for side in &mut shown.sides {
                if side.file.file_hash == hash {
                    if decoded {
                        side.original_since = Some(Instant::now());
                    }
                    on_screen = true;
                }
            }
        }
        if on_screen {
            self.refresh_difference(cx);
            self.changed(cx);
        }
        self.compare_ahead(cx);
    }

    /// The decoded thumbnail and the decoded original of a file, each if it
    /// exists.
    fn pictures_of(&self, side: &Side) -> (Option<Arc<RenderImage>>, Option<Arc<RenderImage>>) {
        self.pictures
            .get(&side.file.file_hash)
            .map_or((None, None), |pictures| {
                (pictures.thumbnail.clone(), pictures.original.clone())
            })
    }

    /// Computes the difference picture of the pair on screen from the best
    /// pictures that are decoded: each file's original, or its thumbnail
    /// while the original is not decoded. When both pictures are originals,
    /// the computation also counts the pixels that differ.
    fn refresh_difference(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = &self.shown else {
            return;
        };
        // While an original is still being decoded, a difference picture
        // that already exists is kept. It is computed again when that
        // original arrives.
        let waiting = shown.hashes().any(|hash| {
            self.pictures
                .get(hash)
                .is_some_and(|pictures| pictures.original_pending)
        });
        let [left, right] = shown.sides.each_ref().map(|side| self.pictures_of(side));
        let from_originals = [left.1.is_some(), right.1.is_some()];
        if shown
            .difference
            .as_ref()
            .is_some_and(|(_, made)| *made == from_originals || waiting)
        {
            return;
        }
        let (Some(left), Some(right)) = (left.1.or(left.0), right.1.or(right.0)) else {
            return;
        };
        let key = shown.key;
        let work = cx
            .background_executor()
            .spawn(async move { difference::compute(&left, &right) });
        self.difference_task = Some(cx.spawn(async move |this, cx| {
            let made = work.await;
            let _ = this.update(cx, |this, cx| {
                this.store_difference(key, made, from_originals, cx)
            });
        }));
    }

    /// Compares the two originals of the pair in `ahead` once both are
    /// decoded, so that the result is known before the pair is put on
    /// screen.
    fn compare_ahead(&mut self, cx: &mut Context<Self>) {
        let Some(ahead) = &self.ahead else {
            return;
        };
        if ahead.counted != Counted::Waiting || self.ahead_task.is_some() {
            return;
        }
        let [left, right] = ahead.sides.each_ref().map(|side| self.pictures_of(side).1);
        let (Some(left), Some(right)) = (left, right) else {
            return;
        };
        let key = ahead.key;
        let work = cx
            .background_executor()
            .spawn(async move { difference::compute(&left, &right) });
        self.ahead_task = Some(cx.spawn(async move |this, cx| {
            let made = work.await;
            let _ = this.update(cx, |this, cx| {
                this.store_difference(key, made, [true, true], cx)
            });
        }));
    }

    /// Stores a computed difference picture in the pair with this key, which
    /// is the pair on screen or the pair in `ahead`. `from_originals` says for each
    /// file whether its original was used.
    fn store_difference(
        &mut self,
        key: PairKey,
        made: Option<(Arc<RenderImage>, difference::Measure)>,
        from_originals: [bool; 2],
        cx: &mut Context<Self>,
    ) {
        let on_screen = self.shown.as_ref().is_some_and(|shown| shown.key == key);
        let pair = if on_screen {
            self.shown.as_mut()
        } else {
            self.ahead.as_mut().filter(|ahead| ahead.key == key)
        };
        let (Some(pair), Some((image, measure))) = (pair, made) else {
            return;
        };
        if let Some((old, _)) = pair.difference.replace((image, from_originals)) {
            crate::atlas::release(old, cx);
        }
        if from_originals == [true, true] && pair.counted == Counted::Waiting {
            pair.counted = Counted::Done(measure);
        }
        if on_screen {
            self.changed(cx);
        }
    }

    // The functions below handle the mouse wheel and a drag on a picture.

    fn pane_extent(&self, side: usize) -> Extent {
        extent(self.panes[side])
    }

    fn image(&self, side: usize) -> Extent {
        self.shown
            .as_ref()
            .map_or_else(Extent::default, |shown| shown.sides[side].image())
    }

    /// Runs when the mouse wheel turns over a picture. It zooms both
    /// pictures around the pointer. It never moves to another pair.
    fn wheel(&mut self, side: usize, event: &ScrollEvent, window: &Window, cx: &mut Context<Self>) {
        let bounds = self.panes[side];
        let delta = event.delta.pixel_delta(window.line_height());
        let offset = event.position - bounds.center();
        // GPUI reports how far the content should move. The DOM's `deltaY`,
        // which the zoom arithmetic expects, has the opposite sign.
        self.zoom.wheel(
            self.pane_extent(side),
            self.image(side),
            pane_edge(side),
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
            self.pane_extent(side),
            self.image(side),
            center,
            (f32::from(moved.x), f32::from(moved.y)),
        );
        cx.notify();
    }
}

/// The edge that a picture narrower than its pane is drawn against. The
/// left picture is drawn against the right edge of its pane and the right
/// picture against the left edge of its pane, so the two pictures are next
/// to each other.
fn pane_edge(side: usize) -> compare::PaneEdge {
    if side == 0 {
        compare::PaneEdge::Right
    } else {
        compare::PaneEdge::Left
    }
}

fn extent(bounds: Bounds<Pixels>) -> Extent {
    Extent::new(f32::from(bounds.size.width), f32::from(bounds.size.height))
}

/// Starts decoding an image file on a background thread. A JPEG XL file is
/// decoded with the viewer's JPEG XL decoder, and every other file with
/// GPUI's image loader.
fn load_image(path: PathBuf, mime: &str, cx: &mut App) -> Task<Option<Arc<RenderImage>>> {
    use crate::viewer::document;
    if document::kind::renderer_kind(mime) == document::kind::RendererKind::JpegXl {
        return cx.background_executor().spawn(async move {
            document::pages::decode_jpeg_xl(&path)
                .map(document::render_image)
                .ok()
        });
    }
    let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
    cx.background_executor()
        .spawn(async move { load.await.ok() })
}

/// The view that draws this screen's controls in the titlebar. The drawing
/// code is `DuplicatesScreen::titlebar` in `view.rs`.
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
