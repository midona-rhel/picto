//! The part of `Session` that changes which destination is open: opening a
//! destination, going Back and Forward, saving the grid of the destination
//! the user leaves and showing it again later, and running the fade between
//! two destinations.
use super::{Change, SavedGrid, Session, Transition, Visit};
use crate::backend::Navigation;
use crate::grid::filters::Filters;
use crate::i18n::t;
use crate::motion;
use gpui::*;
use picto_library::{RootId, TagRecord};
use std::{collections::HashMap, time::Instant};

impl Session {
    /// Replaces the list of destinations with a list that was read again,
    /// for example after the sidebar edited the folders. Destinations are
    /// matched between the two lists by their id (`Navigation::id`).
    /// Everything that refers to a destination by its index (the open
    /// destination, the displayed one, the history and the saved grids) is
    /// changed to the destination's new index, and history entries and saved
    /// grids of destinations that no longer exist are removed. When the open
    /// destination no longer exists, its nearest ancestor that still exists
    /// opens instead, or the first destination when no ancestor exists.
    pub fn replace_navigation(
        &mut self,
        nav: Vec<Navigation>,
        tags: Vec<TagRecord>,
        cx: &mut Context<Self>,
    ) {
        // A destination that lists the items using one item's media
        // (`ItemScope::MediaMatches`) has no sidebar row, so the list that
        // was read again does not contain it. Copy these destinations from
        // the old list, so that they still exist in the new one.
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
            .filter_map(|((index, filters), grid)| Some(((moved(index)?, filters), grid)))
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
                let filters = self.filters_step();
                self.history.push(Visit {
                    scope: fallback,
                    viewing: false,
                    place: None,
                    filters,
                });
                self.history_index = self.history.len() - 1;
                self.open(fallback, None, None, cx);
                // `open` saved the grid on screen under the index of the
                // fallback destination, but that grid holds the items of the
                // deleted destination. Remove it, so that Back and Forward
                // do not show those items for the fallback destination.
                self.saved.retain(|(scope, _), _| *scope != fallback);
            }
        }
        cx.emit(Change::Navigation);
    }

    // The methods below open destinations and go Back and Forward.

    /// Opens the destination that lists every item which uses the media of
    /// `item`. The menu command Find Items with This Media calls this. The
    /// destination has no sidebar row and is titled "Items with This Media".
    /// It is added to `nav` the first time it is opened for this item.
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
                    label: t("Items with This Media").into(),
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

    /// Opens the destination at `index` in `nav`, which the user chose in
    /// the sidebar, and adds a Back and Forward history entry for it. When
    /// that destination is already open, this only closes the viewer and, if
    /// a place inside a screen is open, returns to the screen's top level.
    pub fn navigate(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.scope {
            // The user chose the screen that is already open while a place
            // inside it is open. Return to the screen's top level.
            if self.place().is_some() {
                self.open_place(None, cx);
            }
            self.close_viewer(cx);
            return;
        }
        // A destination chosen in the sidebar opens with its grid at the top
        // and without the filter bar's filters, unless the filters are
        // locked.
        let filters = if self.filters_locked {
            self.filters.clone()
        } else {
            Filters::default()
        };
        let step = self.filter_step(&filters);
        self.record(Visit {
            scope: index,
            viewing: false,
            place: None,
            filters: step,
        });
        self.open(index, None, Some(filters), cx);
    }

    /// Adds `visit` to the history after the current entry and makes it the
    /// current entry. The entries after the current one, which Forward would
    /// have gone to, are removed first.
    pub(super) fn record(&mut self, visit: Visit) {
        self.history.truncate(self.history_index + 1);
        self.history.push(visit);
        self.history_index = self.history.len() - 1;
    }

    /// Whether the history has an entry `delta` steps from the current one.
    /// A `delta` of -1 is Back and a `delta` of 1 is Forward.
    pub fn can_step_history(&self, delta: isize) -> bool {
        self.history_index
            .checked_add_signed(delta)
            .is_some_and(|target| target < self.history.len())
    }

    /// Goes `delta` entries along the history: -1 is Back and 1 is Forward.
    /// Does nothing when the history has no such entry. When the entry is
    /// for the open destination, only the filter bar's filters or the viewer
    /// change. When it is for another destination, that destination opens,
    /// with its saved grid if one can be used.
    pub fn step_history(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(target) = self.history_index.checked_add_signed(delta) else {
            return;
        };
        let Some(&visit) = self.history.get(target) else {
            return;
        };
        self.history_index = target;
        let step_filters = self.filter_steps[visit.filters].clone();
        if visit.scope == self.scope {
            if step_filters != self.filters && !self.filters_locked {
                // The entry has other filters than the grid has now. Show the
                // grid with the entry's filters. If a grid was saved with
                // those filters and with an empty search text, it is shown as
                // it was saved; otherwise the results are read again.
                self.save_grid();
                let saved = self
                    .saved
                    .remove(&(visit.scope, visit.filters))
                    .filter(|saved| saved.search_text.is_empty());
                self.filter = Self::filter_of(&step_filters);
                self.filters = step_filters;
                if self.viewer {
                    self.viewer = false;
                    cx.emit(Change::Viewer);
                }
                match saved {
                    // The saved grid is shown at the scroll position it had,
                    // but with nothing selected.
                    Some(saved) => {
                        self.restore_grid(saved, cx);
                        self.clear_selection(cx);
                    }
                    None => self.query(cx),
                }
                cx.emit(Change::Navigation);
                return;
            }
            // The entry has the same destination and the same filters, or the
            // filters are locked. Only the viewer opens or closes, and the
            // grid is not changed.
            let viewing = visit.viewing && self.focus.is_some();
            if self.viewer != viewing {
                self.viewer = viewing;
                cx.emit(Change::Viewer);
            }
            cx.emit(Change::Navigation);
            return;
        }
        // The entry is for another destination. It opens with the filters the
        // entry had, or with the current filters when the filters are locked.
        // Its saved grid is shown only if it was saved with an empty search
        // text and with exactly these filters; otherwise the destination's
        // results are read again.
        let filters = if self.filters_locked {
            self.filters.clone()
        } else {
            step_filters
        };
        let step = self.filter_step(&filters);
        let saved = self
            .saved
            .remove(&(visit.scope, step))
            .filter(|saved| saved.search_text.is_empty() && saved.filters == filters);
        self.open(visit.scope, saved, Some(filters), cx);
    }

    /// Saves the grid on screen in `saved`, under the displayed destination
    /// and the current filters, so that Back and Forward can show it again:
    /// the window of results, the scroll position, the view preferences, the
    /// zoom and the selection. Saves nothing when a screen is displayed or
    /// the grid has no items.
    pub(super) fn save_grid(&mut self) {
        if self
            .nav
            .get(self.displayed)
            .is_none_or(|entry| entry.screen.is_some())
            || self.items.is_empty()
        {
            return;
        }
        let filters = self.filters_step();
        self.saved.insert(
            (self.displayed, filters),
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

    /// Replaces the current results with a saved grid and starts the
    /// fade-in. The grid scrolls to the saved offset and the saved selection
    /// is selected again. The viewer opens if the current history entry was
    /// made with the viewer open.
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
        // Restore the measured height per item and the position of the
        // window's tiles in the scrollable height. The layout is then the
        // same as when the grid was saved, so the saved scroll offset shows
        // the same items.
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
        // When the current history entry was made with the viewer open, the
        // viewer opens again on the same item.
        let viewing = self.history[self.history_index].viewing && self.focus.is_some();
        if self.viewer != viewing {
            self.viewer = viewing;
            cx.emit(Change::Viewer);
        }
        cx.emit(Change::Navigation);
        cx.emit(Change::Results);
    }

    /// Opens the destination at `index`. The grid on screen is saved first
    /// and the search text is cleared. When `filters` is given, they replace
    /// the filter bar's filters. What is on screen fades out. Then the
    /// destination's screen is shown, or the grid `saved` when one is given,
    /// or the results that are read from the library. This method does not
    /// add a history entry; its callers do.
    pub(super) fn open(
        &mut self,
        index: usize,
        saved: Option<SavedGrid>,
        filters: Option<Filters>,
        cx: &mut Context<Self>,
    ) {
        self.save_grid();
        // Every destination opens with an empty search text, also when Back
        // or Forward opens it.
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
            // What was on screen is still fading out, or is already
            // invisible. The fade continues unchanged and ends by showing the
            // destination that was opened last.
            Transition::FadingOut(_) | Transition::Waiting => self.transition,
            // A fade-in is running. Start the fade-out near the opacity the
            // fade-in has reached, by setting its start time as far in the
            // past as a fade-out needs to get down to that opacity.
            Transition::FadingIn(started) => {
                let shown = motion::progress(started.elapsed(), motion::SCOPE_FADE);
                let now = Instant::now();
                Transition::FadingOut(
                    now.checked_sub(motion::SCOPE_FADE.mul_f32(1. - shown))
                        .unwrap_or(now),
                )
            }
            Transition::Idle => Transition::FadingOut(Instant::now()),
        };
        if screen {
            // The destination is a screen, so no results are read. Raising
            // `generation` makes the session ignore a reply that is still on
            // its way for the previous destination.
            self.generation += 1;
            self.loading = false;
            if matches!(self.transition, Transition::Waiting) {
                self.commit_screen();
            }
        } else if let Some(saved) = saved {
            // A saved grid is shown, so no results are read, and raising
            // `generation` makes the session ignore a reply that is still on
            // its way. The saved grid is shown now if what was on screen is
            // already invisible. Otherwise it waits in `pending_restore`
            // until the fade-out has finished.
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

    /// Shows the open destination when it is a screen and not a grid:
    /// empties the results, makes the destination the displayed one, and
    /// starts the fade-in.
    fn commit_screen(&mut self) {
        self.clear_results();
        self.total_size_bytes = 0;
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
    }

    /// Shows the open destination with its error in place of the grid, and
    /// starts the fade-in. The results are emptied, so that the items of the
    /// previous destination are not shown when the read is tried again.
    pub(super) fn show_error(&mut self, cx: &mut Context<Self>) {
        self.clear_results();
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
        cx.emit(Change::Navigation);
        cx.emit(Change::Selection);
    }

    /// Empties the results: no items, a total of 0, no requests for other
    /// windows, and no selection.
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

    /// Moves the fade to its next stage when the current stage has lasted
    /// 170 ms (`motion::SCOPE_FADE`). When the fade-out has finished, this
    /// shows what is ready for the open destination: its screen, a saved
    /// grid, results that have already arrived, or its error. When none of
    /// these is ready, the grid stays invisible until the results arrive.
    /// When the fade-in has finished, the fade is over. The views that fade
    /// call this each time they are drawn.
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

/// Returns the history as it is after the list of destinations was
/// replaced, together with the index of the current entry in it. `moved`
/// gives the new index of a destination, or `None` when the destination no
/// longer exists. Entries of destinations that no longer exist are removed,
/// and the other entries get their destination's new index. When the current
/// entry is removed, the kept entry before it becomes the current one.
fn remap_history(
    history: &[Visit],
    index: usize,
    moved: impl Fn(usize) -> Option<usize>,
) -> (Vec<Visit>, usize) {
    let mut kept = Vec::with_capacity(history.len());
    let mut current = 0;
    for (at, visit) in history.iter().enumerate() {
        if let Some(scope) = moved(visit.scope) {
            // Skip an entry that is equal to the entry kept before it. Two
            // equal entries become neighbours when the entries that were
            // between them have been removed.
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
            filters: 0,
        };
        // There were destinations 0 to 3. Destination 2 was deleted, and
        // destination 3 now has index 2.
        let moved = |index: usize| match index {
            2 => None,
            3 => Some(2),
            other => Some(other),
        };
        let history = [visit(0), visit(2), visit(3), visit(1)];
        let (kept, current) = remap_history(&history, 2, moved);
        assert!(kept == [visit(0), visit(2), visit(1)]);
        assert_eq!(current, 1);
        // The destination of the current entry was deleted, so the entry
        // before it becomes the current one.
        let (kept, current) = remap_history(&history, 1, moved);
        assert_eq!(kept.len(), 3);
        assert_eq!(current, 0);
        // Two equal entries that become neighbours, because the entry between
        // them is removed, are kept as one entry.
        let (kept, _) = remap_history(&[visit(1), visit(2), visit(1)], 2, moved);
        assert!(kept == [visit(1)]);
    }
}
