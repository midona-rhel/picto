//! The selection: items chosen one by one or query-wide, the item shown,
//! subfolder tiles, and keyboard moves.
use super::{Change, Focused, Session};
use crate::backend;
use crate::grid::nav;
use crate::grid::selection::Selection;
use gpui::*;
use picto_library::RootId;
use picto_library::predicate::FilterExpr;

impl Session {
    // ── Selection ───────────────────────────────────────────────

    pub(super) fn forget_selection(&mut self) {
        self.selection = Selection::default();
        self.folder_selection.clear();
        self.focus = None;
        self.anchor = None;
        self.member = 0;
        self.details = None;
    }

    /// Shows the item at `ordinal` in the inspector, or nothing. The
    /// previous item's details stay up until this one's arrive, and
    /// choosing the item already shown asks for nothing.
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

    /// Selects one item alone, as a plain click does.
    pub fn select(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(end) = self.end_at(ordinal) else {
            return;
        };
        self.selection = Selection::only(end);
        self.anchor = Some(end);
        self.focus_on(Some(ordinal), cx);
    }

    /// Adds or removes one item, as a command-click does.
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

    /// Selects from the anchor to `ordinal`, as a shift-click or shift-move
    /// does; `keep` adds the range to what is selected already. A range
    /// from an anchor outside the window is a span of the order, which the
    /// library resolves.
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

    /// Everything the destination holds, loaded or not.
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

    /// Whether the selection reaches past the items chosen one by one (the
    /// whole destination, or a range), so that only the library knows all
    /// it holds (production's `query_results` selection).
    pub fn selects_whole(&self) -> bool {
        self.selection.query_wide()
    }

    /// How many items are selected, loaded or not.
    pub fn selection_count(&self) -> u64 {
        self.selection.count(self.total)
    }

    /// What an edit of the grid's selection addresses.
    pub fn selection_target(&self) -> Option<picto_library::selection::SelectionTarget> {
        if self.selection.is_empty() {
            return None;
        }
        self.selection.target(self.ran.as_ref())
    }

    /// What an edit of `ids` addresses. When they are the grid's
    /// selection, as [`Session::selected_ids`] or the window gives it, the
    /// edit addresses all of it: the items chosen that are not loaded, or
    /// the destination's query for a query-wide selection, so that it
    /// reaches every item. Other ids (the viewer's item, a tile dragged
    /// alone) are addressed as they are.
    pub fn target_for(&self, ids: Vec<RootId>) -> picto_library::selection::SelectionTarget {
        let loaded = self
            .selected_loaded()
            .into_iter()
            .map(|item| item.root_id)
            .collect::<Vec<_>>();
        target_of(ids, &loaded, &self.selected_ids(), self.selection_target())
    }

    /// Replaces the selection with the items at these ordinals, as a
    /// finished rubber band does; `keep` adds them to the items chosen. The
    /// item shown stays if it is still selected; otherwise the last one is
    /// shown. No range anchor remains.
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

    /// Selects these subfolder tiles alone (`replace_folders`), or adds or
    /// removes one when `toggle` (`toggle_folder`, a command-click, which
    /// starts afresh from a selection of items). Items are deselected.
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

    /// The one subfolder tile selected with no items, which the inspector
    /// describes (`selectedSubfolderNodeIdAtom`).
    pub fn selected_folder(&self) -> Option<&str> {
        match self.folder_selection.as_slice() {
            [node] if self.selection.is_empty() => Some(node),
            _ => None,
        }
    }

    /// Whether the search field narrows the results (three characters or more).
    pub fn searching(&self) -> bool {
        backend::text_filter(&self.search_text) != FilterExpr::default()
    }

    /// Selects `id` alone once the destination, read again now, says where
    /// it is: an item an edit has just made, which the loaded results do
    /// not hold yet.
    pub fn select_when_listed(&mut self, id: RootId, cx: &mut Context<Self>) {
        self.pending_select = Some(id);
        self.refresh_results(cx);
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() || self.focus.is_some() || !self.folder_selection.is_empty() {
            self.selection = Selection::default();
            self.anchor = None;
            self.focus_on(None, cx);
        }
    }

