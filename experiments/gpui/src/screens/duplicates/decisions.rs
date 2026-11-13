//! The decisions on a pair and the movement through the queue: the next or
//! previous pair, Keep Both, Not Duplicates, keeping one side, Smart Merge's
//! survivor, the fit and actual-size zoom, and what the toolbar shows.
use super::compare::{Mode, pair_key};
use super::{Decision, DuplicatesScreen, Fetch, ToolbarModel};
use gpui::*;
use picto_library::DuplicateQualityDecision;

impl DuplicatesScreen {
    /// `requestIndex`: another pair of the queue, one at a time.
    pub(super) fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.navigating || self.resolving {
            return;
        }
        let Some(index) = self.index() else {
            return;
        };
        let target = index
            .saturating_add_signed(delta)
            .min(self.pairs.len().saturating_sub(1));
        if target == index {
            return;
        }
        let pair = &self.pairs[target];
        self.fetch(Fetch::Pair(pair_key(pair.file_id_a, pair.file_id_b)), cx);
    }

    /// Settles the pair on screen as two different files. "Not duplicates"
    /// and "Keep both" both record this; neither touches a file.
    pub(super) fn keep_both(&mut self, cx: &mut Context<Self>) {
        self.decide(Decision::KeepBoth, cx);
    }

    /// `resolveCurrent`: makes a decision on the pair on screen, then shows
    /// the next one. Keeping one file merges the other's item into it; the
    /// merged-away file is deleted from the copy by the application.
    pub(super) fn decide(&mut self, decision: Decision, cx: &mut Context<Self>) {
        if self.resolving || self.navigating {
            return;
        }
        let Some((a, b)) = self.shown.as_ref().map(|shown| shown.files) else {
            return;
        };
        self.difference_hovered = false;
        self.set_smart_hovered(false, cx);
        self.fetch(Fetch::Resolve(a, b, decision), cx);
    }

    /// Keeps the left (0) or right (1) file.
    pub(super) fn keep(&mut self, side: usize, cx: &mut Context<Self>) {
        let Some((a, b)) = self.shown.as_ref().map(|shown| shown.files) else {
            return;
        };
        self.decide(Decision::Keep(if side == 0 { a } else { b }), cx);
    }

    /// Rings the file Smart merge would keep while it is hovered.
    pub(super) fn set_smart_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.smart_hovered != hovered {
            self.smart_hovered = hovered;
            cx.notify();
        }
    }

    /// The side Smart merge keeps (`smartMergeWinner`), when it is clear.
    pub(super) fn survivor_side(&self) -> Option<usize> {
        let shown = self.shown.as_ref()?;
        let pair = self.pair(shown.key)?;
        survivor(pair.decision)
    }

    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        self.zoom.fit();
        self.changed(cx);
    }

    pub(super) fn actual(&mut self, cx: &mut Context<Self>) {
        self.zoom.actual();
        self.changed(cx);
    }

    fn index(&self) -> Option<usize> {
        let key = self.shown.as_ref()?.key;
        self.pairs
            .iter()
            .position(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
    }

    pub(super) fn toolbar_model(&self) -> Option<ToolbarModel> {
        if self.loading {
            return None;
        }
        let index = self.index()?;
        Some(ToolbarModel {
            index,
            total: self.total,
            can_previous: index > 0,
            can_next: index + 1 < self.pairs.len(),
            disabled: self.resolving,
            fit: self.zoom.mode == Mode::Fit,
            actual: self.zoom.mode == Mode::Actual,
        })
    }
}

/// `smartMergeWinner`: the side the library judges better, if it can tell.
fn survivor(decision: DuplicateQualityDecision) -> Option<usize> {
    match decision {
        DuplicateQualityDecision::LeftBetter | DuplicateQualityDecision::AutoTieLeft => Some(0),
        DuplicateQualityDecision::RightBetter | DuplicateQualityDecision::AutoTieRight => Some(1),
        DuplicateQualityDecision::NeedsChoice => None,
    }
}
