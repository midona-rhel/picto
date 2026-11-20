//! The decisions on a pair and the movement through the queue: the next or
//! previous pair, Keep Both, Not Duplicates, keeping one side, Smart Merge's
//! survivor, the difference's states, the fit and actual-size zoom, and what
//! the toolbar shows.
use super::compare::{Mode, pair_key};
use super::{Decision, DuplicatesScreen, Fetch, ToolbarModel};
use crate::i18n::{en, t};
use crate::session::{Report, Reread};
use crate::ui;
use gpui::*;
use picto_library::{DuplicateQualityDecision, DuplicateResolutionChoice};

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
    pub(super) fn keep_both(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(Decision::KeepBoth, window, cx);
    }

    /// `resolveCurrent`: makes a decision on the pair on screen, then shows
    /// the next one. Keeping one file merges the other's item into it; the
    /// merged-away file is deleted from the library by the application.
    /// Smart merge on a pair the library cannot rank says so and leaves it.
    pub(super) fn decide(
        &mut self,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.resolving || self.navigating {
            return;
        }
        let Some((key, (a, b))) = self.shown.as_ref().map(|shown| (shown.key, shown.files)) else {
            return;
        };
        self.resolving = true;
        // The buttons are disabled while the decision is written, which
        // takes the pointer and the keyboard from them: the keyboard goes
        // back to the screen, whose keys keep working.
        self.difference_hovered = false;
        self.difference_focused = false;
        self.difference_pressed = false;
        self.smart_hovered = false;
        self.smart_focused = false;
        if self.focus.contains_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        let choice = match decision {
            Decision::KeepBoth => Some(DuplicateResolutionChoice::KeepBoth),
            Decision::Keep(winner) => Some(DuplicateResolutionChoice::KeepFile {
                winner_file_id: winner,
            }),
            Decision::Smart => None,
        };
        crate::session::edit_then(
            &self.session,
            Report::failure(t("Unable to resolve duplicate pair")),
            Reread::NOTHING,
            move |library| match choice {
                Some(choice) => library
                    .resolve_duplicate(a, b, choice, crate::now_ms())
                    .map(Some),
                None => library.resolve_duplicate_automatically(a, b, crate::now_ms()),
            },
            cx,
            move |this: &mut Self, result, cx| match result {
                Ok(Some(resolved)) => {
                    this.own_revision = resolved.receipt.revision;
                    this.fetch(Fetch::Settled(key), cx);
                }
                Ok(None) => {
                    // The pair stays.
                    ui::notify::warning(
                        t("Smart merge needs a choice"),
                        en("No clear quality winner. Choose left or right, or keep both."),
                        cx,
                    );
                    this.resolving = false;
                    this.changed(cx);
                }
                Err(error) => {
                    this.error = Some(error.to_string());
                    this.resolving = false;
                    this.changed(cx);
                }
            },
        );
        self.changed(cx);
    }

    /// Keeps the left (0) or right (1) file.
    pub(super) fn keep(&mut self, side: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((a, b)) = self.shown.as_ref().map(|shown| shown.files) else {
            return;
        };
        self.decide(Decision::Keep(if side == 0 { a } else { b }), window, cx);
    }

    /// The side Smart merge keeps (`smartMergeWinner`), when it is clear.
    pub(super) fn survivor_side(&self) -> Option<usize> {
        let shown = self.shown.as_ref()?;
        let pair = self.pair(shown.key)?;
        survivor(pair.decision)
    }

    /// `mergePreviewActive`: Smart merge is under the pointer or has the
    /// keyboard, nothing is being written, and the library can tell which
    /// file it would keep.
    pub(super) fn merge_previewed(&self) -> Option<usize> {
        ((self.smart_hovered || self.smart_focused) && !self.resolving)
            .then(|| self.survivor_side())
            .flatten()
    }

    /// `differenceActive`: the difference shows while its button is under
    /// the pointer or has the keyboard, or its key is held down.
    pub(super) fn difference_shown(&self) -> bool {
        !self.resolving
            && (self.difference_hovered
                || self.difference_focused
                || self.difference_pressed
                || self.difference_held)
    }

    /// The difference's key going down and coming up.
    pub(super) fn hold_difference(&mut self, held: bool, cx: &mut Context<Self>) {
        if self.difference_held != held {
            self.difference_held = held;
            cx.notify();
        }
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
