//! What the Duplicates screen does when the user acts: moving to the
//! previous or next pair, making a decision on the pair on screen, showing
//! the pixel difference, and changing the zoom. It also works out what the
//! titlebar controls show.
use super::compare::{Mode, pair_key};
use super::{Decision, DuplicatesScreen, Fetch, ToolbarModel};
use crate::i18n::{en, t};
use crate::session::{Report, Reread};
use crate::ui;
use gpui::*;
use picto_library::{DuplicateQualityDecision, DuplicateResolutionChoice};

impl DuplicatesScreen {
    /// Moves `delta` pairs forward (or backward when `delta` is negative) in
    /// the queue. It does nothing while a move or a decision is under way.
    /// The Electron screen's `requestIndex` behaves the same way.
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

    /// Records that the two files of the pair on screen are different
    /// files. The Not duplicates button and the Keep both button both call
    /// this. Neither file is changed.
    pub(super) fn keep_both(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(Decision::KeepBoth, window, cx);
    }

    /// Writes a decision on the pair on screen to the library and then
    /// shows the next pair. The Electron screen's `resolveCurrent` does the
    /// same. When the decision keeps one file, the library merges the other
    /// file's item into the kept file's item, and the application deletes
    /// the other file. When the decision is Smart merge and the library
    /// cannot rate one file as better, a warning says so and the pair stays
    /// on screen.
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
        // The buttons are disabled while the decision is being written. A
        // disabled button reports neither the pointer nor keyboard focus, so
        // the flags that follow them are cleared here. If one of the
        // screen's buttons has keyboard focus, focus moves to the screen
        // itself, so that the screen's keys keep working.
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
                    // The library could not rate one file as better, so it
                    // changed nothing and the pair stays on screen.
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

    /// Keeps the left file when `side` is 0 and the right file when `side`
    /// is 1.
    pub(super) fn keep(&mut self, side: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((a, b)) = self.shown.as_ref().map(|shown| shown.files) else {
            return;
        };
        self.decide(Decision::Keep(if side == 0 { a } else { b }), window, cx);
    }

    /// The file that Smart merge would keep: 0 for the left file, 1 for the
    /// right file, and `None` when the library cannot rate one file as
    /// better. The Electron screen calls this `smartMergeWinner`.
    pub(super) fn smart_merge_keeps(&self) -> Option<usize> {
        let shown = self.shown.as_ref()?;
        let pair = self.pair(shown.key)?;
        match pair.decision {
            DuplicateQualityDecision::LeftBetter | DuplicateQualityDecision::AutoTieLeft => Some(0),
            DuplicateQualityDecision::RightBetter | DuplicateQualityDecision::AutoTieRight => {
                Some(1)
            }
            DuplicateQualityDecision::NeedsChoice => None,
        }
    }

    /// The picture that gets the selection outline: the file that Smart
    /// merge would keep, while the pointer is on the Smart merge button or
    /// the button has keyboard focus, and no decision is being written. The
    /// Electron screen calls this condition `mergePreviewActive`.
    pub(super) fn outlined_side(&self) -> Option<usize> {
        ((self.smart_hovered || self.smart_focused) && !self.resolving)
            .then(|| self.smart_merge_keeps())
            .flatten()
    }

    /// True when the pictures are replaced by the difference picture: while
    /// the pointer is on the Show Difference button, while that button has
    /// keyboard focus, after a click on it, or while its key is held down.
    /// The Electron screen calls this `differenceActive`.
    pub(super) fn difference_shown(&self) -> bool {
        !self.resolving
            && (self.difference_hovered
                || self.difference_focused
                || self.difference_pressed
                || self.difference_held)
    }

    /// Records that the Show Difference key was pressed (`held` is true) or
    /// released (`held` is false).
    pub(super) fn hold_difference(&mut self, held: bool, cx: &mut Context<Self>) {
        if self.difference_held != held {
            self.difference_held = held;
            cx.notify();
        }
    }

    /// Zooms both pictures so that each fits its pane.
    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        self.zoom.fit();
        self.changed(cx);
    }

    /// Zooms both pictures so that one picture pixel is one screen point.
    pub(super) fn actual(&mut self, cx: &mut Context<Self>) {
        self.zoom.actual();
        self.changed(cx);
    }

    /// The position of the pair on screen in the queue, counted from 0.
    fn index(&self) -> Option<usize> {
        let key = self.shown.as_ref()?.key;
        self.pairs
            .iter()
            .position(|pair| pair_key(pair.file_id_a, pair.file_id_b) == key)
    }

    /// What the titlebar controls show, or `None` while the queue is being
    /// read for the first time or no pair is on screen.
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
