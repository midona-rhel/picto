//! Destinations: opening one, Back and Forward, the grids left behind,
//! and the fade between two.
use super::{Change, SavedGrid, Session, Transition, Visit};
use crate::backend::Navigation;
use crate::grid::filters::Filters;
use crate::motion;
use gpui::*;
use picto_library::{RootId, TagRecord};
use std::{collections::HashMap, time::Instant};

impl Session {
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
        // A media-matches destination has no row of its own: it stays while
        // it is open or in the history.
        let mut nav = nav;
        nav.extend(
            self.nav
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.scope,
                        picto_library::query::ItemScope::MediaMatches { .. }
                    )
                })
                .cloned(),
        );
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

    // ── Navigation ──────────────────────────────────────────────

    /// Opens a sidebar destination and records it for Back and Forward.
    /// Choosing the destination already showing does nothing, unless the
    /// viewer is over it, which this closes.
    /// `navigateToNode('media-matches:<id>')`: every item that uses the
    /// media of `item` (Find Items with This Media), a destination with no
    /// sidebar row, titled "Items with This Media".
    pub fn open_media_matches(&mut self, item: RootId, cx: &mut Context<Self>) {
        let scope = picto_library::query::ItemScope::MediaMatches { item_id: item.0 };
        let index = match self
            .nav
            .iter()
            .position(|entry| entry.screen.is_none() && entry.scope == scope)
        {
            Some(index) => index,
            None => {
                self.nav.push(Navigation {
                    label: "Items with This Media".into(),
                    count: None,
                    scope,
                    screen: None,
                    random: false,
                    icon: None,
                    tint: None,
                    group: false,
                    watched: false,
                    depth: 0,
                    parent: None,
                });
                self.nav.len() - 1
            }
        };
        self.navigate(index, cx);
    }

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
    pub(super) fn record(&mut self, visit: Visit) {
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
            .filter(|saved| saved.search_text.is_empty() && saved.filters == filters);
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
    pub(super) fn open(
        &mut self,
        index: usize,
        saved: Option<SavedGrid>,
        filters: Option<Filters>,
        cx: &mut Context<Self>,
    ) {
        self.save_grid();
        // Every destination opens without a search (`prepareNavigation`
        // sets `searchText: ''`), Back and Forward included.
        self.search_text.clear();
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
    pub(super) fn show_error(&mut self, cx: &mut Context<Self>) {
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

#[cfg(test)]
mod tests {
    use super::remap_history;
    use crate::session::Visit;

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
