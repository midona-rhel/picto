//! The Inbox review over the media view and Quick Look, as `GridScreen.tsx`
//! wires `InboxReviewControls` to its reviewer: the pill shows while the item
//! being viewed is in the Inbox, a press (or its shortcut) moves the view on
//! at once and saves through the session in the background, and a reject
//! that deletes goes through the item menu's permanent delete, which asks
//! first when it cannot be undone.
use super::Shell;
use crate::i18n::t;
use crate::viewer::review::{self, Decision, Flash, View};
use gpui::*;
use picto_library::selection::SelectionTarget;
use picto_library::{Lifecycle, RootId};
use std::time::Instant;

/// A review waiting for the items after the loaded ones: the reviewed item
/// is the last loaded, but not the last in the list.
/// Tells a review key's first press from the repeats of holding it down,
/// which review nothing (`event.repeat`). GPUI hands an action no repeat
/// flag, so the keys around it are watched: a review key is held from its
/// press until it is released or another key is pressed.
#[derive(Default)]
pub(super) struct ReviewKeys {
    /// The review key held down.
    held: Option<String>,
    /// A review key's action ran for the keystroke being dispatched.
    pressed: bool,
}

impl ReviewKeys {
    /// Before a keystroke is dispatched: another key ends the hold.
    fn before(&mut self, key: &str) {
        if self.held.as_deref() != Some(key) {
            self.held = None;
        }
    }

    /// From the review action: whether its keystroke repeats the held key.
    /// A first press is remembered for [`Self::after`].
    fn repeat(&mut self) -> bool {
        if self.held.is_some() {
            return true;
        }
        self.pressed = true;
        false
    }

    /// After a keystroke was dispatched: the review key it pressed is held.
    fn after(&mut self, key: &str) {
        if std::mem::take(&mut self.pressed) {
            self.held = Some(key.to_owned());
        }
    }

    pub(super) fn released(&mut self) {
        self.held = None;
    }
}

pub(super) struct Pending {
    ticket: u64,
    id: RootId,
    decision: Decision,
    view: View,
}

impl Shell {
    /// Whether the media view or Quick Look is open.
    pub(super) fn viewing(&self, cx: &App) -> bool {
        self.session.read(cx).viewer || self.quick_look.read(cx).is_open()
    }

    /// `resolveInboxReviewItemId`: the item being viewed, while it is in the
    /// Inbox.
    fn review_item(&self, cx: &App) -> Option<RootId> {
        if !self.viewing(cx) {
            return None;
        }
        self.session
            .read(cx)
            .selected_item()
            .filter(|item| item.lifecycle == Lifecycle::Inbox)
            .map(|item| item.root_id)
    }

    fn review_view(&self, cx: &App) -> View {
        if self.quick_look.read(cx).is_open() {
            View::QuickLook
        } else {
            View::Detail
        }
    }

    /// The item `view` shows, or none while it is closed.
    fn shown_in(&self, view: View, cx: &App) -> Option<RootId> {
        let open = match view {
            View::QuickLook => self.quick_look.read(cx).is_open(),
            View::Detail => self.session.read(cx).viewer,
        };
        let session = self.session.read(cx);
        open.then(|| session.selected_item().map(|item| item.root_id))
            .flatten()
    }

    fn listed_ids(&self, cx: &App) -> Vec<RootId> {
        self.session
            .read(cx)
            .items
            .iter()
            .map(|item| item.root_id)
            .collect()
    }

    /// A press of Accept or Reject, or its key: reviews the item being
    /// viewed and flashes the button when the review is taken.
    pub(super) fn press_review(
        &mut self,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.review_item(cx) else {
            return;
        };
        if self.review(id, decision, window, cx) {
            self.review_flash = Some(Flash {
                decision,
                since: Instant::now(),
            });
            cx.notify();
        }
    }

    /// The review shortcuts while a viewer has the keyboard: a key held
    /// down reviews once, not once per repeat.
    pub(super) fn review_key(
        &mut self,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review_keys.repeat() {
            return;
        }
        self.press_review(decision, window, cx);
    }

