//! The Duplicates screen: one desk for deciding on the library's matching
//! files. The core reports matching pairs; files that match one another are
//! reviewed together as a set (`sets`). Down the left, the queue of sets,
//! most alike first, in Identical and Similar tabs (`queue`). In the middle,
//! the stage: one file of the set at a time, flipping in place at the same
//! place and scale, its difference from the other file while the
//! difference button is held, and what was measured between them, in words
//! (`stage`). In the inspector's place, the rail: the files side by side,
//! and what the one item left after the merge will hold (`rail`). The file
//! shown is the one kept: the user flips to the file to keep and merges
//! the rest into it, or says they are not duplicates, or skips.
mod compare;
mod difference;
mod queue;
mod rail;
mod sets;
mod stage;
mod view;

use crate::application::Event as BackendEvent;
use crate::backend::Screen;
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

// The desk's actions. Their keys come from the shortcut registry
// (`crate::keymap`) and win over the grid's while the screen has the
// keyboard.
actions!(
    duplicates,
    [
        PreviousFile,
        NextFile,
        PreviousSet,
        NextSet,
        Merge,
        Skip,
        NotDuplicate,
        FitPair,
        ActualPixels
    ]
);

// i18n: a key context
const CONTEXT: &str = "Duplicates";
/// The library hands at most this many pairs at a time.
const QUEUE_LIMIT: usize = 500;
/// `scanDuplicates(distanceThreshold = 16)`.
const SCAN_DISTANCE: u32 = 16;

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
}

/// A set as the screen holds it: the set and its files' facts.
pub struct Entry {
    pub set: Set,
    pub facts: Vec<Facts>,
}

/// The queue's two tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Identical,
    Similar,
}

