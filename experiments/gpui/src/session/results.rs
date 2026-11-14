//! The destination's results: the query, its windows, refreshes in place,
//! the replies from the library's thread, view preferences and the zoom.
use super::{
    Change, Focused, REFRESH_MAX, REFRESH_MIN, REFRESH_RETRY, Session, Transition, VIEW_SAVE_DELAY,
    Visit,
};
use crate::backend::{self, Arrived, Fetch, Reply, Request};
use crate::grid::selection::End;
use crate::grid::{filters::Filters, prefs::ViewPrefs, window};
use gpui::*;
use picto_library::RootId;
use picto_library::predicate::FilterExpr;
use picto_library::query::WindowRequest;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

impl Session {
    /// `requestReconcile`: background work (an import, a subscription
    /// run) changed the library, so the shown results are read again in
    /// place, soon after, coalesced with whatever else arrives meanwhile,
    /// and never under a gesture or a change of destination.
    pub(super) fn request_refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_wanted = true;
        self.schedule_refresh(REFRESH_MIN, cx);
    }

    fn schedule_refresh(&mut self, earliest: Duration, cx: &mut Context<Self>) {
        if !self.refresh_wanted || self.refresh_timer.is_some() {
            return;
        }
        let since = self
            .refresh_finished
            .map_or(Duration::MAX, |finished| finished.elapsed());
        let delay = refresh_delay(self.refresh_took, since).max(earliest);
        self.refresh_timer = Some(cx.spawn(async move |session, cx| {
            cx.background_executor().timer(delay).await;
            let _ = session.update(cx, |this: &mut Self, cx| {
                this.refresh_timer = None;
                this.run_wanted_refresh(cx);
            });
        }));
    }

    fn run_wanted_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.refresh_wanted {
            return;
        }
        let busy = self.pointer_busy
            || cx.has_active_drag()
            || self.loading
            || !matches!(self.transition, Transition::Idle)
            || self.scope != self.displayed;
        if busy {
            self.schedule_refresh(REFRESH_RETRY, cx);
            return;
        }
        self.refresh_wanted = false;
        self.refresh_started = Some(Instant::now());
        self.refresh_results(cx);
    }

    /// Fetches the current destination again after an edit that may change
    /// what it holds, keeping the selection (by item) and the scroll, as the
    /// Electron grid refreshes in place.
    pub fn refresh_results(&mut self, cx: &mut Context<Self>) {
        if self
            .nav
            .get(self.scope)
            .is_none_or(|entry| entry.screen.is_some())
        {
            return;
        }
        let Some(query) = self.ran.clone() else {
            self.query(cx);
            return;
        };
        self.saved.clear();
        self.refreshing = true;
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.window_active = None;
        self.window_queued = None;
        // Where the selection's items and the shown item are now is read
        // with the window.
        let mut tracked = self.selection.tracked();
        tracked.extend(self.selected_item().map(|item| item.root_id));
        tracked.extend(self.anchor.map(|anchor| anchor.id));
        tracked.sort_unstable();
        tracked.dedup();
        // The same window, as many items as it holds (`reconcileSnapshot`).
        let limit = self.items.len().clamp(window::PAGE, window::WINDOW);
        let _ = self.sender.send(Request::Window(
            self.generation,
            Box::new(query),
            WindowRequest::at(self.start, limit),
            Fetch::Refresh(tracked),
        ));
        cx.emit(Change::Results);
    }

    // ── Results ─────────────────────────────────────────────────

    /// Reads the destination afresh, from its first page (`loadFirstPage`).
    pub(super) fn query(&mut self, cx: &mut Context<Self>) {
        self.refreshing = false;
        let Some(entry) = self.nav.get(self.scope) else {
            return;
        };
        if entry.screen.is_some() {
            return;
        }
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.window_active = None;
        self.window_queued = None;
        self.pending_focus = None;
        let _ = self.sender.send(Request::Query(
            self.generation,
            entry.scope.clone(),
            FilterExpr::All(vec![
                backend::text_filter(&self.search_text),
                self.filter.clone().unwrap_or_default(),
            ]),
            entry.random,
            WindowRequest::at(0, window::PAGE),
        ));
        cx.emit(Change::Results);
    }

    /// `requestWindowAt`: asks for the window that shows `ordinal`, unless
    /// it is held or already asked for. One read is on its way at a time;
    /// a newer ask waits behind it and replaces any that waited before.
    pub fn request_window_at(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let total = self.total as usize;
        if self.loading || total == 0 || self.ran.is_none() {
            return;
        }
        let start = window::start_at(ordinal, total);
        if window::holds(self.start, self.items.len(), start, total) {
            // What is on its way is no longer wanted.
            self.window_request += 1;
            self.window_queued = None;
            return;
        }
        let latest = self.window_request;
        let pending = self.window_queued.or(self.window_active);
        if pending == Some((latest, start)) {
            return;
        }
        self.window_request += 1;
        let request = (self.window_request, start);
        if self.window_active.is_some() {
            self.window_queued = Some(request);
        } else {
            self.send_window(request, cx);
        }
    }

    fn send_window(&mut self, (id, start): (u64, usize), cx: &mut Context<Self>) {
        let Some(query) = self.ran.clone() else {
            return;
        };
        self.window_active = Some((id, start));
        crate::stats::window_asked();
        let _ = self.sender.send(Request::Window(
            self.generation,
            Box::new(query),
            WindowRequest::at(start, window::WINDOW),
            Fetch::Move(id),
        ));
        cx.emit(Change::Results);
    }

    /// Asks for the window after the one held, as the Inbox review does
    /// before moving past the window's last item.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.has_following() {
            self.request_window_at(self.start + self.items.len(), cx);
        }
    }

    pub fn set_search(&mut self, text: String, cx: &mut Context<Self>) {
        if text != self.search_text {
            self.search_text = text;
            self.query(cx);
        }
    }

    /// Narrows the current destination, or clears the narrowing.
    pub fn set_filter(&mut self, filter: Option<FilterExpr>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.close_viewer(cx);
        self.query(cx);
    }

    /// Replaces the filter bar's filters and narrows the grid by them.
    pub fn set_filters(&mut self, filters: Filters, cx: &mut Context<Self>) {
        if filters == self.filters {
            return;
        }
        let filter = Self::filter_of(&filters);
        self.filters = filters;
        self.set_filter(filter, cx);
    }

    pub(super) fn filter_of(filters: &Filters) -> Option<FilterExpr> {
        let expr = filters.to_expr();
        (expr != FilterExpr::default()).then_some(expr)
    }

    /// Shows or hides the filter bar.
    pub fn set_filter_bar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.filter_bar_open != open {
            self.filter_bar_open = open;
            cx.emit(Change::Results);
        }
    }

    /// Keeps the filters when another destination opens, or not.
    pub fn set_filters_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        if self.filters_locked != locked {
            self.filters_locked = locked;
            cx.emit(Change::Results);
        }
    }

    /// Opens a destination with these filters, as a step of Back and
    /// Forward (`navigateWithGridFilters`); on the destination already
    /// showing, only the filters change.
    pub fn navigate_with_filters(
        &mut self,
        index: usize,
        filters: Filters,
        cx: &mut Context<Self>,
    ) {
        if index == self.scope {
            self.set_filters(filters, cx);
            return;
        }
        self.record(Visit {
            scope: index,
            viewing: false,
            place: None,
        });
        self.open(index, None, Some(filters), cx);
    }

    /// Changes the current destination's view (`gridController.updateView`)
    /// and saves the change to its `view_pref` row (`saveViewPref`), as
    /// `settings.view.patch` does: out of undo. With `refetch` the results
    /// are fetched again behind the scope fade, as a new sort needs.
    pub fn update_view(&mut self, patch: serde_json::Value, refetch: bool, cx: &mut Context<Self>) {
        let Some(entry) = self.nav.get(self.scope) else {
            return;
        };
        if entry.screen.is_some() {
            return;
        }
        let scope = entry.scope.clone();
        self.prefs = self.prefs.patched(&scope, &patch);
        let mut merged = match self.view_patch.take() {
            Some((_, serde_json::Value::Object(earlier))) => earlier,
            _ => serde_json::Map::new(),
        };
        let fields = patch.as_object().cloned().unwrap_or_default();
        merged.extend(fields.clone());
        self.view_patch = Some((self.generation, serde_json::Value::Object(merged)));
        let key = crate::grid::prefs::scope_key(&scope);
        self.write_view(key, serde_json::Value::Object(fields));
        if refetch {
            self.close_viewer(cx);
            if matches!(self.transition, Transition::Idle) {
                self.transition = Transition::FadingOut(Instant::now());
            }
            self.query(cx);
            cx.emit(Change::Navigation);
        } else {
            cx.emit(Change::Results);
        }
    }

    /// Saves fields of a destination's view to its `view_pref` row, as
    /// `settings.view.patch` does: out of undo.
    fn write_view(&self, key: String, fields: serde_json::Value) {
        let fields = fields.as_object().cloned().unwrap_or_default();
        let _ = self.sender.send(Request::Job(Box::new(move |library| {
            let saved = library.update_auxiliary_json(
                "settings.view.patch",
                "Change view preferences",
                "view_pref",
                &key,
                false,
                move |stored| {
                    Ok(Some(
                        crate::settings::model::patched(stored, &fields).to_string(),
                    ))
                },
            );
            if let Err(error) = saved {
                eprintln!("picto: could not save the view of {key}: {error}");
            }
        })));
    }

    /// The preferences a page of `generation` brought, with any view change
    /// made after that query was sent.
    fn fresh_prefs(&mut self, generation: u64, prefs: ViewPrefs) -> ViewPrefs {
        match &self.view_patch {
            Some((sent, patch)) if generation <= *sent => {
                let scope = self.nav.get(self.scope).map(|entry| entry.scope.clone());
                scope.map_or(prefs.clone(), |scope| prefs.patched(&scope, patch))
            }
            Some(_) => {
                self.view_patch = None;
                prefs
            }
            None => prefs,
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

    /// Zooms the grid, and saves the zoom to the destination's view
    /// (`saveViewPref({ target_size })`) once it has rested for 500 ms
    /// (`VIEW_PREFS_SAVE_DEBOUNCE_MS`), so a slider drag writes once.
    pub fn set_tile(&mut self, tile: f32, cx: &mut Context<Self>) {
        self.tile = tile;
        let scope = self
            .nav
            .get(self.scope)
            .filter(|entry| entry.screen.is_none())
            .map(|entry| entry.scope.clone());
        if let Some(scope) = scope {
            let patch = serde_json::json!({ "target_size": tile });
            self.prefs = self.prefs.patched(&scope, &patch);
            // A page of an earlier query keeps the new zoom.
            let mut merged = match self.view_patch.take() {
                Some((_, serde_json::Value::Object(earlier))) => earlier,
                _ => serde_json::Map::new(),
            };
            merged.insert("target_size".into(), serde_json::json!(tile));
            self.view_patch = Some((self.generation, serde_json::Value::Object(merged)));
            let key = crate::grid::prefs::scope_key(&scope);
            // A zoom of another destination still waiting is saved now.
            if let Some((waiting, size, _)) = self.tile_save.take()
                && waiting != key
            {
                self.write_view(waiting, serde_json::json!({ "target_size": size }));
            }
            let timer = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(VIEW_SAVE_DELAY).await;
                let _ = this.update(cx, |this, _| {
                    if let Some((key, size, _)) = this.tile_save.take() {
                        this.write_view(key, serde_json::json!({ "target_size": size }));
                    }
                });
            });
            self.tile_save = Some((key, tile, timer));
        }
        cx.emit(Change::Results);
    }

    pub(super) fn receive(&mut self, cx: &mut Context<Self>) {
        self.keep_recents(cx);
        self.receive_events(cx);
        while let Ok(reply) = self.receiver.try_recv() {
            match reply {
                Reply::Ready(nav, tags) => {
                    self.nav = nav;
                    self.tags = tags.into_iter().map(|tag| (tag.tag_id.0, tag)).collect();
                    cx.emit(Change::Navigation);
                    self.query(cx);
                    self.refresh_activity(cx);
                }
                Reply::Arrived(arrived) if arrived.generation == self.generation => {
                    match arrived.fetch {
                        Fetch::Open => {
                            if matches!(self.transition, Transition::FadingOut(_)) {
                                // The previous grid is still fading out. Hold
                                // the destination until it is gone.
                                self.pending_page = Some(arrived);
                            } else {
                                self.apply_open(arrived, cx);
                            }
                        }
                        Fetch::Move(id) => self.apply_move(id, arrived, cx),
                        Fetch::Refresh(_) => self.apply_refresh(arrived, cx),
                    }
                }
                Reply::Failed(generation, Fetch::Move(id), error)
                    if generation == self.generation =>
                {
                    eprintln!("picto: {error}");
                    if self.window_active.is_some_and(|(active, _)| active == id) {
                        self.window_active = None;
                    }
                    if let Some(queued) = self.window_queued.take() {
                        self.send_window(queued, cx);
                    }
                    cx.emit(Change::Results);
                }
                // A query that failed is the grid's error, shown in place of
                // the destination. A failed edit leaves the grid as it is.
                Reply::Failed(generation, _, error) if generation == self.generation => {
                    self.error = Some(error);
                    self.loading = false;
                    self.refreshing = false;
                    if matches!(self.transition, Transition::Waiting) {
                        self.show_error(cx);
                    }
                    cx.emit(Change::Results);
                }
                Reply::Error(error) => eprintln!("picto: {error}"),
                _ => {}
            }
        }
    }

    /// Puts a window's items in place of the held ones.
    fn take_window(&mut self, arrived: &mut Arrived) {
        let window = &mut arrived.window;
        self.items = std::mem::take(&mut window.page.items);
        self.start = window.start;
        self.head = std::mem::take(&mut window.head_items);
        self.total = window.page.total;
        self.total_size_bytes = window.page.total_size_bytes;
        self.window_epoch += 1;
    }

    /// A destination's first window, read afresh: it opens with nothing
    /// selected.
    pub(super) fn apply_open(&mut self, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        let prefs = arrived.prefs.take().unwrap_or_else(|| self.prefs.clone());
        let prefs = self.fresh_prefs(arrived.generation, prefs);
        // A scope opens at its saved zoom. Searching inside it keeps
        // whatever the zoom control was moved to since.
        if self.prefs_scope != Some(self.scope) {
            self.prefs_scope = Some(self.scope);
            self.tile = prefs.target_size;
        }
        self.prefs = prefs;
        self.take_window(&mut arrived);
        self.ran = Some(arrived.query.clone());
        self.loading = false;
        self.refreshing = false;
        self.results_epoch += 1;
        self.density = None;
        self.restore_scroll = None;
        self.restore_window_top = None;
        self.forget_selection();
        if let Some(ids) = self.reselect.take() {
            let ordinals = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| ids.contains(&item.root_id))
                .map(|(index, _)| self.start + index)
                .collect::<std::collections::BTreeSet<_>>();
            if !ordinals.is_empty() {
                self.set_selection(ordinals, false, cx);
            }
        }
        crate::stats::results_opened(self.total);
        self.displayed = self.scope;
        if matches!(self.transition, Transition::Waiting) {
            self.transition = Transition::FadingIn(Instant::now());
        }
        cx.emit(Change::Navigation);
        cx.emit(Change::Selection);
        cx.emit(Change::Results);
    }

    /// Another window of the results arrived. Only the latest asked for is
    /// shown; the one waiting behind it is asked for next.
    fn apply_move(&mut self, id: u64, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        if self.window_active.is_some_and(|(active, _)| active == id) {
            self.window_active = None;
        }
        if id == self.window_request {
            crate::stats::window_arrived(arrived.window.start, arrived.window.page.items.len());
            self.take_window(&mut arrived);
            // A move to an item that was not held lands now, or, when a
            // later scroll asked for another window, not at all.
            if let Some((ordinal, extend)) = self.pending_focus.take() {
                if self.item_at(ordinal).is_none() {
                    if self.reveal == Some(ordinal) {
                        self.reveal = None;
                    }
                } else if extend {
                    self.select_to(ordinal, false, cx);
                } else {
                    self.select(ordinal, cx);
                }
            }
        }
        if let Some(queued) = self.window_queued.take() {
            self.send_window(queued, cx);
        }
        cx.emit(Change::Results);
    }

    /// The window read again in place: the same items stay selected, each
    /// followed to where it is now, and the grid keeps the topmost item it
    /// shows where it was.
    fn apply_refresh(&mut self, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        let Fetch::Refresh(tracked) = std::mem::replace(&mut arrived.fetch, Fetch::Open) else {
            return;
        };
        self.take_window(&mut arrived);
        crate::stats::window_refreshed(self.start, self.items.len());
        self.ran = Some(arrived.query.clone());
        self.loading = false;
        self.refreshing = false;
        if let Some(started) = self.refresh_started.take() {
            self.refresh_took = started.elapsed();
            self.refresh_finished = Some(Instant::now());
        }
        let places = tracked
            .into_iter()
            .zip(
                arrived
                    .places
                    .iter()
                    .map(|place| place.map(|ordinal| ordinal as usize)),
            )
            .collect::<HashMap<_, _>>();
        self.selection.reconcile(&places);
        let moved = |end: End| match places.get(&end.id) {
            Some(Some(ordinal)) => Some(End {
                id: end.id,
                ordinal: *ordinal,
            }),
            Some(None) => None,
            None => Some(end),
        };
        self.anchor = self.anchor.and_then(moved);
        // The shown item stays, where it is now; if it left, the last
        // selected item the window holds is shown.
        let shown = self.focus.as_ref().and_then(|focused| {
            moved(End {
                id: focused.item.root_id,
                ordinal: focused.ordinal,
            })
        });
        match shown {
            Some(end) => {
                if let Some(item) = self.item_at(end.ordinal).cloned() {
                    self.focus = Some(Focused {
                        ordinal: end.ordinal,
                        item,
                    });
                } else if let Some(focused) = &mut self.focus {
                    focused.ordinal = end.ordinal;
                }
                cx.emit(Change::Selection);
            }
            None => {
                let last = self
                    .selected_loaded()
                    .last()
                    .and_then(|item| self.position_of(item.root_id));
                self.focus_on(last, cx);
            }
        }
        cx.emit(Change::Results);
    }

    /// The selected items the window holds, by ordinal.
    pub fn selected_ordinals(&self) -> Vec<usize> {
        (self.start..self.start + self.items.len())
            .filter(|ordinal| self.is_selected(*ordinal))
            .collect()
    }

    /// Where the window holds an item, as an ordinal.
    pub fn position_of(&self, id: RootId) -> Option<usize> {
        self.items
            .iter()
            .position(|item| item.root_id == id)
            .map(|offset| self.start + offset)
    }
}

/// How long a background refresh waits, `since` the last one finished,
/// which `took` that long: three times as long, between 40 ms and a second,
/// less the time already passed, so that under a stream of changes about a
/// quarter of the time goes to refreshing.
fn refresh_delay(took: Duration, since: Duration) -> Duration {
    (took * 3)
        .clamp(REFRESH_MIN, REFRESH_MAX)
        .saturating_sub(since)
}

#[cfg(test)]
mod tests {
    use super::refresh_delay;
    use std::time::Duration;

    #[test]
    fn background_refreshes_are_spaced_by_how_long_the_last_took() {
        let ms = Duration::from_millis;
        // A quick refresh long ago: at once.
        assert_eq!(refresh_delay(ms(5), ms(5000)), Duration::ZERO);
        // A quick one just now: 40 ms after it.
        assert_eq!(refresh_delay(ms(5), ms(10)), ms(30));
        // A slow one just now: three times as long, at most a second.
        assert_eq!(refresh_delay(ms(200), Duration::ZERO), ms(600));
        assert_eq!(refresh_delay(ms(900), ms(100)), ms(900));
    }
}