    /// `review` in `inboxReview.ts`. False when the review is not taken: a
    /// question is open, or the item is already reviewed.
    fn review(
        &mut self,
        id: RootId,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.asking() || self.reviewer.reviewed(id) {
            return false;
        }
        let view = self.review_view(cx);
        let ticket = self.reviewer.begin(id);
        let items = self.listed_ids(cx);
        let next = self.reviewer.following(&items, id).flatten();
        if next.is_some() || !self.session.read(cx).has_following() {
            let destination = next.or_else(|| self.reviewer.preceding(&items, id));
            self.move_review_on(ticket, view, destination, window, cx);
            self.save_review(ticket, id, decision, window, cx);
        } else {
            // The next item is in the window that follows, not held: load
            // it before saving, so the item is still listed to find its
            // successor from (the next window starts a page before it).
            self.pending_review = Some(Pending {
                ticket,
                id,
                decision,
                view,
            });
            self.session.update(cx, |session, cx| session.load_more(cx));
        }
        true
    }

    /// The page a review waited for has arrived (or failed to).
    pub(super) fn continue_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        if session.loading || session.window_loading() {
            return;
        }
        let Some(Pending {
            ticket,
            id,
            decision,
            view,
        }) = self.pending_review.take()
        else {
            return;
        };
        let items = self.listed_ids(cx);
        let loaded = self.reviewer.following(&items, id).flatten();
        if loaded.is_none() && self.session.read(cx).has_following() {
            // The next items did not load: stay on the item, unsaved, so
            // the review can be tried again rather than walking backwards.
            self.reviewer.abandon(ticket, id);
            crate::ui::notify::error(
                review_failure(decision),
                t("The next items could not be loaded."),
                cx,
            );
            return;
        }
        // Only while the view is still on the reviewed item.
        if self.shown_in(view, cx) == Some(id) {
            let destination = loaded.or_else(|| self.reviewer.preceding(&items, id));
            self.move_review_on(ticket, view, destination, window, cx);
        }
        self.save_review(ticket, id, decision, window, cx);
    }

    /// `moveOn`: shows the item the review lands on, or closes the view
    /// when none is left, leaving nothing selected.
    fn move_review_on(
        &mut self,
        ticket: u64,
        view: View,
        destination: Option<RootId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.reviewer.move_on(ticket, destination) {
            return;
        }
        self.show_for_review(view, destination, window, cx);
        if destination.is_none() {
            self.session
                .update(cx, |session, cx| session.clear_selection(cx));
        }
    }

    /// `showReviewItem`: shows and selects `item` in `view`, opening the
    /// view again if the last item closed it, or closes `view` for none.
    fn show_for_review(
        &mut self,
        view: View,
        item: Option<RootId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ordinal = item.and_then(|id| self.session.read(cx).position_of(id));
        match (ordinal, view) {
            (Some(ordinal), View::Detail) => self.session.update(cx, |session, cx| {
                session.select(ordinal, cx);
                session.open_viewer(cx);
            }),
            (Some(ordinal), View::QuickLook) => {
                self.session
                    .update(cx, |session, cx| session.select(ordinal, cx));
                self.quick_look
                    .update(cx, |quick_look, cx| quick_look.open(window, cx));
            }
            (None, View::Detail) => self
                .session
                .update(cx, |session, cx| session.close_viewer(cx)),
            (None, View::QuickLook) => self
                .quick_look
                .update(cx, |quick_look, cx| quick_look.close(window, cx)),
        }
    }

    /// Saves the review of `id` through the session's lifecycle edit, or,
    /// for a reject that deletes, through the item menu's permanent delete.
    fn save_review(
        &mut self,
        ticket: u64,
        id: RootId,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = SelectionTarget::Explicit { root_ids: vec![id] };
        let lifecycle = match decision {
            Decision::Accept => Lifecycle::Active,
            Decision::Reject if crate::settings::current(cx).reject_action == "delete" => {
                self.reject_by_deleting(
                    target,
                    std::rc::Rc::new(move |this, committed, window, cx| {
                        this.review_saved(ticket, id, committed, window, cx)
                    }),
                    window,
                    cx,
                );
                return;
            }
            Decision::Reject => Lifecycle::Trash,
        };
        // The session reports a failure.
        crate::session::edit_then_in(
            &self.session,
            crate::session::Report::failure(review_failure(decision)),
            crate::session::Reread::RESULTS,
            move |library| library.set_lifecycle(&target, lifecycle).map(|_| ()),
            window,
            cx,
            move |this, saved, window, cx| this.review_saved(ticket, id, saved.is_ok(), window, cx),
        );
    }

    /// The save is over: a review that did not happen takes the view back
    /// to the item, while the view is still where it left it.
    fn review_saved(
        &mut self,
        ticket: u64,
        id: RootId,
        committed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let listed = self.listed_ids(cx).contains(&id);
        let view = self.review_view(cx);
        // The view the review left; a closed one is the one it closed.
        let shown = self.shown_in(view, cx);
        if let Some(back) = self.reviewer.saved(ticket, id, committed, listed, shown) {
            self.show_for_review(view, Some(back), window, cx);
        }
        cx.notify();
    }

    /// A key pressed while a review button has the focus shows its focus
    /// ring, as the keyboard makes `:focus-visible` match.
    pub(super) fn review_keystroke(&mut self, event: &KeystrokeEvent, cx: &mut Context<Self>) {
        self.review_keys.after(&event.keystroke.key);
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control {
            return;
        }
        if let Some((_, shown)) = &mut self.review_focus
            && !*shown
        {
            *shown = true;
            cx.notify();
        }
    }

    /// Before a keystroke reaches the window's bindings.
    pub(super) fn review_keystroke_coming(&mut self, event: &KeystrokeEvent) {
        self.review_keys.before(&event.keystroke.key);
    }

    /// The list changed: saved items that left it stop being skipped, and
    /// a review waiting for the next page goes on.
    pub(super) fn review_items_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.listed_ids(cx);
        self.reviewer.items_changed(&items);
        if self.pending_review.is_some() {
            self.continue_review(window, cx);
        }
    }

    /// The pill, while the item being viewed is in the Inbox.
    pub(super) fn review_controls(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.review_flash.is_some_and(|flash| flash.over()) {
            self.review_flash = None;
        }
        if self.review_item(cx).is_none() {
            // The buttons are gone, and their focus with them.
            self.review_focus = None;
            return None;
        }
        let paused = self.asking();
        let ring = self
            .review_focus
            .and_then(|(decision, shown)| shown.then_some(decision));
        let shell = cx.entity().downgrade();
        let blur = shell.clone();
        Some(
            review::controls(paused, self.review_flash, move |decision, window, cx| {
                let _ = shell.update(cx, |this, cx| {
                    // A clicked button takes the focus.
                    this.review_focus = Some((decision, false));
                    this.press_review(decision, window, cx)
                });
            })
            .focus_ring(ring)
            .on_blur(move |_, cx| {
                let _ = blur.update(cx, |this, cx| {
                    if this.review_focus.take().is_some() {
                        cx.notify();
                    }
                });
            })
            .into_any_element(),
        )
    }
}

/// `reportFailure`'s title for a review that could not be saved.
fn review_failure(decision: Decision) -> &'static str {
    match decision {
        Decision::Accept => t("Unable to accept item"),
        Decision::Reject => t("Unable to reject item"),
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewKeys;

    #[test]
    fn a_held_review_key_reviews_once() {
        let mut keys = ReviewKeys::default();
        keys.before("z");
        assert!(!keys.repeat());
        keys.after("z");
        // The key repeats while held.
        keys.before("z");
        assert!(keys.repeat());
        keys.after("z");
        // Another key ends the hold.
        keys.before("x");
        assert!(!keys.repeat());
        keys.after("x");
        keys.released();
        keys.before("x");
        assert!(!keys.repeat());
        // A key that reviews nothing holds nothing.
        keys.after("x");
        keys.released();
        keys.before("right");
        keys.after("right");
        keys.before("z");
        assert!(!keys.repeat());
    }
}