    /// The ordinal a keyboard move lands on, given the grid's current
    /// geometry, in the whole destination.
    pub fn move_target(&self, action: nav::Move, width: f32, height: f32) -> Option<usize> {
        let action = match action {
            // With the viewer open only the item changes, one at a time.
            nav::Move::Left | nav::Move::Right => action,
            _ if self.viewer => return None,
            _ => action,
        };
        // With nothing selected, any move selects the first item.
        let Some(focus) = self.focus_ordinal() else {
            return (self.total > 0).then_some(0);
        };
        nav::target(
            action,
            focus,
            self.total as usize,
            nav::column_count(width, self.tile, self.prefs.gap),
            nav::visible_rows(height, self.tile, self.prefs.gap),
        )
    }

    /// Moves to the item at `ordinal` (a keyboard move, the viewer's next
    /// and previous), selecting it alone or, with `extend`, as a range,
    /// and asks the grid to bring it into view. An item outside the window
    /// is selected once its window arrives; until then what is shown stays.
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
            // While the destination is read again there is no window to
            // move; the key does nothing.
            self.reveal = Some(ordinal);
            self.pending_focus = Some((ordinal, extend));
            self.request_window_at(ordinal, cx);
            cx.emit(Change::Results);
        }
    }
}

/// [`Session::target_for`]: `selection` (what an edit of the grid's
/// selection addresses) when `ids` are the selected items, loaded or by id;
/// else `ids` alone.
fn target_of(
    ids: Vec<RootId>,
    loaded: &[RootId],
    selected: &[RootId],
    selection: Option<picto_library::selection::SelectionTarget>,
) -> picto_library::selection::SelectionTarget {
    let whole = same_items(&ids, loaded) || same_items(&ids, selected);
    match selection.filter(|_| whole) {
        Some(target) => target,
        None => picto_library::selection::SelectionTarget::Explicit { root_ids: ids },
    }
}

/// Whether `ids` are the same items as `selected`: the grid's selection,
/// rather than one item acted on by itself (the viewer's, a tile dragged
/// alone).
fn same_items(ids: &[RootId], selected: &[RootId]) -> bool {
    if ids.is_empty() || ids.len() != selected.len() {
        return false;
    }
    let mut ids = ids.to_vec();
    let mut selected = selected.to_vec();
    ids.sort_unstable();
    selected.sort_unstable();
    ids == selected
}

#[cfg(test)]
mod tests {
    use super::{same_items, target_of};
    use picto_library::RootId;

    #[test]
    fn only_the_grid_selection_stands_for_the_whole_destination() {
        let ids = |range: std::ops::Range<u32>| range.map(RootId).collect::<Vec<_>>();
        // The loaded selection, acted on as one, in any order.
        let mut reversed = ids(0..500);
        reversed.reverse();
        assert!(same_items(&reversed, &ids(0..500)));
        // One item acted on by itself while everything is selected.
        assert!(!same_items(&ids(3..4), &ids(0..500)));
        assert!(!same_items(&[], &[]));
    }

    #[test]
    fn an_edit_of_one_shown_item_leaves_the_rest_of_the_selection_alone() {
        use picto_library::query::{ItemScope, RootQuery};
        use picto_library::selection::SelectionTarget;
        let ids = |range: std::ops::Range<u32>| range.map(RootId).collect::<Vec<_>>();
        // The whole destination is selected; 100 items are loaded.
        let whole = SelectionTarget::Query {
            query: RootQuery {
                scope: ItemScope::All,
                view: Default::default(),
            },
            excluded_root_ids: Vec::new(),
        };
        let edit = |acted_on| target_of(acted_on, &ids(0..100), &ids(0..100), Some(whole.clone()));
        // The viewer's menu moves the item it shows, and only that one.
        assert_eq!(
            edit(ids(7..8)),
            SelectionTarget::Explicit {
                root_ids: ids(7..8)
            }
        );
        // The grid's menu, opened on the selection, moves all of it.
        assert_eq!(edit(ids(0..100)), whole);
    }
}
