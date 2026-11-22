//! The grid's selection. The grid holds only a part of a destination's
//! items, and a selection can include items that the grid has not loaded. A
//! selection is one of three kinds: items chosen one by one, held by id;
//! every item of the destination (Select All) except the items taken out
//! afterwards; or spans of the destination's order made with shift-clicks,
//! together with the items added outside the spans and without the items
//! taken out inside them. For the last two kinds, an edit names the
//! selection to the library as a query or as spans
//! (`SelectionTarget::Query`, `SelectionTarget::Range`), and the library
//! works out which items it holds. An edit never works from the items the
//! grid has loaded.
use picto_library::RootId;
use picto_library::query::RootQuery;
use picto_library::selection::{RangeSpan, SelectionTarget};
use std::collections::{BTreeMap, HashMap};

/// What a left press on a tile or on the grid's empty space does, which
/// depends on the keys held. The key that adds to the selection is Command
/// on macOS and Control on Windows and Linux. On macOS a press with Control
/// held is a secondary click, which opens the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Selects only the pressed item. On empty space it starts a rubber band
    /// that replaces the selection.
    Plain,
    /// Adds the pressed item to the selection, or takes it out if it is
    /// selected.
    Toggle,
    /// Selects the items from the anchor to the pressed item. With `keep`
    /// they are added to the selection, and without it they replace the
    /// selection.
    Range { keep: bool },
    /// A secondary click, which opens the menu as a right-click does.
    Menu,
}

impl Press {
    /// Returns what a left press with `modifiers` held does. `mac` says
    /// whether the platform is macOS.
    pub fn of(modifiers: gpui::Modifiers, mac: bool) -> Self {
        if mac && modifiers.control {
            return Press::Menu;
        }
        let toggle = if mac {
            modifiers.platform
        } else {
            modifiers.control
        };
        match (modifiers.shift, toggle) {
            (true, keep) => Press::Range { keep },
            (false, true) => Press::Toggle,
            (false, false) => Press::Plain,
        }
    }

    /// Returns what a left press with `modifiers` held does on the platform
    /// the app runs on.
    pub fn here(modifiers: gpui::Modifiers) -> Self {
        Self::of(modifiers, cfg!(target_os = "macos"))
    }

    /// Whether a rubber band started with this press adds to the selection
    /// instead of replacing it.
    pub fn keeps(self) -> bool {
        matches!(self, Press::Toggle | Press::Range { .. })
    }
}

/// An item and its position in the destination's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    pub id: RootId,
    pub ordinal: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// Items chosen one by one, each with the ordinal at which the grid
    /// showed it.
    Explicit(BTreeMap<RootId, usize>),
    /// Every item of the destination.
    Whole,
    /// Spans of the destination's order, each from `low` to `high`, sorted,
    /// with no two of them overlapping or adjacent; and the items added
    /// outside the spans, each with its ordinal.
    Range {
        spans: Vec<(End, End)>,
        included: BTreeMap<RootId, usize>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    kind: Kind,
    /// The items taken out of the whole destination or out of a span, each
    /// with the ordinal at which the grid showed it.
    excluded: BTreeMap<RootId, usize>,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            kind: Kind::Explicit(BTreeMap::new()),
            excluded: BTreeMap::new(),
        }
    }
}

/// Returns the spans sorted by their start, with spans that overlap or are
/// adjacent joined into one.
fn merge(mut spans: Vec<(End, End)>) -> Vec<(End, End)> {
    spans.sort_by_key(|(low, _)| low.ordinal);
    let mut merged: Vec<(End, End)> = Vec::with_capacity(spans.len());
    for (low, high) in spans {
        match merged.last_mut() {
            Some(last) if low.ordinal <= last.1.ordinal + 1 => {
                if high.ordinal > last.1.ordinal {
                    last.1 = high;
                }
            }
            _ => merged.push((low, high)),
        }
    }
    merged
}

fn span_of(a: End, b: End) -> (End, End) {
    if a.ordinal <= b.ordinal {
        (a, b)
    } else {
        (b, a)
    }
}

fn in_spans(spans: &[(End, End)], ordinal: usize) -> bool {
    spans
        .iter()
        .any(|(low, high)| low.ordinal <= ordinal && ordinal <= high.ordinal)
}

