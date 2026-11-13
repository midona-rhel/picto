//! What is being browsed: the destination, its results, the selection and the
//! viewer. One model that every view reads, so a view repaints only for the
//! kind of change it shows and no view owns another's state.
use crate::backend::{self, Navigation, Reply, Request};
use crate::grid::{nav, prefs::ViewPrefs};
use crate::motion;
use gpui::*;
use picto_library::predicate::FilterExpr;
use picto_library::query::{RootPage, RootSummary};
use picto_library::{Rating, RootDetails, RootId, TagRecord};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
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
    /// The viewer opened, closed, or moved to another member.
    Viewer,
}

pub struct Session {
    sender: Sender<Request>,
    receiver: Receiver<Reply>,
    pub nav: Vec<Navigation>,
    pub tags: HashMap<u32, TagRecord>,
    /// The destination chosen in the sidebar.
    pub scope: usize,
    /// The destination shown in the grid and title; trails `scope` while the
    /// previous one fades out.
    pub displayed: usize,
    /// Visited destinations, for Back and Forward.
    history: Vec<usize>,
    history_index: usize,
    pub transition: Transition,
    /// A destination's first page that arrived before the fade-out finished.
    pending_page: Option<(RootPage, ViewPrefs)>,
    pub items: Vec<RootSummary>,
    /// Counts each time the items are replaced rather than extended.
    pub results_epoch: u64,
    pub total: u64,
    cursor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    generation: u64,
    pub prefs: ViewPrefs,
    prefs_scope: Option<usize>,
    /// The zoom: the target tile size.
    pub tile: f32,
    /// The selected items, by position in `items`.
    pub selection: std::collections::BTreeSet<usize>,
    /// The selected item the inspector and viewer show: the one last chosen.
    pub focus: Option<usize>,
    /// Where a shift-click or shift-move range starts.
    anchor: Option<usize>,
    pub details: Option<RootDetails>,
    pub viewer: bool,
    /// Which member of a collection the viewer shows.
    pub member: usize,
    /// A bundled sample shown in the viewer instead of a library item.
    pub demo: Option<(&'static str, &'static str)>,
    search_text: String,
    pub filter: Option<FilterExpr>,
    blobs: PathBuf,
    pub library_cover: Option<PathBuf>,
    pub fixtures: tempfile::TempDir,
}

impl EventEmitter<Change> for Session {}

