//! The Duplicates screen: the library's matching files gathered into sets
//! (`sets`), each with the file to keep marked, merged, compared or kept
//! apart one set at a time or checked and merged together. Three views
//! share the screen: the list of sets (`list`), Compare, one picture at a
//! time flipping between a set's files in place (`compare_view`), and the
//! check before a merge of sets whose names, links, folders, tags or notes
//! differ (`verify`). Their controls sit in the titlebar
//! ([`DuplicatesToolbar`]).
mod compare;
mod compare_view;
mod difference;
mod list;
mod sets;
mod verify;
mod view;

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
use crate::grid::preview::HoverPreview;
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::{en, t};
use crate::motion;
use crate::session::{Change, Report, Reread, Session};
use crate::ui;
use compare::LinkedZoom;
use gpui::*;
use picto_library::{DuplicateResolutionChoice, FileId, RootDetails};
use sets::Set;
use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc};

// The screen's actions. Their keys come from the shortcut registry
// (`crate::keymap`) and win over the grid's while the screen has the
// keyboard.
actions!(
    duplicates,
    [
        Previous,
        Next,
        SetAbove,
        SetBelow,
        Merge,
        OpenCompare,
        KeepAll,
        NotDuplicate,
        FitPair,
        ActualPixels,
        Leave
    ]
);

// i18n: a key context
const CONTEXT: &str = "Duplicates";
/// The session's place for the check before merging; Compare's place is
/// its set's key.
const VERIFY_PLACE: i64 = -1;
/// The library hands at most this many pairs at a time.
const QUEUE_LIMIT: usize = 500;
/// `scanDuplicates(distanceThreshold = 16)`.
const SCAN_DISTANCE: u32 = 16;
/// The Similar tab's similarity floor, in whole percent: Eagle's range.
const SIMILARITY_MIN: u32 = 80;

/// A file of a set, with its item's details once read.
#[derive(Clone)]
pub struct Facts {
    pub details: Option<RootDetails>,
}

impl Facts {
    /// The item's name, or the file's hash while it is not known.
    fn name(&self, set: &Set, index: usize) -> String {
        let hash = &set.files[index].file.file_hash;
        self.media(hash)
            .map(|media| media.media_name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| {
                self.details
                    .as_ref()
                    .map(|details| details.root.name.clone())
            })
            .unwrap_or_else(|| hash.chars().take(12).collect())
    }

    fn media(&self, hash: &str) -> Option<&picto_library::MediaRecord> {
        let details = self.details.as_ref()?;
        details
            .media
            .iter()
            .find(|media| media.facts.content_hash == hash)
            .or(details.media.first())
    }

    fn tags(&self) -> Option<usize> {
        self.details.as_ref().map(|details| details.tag_ids.len())
    }
}

/// A set as the screen holds it: the set and its files' facts.
pub struct Entry {
    pub set: Set,
    pub facts: Vec<Facts>,
}

/// The two tabs of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Identical,
    Similar,
}

/// The list's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Saving,
    Files,
    Alike,
}

impl Sort {
    // i18n: values, not shown
    const VALUES: [(Sort, &'static str); 3] = [
        (Sort::Saving, "saving"),
        (Sort::Files, "files"),
        (Sort::Alike, "alike"),
    ];
}

/// Which view of the screen shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    List,
    /// One set's files one at a time, `shown` the one on screen.
    Compare {
        set: u32,
        shown: usize,
    },
    /// The check before merging the checked sets that differ, `current` the
    /// one shown of `sets`.
    Verify {
        sets: Vec<u32>,
        current: usize,
    },
}

/// Compare's pictures and pointer state.
#[derive(Default)]
struct CompareState {
    zoom: LinkedZoom,
    /// Each file's original, by file, once decoded.
    originals: HashMap<u32, Arc<RenderImage>>,
    loads: HashMap<u32, Task<()>>,
    /// The shown file against the kept one, and the two it was made from.
    difference: Option<((u32, u32), Arc<RenderImage>)>,
    difference_task: Option<Task<()>>,
    difference_hovered: bool,
    pane: Bounds<Pixels>,
    drag: Option<(Point<Pixels>, (f32, f32))>,
    /// The wheel's travel toward the next flip.
    wheel: f32,
}