/// The stage's pictures and pointer state.
#[derive(Default)]
struct StageState {
    zoom: LinkedZoom,
    /// Each file's original, by file, once decoded.
    originals: HashMap<u32, Arc<RenderImage>>,
    loads: HashMap<u32, Task<()>>,
    /// The difference of the shown file from its partner, what it measured,
    /// and the two files it was made from.
    difference: Option<((u32, u32), Arc<RenderImage>, difference::Measure)>,
    difference_task: Option<Task<()>>,
    /// The difference button is held under the pointer.
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
    /// When the library had more pairs than it hands at once (500 of each
    /// kind), how many were read and how many there are.
    capped: Option<(usize, u64)>,
    tab: Tab,
    /// The tab's sets, most alike first, as indices of `entries`.
    order: Vec<usize>,
    /// The set on the stage, by its key, and the file shown of it.
    current: Option<u32>,
    shown: usize,
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
    /// The queue's scroll and the area it was last laid out in.
    queue_scroll: ScrollHandle,
    queue_bounds: Bounds<Pixels>,
    /// The screen's width as last laid out, which decides whether the
    /// queue shows its sets' names or only their pictures.
    width: f32,
    art: Option<crate::grid::art::Art>,
    stage: StageState,
    /// A file's menu.
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
        let controls = cx.new(|_| DuplicatesToolbar { screen, width: 0. });
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
            capped: None,
            tab: Tab::Identical,
            order: Vec::new(),
            current: None,
            shown: 0,
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
            queue_scroll: ScrollHandle::new(),
            queue_bounds: Bounds::default(),
            width: 0.,
            art: None,
            stage: StageState::default(),
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
        } else {
            self.load_compared(cx);
        }
        self.fetch(cx);
    }

    fn left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Nothing still on its way applies any more; what is on screen
        // stays while the screen fades out.
        self.generation += 1;
        self.fetch_task = None;
        self.stage.drag = None;
        self.stage.difference_hovered = false;
        self.focused_once = false;
        if self.focus.contains_focused(window, cx) {
            match self.return_focus.take() {
                Some(handle) => window.focus(&handle, cx),
                None => window.blur(cx),
            }
        }
        self.changed(cx);
    }

    /// Lets the originals and the difference picture go once the screen is
    /// no longer painted.
    fn release_originals(&mut self, cx: &mut Context<Self>) {
        let stage = &mut self.stage;
        stage.loads.clear();
        stage.difference_task = None;
        let images = stage
            .originals
            .drain()
            .map(|(_, image)| image)
            .chain(stage.difference.take().map(|(_, image, _)| image))
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
            let shown = pairs.len();
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
            Ok::<_, String>((entries, capped.then_some(shown), count))
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
        reply: Result<(Vec<Entry>, Option<usize>, u64), String>,
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
        let shown_file = self.shown_file();
        self.entries = entries;
        self.capped = capped.map(|shown| (shown, count));
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
        // The set on the stage stays, showing the same file, if it is still
        // there; else the first of the queue.
        let still = self
            .current
            .and_then(|key| self.entry_index(key))
            .filter(|index| self.order.contains(index));
        match still {
            Some(index) => {
                self.shown = shown_file
                    .and_then(|file| self.entries[index].set.index_of(file))
                    .unwrap_or(0);
            }
            None => self.show_set(self.order.first().copied(), cx),
        }
        self.load_compared(cx);
        self.changed(cx);
    }

    /// The tab's sets, most alike first: the queue's order is how alike
    /// the files are, nothing else, so the surest sets come first.
    fn reorder(&mut self) {
        let identical = self.tab == Tab::Identical;
        let mut order = (0..self.entries.len())
            .filter(|&index| self.entries[index].set.identical() == identical)
            .collect::<Vec<_>>();
        order.sort_by_key(|&index| self.entries[index].set.distance());
        self.order = order;
    }

    fn entry_index(&self, key: u32) -> Option<usize> {
        self.entries.iter().position(|entry| entry.set.key == key)
    }

    /// The set on the stage, as an index of `entries`.
    fn current_index(&self) -> Option<usize> {
        self.current.and_then(|key| self.entry_index(key))
    }

    /// The file shown, which is the one a merge keeps.
    fn shown_file(&self) -> Option<FileId> {
        let index = self.current_index()?;
        Some(self.entries[index].set.files.get(self.shown)?.file.file_id)
    }

    /// The file the shown one is compared with: the next of the set.
    fn partner(&self) -> Option<usize> {
        let index = self.current_index()?;
        let count = self.entries[index].set.files.len();
        (count > 1).then(|| (self.shown + 1) % count)
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

    /// Settles `steps` as one undoable step, moves to the next set, then
    /// reads the pairs again. A merge says what it did, with Undo.
    fn resolve(
        &mut self,
        steps: Vec<(FileId, FileId, DuplicateResolutionChoice)>,
        merged: bool,
        cx: &mut Context<Self>,
    ) {
        if self.busy || steps.is_empty() {
            return;
        }
        self.busy = true;
        self.stage.difference_hovered = false;
        self.advance(cx);
        let label = if merged {
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
                if result.is_ok() && merged {
                    let session = this.session.clone();
                    ui::notify::show(
                        ui::notify::Tone::Success,
                        ui::notify::Options::new(en("Merged"), "")
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

    /// Enter: keeps the file shown and merges the rest of its set into it.
    fn merge(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.current_index() else {
            return;
        };
        let steps = self.entries[index].set.merge_steps(self.shown);
        self.resolve(steps, true, cx);
    }

    /// N: the set's files are different pictures; none is touched, and a
    /// later scan does not bring the pairs back.
    fn not_duplicates(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.current_index() else {
            return;
        };
        let steps = self.entries[index].set.apart_steps();
        self.resolve(steps, false, cx);
    }

    /// Takes one file out of the set on the stage.
    fn remove_file(&mut self, file: usize, cx: &mut Context<Self>) {
        let Some(index) = self.current_index() else {
            return;
        };
        let steps = self.entries[index].set.without_steps(file);
        if self.busy || steps.is_empty() {
            return;
        }
        self.busy = true;
        crate::session::edit_then(
            &self.session,
            Report::failure(t("Unable to resolve duplicate pair")),
            Reread::NOTHING,
            move |library| {
                library.resolve_duplicates(&steps, "Keep duplicates apart", crate::now_ms())
            },
            cx,
            move |this: &mut Self, _, cx| {
                this.busy = false;
                this.fetch(cx);
            },
        );
        self.changed(cx);
    }

    /// Puts the next set of the queue on the stage, or the one before it
    /// when this was the last.
    fn advance(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self
            .current_index()
            .and_then(|index| self.order.iter().position(|&each| each == index))
        else {
            return;
        };
        let next = self
            .order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|before| self.order.get(before)))
            .copied();
        self.show_set(next, cx);
    }

    /// Space: the next set, this one left for later.
    fn skip(&mut self, cx: &mut Context<Self>) {
        self.step_set(1, cx);
    }

    /// ↑ and ↓: the set before or after in the queue.
    fn step_set(&mut self, delta: isize, cx: &mut Context<Self>) {
        let at = self
            .current_index()
            .and_then(|index| self.order.iter().position(|&each| each == index));
        let target = match at {
            Some(at) => at.checked_add_signed(delta),
            None => Some(0),
        };
        if let Some(index) = target.and_then(|target| self.order.get(target)).copied() {
            self.show_set(Some(index), cx);
            self.reveal_current();
            self.changed(cx);
        }
    }

    /// Puts a set on the stage, its first file shown, at the fitted zoom.
    fn show_set(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let key = index.map(|index| self.entries[index].set.key);
        if key == self.current {
            return;
        }
        self.current = key;
        self.shown = 0;
        self.stage.zoom.fit();
        self.stage.wheel = 0.;
        self.stage.drag = None;
        self.release_originals(cx);
        self.load_compared(cx);
        self.changed(cx);
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.queue_scroll.set_offset(point(px(0.), px(0.)));
            self.reorder();
            self.show_set(self.order.first().copied(), cx);
            self.changed(cx);
        }
    }

    /// ← and →, a click on the strip: another file of the set, at the same
    /// place and scale.
    fn show_file(&mut self, file: usize, cx: &mut Context<Self>) {
        let Some(index) = self.current_index() else {
            return;
        };
        if file >= self.entries[index].set.files.len() || file == self.shown {
            return;
        }
        self.shown = file;
        self.load_compared(cx);
        self.changed(cx);
    }

    fn flip(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(index) = self.current_index() else {
            return;
        };
        let count = self.entries[index].set.files.len() as isize;
        let next = (self.shown as isize + delta).rem_euclid(count.max(1)) as usize;
        self.show_file(next, cx);
    }

    /// Scrolls the queue so the set on the stage shows.
    fn reveal_current(&mut self) {
        let Some(at) = self
            .current_index()
            .and_then(|index| self.order.iter().position(|&each| each == index))
        else {
            return;
        };
        let top = queue::row_top(at);
        let bottom = top + queue::ROW;
        let view = f32::from(self.queue_bounds.size.height);
        let scrolled = -f32::from(self.queue_scroll.offset().y);
        let target = if top < scrolled {
            top
        } else if bottom > scrolled + view {
            bottom - view
        } else {
            scrolled
        };
        self.queue_scroll
            .set_offset(point(px(0.), px(-target.max(0.))));
    }

    // ── The stage's pictures ───────────────────────────────────

    /// Loads the originals the stage needs: the shown file, its partner and
    /// the file after, each once.
    fn load_compared(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.current_index() else {
            return;
        };
        let entry = &self.entries[index];
        let count = entry.set.files.len();
        let wanted = [
            self.shown,
            (self.shown + 1) % count,
            (self.shown + 2) % count,
        ];
        let mut paths = Vec::new();
        for file in wanted {
            let side = &entry.set.files[file];
            let id = side.file.file_id.0;
            if self.stage.originals.contains_key(&id) || self.stage.loads.contains_key(&id) {
                continue;
            }
            if let Some(path) = original(entry, file) {
                paths.push((id, path));
            }
        }
        for (id, path) in paths {
            let load = load_image(path, cx);
            self.stage.loads.insert(
                id,
                cx.spawn(async move |this, cx| {
                    let image = load.await;
                    let _ = this.update(cx, |this, cx| {
                        this.stage.loads.remove(&id);
                        if let Some(image) = image {
                            if let Some(old) = this.stage.originals.insert(id, image) {
                                crate::atlas::release(old, cx);
                            }
                            this.refresh_difference(cx);
                            this.changed(cx);
                        }
                    });
                }),
            );
        }
        self.refresh_difference(cx);
    }

    /// The shown file and its partner, by file id.
    fn compared_pair(&self) -> Option<(u32, u32)> {
        let index = self.current_index()?;
        let files = &self.entries[index].set.files;
        Some((
            files[self.shown].file.file_id.0,
            files[self.partner()?].file.file_id.0,
        ))
    }

    /// Makes the difference of the shown file from its partner, and what it
    /// measures, from their originals once both are decoded.
    fn refresh_difference(&mut self, cx: &mut Context<Self>) {
        let Some((shown, partner)) = self.compared_pair() else {
            return;
        };
        if self
            .stage
            .difference
            .as_ref()
            .is_some_and(|(pair, _, _)| *pair == (shown, partner))
        {
            return;
        }
        let (Some(left), Some(right)) = (
            self.stage.originals.get(&shown).cloned(),
            self.stage.originals.get(&partner).cloned(),
        ) else {
            return;
        };
        let work = cx
            .background_executor()
            .spawn(async move { difference::compute(&left, &right) });
        self.stage.difference_task = Some(cx.spawn(async move |this, cx| {
            let made = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.compared_pair() != Some((shown, partner)) {
                    if let Some((image, _)) = made {
                        crate::atlas::release(image, cx);
                    }
                    return;
                }
                if let Some((image, measure)) = made
                    && let Some((_, old, _)) =
                        this.stage
                            .difference
                            .replace(((shown, partner), image, measure))
                {
                    crate::atlas::release(old, cx);
                }
                cx.notify();
            });
        }));
    }

    /// What comparing the shown file with its partner measured, once it
    /// has.
    fn measure(&self) -> Option<difference::Measure> {
        let pair = self.compared_pair()?;
        self.stage
            .difference
            .as_ref()
            .filter(|(made, _, _)| *made == pair)
            .map(|(_, _, measure)| *measure)
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        self.stage.zoom.fit();
        self.changed(cx);
    }

    fn actual(&mut self, cx: &mut Context<Self>) {
        self.stage.zoom.actual();
        self.changed(cx);
    }

    /// The wheel over the picture: with Command (Control elsewhere) it
    /// zooms about the pointer; otherwise it flips between the files, one
    /// file per notch of travel.
    fn wheel(&mut self, event: &ScrollEvent, window: &Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height());
        if event.modifiers.secondary(cx) {
            let Some(image) = self.shown_extent() else {
                return;
            };
            let bounds = self.stage.pane;
            let offset = event.position - bounds.center();
            self.stage.zoom.wheel(
                pane_extent(bounds),
                image,
                (f32::from(offset.x), f32::from(offset.y)),
                -f32::from(delta.y),
            );
            self.changed(cx);
            return;
        }
        const NOTCH: f32 = 60.;
        self.stage.wheel += f32::from(delta.y) + f32::from(delta.x);
        if self.stage.wheel.abs() >= NOTCH {
            let step = if self.stage.wheel > 0. { -1 } else { 1 };
            self.stage.wheel = 0.;
            self.flip(step, cx);
        }
    }

    /// The shown file's size in pixels.
    fn shown_extent(&self) -> Option<compare::Extent> {
        let index = self.current_index()?;
        let file = &self.entries[index].set.files[self.shown].file;
        Some(compare::Extent::new(
            file.pixel_width? as f32,
            file.pixel_height? as f32,
        ))
    }
}

