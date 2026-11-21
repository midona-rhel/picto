//! The part of `Session` that reads the open destination's results: the first
//! query, the requests for other windows of the results, refreshes in place,
//! and the handling of replies from the library's thread. It also holds the
//! search, the filters, the view preferences and the zoom.
use super::{
    Change, Focused, QUIT_WAIT, REFRESH_MAX, REFRESH_MIN, REFRESH_RETRY, Session, Transition,
    VIEW_SAVE_DELAY, Visit,
};
use crate::backend::{self, Arrived, Fetch, Reply, Request};
use crate::grid::selection::{End, Selection};
use crate::grid::{filters::Filters, layout, prefs::ViewPrefs, window};
use gpui::*;
use picto_library::RootId;
use picto_library::predicate::FilterExpr;
use picto_library::query::WindowRequest;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

impl Session {
    /// Called when background work (an import, a subscription run) changed the
    /// library (`requestReconcile`). The shown results are read again in place
    /// after a short delay, and changes that arrive during the delay are
    /// covered by the same read. The read does not start while the pointer is
    /// pressed or dragging in the grid, while results are loading, or while
    /// the fade between destinations is running.
    pub(super) fn request_refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_wanted = true;
        self.schedule_refresh(REFRESH_MIN, cx);
    }

    /// Starts the timer for a wanted background refresh, unless the timer is
    /// already running. The delay is the one `refresh_delay` gives, and at
    /// least `earliest`.
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

    /// Starts the wanted background refresh. When the grid cannot be refreshed
    /// now, it tries again after 100 ms (`REFRESH_RETRY`).
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

    /// Reads the open destination's results again in place, after an edit or a
    /// background change that may have changed which items it holds. The same
    /// items stay selected, found by their ids, and the grid stays at its
    /// scroll position, as when the Electron grid refreshes in place. Does
    /// nothing when the destination is a screen. When no results have been
    /// read yet, it reads the destination from its first page instead. The
    /// saved grids of other destinations are discarded, because they may be
    /// out of date too.
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
        // Collect the ids of the items whose new ordinals the refresh must
        // report: the items the selection tracks, the item the inspector
        // shows, the range anchor, and an item waiting to be selected. The
        // library's thread reads their ordinals together with the window.
        let mut tracked = self.selection.tracked();
        tracked.extend(self.selected_item().map(|item| item.root_id));
        tracked.extend(self.anchor.map(|anchor| anchor.id));
        tracked.extend(self.pending_select);
        tracked.sort_unstable();
        tracked.dedup();
        // Ask for the window at the same start with as many items as it holds
        // now, but at least 500 (`window::PAGE`) and at most 1,500
        // (`window::WINDOW`), as `reconcileSnapshot` does.
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

    /// Reads the open destination's results from the beginning
    /// (`loadFirstPage`): sends a query for the first 500 items with the
    /// current search text and filters. Requests for other windows that are
    /// not answered yet, and items waiting to be selected, are dropped.
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
        self.pending_select = None;
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

    /// Requests the window of results that contains `ordinal`
    /// (`requestWindowAt`), unless the session already holds that window or
    /// has already requested it. Only one request is sent to the library's
    /// thread at a time. A request made while one is not answered yet waits in
    /// `window_queued`, and replaces a request that was waiting there.
    pub fn request_window_at(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let total = self.total as usize;
        if self.loading || total == 0 || self.ran.is_none() {
            return;
        }
        let start = window::start_at(ordinal, total);
        if window::holds(self.start, self.items.len(), start, total) {
            // The window the session holds is the one wanted. Raise the
            // request number, so that a window still on its way is dropped
            // when it arrives, and drop the queued request.
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

    /// Sends the request for the window that starts at ordinal `start` to the
    /// library's thread and records it as the active request.
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

    /// Requests the window that contains the first item after the window held,
    /// if the destination has more items. The Inbox review calls this before
    /// it moves on from the last item of the window.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.has_following() {
            self.request_window_at(self.start + self.items.len(), cx);
        }
    }

    /// Sets the search text and, when it changed, reads the results from the
    /// beginning.
    pub fn set_search(&mut self, text: String, cx: &mut Context<Self>) {
        if text != self.search_text {
            self.search_text = text;
            self.query(cx);
        }
    }

    /// Sets the filter that the destination's results are limited by, or
    /// removes it with `None`. Closes the viewer and reads the results from
    /// the beginning.
    pub fn set_filter(&mut self, filter: Option<FilterExpr>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.close_viewer(cx);
        self.query(cx);
    }

    /// Replaces the filter bar's filters and reads the results with them
    /// (`navigateWithGridFilters`). The change adds a Back and Forward history
    /// entry. The grid as it was with the previous filters is saved first, so
    /// Back shows it again at its scroll position. The grid with the new
    /// filters starts at the top.
    pub fn set_filters(&mut self, filters: Filters, cx: &mut Context<Self>) {
        if filters == self.filters {
            return;
        }
        self.close_viewer(cx);
        self.save_grid();
        let step = self.filter_step(&filters);
        self.record(Visit {
            scope: self.scope,
            viewing: false,
            place: None,
            filters: step,
        });
        let filter = Self::filter_of(&filters);
        self.filters = filters;
        self.set_filter(filter, cx);
        cx.emit(Change::Navigation);
    }

    /// Returns the index of `filters` in `filter_steps`, and adds it to the
    /// list first when it is not there.
    pub(super) fn filter_step(&mut self, filters: &Filters) -> usize {
        match self.filter_steps.iter().position(|step| step == filters) {
            Some(step) => step,
            None => {
                self.filter_steps.push(filters.clone());
                self.filter_steps.len() - 1
            }
        }
    }

    /// Returns the index in `filter_steps` of the filters the grid has now.
    pub(super) fn filters_step(&mut self) -> usize {
        let filters = self.filters.clone();
        self.filter_step(&filters)
    }

    /// The filter expression for the filter bar's filters, or `None` when they
    /// filter nothing.
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

    /// Sets whether the filter bar's filters are kept when another destination
    /// opens.
    pub fn set_filters_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        if self.filters_locked != locked {
            self.filters_locked = locked;
            cx.emit(Change::Results);
        }
    }

    /// Opens the destination at `index` with `filters` as the filter bar's
    /// filters, and adds a Back and Forward history entry
    /// (`navigateWithGridFilters`). When that destination is already open,
    /// only the filters change.
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
        let step = self.filter_step(&filters);
        self.record(Visit {
            scope: index,
            viewing: false,
            place: None,
            filters: step,
        });
        self.open(index, None, Some(filters), cx);
    }

    /// Changes the open destination's view preferences with the fields in
    /// `patch` (`gridController.updateView`). The grid shows the change at
    /// once, and the fields are saved to the destination's `view_pref` row
    /// (`saveViewPref`). Like the `settings.view.patch` command, the save is
    /// not recorded in the undo history. With `refetch` set, the results are
    /// also read again from the beginning, and the grid fades out and in while
    /// they load. A change of sort needs this.
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

    /// Saves `fields` to the `view_pref` row with the key `key`, on the
    /// library's thread. They are merged into the fields stored there. Like
    /// the `settings.view.patch` command, the save is not recorded in the undo
    /// history. A failure is printed to the console.
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

    /// Returns the view preferences to use when results of the query with the
    /// number `generation` arrive with `prefs`. If the user changed the view
    /// after that query was sent (`view_patch`), the results were read with
    /// the old preferences, so the changed fields are applied on top. Results
    /// of a newer query already include the change, so `view_patch` is
    /// cleared.
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

    /// The grid's current zoom, for the toolbar's zoom control. In the grid
    /// and waterfall layouts it is the number of columns at the width the
    /// grid was last drawn with, and the largest number the zoom allows at
    /// that width. In the justified layout it is the target row height.
    pub fn zoom(&self) -> layout::Zoom {
        layout::zoom(self.grid_width, self.tile, self.prefs.gap, self.prefs.mode)
    }

    /// Makes one step of the zoom. The toolbar's − and + buttons, the zoom
    /// keys and the wheel all call this. With `larger` set, the grid and
    /// waterfall layouts lose one column and justified rows become 1.1 times
    /// taller. Without it they gain one column, or rows become 1.1 times
    /// shorter. It does nothing at the end of the zoom's range.
    pub fn zoom_step(&mut self, larger: bool, cx: &mut Context<Self>) {
        let (width, gap, mode) = (self.grid_width, self.prefs.gap, self.prefs.mode);
        if let Some(size) = layout::zoom_step(width, self.tile, gap, mode, larger) {
            self.set_tile(size, cx);
        }
    }

    /// Zooms the grid and waterfall layouts to `columns` columns. The tile
    /// size that is saved is the one at which that many columns exactly fill
    /// the grid's current width. With that size, the destination opens with
    /// the same number of columns at the same width, and with more columns
    /// in a wider window.
    pub fn zoom_to_columns(&mut self, columns: usize, cx: &mut Context<Self>) {
        let (width, gap) = (self.grid_width, self.prefs.gap);
        // The grid has not been drawn yet, so its width is not known.
        if width <= 0. {
            return;
        }
        let columns = columns.clamp(1, layout::most_columns(width, gap));
        if columns != layout::columns(width, self.tile, gap) {
            self.set_tile(layout::size_for_columns(width, columns, gap), cx);
        }
    }

    /// Sets the justified layout's target row height to `height` pixels,
    /// limited to the zoom's range of 150 to 900.
    pub fn zoom_to_height(&mut self, height: f32, cx: &mut Context<Self>) {
        let height = height.clamp(layout::SMALLEST_TILE, layout::TALLEST_ROW);
        if height != self.tile {
            self.set_tile(height, cx);
        }
    }

    /// Records the width the grid lays its tiles out in. The grid calls
    /// this each time it draws. When the width changes, `Change::GridWidth`
    /// tells the toolbar to show the number of columns for the new width.
    pub fn set_grid_width(&mut self, width: f32, cx: &mut Context<Self>) {
        if width != self.grid_width {
            self.grid_width = width;
            cx.emit(Change::GridWidth);
        }
    }

    /// Sets the zoom's tile size and shows it in the grid at once. The size
    /// is saved to the destination's view preferences
    /// (`saveViewPref({ target_size })`) 500 ms after the last change
    /// (`VIEW_PREFS_SAVE_DEBOUNCE_MS`), so dragging the zoom control writes
    /// to the library once and not for every position on the way.
    fn set_tile(&mut self, tile: f32, cx: &mut Context<Self>) {
        self.tile = tile;
        let scope = self
            .nav
            .get(self.scope)
            .filter(|entry| entry.screen.is_none())
            .map(|entry| entry.scope.clone());
        if let Some(scope) = scope {
            let patch = serde_json::json!({ "target_size": tile });
            self.prefs = self.prefs.patched(&scope, &patch);
            // Record the new tile size in `view_patch`, so that results of a
            // query sent before this change do not bring the old tile size
            // back in their view preferences (see `fresh_prefs`).
            let mut merged = match self.view_patch.take() {
                Some((_, serde_json::Value::Object(earlier))) => earlier,
                _ => serde_json::Map::new(),
            };
            merged.insert("target_size".into(), serde_json::json!(tile));
            self.view_patch = Some((self.generation, serde_json::Value::Object(merged)));
            let key = crate::grid::prefs::scope_key(&scope);
            // A zoom change for a different destination is still waiting for
            // its 500 ms delay. Save it now, because the timer is about to be
            // replaced by the one for this destination.
            if let Some((waiting, size, _)) = self.tile_save.take()
                && waiting != key
            {
                self.write_view(waiting, serde_json::json!({ "target_size": size }));
            }
            let timer = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(VIEW_SAVE_DELAY).await;
                let _ = this.update(cx, |this, _| this.save_waiting_zoom());
            });
            self.tile_save = Some((key, tile, timer));
        }
        cx.emit(Change::Results);
    }

    /// Saves the zoom change that is waiting for its 500 ms delay, if there is
    /// one, to its destination's view preferences.
    fn save_waiting_zoom(&mut self) {
        if let Some((key, size, _)) = self.tile_save.take() {
            self.write_view(key, serde_json::json!({ "target_size": size }));
        }
    }

    /// Called when the app quits. Saves the zoom change that is waiting for
    /// its 500 ms delay, then blocks for at most 200 ms (`QUIT_WAIT`) until
    /// the library's thread has run the save, so that the change is written
    /// before the library is closed and the process ends.
    pub(super) fn save_view_now(&mut self) {
        if self.tile_save.is_none() {
            return;
        }
        self.save_waiting_zoom();
        let (written, waited) = std::sync::mpsc::channel();
        let _ = self.sender.send(Request::Job(Box::new(move |_| {
            let _ = written.send(());
        })));
        let _ = waited.recv_timeout(QUIT_WAIT);
    }

    /// Handles everything that arrived since the last call: writes changed
    /// recents to the app's state, handles the application layer's events,
    /// then handles every reply from the library's thread.
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
                                // The previous grid is still fading out. Keep
                                // these results in `pending_page` and show
                                // them when the fade-out finishes.
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
                // The first read of a destination, or a refresh in place,
                // failed. The grid shows the error in place of the items. A
                // failed request for another window is handled by the arm
                // above and leaves the grid as it is. Failed edits do not
                // arrive here; `Session::edit` reports them.
                Reply::Failed(generation, _, error) if generation == self.generation => {
                    self.error = Some(error);
                    self.loading = false;
                    self.refreshing = false;
                    if matches!(self.transition, Transition::Waiting) {
                        self.show_error(cx);
                    }
                    cx.emit(Change::Results);
                }
                // The library's thread stopped with an error: the library
                // could not be opened, or its destinations could not be read.
                // The grid shows the error instead of staying in the loading
                // state. No more replies will arrive.
                Reply::Error(error) => {
                    eprintln!("picto: {error}");
                    self.error = Some(error);
                    self.loading = false;
                    self.refreshing = false;
                    if matches!(self.transition, Transition::Waiting) {
                        self.show_error(cx);
                    }
                    cx.emit(Change::Results);
                }
                _ => {}
            }
        }
    }

    /// Replaces the window the session holds with the one that arrived: its
    /// items, its start ordinal, the destination's first items, the total
    /// count and the total size.
    fn take_window(&mut self, arrived: &mut Arrived) {
        let window = &mut arrived.window;
        self.items = std::mem::take(&mut window.page.items);
        self.start = window.start;
        self.head = std::mem::take(&mut window.head_items);
        self.total = window.page.total;
        self.total_size_bytes = window.page.total_size_bytes;
        self.window_epoch += 1;
    }

    /// Shows the first window of results of a destination that was read from
    /// the beginning. The selection is cleared and the grid starts at the top.
    /// If the fade was waiting for these results, the fade-in starts.
    pub(super) fn apply_open(&mut self, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        let prefs = arrived.prefs.take().unwrap_or_else(|| self.prefs.clone());
        let prefs = self.fresh_prefs(arrived.generation, prefs);
        // Take the zoom from the saved view preferences only when these are
        // the first results of this destination since it was opened. When the
        // same destination is read again, for example after a search, the zoom
        // stays at what the user set with the zoom control.
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

    /// Handles another window of the same results arriving. It is shown only
    /// when it answers the newest request (`window_request`); an older one is
    /// dropped. Then the queued request, if there is one, is sent.
    fn apply_move(&mut self, id: u64, mut arrived: Box<Arrived>, cx: &mut Context<Self>) {
        if self.window_active.is_some_and(|(active, _)| active == id) {
            self.window_active = None;
        }
        if id == self.window_request {
            crate::stats::window_arrived(arrived.window.start, arrived.window.page.items.len());
            self.take_window(&mut arrived);
            // A keyboard move or the viewer moved to an item that the previous
            // window did not hold (`pending_focus`). Select it now if this
            // window holds it. If it does not, because a later scroll
            // requested a different window, give up the move and stop asking
            // the grid to scroll to it.
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

    /// Shows the window that was read again in place. The same items stay
    /// selected: the reply reports the new ordinal of each tracked item, or
    /// that the item is no longer in the destination, and the selection, the
    /// range anchor and the item the inspector shows are updated from that.
    /// `results_epoch` does not change, so the grid does not reset its scroll
    /// position; it keeps the topmost tile it shows at the same place on
    /// screen (`GridView::snapshot` in `grid/frame.rs`).
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
        if self.selection.reconcile(&places) {
            crate::ui::notify::warning(
                crate::i18n::t("Part of the selection left this view"),
                crate::i18n::t(
                    "A selected range lost its first or last item, so that range is no longer selected.",
                ),
                cx,
            );
        }
        let moved = |end: End| match places.get(&end.id) {
            Some(Some(ordinal)) => Some(End {
                id: end.id,
                ordinal: *ordinal,
            }),
            Some(None) => None,
            None => Some(end),
        };
        self.anchor = self.anchor.and_then(moved);
        // The item the inspector shows stays the same and gets its new
        // ordinal. If it is no longer in the destination, the inspector shows
        // the last selected item that the window holds, or nothing.
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
        if let Some(id) = self.pending_select.take()
            && let Some(Some(ordinal)) = places.get(&id)
        {
            let end = End {
                id,
                ordinal: *ordinal,
            };
            self.selection = Selection::only(end);
            self.anchor = Some(end);
            self.focus_on(Some(*ordinal), cx);
        }
        cx.emit(Change::Results);
    }

    /// The ordinals of the selected items that the window holds.
    pub fn selected_ordinals(&self) -> Vec<usize> {
        (self.start..self.start + self.items.len())
            .filter(|ordinal| self.is_selected(*ordinal))
            .collect()
    }

    /// The ordinal of the item with this id, or `None` when the window does
    /// not hold it.
    pub fn position_of(&self, id: RootId) -> Option<usize> {
        self.items
            .iter()
            .position(|item| item.root_id == id)
            .map(|offset| self.start + offset)
    }
}

/// How long the next background refresh waits before it starts. `took` is how
/// long the previous refresh took, and `since` is the time that has passed
/// since it finished. The wait is three times `took`, but at least 40 ms and
/// at most 1 second, minus `since`. When changes keep arriving, the grid then
/// spends about a quarter of the time refreshing: one part refreshing and
/// three parts waiting.
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
        // The last refresh took 5 ms and finished 5 seconds ago: no wait.
        assert_eq!(refresh_delay(ms(5), ms(5000)), Duration::ZERO);
        // The last refresh took 5 ms and finished 10 ms ago: the wait is the
        // 40 ms minimum less those 10 ms.
        assert_eq!(refresh_delay(ms(5), ms(10)), ms(30));
        // The last refresh took 200 ms and has just finished: the wait is
        // three times that. For one that took 900 ms the wait is limited to
        // the maximum of one second, less the 100 ms that have passed.
        assert_eq!(refresh_delay(ms(200), Duration::ZERO), ms(600));
        assert_eq!(refresh_delay(ms(900), ms(100)), ms(900));
    }
}