pub struct DuplicatesScreen {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    focus: FocusHandle,
    /// Whatever had the keyboard before this screen took it.
    return_focus: Option<FocusHandle>,
    controls: Entity<DuplicatesToolbar>,
    /// The session's destination is this screen.
    visible: bool,
    /// The keyboard was taken for this visit.
    focused_once: bool,
    entries: Vec<Entry>,
    /// The library had more pairs than it hands at once.
    capped: bool,
    tab: Tab,
    sort: Sort,
    /// The Similar tab shows sets at least this alike, in percent.
    similarity: u32,
    /// The file to keep where the user chose one, by set.
    keepers: HashMap<u32, FileId>,
    /// Sets checked or unchecked by the user, by set.
    checks: HashMap<u32, bool>,
    /// The sets the tab shows, in order, as indices of `entries`.
    order: Vec<usize>,
    /// The keyboard's place: a set and one of its files.
    cursor: Option<(u32, usize)>,
    mode: Mode,
    /// The sets the check before merging shows, kept while it is open so
    /// Back and Forward can return to it.
    verify_sets: Vec<u32>,
    /// The pairs are being read for the first time.
    loading: bool,
    /// The loading message, which waits 200 ms.
    loading_message: bool,
    /// A decision is being written.
    busy: bool,
    error: Option<String>,
    /// Counts reads, so a late answer to an earlier one is dropped.
    generation: u64,
    fetch_task: Option<Task<()>>,
    message_task: Option<Task<()>>,
    scanning: bool,
    /// The scan's progress bar, which waits 200 ms.
    scan_progress: bool,
    scan_task: Option<Task<()>>,
    /// The list's scroll and the area it was last laid out in.
    scroll: ScrollHandle,
    bounds: Bounds<Pixels>,
    /// A set wider than its place scrolls sideways, each with its own.
    rows: HashMap<u32, ScrollHandle>,
    art: Option<crate::grid::art::Art>,
    preview: HoverPreview,
    compare: CompareState,
    /// A tile's menu.
    menu: ui::menu::Host,
    _session: Vec<Subscription>,
}