impl Session {
    pub fn new(database: PathBuf, blobs: PathBuf, cx: &mut Context<Self>) -> Self {
        let (sender, receiver) = backend::start(database);
        cx.spawn(async move |session, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if session
                    .update(cx, |this: &mut Self, cx| this.receive(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let library_cover = blobs.parent().and_then(|root| {
            ["jpg", "png"]
                .into_iter()
                .map(|ext| root.join(format!(".picto-library-cover.{ext}")))
                .find(|path| path.is_file())
        });
        Self {
            sender,
            receiver,
            nav: Vec::new(),
            tags: HashMap::new(),
            scope: 0,
            displayed: 0,
            history: vec![0],
            history_index: 0,
            transition: Transition::Idle,
            pending_page: None,
            items: Vec::new(),
            results_epoch: 0,
            total: 0,
            cursor: None,
            loading: true,
            error: None,
            generation: 0,
            prefs: ViewPrefs::default(),
            prefs_scope: None,
            tile: 200.,
            selection: Default::default(),
            focus: None,
            anchor: None,
            details: None,
            viewer: false,
            member: 0,
            demo: None,
            search_text: String::new(),
            filter: None,
            blobs,
            library_cover,
            fixtures: crate::demo::fixtures().expect("create disposable media fixtures"),
        }
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
        self.items.get(self.focus?)
    }

    // ── Results ─────────────────────────────────────────────────

    fn query(&mut self, append: bool, cx: &mut Context<Self>) {
        let Some(entry) = self.nav.get(self.scope) else {
            return;
        };
        if entry.screen {
            return;
        }
        self.generation += 1;
        self.loading = true;
        self.error = None;
        let _ = self.sender.send(Request::Query(
            self.generation,
            entry.scope.clone(),
            FilterExpr::All(vec![
                backend::text_filter(&self.search_text),
                self.filter.clone().unwrap_or_default(),
            ]),
            if append { self.cursor.clone() } else { None },
            entry.random,
        ));
        cx.emit(Change::Results);
    }

    /// Fetches the next page once the grid is scrolled near the end.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.cursor.is_some() && !self.loading {
            self.query(true, cx);
        }
    }

    pub fn set_search(&mut self, text: String, cx: &mut Context<Self>) {
        if text != self.search_text {
            self.search_text = text;
            self.query(false, cx);
        }
    }

    /// Narrows the current destination, or clears the narrowing.
    pub fn set_filter(&mut self, filter: Option<FilterExpr>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.close_viewer(cx);
        self.query(false, cx);
    }

    /// Narrows the grid by one more clause, keeping the filters it has.
    pub fn add_filter(
        &mut self,
        clause: picto_library::predicate::FilterClause,
        cx: &mut Context<Self>,
    ) {
        let clause = FilterExpr::Clause(clause);
        let filter = match self.filter.take() {
            Some(FilterExpr::All(mut clauses)) => {
                clauses.push(clause);
                FilterExpr::All(clauses)
            }
            Some(other) => FilterExpr::All(vec![other, clause]),
            None => clause,
        };
        self.set_filter(Some(filter), cx);
    }

    /// The library folder's name without its `.library` suffix.
    pub fn library_name(&self) -> Option<String> {
        let name = self.blobs.parent()?.file_name()?.to_string_lossy();
        Some(name.trim_end_matches(".library").to_owned())
    }

    pub fn set_tile(&mut self, tile: f32, cx: &mut Context<Self>) {
        self.tile = tile;
        cx.emit(Change::Results);
    }

    fn receive(&mut self, cx: &mut Context<Self>) {
        while let Ok(reply) = self.receiver.try_recv() {
            match reply {
                Reply::Ready(nav, tags) => {
                    self.nav = nav;
                    self.tags = tags.into_iter().map(|tag| (tag.tag_id.0, tag)).collect();
                    cx.emit(Change::Navigation);
                    self.query(false, cx);
                }
                Reply::Page(generation, append, page, prefs) if generation == self.generation => {
                    if !append && matches!(self.transition, Transition::FadingOut(_)) {
                        // The previous grid is still fading out. Hold the
                        // destination until it is gone.
                        self.pending_page = Some((page, prefs));
                    } else {
                        self.apply_page(append, page, prefs, cx);
                    }
                }
                Reply::Details(id, details)
                    if self.selected_item().is_some_and(|item| item.root_id == id) =>
                {
                    if let Some(item) = self.focus.and_then(|focus| self.items.get_mut(focus)) {
                        item.name = details.root.name.clone();
                    }
                    self.details = Some(details);
                    cx.emit(Change::Details);
                }
                Reply::Error(error) => {
                    self.error = Some(error);
                    self.loading = false;
                    cx.emit(Change::Results);
                }
                _ => {}
            }
        }
    }

    fn apply_page(
        &mut self,
        append: bool,
        page: RootPage,
        prefs: ViewPrefs,
        cx: &mut Context<Self>,
    ) {
        // A scope opens at its saved zoom. Searching inside it keeps
        // whatever the zoom control was moved to since.
        if self.prefs_scope != Some(self.scope) {
            self.prefs_scope = Some(self.scope);
            self.tile = prefs.target_size;
        }
        self.prefs = prefs;
        self.total = page.total;
        self.cursor = page.next_cursor;
        self.loading = false;
        if append {
            self.items.extend(page.items);
        } else {
            self.items = page.items;
            self.results_epoch += 1;
            // A new result set opens with nothing selected.
            self.forget_selection();
            self.displayed = self.scope;
            if matches!(self.transition, Transition::Waiting) {
                self.transition = Transition::FadingIn(Instant::now());
            }
            cx.emit(Change::Navigation);
            cx.emit(Change::Selection);
        }
        cx.emit(Change::Results);
    }

    // ── Navigation ──────────────────────────────────────────────

    /// Opens a sidebar destination and records it for Back and Forward.
    pub fn navigate(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.history.get(self.history_index) != Some(&index) {
            self.history.truncate(self.history_index + 1);
            self.history.push(index);
            self.history_index = self.history.len() - 1;
        }
        self.open(index, cx);
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
        if let Some(&index) = self.history.get(target) {
            self.history_index = target;
            self.open(index, cx);
        }
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        self.scope = index;
        self.pending_page = None;
        self.close_viewer(cx);
        let screen = self.nav.get(index).is_some_and(|entry| entry.screen);
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
        } else {
            self.query(false, cx);
        }
        cx.emit(Change::Navigation);
    }

    /// Shows a destination that is a screen, not a grid, on the hidden surface.
    fn commit_screen(&mut self) {
        self.items.clear();
        self.results_epoch += 1;
        self.forget_selection();
        self.total = 0;
        self.cursor = None;
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
    }