/// Returns the ids of `items`, ordered by the ordinal at which the grid
/// showed each.
fn in_order(items: &BTreeMap<RootId, usize>) -> Vec<RootId> {
    let mut ordered = items.iter().map(|(id, at)| (*at, *id)).collect::<Vec<_>>();
    ordered.sort_unstable();
    ordered.into_iter().map(|(_, id)| id).collect()
}

impl Selection {
    /// Returns a selection of one item, which is what a plain click leaves.
    pub fn only(end: End) -> Self {
        Self {
            kind: Kind::Explicit([(end.id, end.ordinal)].into()),
            ..Self::default()
        }
    }

    /// Returns a selection of these items, chosen one by one, which is what
    /// a rubber band leaves.
    pub fn of(ends: impl IntoIterator<Item = End>) -> Self {
        Self {
            kind: Kind::Explicit(ends.into_iter().map(|end| (end.id, end.ordinal)).collect()),
            ..Self::default()
        }
    }

    /// Returns a selection of every item of the destination.
    pub fn whole() -> Self {
        Self {
            kind: Kind::Whole,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Explicit(items) => items.is_empty(),
            Kind::Whole => false,
            Kind::Range { spans, included } => spans.is_empty() && included.is_empty(),
        }
    }

    /// Whether the selection is the whole destination or holds spans. Such
    /// a selection can include items the grid has not loaded, so an edit
    /// names it to the library by the destination's query.
    pub fn query_wide(&self) -> bool {
        !matches!(self.kind, Kind::Explicit(_))
    }

    /// Whether the item `id`, which is at `ordinal` in the destination's
    /// order, is selected.
    pub fn contains(&self, id: RootId, ordinal: usize) -> bool {
        match &self.kind {
            Kind::Explicit(items) => items.contains_key(&id),
            Kind::Whole => !self.excluded.contains_key(&id),
            Kind::Range { spans, included } => {
                included.contains_key(&id)
                    || (in_spans(spans, ordinal) && !self.excluded.contains_key(&id))
            }
        }
    }

    /// Returns the number of selected items in a destination of `total`
    /// items.
    pub fn count(&self, total: u64) -> u64 {
        let excluded = self.excluded.len() as u64;
        match &self.kind {
            Kind::Explicit(items) => items.len() as u64,
            Kind::Whole => total.saturating_sub(excluded),
            Kind::Range { spans, included } => {
                let spanned = spans
                    .iter()
                    .map(|(low, high)| (high.ordinal - low.ordinal + 1) as u64)
                    .sum::<u64>();
                (spanned + included.len() as u64).saturating_sub(excluded)
            }
        }
    }

    /// Returns the items chosen one by one, in the grid's order. For an
    /// explicit selection these are all of its items. For a selection with
    /// spans they are the items added outside the spans. For the whole
    /// destination there are none.
    pub fn chosen(&self) -> Vec<RootId> {
        match &self.kind {
            Kind::Explicit(items)
            | Kind::Range {
                included: items, ..
            } => in_order(items),
            Kind::Whole => Vec::new(),
        }
    }

    /// Applies a click with the key that adds to the selection: adds the
    /// item if it is not selected, and takes it out if it is. When that
    /// takes out the last selected item of a destination of `total` items,
    /// the selection becomes empty.
    pub fn toggle(&mut self, end: End, total: u64) {
        let flip = |items: &mut BTreeMap<RootId, usize>| {
            if items.remove(&end.id).is_none() {
                items.insert(end.id, end.ordinal);
            }
        };
        match &mut self.kind {
            Kind::Explicit(items) => flip(items),
            Kind::Whole => flip(&mut self.excluded),
            Kind::Range { spans, included } => {
                if in_spans(spans, end.ordinal) {
                    flip(&mut self.excluded);
                } else {
                    flip(included);
                }
            }
        }
        if self.query_wide() && self.count(total) == 0 {
            *self = Self::default();
        }
    }