impl DuplicatesScreen {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
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
            // A scan made elsewhere, or an undo, changes the pairs; a
            // decision of this screen reads them itself.
            cx.subscribe(&session, |this, _, event: &BackendEvent, cx| {
                if let BackendEvent::Changed(changed) = event
                    && crate::application::touches(changed, "duplicates")
                    && this.visible
                    && !this.loading
                    && !this.busy
                    && !this.scanning
                {
                    this.fetch(cx);
                }
            }),
            cx.observe(&thumbnails, |this, _, cx| {
                if this.visible {
                    cx.notify();
                }
            }),
        ];
        let focus = cx.focus_handle();
        Self {
            session,
            thumbnails,
            menu: ui::menu::Host::deferred(ui::layer::MENU, ui::menu::Return::To(focus.clone())),
            focus,
            return_focus: None,
            controls,
            visible: false,
            focused_once: false,
            entries: Vec::new(),
            capped: false,
            tab: Tab::Identical,
            sort: Sort::Saving,
            similarity: SIMILARITY_MIN,
            keepers: HashMap::new(),
            checks: HashMap::new(),
            order: Vec::new(),
            cursor: None,
            mode: Mode::List,
            verify_sets: Vec::new(),
            loading: true,
            loading_message: false,
            busy: false,
            error: None,
            generation: 0,
            fetch_task: None,
            message_task: None,
            scanning: false,
            scan_progress: false,
            scan_task: None,
            scroll: ScrollHandle::new(),
            bounds: Bounds::default(),
            rows: HashMap::new(),
            art: None,
            preview: HoverPreview::after(motion::DUPLICATE_PREVIEW_DELAY),
            compare: CompareState::default(),
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

    /// Follows the session: reads the pairs when the screen is chosen,
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
        if chosen {
            self.sync_place(cx);
        }
    }

    /// Shows the view the session's place names: Compare and the check
    /// before merging are places of the screen, so the scope title names
    /// them and Back and Forward step between them and the list.
    fn sync_place(&mut self, cx: &mut Context<Self>) {
        let place = self.session.read(cx).place();
        let mode = match place {
            None => Mode::List,
            Some(VERIFY_PLACE) if !self.verify_sets.is_empty() => Mode::Verify {
                sets: self.verify_sets.clone(),
                current: match &self.mode {
                    Mode::Verify { current, .. } => *current,
                    _ => 0,
                },
            },
            Some(key) => match self.entry_index(key as u32) {
                Some(index) => {
                    let set = key as u32;
                    let shown = match self.mode {
                        Mode::Compare { set: now, shown } if now == set => shown,
                        _ => self
                            .cursor
                            .filter(|(cursor, _)| *cursor == set)
                            .map_or_else(|| self.keeper(index), |(_, file)| file),
                    };
                    Mode::Compare { set, shown }
                }
                None => Mode::List,
            },
        };
        if mode != self.mode {
            self.show(mode, cx);
        }
        self.name_place(cx);
    }

    /// Puts `mode` on screen without a step of history.
    fn show(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let leaving = matches!(self.mode, Mode::Compare { .. })
            && !matches!(&mode, Mode::Compare { set, .. } if Some(*set) == self.compared_set());
        if leaving {
            self.compare.drag = None;
            self.compare.difference_hovered = false;
            self.compare.zoom.fit();
            self.release_originals(cx);
        }
        if let Mode::Verify { sets, .. } = &mode {
            self.verify_sets = sets.clone();
        }
        if let Mode::Compare { set, shown } = mode {
            self.cursor = Some((set, shown));
        }
        self.mode = mode;
        self.preview.close(cx);
        self.load_compared(cx);
        self.changed(cx);
    }

    /// Goes to `mode` as a step of Back and Forward.
    fn go(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let place = match &mode {
            Mode::List => None,
            Mode::Compare { set, .. } => Some(*set as i64),
            Mode::Verify { .. } => Some(VERIFY_PLACE),
        };
        self.show(mode, cx);
        self.session
            .update(cx, |session, cx| session.open_place(place, cx));
        self.name_place(cx);
    }

    fn compared_set(&self) -> Option<u32> {
        match self.mode {
            Mode::Compare { set, .. } => Some(set),
            _ => None,
        }
    }

    /// Names the open place in the scope title.
    fn name_place(&self, cx: &mut Context<Self>) {
        let label = match &self.mode {
            Mode::List => None,
            Mode::Compare { set, .. } => self.entry_index(*set).map(|index| {
                let keeper = self.keeper(index);
                let entry = &self.entries[index];
                SharedString::from(entry.facts[keeper].name(&entry.set, keeper))
            }),
            Mode::Verify { .. } => Some(SharedString::from(en("Verify Before Merge"))),
        };
        self.session
            .update(cx, |session, cx| session.set_place_label(label, cx));
    }

    fn opened(&mut self, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
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
        }
        self.fetch(cx);
    }

    fn left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Nothing still on its way applies any more; what is on screen
        // stays while the screen fades out.
        self.generation += 1;
        self.fetch_task = None;
        self.compare.drag = None;
        self.compare.difference_hovered = false;
        self.focused_once = false;
        self.preview.close(cx);
        if self.focus.contains_focused(window, cx) {
            match self.return_focus.take() {
                Some(handle) => window.focus(&handle, cx),
                None => window.blur(cx),
            }
        }
        self.changed(cx);
    }

    /// Lets Compare's originals and difference picture go once the screen
    /// is no longer painted.
    fn release_originals(&mut self, cx: &mut Context<Self>) {
        let compare = &mut self.compare;
        compare.loads.clear();
        compare.difference_task = None;
        let images = compare
            .originals
            .drain()
            .map(|(_, image)| image)
            .chain(compare.difference.take().map(|(_, image)| image))
            .collect::<Vec<_>>();
        for image in images {
            crate::atlas::release(image, cx);
        }
    }

    // ── Reading ─────────────────────────────────────────────────

    /// Reads the pairs, builds the sets and reads each file's item.
    fn fetch(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let receiver = self.session.read(cx).run(move |library| {
            let error = |error: picto_library::LibraryError| error.to_string();
            // The library hands its pairs least apart first, so the similar
            // ones are read in a window of their own, past the identical.
            let mut pairs = library.duplicate_candidates(QUEUE_LIMIT).map_err(error)?;
            let similar = library
                .duplicate_candidates_from(1, QUEUE_LIMIT)
                .map_err(error)?;
            let capped = pairs.len() >= QUEUE_LIMIT || similar.len() >= QUEUE_LIMIT;
            let known = pairs
                .iter()
                .map(|pair| (pair.file_id_a, pair.file_id_b))
                .collect::<std::collections::HashSet<_>>();
            pairs.extend(
                similar
                    .into_iter()
                    .filter(|pair| !known.contains(&(pair.file_id_a, pair.file_id_b))),
            );
            let entries = sets::build(&pairs)
                .into_iter()
                .map(|set| {
                    let facts = set
                        .files
                        .iter()
                        .map(|side| Facts {
                            details: side.occurrences.first().and_then(|occurrence| {
                                library.details(occurrence.root_item_id).ok()
                            }),
                        })
                        .collect();
                    Entry { set, facts }
                })
                .collect::<Vec<_>>();
            let count = library.sidebar_counts().map_err(error)?.duplicates;
            Ok::<_, String>((entries, capped, count))
        });
        self.fetch_task = Some(cx.spawn(async move |this, cx| {
            let Ok(reply) = receiver.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.received(generation, reply, cx));
        }));
        self.changed(cx);
    }

    fn received(
        &mut self,
        generation: u64,
        reply: Result<(Vec<Entry>, bool, u64), String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        self.loading_message = false;
        self.message_task = None;
        if self.scanning {
            self.end_scan();
        }
        let (entries, capped, count) = match reply {
            Ok(reply) => reply,
            Err(error) => {
                ui::notify::error(t("Unable to load duplicate review"), error.clone(), cx);
                self.error = Some(error);
                self.changed(cx);
                return;
            }
        };
        self.error = None;
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
        // Choices about sets that are gone are forgotten; a chosen file
        // that is no longer in its set is too.
        let alive = entries
            .iter()
            .map(|entry| (entry.set.key, &entry.set))
            .collect::<HashMap<_, _>>();
        self.keepers.retain(|key, file| {
            alive
                .get(key)
                .is_some_and(|set| set.index_of(*file).is_some())
        });
        self.checks.retain(|key, _| alive.contains_key(key));
        self.rows.retain(|key, _| alive.contains_key(key));
        self.entries = entries;
        self.capped = capped;
        // A tab with nothing in it gives way to the other.
        let has = |tab: Tab| {
            self.entries
                .iter()
                .any(|entry| (tab == Tab::Identical) == entry.set.identical())
        };
        if !has(self.tab) && has(other(self.tab)) {
            self.tab = other(self.tab);
        }
        self.reorder();
        self.settle_mode(cx);
        self.changed(cx);
    }

    /// The tab's sets in the chosen order.
    fn reorder(&mut self) {
        let identical = self.tab == Tab::Identical;
        let floor = 10_000u32.saturating_sub(self.similarity * 100);
        let mut order = (0..self.entries.len())
            .filter(|&index| {
                let set = &self.entries[index].set;
                set.identical() == identical && (identical || set.distance() <= floor)
            })
            .collect::<Vec<_>>();
        let entries = &self.entries;
        let saving = |index: usize| entries[index].set.saving(self.keeper(index));
        match self.sort {
            Sort::Saving => order.sort_by_key(|&index| std::cmp::Reverse(saving(index))),
            Sort::Files => order.sort_by_key(|&index| {
                (
                    std::cmp::Reverse(entries[index].set.files.len()),
                    std::cmp::Reverse(saving(index)),
                )
            }),
            Sort::Alike => order.sort_by_key(|&index| entries[index].set.distance()),
        }
        self.order = order;
        // The keyboard stays on its set if it is still shown.
        let still = self.cursor.filter(|(key, _)| {
            self.order
                .iter()
                .any(|&index| entries[index].set.key == *key)
        });
        self.cursor = still.or_else(|| {
            self.order
                .first()
                .map(|&index| (entries[index].set.key, self.keeper(index)))
        });
    }

    /// Leaves a view whose sets are gone.
    fn settle_mode(&mut self, cx: &mut Context<Self>) {
        match &self.mode {
            Mode::Compare { set, .. } if self.entry(*set).is_none() => {
                self.go(Mode::List, cx);
            }
            Mode::Verify { sets, .. } => {
                let left = sets
                    .iter()
                    .copied()
                    .filter(|key| self.entry(*key).is_some())
                    .collect::<Vec<_>>();
                if left.is_empty() {
                    self.verify_sets.clear();
                    self.go(Mode::List, cx);
                } else {
                    self.verify_sets = left.clone();
                    self.mode = Mode::Verify {
                        sets: left,
                        current: 0,
                    };
                }
            }
            _ => {}
        }
    }

    fn entry(&self, key: u32) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.set.key == key)
    }

    fn entry_index(&self, key: u32) -> Option<usize> {
        self.entries.iter().position(|entry| entry.set.key == key)
    }

    /// The file kept of the set at `index` of `entries`.
    fn keeper(&self, index: usize) -> usize {
        let set = &self.entries[index].set;
        self.keepers
            .get(&set.key)
            .and_then(|file| set.index_of(*file))
            .unwrap_or(set.best)
    }

    /// Whether the set at `index` is in the bulk merge: identical sets and
    /// sets with a clear best file are, unless the user said otherwise.
    fn checked(&self, index: usize) -> bool {
        let set = &self.entries[index].set;
        self.checks
            .get(&set.key)
            .copied()
            .unwrap_or(set.verdict != sets::Verdict::Unsure)
    }

    /// The tab's checked sets and what merging them frees.
    fn selected(&self) -> (Vec<usize>, i64) {
        let chosen = self
            .order
            .iter()
            .copied()
            .filter(|&index| self.checked(index))
            .collect::<Vec<_>>();
        let saving = chosen
            .iter()
            .map(|&index| self.entries[index].set.saving(self.keeper(index)))
            .sum();
        (chosen, saving)
    }

    /// How many sets each tab holds.
    fn tab_counts(&self) -> (usize, usize) {
        let identical = self
            .entries
            .iter()
            .filter(|entry| entry.set.identical())
            .count();
        (identical, self.entries.len() - identical)
    }

    // ── Scanning ────────────────────────────────────────────────

    /// "Scan library": the application compares every image's perceptual
    /// hash (`duplicates.scan`), then the pairs are read again. The
    /// progress bar shows if it takes longer than 200 ms.
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
                    this.fetch(cx);
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
    }

    // ── Decisions ───────────────────────────────────────────────

    /// Settles `steps` as one undoable step, then reads the pairs again.
    /// A merge says what it did, with Undo.
    fn resolve(
        &mut self,
        steps: Vec<(FileId, FileId, DuplicateResolutionChoice)>,
        merged: Option<(usize, i64)>,
        cx: &mut Context<Self>,
    ) {
        if self.busy || steps.is_empty() {
            return;
        }
        self.busy = true;
        self.preview.close(cx);
        self.compare.difference_hovered = false;
        let label = if merged.is_some() {
            "Merge duplicates"
        } else {
            "Keep duplicates apart"
        };
        crate::session::edit_then(
            &self.session,
            Report::failure(t("Unable to resolve duplicate pair")),
            Reread::NOTHING,
            move |library| library.resolve_duplicates(&steps, label, crate::now_ms()),
            cx,
            move |this: &mut Self, result, cx| {
                this.busy = false;
                if result.is_ok()
                    && let Some((sets, saving)) = merged
                {
                    let session = this.session.clone();
                    ui::notify::show(
                        ui::notify::Tone::Success,
                        ui::notify::Options::new(merged_message(sets, saving), "")
                            .action(t("Undo"), move |_, cx| {
                                session.update(cx, |session, cx| session.undo(cx))
                            }),
                        cx,
                    );
                }
                this.fetch(cx);
            },
        );
        self.changed(cx);
    }

    /// Merges one set into the file kept.
    fn merge_set(&mut self, key: u32, cx: &mut Context<Self>) {
        let Some(index) = self.entry_index(key) else {
            return;
        };
        let keeper = self.keeper(index);
        let set = &self.entries[index].set;
        let steps = set.merge_steps(keeper);
        let saving = set.saving(keeper);
        self.advance_from(key, cx);
        self.resolve(steps, Some((1, saving)), cx);
    }

    /// Keeps every file of a set: "Not duplicates" and "Keep all, related"
    /// both record its pairs as different files, which a later scan does
    /// not bring back.
    fn keep_apart(&mut self, key: u32, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(key) else {
            return;
        };
        let steps = entry.set.apart_steps();
        self.advance_from(key, cx);
        self.resolve(steps, None, cx);
    }

    /// Takes one file out of its set.
    fn remove_file(&mut self, key: u32, file: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(key) else {
            return;
        };
        let steps = entry.set.without_steps(file);
        self.resolve(steps, None, cx);
    }

    /// Merges every checked set of the tab, as one undoable step.
    fn merge_checked(&mut self, cx: &mut Context<Self>) {
        let (chosen, saving) = self.selected();
        let steps = chosen
            .iter()
            .flat_map(|&index| self.entries[index].set.merge_steps(self.keeper(index)))
            .collect::<Vec<_>>();
        if self.mode != Mode::List {
            self.go(Mode::List, cx);
        }
        self.verify_sets.clear();
        self.resolve(steps, Some((chosen.len(), saving)), cx);
    }

    /// The footer's button: the check first when a checked set differs in
    /// what a merge combines, else the merge.
    fn next_or_merge(&mut self, cx: &mut Context<Self>) {
        let (chosen, _) = self.selected();
        let differing = chosen
            .iter()
            .filter(|&&index| verify::differs(&self.entries[index]))
            .map(|&index| self.entries[index].set.key)
            .collect::<Vec<_>>();
        if differing.is_empty() {
            self.merge_checked(cx);
        } else {
            self.go(
                Mode::Verify {
                    sets: differing,
                    current: 0,
                },
                cx,
            );
        }
    }

    /// Moves the keyboard, and Compare, past a set about to go.
    fn advance_from(&mut self, key: u32, cx: &mut Context<Self>) {
        let at = self
            .order
            .iter()
            .position(|&index| self.entries[index].set.key == key);
        let next = at.and_then(|at| {
            self.order
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|before| self.order.get(before)))
                .copied()
        });
        let next = next.map(|index| (self.entries[index].set.key, self.keeper(index)));
        if self.cursor.is_some_and(|(cursor, _)| cursor == key) {
            self.cursor = next;
        }
        if matches!(self.mode, Mode::Compare { set, .. } if set == key) {
            let mode = match next {
                Some((set, shown)) => Mode::Compare { set, shown },
                None => Mode::List,
            };
            self.go(mode, cx);
        }
    }

    /// Makes `file` the one kept of its set.
    fn choose_keeper(&mut self, key: u32, file: usize, cx: &mut Context<Self>) {
        let Some(index) = self.entry_index(key) else {
            return;
        };
        let id = self.entries[index].set.files[file].file.file_id;
        self.keepers.insert(key, id);
        self.cursor = Some((key, file));
        self.changed(cx);
    }

    fn toggle_check(&mut self, key: u32, cx: &mut Context<Self>) {
        let Some(index) = self.entry_index(key) else {
            return;
        };
        let checked = !self.checked(index);
        self.checks.insert(key, checked);
        self.changed(cx);
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.cursor = None;
            self.scroll.set_offset(point(px(0.), px(0.)));
            self.reorder();
            self.changed(cx);
        }
    }

    fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        self.sort = sort;
        self.reorder();
        self.changed(cx);
    }

    fn set_similarity(&mut self, similarity: u32, cx: &mut Context<Self>) {
        if self.similarity != similarity {
            self.similarity = similarity;
            self.reorder();
            self.changed(cx);
        }
    }

    // ── Compare ─────────────────────────────────────────────────

    /// Opens a set in Compare at one of its files.
    fn open_compare(&mut self, key: u32, shown: usize, cx: &mut Context<Self>) {
        self.compare.zoom.fit();
        self.compare.wheel = 0.;
        self.go(Mode::Compare { set: key, shown }, cx);
    }

    fn back_to_list(&mut self, cx: &mut Context<Self>) {
        self.go(Mode::List, cx);
    }

    /// Shows another file of the set in Compare, keeping the zoom.
    fn flip(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Mode::Compare { set, shown } = self.mode else {
            return;
        };
        let Some(count) = self.entry(set).map(|entry| entry.set.files.len()) else {
            return;
        };
        let next = (shown as isize + delta).rem_euclid(count as isize) as usize;
        self.mode = Mode::Compare { set, shown: next };
        self.cursor = Some((set, next));
        self.load_compared(cx);
        self.changed(cx);
    }

    /// Skips to the next set in Compare, or back to the list after the last.
    fn skip(&mut self, cx: &mut Context<Self>) {
        let Mode::Compare { set, .. } = self.mode else {
            return;
        };
        let at = self
            .order
            .iter()
            .position(|&index| self.entries[index].set.key == set);
        match at.and_then(|at| self.order.get(at + 1)).copied() {
            Some(index) => {
                let key = self.entries[index].set.key;
                let keeper = self.keeper(index);
                self.open_compare(key, keeper, cx);
            }
            None => self.back_to_list(cx),
        }
    }

    /// Loads the originals Compare needs: the shown file, the kept one and
    /// the shown file's neighbours, each once.
    fn load_compared(&mut self, cx: &mut Context<Self>) {
        let Mode::Compare { set, shown } = self.mode else {
            return;
        };
        let Some(index) = self.entry_index(set) else {
            return;
        };
        let keeper = self.keeper(index);
        let entry = &self.entries[index];
        let count = entry.set.files.len();
        let wanted = [
            shown,
            keeper,
            (shown + 1) % count,
            (shown + count - 1) % count,
        ];
        let mut paths = Vec::new();
        for file in wanted {
            let side = &entry.set.files[file];
            let id = side.file.file_id.0;
            if self.compare.originals.contains_key(&id) || self.compare.loads.contains_key(&id) {
                continue;
            }
            let path = side
                .file
                .mime_type
                .starts_with("image/")
                .then(|| entry.facts[file].media(&side.file.file_hash))
                .flatten()
                .filter(|media| media.facts.content_hash == side.file.file_hash)
                .map(|media| PathBuf::from(&media.file_path));
            if let Some(path) = path {
                paths.push((id, path));
            }
        }
        for (id, path) in paths {
            let load = load_image(path, cx);
            self.compare.loads.insert(
                id,
                cx.spawn(async move |this, cx| {
                    let image = load.await;
                    let _ = this.update(cx, |this, cx| {
                        this.compare.loads.remove(&id);
                        if let Some(image) = image {
                            if let Some(old) = this.compare.originals.insert(id, image) {
                                crate::atlas::release(old, cx);
                            }
                            this.refresh_difference(cx);
                            cx.notify();
                        }
                    });
                }),
            );
        }
        self.refresh_difference(cx);
    }

    /// The shown file and the kept one in Compare, by file id.
    fn compared_pair(&self) -> Option<(u32, u32)> {
        let Mode::Compare { set, shown } = self.mode else {
            return None;
        };
        let index = self.entry_index(set)?;
        let files = &self.entries[index].set.files;
        Some((
            files[shown].file.file_id.0,
            files[self.keeper(index)].file.file_id.0,
        ))
    }

    /// Makes the difference of the shown file against the kept one, from
    /// their originals, when both are decoded and it is not made yet.
    fn refresh_difference(&mut self, cx: &mut Context<Self>) {
        let Some((shown, kept)) = self.compared_pair().filter(|(shown, kept)| shown != kept) else {
            return;
        };
        if self
            .compare
            .difference
            .as_ref()
            .is_some_and(|(pair, _)| *pair == (shown, kept))
        {
            return;
        }
        let (Some(left), Some(right)) = (
            self.compare.originals.get(&kept).cloned(),
            self.compare.originals.get(&shown).cloned(),
        ) else {
            return;
        };
        let work = cx
            .background_executor()
            .spawn(async move { difference::compute(&right, &left) });
        self.compare.difference_task = Some(cx.spawn(async move |this, cx| {
            let image = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.compared_pair() != Some((shown, kept)) {
                    if let Some(image) = image {
                        crate::atlas::release(image, cx);
                    }
                    return;
                }
                if let Some(image) = image
                    && let Some((_, old)) = this.compare.difference.replace(((shown, kept), image))
                {
                    crate::atlas::release(old, cx);
                }
                if this.compare.difference_hovered {
                    cx.notify();
                }
            });
        }));
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        self.compare.zoom.fit();
        self.changed(cx);
    }

    fn actual(&mut self, cx: &mut Context<Self>) {
        self.compare.zoom.actual();
        self.changed(cx);
    }

    /// The window's wheel over the picture: with Command (Control elsewhere)
    /// it zooms about the pointer; otherwise it flips between the files,
    /// one file per notch of travel.
    fn wheel(&mut self, event: &ScrollEvent, window: &Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height());
        if event.modifiers.secondary(cx) {
            let Some(image) = self.shown_extent() else {
                return;
            };
            let bounds = self.compare.pane;
            let offset = event.position - bounds.center();
            self.compare.zoom.wheel(
                pane_extent(bounds),
                image,
                (f32::from(offset.x), f32::from(offset.y)),
                -f32::from(delta.y),
            );
            self.changed(cx);
            return;
        }
        const NOTCH: f32 = 60.;
        self.compare.wheel += f32::from(delta.y) + f32::from(delta.x);
        if self.compare.wheel.abs() >= NOTCH {
            let step = if self.compare.wheel > 0. { -1 } else { 1 };
            self.compare.wheel = 0.;
            self.flip(step, cx);
        }
    }

    /// The shown file's size in pixels.
    fn shown_extent(&self) -> Option<compare::Extent> {
        let Mode::Compare { set, shown } = self.mode else {
            return None;
        };
        let file = &self.entry(set)?.set.files[shown].file;
        Some(compare::Extent::new(
            file.pixel_width? as f32,
            file.pixel_height? as f32,
        ))
    }

    // ── Keys ────────────────────────────────────────────────────

    /// ← and →: the files of a set, running on into the sets before and
    /// after; in Compare, the set's files.
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if matches!(self.mode, Mode::Compare { .. }) {
            self.flip(delta, cx);
            return;
        }
        if self.mode != Mode::List {
            return;
        }
        let Some((key, file)) = self.cursor else {
            self.reorder();
            return self.changed(cx);
        };
        let Some(at) = self
            .order
            .iter()
            .position(|&index| self.entries[index].set.key == key)
        else {
            return;
        };
        let count = self.entries[self.order[at]].set.files.len();
        let target = file as isize + delta;
        self.cursor = if (0..count as isize).contains(&target) {
            Some((key, target as usize))
        } else {
            let next = at as isize + delta.signum();
            match usize::try_from(next)
                .ok()
                .and_then(|next| self.order.get(next))
            {
                Some(&index) => {
                    let files = self.entries[index].set.files.len();
                    Some((
                        self.entries[index].set.key,
                        if delta > 0 { 0 } else { files - 1 },
                    ))
                }
                None => Some((key, file)),
            }
        };
        self.reveal_cursor();
        self.changed(cx);
    }

    /// ↑ and ↓: the set above or below in the list.
    fn step_row(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.mode != Mode::List {
            return;
        }
        let layout = self.layout();
        let Some((key, file)) = self.cursor else {
            return;
        };
        let Some(next) = layout.vertical(&self.order, &self.entries, key, delta) else {
            return;
        };
        let index = self.order[next];
        let files = self.entries[index].set.files.len();
        self.cursor = Some((self.entries[index].set.key, file.min(files - 1)));
        self.reveal_cursor();
        self.changed(cx);
    }

    /// Scrolls the list so the keyboard's set shows.
    fn reveal_cursor(&mut self) {
        let Some((key, file)) = self.cursor else {
            return;
        };
        let layout = self.layout();
        let Some(place) = layout.place_of(&self.order, &self.entries, key) else {
            return;
        };
        let top = place.top;
        let bottom = top + place.height();
        let view = f32::from(self.bounds.size.height);
        let scrolled = -f32::from(self.scroll.offset().y);
        let target = if top < scrolled {
            top
        } else if bottom > scrolled + view {
            bottom - view
        } else {
            scrolled
        };
        self.scroll.set_offset(point(px(0.), px(-target.max(0.))));
        // A set wider than its place brings the file into view.
        if let Some(row) = self.rows.get(&key) {
            let left = file as f32 * (list::TILE + list::TILE_GAP);
            let width = place.width - 2. * list::PAD_X;
            let scrolled = -f32::from(row.offset().x);
            let x = if left < scrolled {
                left
            } else if left + list::TILE > scrolled + width {
                left + list::TILE - width
            } else {
                scrolled
            };
            row.set_offset(point(px(-x.max(0.)), px(0.)));
        }
    }

    /// Enter: merges the set (in Compare, keeping the file shown).
    fn merge_key(&mut self, cx: &mut Context<Self>) {
        match self.mode {
            Mode::List => {
                if let Some((key, _)) = self.cursor {
                    self.merge_set(key, cx);
                }
            }
            Mode::Compare { set, shown } => {
                self.choose_keeper(set, shown, cx);
                self.merge_set(set, cx);
            }
            Mode::Verify { .. } => self.merge_checked(cx),
        }
    }

    /// Space: opens Compare on the keyboard's file; in Compare, skips.
    fn compare_key(&mut self, cx: &mut Context<Self>) {
        match self.mode {
            Mode::List => {
                if let Some((key, file)) = self.cursor {
                    self.open_compare(key, file, cx);
                }
            }
            Mode::Compare { .. } => self.skip(cx),
            Mode::Verify { .. } => {}
        }
    }

    /// N and A: the set on screen, or the keyboard's.
    fn apart_key(&mut self, cx: &mut Context<Self>) {
        let key = match self.mode {
            Mode::Compare { set, .. } => Some(set),
            Mode::List => self.cursor.map(|(key, _)| key),
            Mode::Verify { .. } => None,
        };
        if let Some(key) = key {
            self.keep_apart(key, cx);
        }
    }

    /// Escape: back to the list.
    fn leave(&mut self, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Compare { .. } | Mode::Verify { .. } => self.back_to_list(cx),
            Mode::List => {}
        }
    }
}

