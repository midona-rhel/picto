//! The grid's selection over a destination the grid only partly holds, as
//! the Electron grid keeps it (`state/selection.ts`, `rangeSelection.ts`):
//! items chosen one by one, by id; everything (Select All) less the items
//! taken out since; or shift-click spans of the destination's order, plus
//! items added outside them and less items taken out inside them. Spans and
//! the whole destination reach items that are not loaded; an edit addresses
//! them through the library (`SelectionTarget::Query`, `::Range`), which
//! resolves them, never through what the grid holds.
use picto_library::RootId;
use picto_library::query::RootQuery;
use picto_library::selection::{RangeSpan, SelectionTarget};
use std::collections::{BTreeMap, HashMap};

/// What a left press on a tile or the grid means, by the keys held
/// (`event.metaKey || event.ctrlKey` in production, each platform's own key):
/// on macOS a Control-press is a secondary click, which opens the menu, and
/// the key that adds to the selection is Command there and Control
/// elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Selects only what was pressed (or starts a fresh rubber band).
    Plain,
    /// Adds what was pressed to the selection or takes it out.
    Toggle,
    /// Selects from the anchor to what was pressed; `keep` adds the span to
    /// the selection rather than replacing it.
    Range { keep: bool },
    /// A secondary click: the menu, as a right-click.
    Menu,
}

impl Press {
    /// The press these keys make, on this platform (`mac`).
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

    /// The press these keys make on the platform this runs on.
    pub fn here(modifiers: gpui::Modifiers) -> Self {
        Self::of(modifiers, cfg!(target_os = "macos"))
    }

    /// Whether the press keeps what is selected: a rubber band adds to it.
    pub fn keeps(self) -> bool {
        matches!(self, Press::Toggle | Press::Range { .. })
    }
}

