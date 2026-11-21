//! The part of `Session` that changes the selection: selecting items one by
//! one, in ranges or all at once, choosing the item the inspector and the
//! viewer show, selecting subfolder tiles, and moving the selection with the
//! keyboard.
use super::{Change, Focused, Session};
use crate::backend;
use crate::grid::nav;
use crate::grid::selection::Selection;
use gpui::*;
use picto_library::RootId;
use picto_library::predicate::FilterExpr;

impl Session {
    // The methods below change the selection and report what is selected.

    /// Clears the selection of items and of subfolder tiles, the range anchor,
    /// and the item the inspector shows with its details. It does not emit a
    /// change.
    pub(super) fn forget_selection(&mut self) {
        self.selection = Selection::default();
        self.folder_selection.clear();
        self.focus = None;
        self.anchor = None;
        self.member = 0;
        self.details = None;
    }

    /// Makes the item at `ordinal` the one the inspector and the viewer show,
    /// or none with `None`. The item's details are read from the library.
    /// Until they arrive, the `details` field keeps the previous item's
    /// details. When the item is the one whose details are already shown,
    /// nothing is read. Also clears the selection of subfolder tiles.
    pub(super) fn focus_on(&mut self, ordinal: Option<usize>, cx: &mut Context<Self>) {
        self.folder_selection.clear();
        let showing = self.selected_details().map(|details| details.root.root_id);
        self.focus = ordinal.and_then(|ordinal| {
            Some(Focused {
                ordinal,
                item: self.item_at(ordinal)?.clone(),
            })
        });
        match self.selected_item().map(|item| item.root_id) {
            Some(id) if showing == Some(id) => {}
            Some(id) => {
                self.member = 0;
                self.read_details(id, cx);
            }
            None => {
                self.member = 0;
                self.details = None;
            }
        }
        cx.emit(Change::Selection);
    }

    /// Selects the item at `ordinal` and deselects everything else, as a click
    /// without modifier keys does. The item becomes the range anchor.
    pub fn select(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(end) = self.end_at(ordinal) else {
            return;
        };
        self.selection = Selection::only(end);
        self.anchor = Some(end);
        self.focus_on(Some(ordinal), cx);
    }

    /// Adds the item at `ordinal` to the selection, or removes it if it is
    /// selected, as a Command-click does (a Control-click on Windows and
    /// Linux). An item that was added becomes the range anchor and the item
    /// the inspector shows. After a removal the inspector shows the last
    /// selected item that the window holds.
    pub fn toggle(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(end) = self.end_at(ordinal) else {
            return;
        };
        self.selection.toggle(end, self.total);
        if self.selection.contains(end.id, ordinal) {
            self.anchor = Some(end);
            self.focus_on(Some(ordinal), cx);
        } else {
            let last = self
                .selected_loaded()
                .last()
                .and_then(|item| self.position_of(item.root_id));
            self.focus_on(last, cx);
        }
    }

    /// Selects every item from the range anchor to `ordinal`, as a Shift-click
    /// or a move with Shift held does. With `keep` set, the range is added to
    /// the current selection; otherwise it replaces it. When there is no
    /// anchor, the item at `ordinal` becomes the anchor. When the anchor is
    /// not in the window, the session cannot list the items in between, so the
    /// selection stores the range by its two ends and the library finds the
    /// items when an edit needs them. A range added to a query-wide selection
    /// is stored the same way.
    pub fn select_to(&mut self, ordinal: usize, keep: bool, cx: &mut Context<Self>) {
        let Some(focus) = self.end_at(ordinal) else {
            return;
        };
        let anchor = *self.anchor.get_or_insert(focus);
        let (start, items) = (self.start, &self.items);
        self.selection.extend(anchor, focus, keep, |at| {
            items.get(at.checked_sub(start)?).map(|item| item.root_id)
        });
        self.focus_on(Some(ordinal), cx);
    }