fn other(tab: Tab) -> Tab {
    match tab {
        Tab::Identical => Tab::Similar,
        Tab::Similar => Tab::Identical,
    }
}

fn pane_extent(bounds: Bounds<Pixels>) -> compare::Extent {
    compare::Extent::new(f32::from(bounds.size.width), f32::from(bounds.size.height))
}

/// What "Duplicate scan complete" says after a scan that found `found`
/// pairs.
fn scan_message(found: u64) -> String {
    if found > 0 {
        format!("Found {found} new review pairs")
    } else {
        en("Scan complete - no new review pairs").into()
    }
}

/// What a merge says it did.
fn merged_message(sets: usize, saving: i64) -> String {
    let size = crate::text::size_to_megabytes(saving);
    if sets == 1 {
        format!("Merged 1 set, {size} freed")
    } else {
        format!("Merged {sets} sets, {size} freed")
    }
}

/// Starts decoding an image file off the drawing thread.
fn load_image(path: PathBuf, cx: &mut App) -> Task<Option<Arc<RenderImage>>> {
    let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
    cx.background_executor()
        .spawn(async move { load.await.ok() })
}

/// The screen's titlebar controls: the tabs and the order in the list, the
/// set and its file in Compare.
pub struct DuplicatesToolbar {
    screen: WeakEntity<DuplicatesScreen>,
}