/// An item and where the destination's order has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    pub id: RootId,
    pub ordinal: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// Items chosen one by one, with where the grid showed each.
    Explicit(BTreeMap<RootId, usize>),
    /// Everything the destination holds.
    Whole,
    /// Spans of the order, each `(low, high)`, sorted and apart; and items
    /// added outside them.
    Range {
        spans: Vec<(End, End)>,
        included: BTreeMap<RootId, usize>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    kind: Kind,
    /// Items taken out of the whole destination or out of a span, with
    /// where the grid showed each.
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

/// `mergeSpans`: sorted, with spans that overlap or touch made one.
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

/// Ids by where the grid showed them.
fn in_order(items: &BTreeMap<RootId, usize>) -> Vec<RootId> {
    let mut ordered = items.iter().map(|(id, at)| (*at, *id)).collect::<Vec<_>>();
    ordered.sort_unstable();
    ordered.into_iter().map(|(_, id)| id).collect()
}

impl Selection {
    /// One item alone, as a plain click selects it.
    pub fn only(end: End) -> Self {
        Self {
            kind: Kind::Explicit([(end.id, end.ordinal)].into()),
            ..Self::default()
        }
    }

    /// These items, as a rubber band leaves them.
    pub fn of(ends: impl IntoIterator<Item = End>) -> Self {
        Self {
            kind: Kind::Explicit(ends.into_iter().map(|end| (end.id, end.ordinal)).collect()),
            ..Self::default()
        }
    }

    /// Everything the destination holds.
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

    /// Whether the selection reaches past what was chosen one by one, so an
    /// edit has to address the destination's query.
    pub fn query_wide(&self) -> bool {
        !matches!(self.kind, Kind::Explicit(_))
    }

    /// Whether the item at `ordinal` is selected.
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

    /// How many items are selected in a destination of `total`.
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

    /// The items chosen one by one, in the grid's order: the whole
    /// selection when it is explicit, else the items added outside spans.
    pub fn chosen(&self) -> Vec<RootId> {
        match &self.kind {
            Kind::Explicit(items)
            | Kind::Range {
                included: items, ..
            } => in_order(items),
            Kind::Whole => Vec::new(),
        }
    }

    /// A click with the selection key: adds the item, or takes it out.
    /// Taking out the last item of a destination of `total` leaves nothing
    /// selected.
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

    /// Shift-click from `anchor` to `focus`. With the anchor among the
    /// items `loaded` (by ordinal) and nothing query-wide to extend, the
    /// items between them are chosen one by one, added to the selection
    /// with `keep`; otherwise the run is a span the library resolves.
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

    /// `addSpan`: a span added to what is selected. Items chosen one by one
    /// or taken out that the span covers become part of it.
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

    /// Every item whose place a refresh needs to know: the items chosen,
    /// taken out, and the ends of spans.
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

    /// `reconcileRange`: after the destination was read again, every item
    /// moves to where it is now (`None`: it left the destination). Items
    /// that left are dropped, and so is a span whose end left, since no run
    /// of the order is what it described. Items not in `places` keep theirs.
    pub fn reconcile(&mut self, places: &HashMap<RootId, Option<usize>>) {
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
                let spans = merge(
                    spans
                        .iter()
                        .filter_map(|(low, high)| Some(span_of(end(low)?, end(high)?)))
                        .collect(),
                );
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
    }

    /// What an edit of the selection addresses in the destination `query`
    /// shows.
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
        // macOS: Command toggles, Control is the secondary click.
        assert_eq!(Press::of(keys(false, true, false), true), Press::Toggle);
        assert_eq!(Press::of(keys(true, false, false), true), Press::Menu);
        assert_eq!(Press::of(keys(true, false, true), true), Press::Menu);
        assert_eq!(
            Press::of(keys(false, true, true), true),
            Press::Range { keep: true }
        );
        // Windows and Linux: Control toggles; the Windows key does nothing.
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
        // Item ids differ from ordinals so the two are never confused.
        End {
            id: RootId(ordinal as u32 + 1000),
            ordinal,
        }
    }

    /// A window holding ordinals `start..start + count`.
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
        // Kept: added to what was chosen.
        let mut kept = Selection::only(end(100));
        kept.extend(end(10), end(12), true, window(0, 500));
        assert_eq!(kept.count(2673), 4);
    }

    #[test]
    fn a_range_from_an_unloaded_anchor_is_a_span_the_library_resolves() {
        // The anchor at ordinal 3 scrolled out of the window, which now
        // holds 1,500..3,000; a shift-click on 2,000.
        let mut selection = Selection::only(end(3));
        selection.extend(end(3), end(2000), false, window(1500, 1500));
        assert!(selection.query_wide());
        assert_eq!(selection.count(10_000), 1998);
        // Unloaded items inside the span are selected; outside are not.
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
        // Inside the span: taken out.
        selection.toggle(end(1600), 10_000);
        assert!(!selection.contains(end(1600).id, 1600));
        assert_eq!(selection.count(10_000), 1997);
        // Outside it: added.
        selection.toggle(end(2500), 10_000);
        assert!(selection.contains(end(2500).id, 2500));
        assert_eq!(selection.count(10_000), 1998);
        // A second span with Cmd-Shift covers the added item: it is part
        // of the span, not counted twice.
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
        chosen.reconcile(&places);
        assert_eq!(chosen.count(100), 2);
        assert_eq!(chosen.chosen(), vec![end(3).id, end(1).id]);
        // A span whose end left the destination is dropped.
        let mut spanned = Selection::default();
        spanned.extend(end(3), end(2000), false, window(1500, 10));
        spanned.reconcile(&HashMap::from([(end(3).id, None)]));
        assert!(spanned.is_empty());
        // One whose ends moved follows them.
        let mut moved = Selection::default();
        moved.extend(end(3), end(2000), false, window(1500, 10));
        moved.reconcile(&HashMap::from([(end(3).id, Some(5))]));
        assert_eq!(moved.count(10_000), 1996);
    }
}