/// Where the library keeps a file's original, when it is a still image.
fn original(entry: &Entry, file: usize) -> Option<PathBuf> {
    let side = &entry.set.files[file];
    side.file
        .mime_type
        .starts_with("image/")
        .then(|| entry.facts[file].media(&side.file.file_hash))
        .flatten()
        .filter(|media| media.facts.content_hash == side.file.file_hash)
        .map(|media| PathBuf::from(&media.file_path))
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

/// Starts decoding an image file off the drawing thread.
fn load_image(path: PathBuf, cx: &mut App) -> Task<Option<Arc<RenderImage>>> {
    let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
    cx.background_executor()
        .spawn(async move { load.await.ok() })
}

/// The desk's rail, for the shell to show in the inspector's place while a
/// set is on the stage: between the desk and the grid or the viewer it
/// stays in place, and only what is in it changes.
pub fn rail(screen: &Entity<DuplicatesScreen>, cx: &mut App) -> crate::shell::ScreenRail {
    let view = cx
        .new(|cx| DuplicatesRail {
            screen: screen.downgrade(),
            _observing: cx.observe(screen, |_, _, cx| cx.notify()),
        })
        .into();
    let screen = screen.downgrade();
    crate::shell::ScreenRail {
        view,
        shows: Box::new(move |cx| {
            screen
                .upgrade()
                .is_some_and(|screen| screen.read(cx).current_index().is_some())
        }),
    }
}

/// The desk's rail; it follows the screen.
pub struct DuplicatesRail {
    screen: WeakEntity<DuplicatesScreen>,
    _observing: Subscription,
}

impl Render for DuplicatesRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(screen) = self.screen.upgrade() else {
            return div().into_any_element();
        };
        screen.update(cx, |screen, cx| screen.rail(window, cx))
    }
}

/// The screen's titlebar controls: the stage's zoom and the scan.
pub struct DuplicatesToolbar {
    screen: WeakEntity<DuplicatesScreen>,
    /// The row's width as last laid out: on Windows and Linux the window's
    /// own buttons take part of the titlebar, so the screen's width does
    /// not tell how much room the controls have.
    width: f32,
}