    /// Applies a shift-click that selects from `anchor` to `focus`. `loaded`
    /// returns the id of the loaded item at an ordinal, or `None` when the
    /// grid has not loaded that item.
    ///
    /// When `anchor` is loaded, and either `keep` is false or the selection
    /// is explicit, the loaded items from `anchor` to `focus` are chosen one
    /// by one. With `keep` they are added to the items already chosen.
    ///
    /// In every other case the run from `anchor` to `focus` is stored as a
    /// span, which can include items that are not loaded and which the
    /// library resolves. With `keep` the span is added to the selection, and
    /// without it the span replaces the selection.
    pub fn extend(
        &mut self,
        anchor: End,
        focus: End,
        keep: bool,
        loaded: impl Fn(usize) -> Option<RootId>,
    ) {
        let anchor_loaded = loaded(anchor.ordinal) == Some(anchor.id);
        let span = span_of(anchor, focus);
        if keep && (!anchor_loaded || self.query_wide()) {
            self.add_span(span);
            return;
        }
        if !anchor_loaded {
            *self = Self {
                kind: Kind::Range {
                    spans: vec![span],
                    included: BTreeMap::new(),
                },
                excluded: BTreeMap::new(),
            };
            return;
        }
        let mut items = match (&self.kind, keep) {
            (Kind::Explicit(items), true) => items.clone(),
            _ => BTreeMap::new(),
        };
        for ordinal in span.0.ordinal..=span.1.ordinal {
            if let Some(id) = loaded(ordinal) {
                items.insert(id, ordinal);
            }
        }
        *self = Self {
            kind: Kind::Explicit(items),
            excluded: BTreeMap::new(),
        };
    }

    /// Adds a span to the selection. Items chosen one by one that the span
    /// covers become part of the span, and items taken out that the span
    /// covers are selected again.
    fn add_span(&mut self, span: (End, End)) {
        let inside = |ordinal: usize| span.0.ordinal <= ordinal && ordinal <= span.1.ordinal;
        let outside = |items: &BTreeMap<RootId, usize>| {
            items
                .iter()
                .filter(|(_, at)| !inside(**at))
                .map(|(id, at)| (*id, *at))
                .collect::<BTreeMap<_, _>>()
        };
        self.excluded = outside(&self.excluded);
        self.kind = match &self.kind {
            Kind::Whole => Kind::Whole,
            Kind::Explicit(items) => Kind::Range {
                spans: vec![span],
                included: outside(items),
            },
            Kind::Range { spans, included } => {
                let mut spans = spans.clone();
                spans.push(span);
                Kind::Range {
                    spans: merge(spans),
                    included: outside(included),
                }
            }
        };
    }