    /// Moves a scope change along once its current stage has run its time.
    /// The views that fade call this as they paint.
    pub fn advance_transition(&mut self, cx: &mut Context<Self>) {
        match self.transition {
            Transition::FadingOut(started) if started.elapsed() >= motion::SCOPE_FADE => {
                self.transition = Transition::Waiting;
                if self.nav.get(self.scope).is_some_and(|entry| entry.screen) {
                    self.commit_screen();
                    cx.emit(Change::Results);
                    cx.emit(Change::Selection);
                } else if let Some((page, prefs)) = self.pending_page.take() {
                    self.apply_page(false, page, prefs, cx);
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
        self.selection.clear();
        self.focus = None;
        self.anchor = None;
        self.member = 0;
        self.details = None;
    }

    /// Shows `index` in the inspector, or nothing.
    fn focus_on(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        self.demo = None;
        self.member = 0;
        self.details = None;
        self.focus = index.filter(|index| *index < self.items.len());
        if let Some(item) = self.selected_item() {
            let _ = self.sender.send(Request::Details(item.root_id));
        }
        cx.emit(Change::Selection);
    }

    /// Selects one item alone, as a plain click does.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.items.len() {
            return;
        }
        self.selection = [index].into();
        self.anchor = Some(index);
        self.focus_on(Some(index), cx);
    }

    /// Adds or removes one item, as a command-click does.
    pub fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.items.len() {
            return;
        }
        if self.selection.remove(&index) {
            let last = self.selection.iter().next_back().copied();
            self.focus_on(last, cx);
        } else {
            self.selection.insert(index);
            self.anchor = Some(index);
            self.focus_on(Some(index), cx);
        }
    }

    /// Selects from the anchor to `index`, as a shift-click or shift-move
    /// does; `keep` adds the range to what is selected already.
    pub fn select_to(&mut self, index: usize, keep: bool, cx: &mut Context<Self>) {
        if index >= self.items.len() {
            return;
        }
        let anchor = *self.anchor.get_or_insert(index);
        if !keep {
            self.selection.clear();
        }
        self.selection.extend(anchor.min(index)..=anchor.max(index));
        self.focus_on(Some(index), cx);
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        self.selection = (0..self.items.len()).collect();
        let focus = self.focus.unwrap_or(0);
        self.anchor.get_or_insert(focus);
        self.focus_on(Some(focus), cx);
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            self.selection.clear();
            self.anchor = None;
            self.focus_on(None, cx);
        }
    }

    /// The item a keyboard move lands on, given the grid's current geometry.
    pub fn move_target(&self, action: nav::Move, width: f32, height: f32) -> Option<usize> {
        let action = match action {
            // With the viewer open only the item changes, one at a time.
            nav::Move::Left | nav::Move::Right => action,
            _ if self.viewer => return None,
            _ => action,
        };
        // With nothing selected, any move selects the first item.
        let Some(focus) = self.focus else {
            return (!self.items.is_empty()).then_some(0);
        };
        nav::target(
            action,
            focus,
            self.items.len(),
            nav::column_count(width, self.tile, self.prefs.gap),
            nav::visible_rows(height, self.tile, self.prefs.gap),
        )
    }

    // ── Viewer ──────────────────────────────────────────────────

    pub fn open_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer && self.focus.is_some() {
            self.viewer = true;
            cx.emit(Change::Viewer);
        }
    }

    pub fn close_viewer(&mut self, cx: &mut Context<Self>) {
        if self.viewer {
            self.viewer = false;
            cx.emit(Change::Viewer);
        }
    }

    pub fn toggle_viewer(&mut self, cx: &mut Context<Self>) {
        self.viewer = !self.viewer;
        cx.emit(Change::Viewer);
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

    pub fn rate(&self, id: RootId, rating: Rating) {
        let _ = self.sender.send(Request::Rate(id, rating));
    }

    pub fn rename(&self, id: RootId, name: String) {
        let _ = self.sender.send(Request::Name(id, name));
    }

    pub fn set_notes(&self, id: RootId, notes: String) {
        let _ = self.sender.send(Request::Notes(id, notes));
    }

    pub fn remove_tag(&self, id: RootId, name: String) {
        let _ = self.sender.send(Request::Untag(id, name));
    }

    pub fn remove_from_folder(&self, id: RootId, folder: picto_library::FolderId) {
        let _ = self.sender.send(Request::LeaveFolder(id, folder));
    }

    pub fn undo(&self) {
        if let Some(item) = self.selected_item() {
            let _ = self.sender.send(Request::Undo(item.root_id));
        }
    }

    pub fn redo(&self) {
        if let Some(item) = self.selected_item() {
            let _ = self.sender.send(Request::Redo(item.root_id));
        }
    }
}