    /// Selects every item of the destination, including items the window does
    /// not hold. The inspector keeps showing its item if the window holds it;
    /// otherwise it shows the first item of the window.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.total == 0 {
            return;
        }
        self.selection = Selection::whole();
        let focus = self
            .focus_ordinal()
            .filter(|ordinal| self.item_at(*ordinal).is_some())
            .unwrap_or(self.start);
        if self.anchor.is_none() {
            self.anchor = self.end_at(focus);
        }
        self.focus_on(Some(focus), cx);
    }

    /// Whether the selection is query-wide: Select All, or a range stored by
    /// its two ends. Such a selection can include items the window does not
    /// hold, so only the library can list all of its items.
    pub fn selects_whole(&self) -> bool {
        self.selection.query_wide()
    }

    /// The number of selected items, including selected items the window does
    /// not hold.
    pub fn selection_count(&self) -> u64 {
        self.selection.count(self.total)
    }

    /// The description of the selected items that an edit sends to the
    /// library: a list of ids, or, for a query-wide selection, the query with
    /// its ranges and the items taken out. `None` when nothing is selected.
    pub fn selection_target(&self) -> Option<picto_library::selection::SelectionTarget> {
        if self.selection.is_empty() {
            return None;
        }
        self.selection.target(self.ran.as_ref())
    }

    /// Selects the items at `ordinals`, as a rubber band does when it ends.
    /// With `keep` set, they are added to the items already selected, unless
    /// the selection is query-wide, in which case they replace it. The
    /// inspector keeps showing its item if that item is still selected;
    /// otherwise it shows the item at the highest of `ordinals`. The range
    /// anchor is cleared.
    pub fn set_selection(
        &mut self,
        ordinals: std::collections::BTreeSet<usize>,
        keep: bool,
        cx: &mut Context<Self>,
    ) {
        let ends = ordinals
            .iter()
            .filter_map(|ordinal| self.end_at(*ordinal))
            .collect::<Vec<_>>();
        self.selection = if keep && !self.selection.query_wide() {
            let mut kept = self.selection.clone();
            for end in &ends {
                if !kept.contains(end.id, end.ordinal) {
                    kept.toggle(*end, self.total);
                }
            }
            kept
        } else {
            Selection::of(ends.iter().copied())
        };
        self.anchor = None;
        let focus = self
            .focus_ordinal()
            .filter(|ordinal| self.is_selected(*ordinal))
            .or_else(|| ends.last().map(|end| end.ordinal));
        self.focus_on(focus, cx);
    }

    /// Selects subfolder tiles by node id. Without `toggle`, `nodes` become
    /// the only selected tiles. With `toggle`, as on a Command-click (a
    /// Control-click on Windows and Linux), each of `nodes` is added if it
    /// was not selected and removed if it was; if items were selected, the
    /// toggle starts from no selected tiles. In both cases every item is
    /// deselected.
    pub fn select_folders(&mut self, nodes: Vec<String>, toggle: bool, cx: &mut Context<Self>) {
        let mut chosen = if toggle && self.selection.is_empty() {
            std::mem::take(&mut self.folder_selection)
        } else {
            Vec::new()
        };
        if toggle {
            for node in nodes {
                match chosen.iter().position(|id| *id == node) {
                    Some(at) => {
                        chosen.remove(at);
                    }
                    None => chosen.push(node),
                }
            }
        } else {
            chosen = nodes;
        }
        self.selection = Selection::default();
        self.anchor = None;
        self.focus = None;
        self.member = 0;
        self.details = None;
        self.folder_selection = chosen;
        cx.emit(Change::Selection);
    }

    /// The node id of the selected subfolder tile, when exactly one tile and
    /// no item is selected. The inspector then describes that subfolder.
    pub fn selected_folder(&self) -> Option<&str> {
        match self.folder_selection.as_slice() {
            [node] if self.selection.is_empty() => Some(node),
            _ => None,
        }
    }

    /// Whether the search text filters the results. It does when it has three
    /// characters or more, not counting spaces at either end.
    pub fn searching(&self) -> bool {
        backend::text_filter(&self.search_text) != FilterExpr::default()
    }

    /// Starts a refresh in place and, when it reports the ordinal of the item
    /// `id`, selects that item alone. It is used for an item that an edit has
    /// just created, such as a new collection, which the window does not hold
    /// yet.
    pub fn select_when_listed(&mut self, id: RootId, cx: &mut Context<Self>) {
        self.pending_select = Some(id);
        self.refresh_results(cx);
    }

    /// Deselects every item and every subfolder tile. The inspector then shows
    /// no item.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() || self.focus.is_some() || !self.folder_selection.is_empty() {
            self.selection = Selection::default();
            self.anchor = None;
            self.focus_on(None, cx);
        }
    }

    /// The ordinal that the keyboard move `action` goes to, counted in the
    /// whole destination and not only in the window. `width` and `height` are
    /// the grid's width and the viewport's height in pixels, which give the
    /// number of columns and the number of rows on a page. `None` when the
    /// move does not change the item.
    pub fn move_target(&self, action: nav::Move, width: f32, height: f32) -> Option<usize> {
        let action = match action {
            // While the viewer is open, only Left and Right work: they go to
            // the previous and the next item. The other moves do nothing.
            nav::Move::Left | nav::Move::Right => action,
            _ if self.viewer => return None,
            _ => action,
        };
        // The inspector shows no item, so there is no item to move from. Any
        // move goes to the first item of the destination.
        let Some(focus) = self.focus_ordinal() else {
            return (self.total > 0).then_some(0);
        };
        let (columns, row) = crate::grid::layout::keyboard_step(
            width,
            self.tile,
            self.prefs.gap,
            self.prefs.mode,
            self.prefs.text_height(),
        );
        nav::target(
            action,
            focus,
            self.total as usize,
            columns,
            nav::visible_rows(height, row),
        )
    }

    /// Moves the selection to the item at `ordinal`. Keyboard moves and the
    /// viewer's next and previous call this. The item is selected alone, or
    /// with `extend` the range from the anchor to it is selected, and the grid
    /// is asked to scroll it into view (`reveal`). When the window does not
    /// hold the item, its window is requested and the item is selected when
    /// that window arrives; until then the selection does not change.
    pub fn move_to(&mut self, ordinal: usize, extend: bool, cx: &mut Context<Self>) {
        if ordinal >= self.total as usize {
            return;
        }
        if self.item_at(ordinal).is_some() {
            self.reveal = Some(ordinal);
            self.pending_focus = None;
            if extend {
                self.select_to(ordinal, false, cx);
            } else {
                self.select(ordinal, cx);
            }
        } else if !self.loading {
            // The window does not hold the item, so request the window that
            // does. This is skipped while the results are loading, because a
            // window cannot be requested then, so the key does nothing.
            self.reveal = Some(ordinal);
            self.pending_focus = Some((ordinal, extend));
            self.request_window_at(ordinal, cx);
            cx.emit(Change::Results);
        }
    }
}