    /// Returns the id of every item whose position must be looked up after
    /// the destination is read again: the items chosen one by one, the items
    /// taken out, and the two ends of each span.
    pub fn tracked(&self) -> Vec<RootId> {
        let mut ids = self.excluded.keys().copied().collect::<Vec<_>>();
        match &self.kind {
            Kind::Explicit(items) => ids.extend(items.keys()),
            Kind::Whole => {}
            Kind::Range { spans, included } => {
                ids.extend(included.keys());
                for (low, high) in spans {
                    ids.extend([low.id, high.id]);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Updates the selection after the destination was read again. `places`
    /// gives the new ordinal of an item, or `None` when the item is no longer
    /// in the destination. An item that is not in `places` keeps its ordinal.
    /// Items that left the destination are removed from the selection. A
    /// span is removed when one of its ends left, because it no longer
    /// describes a run of the order. Returns whether a span was removed for
    /// that reason, so that the caller can warn the user.
    pub fn reconcile(&mut self, places: &HashMap<RootId, Option<usize>>) -> bool {
        let mut lost_end = false;
        let moved = |id: &RootId, at: usize| match places.get(id) {
            Some(Some(now)) => Some(*now),
            Some(None) => None,
            None => Some(at),
        };
        let follow = |items: &BTreeMap<RootId, usize>| {
            items
                .iter()
                .filter_map(|(id, at)| Some((*id, moved(id, *at)?)))
                .collect::<BTreeMap<_, _>>()
        };
        self.excluded = follow(&self.excluded);
        self.kind = match &self.kind {
            Kind::Explicit(items) => Kind::Explicit(follow(items)),
            Kind::Whole => Kind::Whole,
            Kind::Range { spans, included } => {
                let end = |end: &End| {
                    Some(End {
                        id: end.id,
                        ordinal: moved(&end.id, end.ordinal)?,
                    })
                };
                let kept = spans
                    .iter()
                    .filter_map(|(low, high)| Some(span_of(end(low)?, end(high)?)))
                    .collect::<Vec<_>>();
                lost_end = kept.len() < spans.len();
                let spans = merge(kept);
                let included = follow(included)
                    .into_iter()
                    .filter(|(_, at)| !in_spans(&spans, *at))
                    .collect();
                self.excluded.retain(|_, at| in_spans(&spans, *at));
                Kind::Range { spans, included }
            }
        };
        if self.is_empty() {
            self.excluded.clear();
        }
        lost_end
    }

    /// Returns the description of the selection that an edit sends to the
    /// library. `query` is the query of the destination that the grid shows.
    /// Returns `None` when the selection is not explicit and there is no
    /// query.
    pub fn target(&self, query: Option<&RootQuery>) -> Option<SelectionTarget> {
        let excluded = || in_order(&self.excluded);
        Some(match &self.kind {
            Kind::Explicit(items) => SelectionTarget::Explicit {
                root_ids: in_order(items),
            },
            Kind::Whole => SelectionTarget::Query {
                query: query?.clone(),
                excluded_root_ids: excluded(),
            },
            Kind::Range { spans, included } => SelectionTarget::Range {
                query: query?.clone(),
                spans: spans
                    .iter()
                    .map(|(low, high)| RangeSpan {
                        anchor_root_id: low.id,
                        focus_root_id: high.id,
                        anchor_index: Some(low.ordinal as u64),
                        focus_index: Some(high.ordinal as u64),
                    })
                    .collect(),
                included_root_ids: in_order(included),
                excluded_root_ids: excluded(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_adds_to_the_selection_with_its_own_key() {
        let keys = |control, platform, shift| gpui::Modifiers {
            control,
            platform,
            shift,
            ..Default::default()
        };
        // On macOS, Command toggles and Control is the secondary click.
        assert_eq!(Press::of(keys(false, true, false), true), Press::Toggle);
        assert_eq!(Press::of(keys(true, false, false), true), Press::Menu);
        assert_eq!(Press::of(keys(true, false, true), true), Press::Menu);
        assert_eq!(
            Press::of(keys(false, true, true), true),
            Press::Range { keep: true }
        );
        // On Windows and Linux, Control toggles and the Windows key does
        // nothing.
        assert_eq!(Press::of(keys(true, false, false), false), Press::Toggle);
        assert_eq!(Press::of(keys(false, true, false), false), Press::Plain);
        assert_eq!(
            Press::of(keys(true, false, true), false),
            Press::Range { keep: true }
        );
        assert_eq!(
            Press::of(keys(false, false, true), false),
            Press::Range { keep: false }
        );
        assert_eq!(Press::of(keys(false, false, false), false), Press::Plain);
        assert!(Press::Toggle.keeps() && Press::Range { keep: false }.keeps());
        assert!(!Press::Plain.keeps());
    }

    fn end(ordinal: usize) -> End {
        // The item ids differ from the ordinals, so that a test cannot pass by
        // using one in place of the other.
        End {
            id: RootId(ordinal as u32 + 1000),
            ordinal,
        }
    }

    /// Returns a function that stands for a grid that has loaded the items
    /// at the ordinals `start..start + count`.
    fn window(start: usize, count: usize) -> impl Fn(usize) -> Option<RootId> {
        move |ordinal| {
            (start..start + count)
                .contains(&ordinal)
                .then(|| end(ordinal).id)
        }
    }

    #[test]
    fn a_range_inside_the_window_is_chosen_item_by_item() {
        let mut selection = Selection::only(end(10));
        selection.extend(end(10), end(14), false, window(0, 500));
        assert!(!selection.query_wide());
        assert_eq!(selection.count(2673), 5);
        assert!(selection.contains(end(12).id, 12));
        assert!(!selection.contains(end(15).id, 15));
        // With `keep`, the items are added to those already chosen.
        let mut kept = Selection::only(end(100));
        kept.extend(end(10), end(12), true, window(0, 500));
        assert_eq!(kept.count(2673), 4);
    }

    #[test]
    fn a_range_from_an_unloaded_anchor_is_a_span_the_library_resolves() {
        // The anchor at ordinal 3 is no longer loaded. The grid now holds the
        // ordinals 1,500 to 2,999, and the shift-click is on ordinal 2,000.
        let mut selection = Selection::only(end(3));
        selection.extend(end(3), end(2000), false, window(1500, 1500));
        assert!(selection.query_wide());
        assert_eq!(selection.count(10_000), 1998);
        // Items inside the span that are not loaded are selected, and items
        // outside it are not.
        assert!(selection.contains(end(700).id, 700));
        assert!(!selection.contains(end(2001).id, 2001));
        let query = RootQuery {
            scope: picto_library::query::ItemScope::All,
            view: Default::default(),
        };
        let Some(SelectionTarget::Range { spans, .. }) = selection.target(Some(&query)) else {
            panic!("a span is a range target");
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].anchor_index, Some(3));
        assert_eq!(spans[0].focus_index, Some(2000));
    }

    #[test]
    fn toggling_inside_and_outside_a_span() {
        let mut selection = Selection::default();
        selection.extend(end(3), end(2000), false, window(1500, 1500));
        // A toggle inside the span takes the item out.
        selection.toggle(end(1600), 10_000);
        assert!(!selection.contains(end(1600).id, 1600));
        assert_eq!(selection.count(10_000), 1997);
        // A toggle outside the span adds the item.
        selection.toggle(end(2500), 10_000);
        assert!(selection.contains(end(2500).id, 2500));
        assert_eq!(selection.count(10_000), 1998);
        // A second span, added with `keep`, covers the added item. The item
        // becomes part of the span and is not counted twice.
        selection.extend(end(2400), end(2600), true, window(1500, 1500));
        assert_eq!(selection.count(10_000), 1997 + 201);
    }

    #[test]
    fn select_all_counts_every_item_and_addresses_the_query() {
        let mut selection = Selection::whole();
        assert_eq!(selection.count(1_000_000), 1_000_000);
        assert!(selection.contains(end(999_999).id, 999_999));
        selection.toggle(end(5), 1_000_000);
        assert_eq!(selection.count(1_000_000), 999_999);
        assert!(!selection.contains(end(5).id, 5));
        let query = RootQuery {
            scope: picto_library::query::ItemScope::Inbox,
            view: Default::default(),
        };
        assert!(matches!(
            selection.target(Some(&query)),
            Some(SelectionTarget::Query { excluded_root_ids, .. }) if excluded_root_ids == [end(5).id]
        ));
        // Taking out the only item leaves nothing selected.
        let mut one = Selection::whole();
        one.toggle(end(0), 1);
        assert!(one.is_empty());
    }

    #[test]
    fn spans_that_touch_become_one() {
        let mut selection = Selection::default();
        selection.extend(end(10), end(20), false, window(5000, 10));
        selection.extend(end(21), end(30), true, window(5000, 10));
        selection.extend(end(50), end(40), true, window(5000, 10));
        assert_eq!(selection.count(100), 21 + 11);
        let query = RootQuery {
            scope: picto_library::query::ItemScope::All,
            view: Default::default(),
        };
        let Some(SelectionTarget::Range { spans, .. }) = selection.target(Some(&query)) else {
            panic!();
        };
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn a_refresh_moves_items_and_drops_those_that_left() {
        let mut chosen = Selection::of([end(1), end(2), end(3)]);
        let places = HashMap::from([(end(1).id, Some(4)), (end(2).id, None)]);
        // Items that left an explicit selection are not reported as the lost
        // end of a span.
        assert!(!chosen.reconcile(&places));
        assert_eq!(chosen.count(100), 2);
        assert_eq!(chosen.chosen(), vec![end(3).id, end(1).id]);
        // A span is removed when one of its ends left the destination.
        let mut spanned = Selection::default();
        spanned.extend(end(3), end(2000), false, window(1500, 10));
        assert!(spanned.reconcile(&HashMap::from([(end(3).id, None)])));
        assert!(spanned.is_empty());
        // A span whose end moved follows that end.
        let mut moved = Selection::default();
        moved.extend(end(3), end(2000), false, window(1500, 10));
        assert!(!moved.reconcile(&HashMap::from([(end(3).id, Some(5))])));
        assert_eq!(moved.count(10_000), 1996);
    }
}
