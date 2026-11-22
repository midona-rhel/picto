//! The Inbox review in the media view and in Quick Look. A pill with an
//! Accept button and a Reject button is shown while the item being viewed is
//! in the Inbox. A click on a button, or the button's shortcut, shows the
//! next item at once and saves the review through the session in the
//! background. When the settings say that rejected items are deleted, a
//! reject goes through the item menu's permanent delete, which asks for
//! confirmation first when the deletion cannot be undone.
//!
//! `Pending` is a review that is waiting for the next page of items to load.
//! Its item is the last item that is loaded but not the last item in the
//! list, so the item that follows it is not known yet.
use super::Shell;
use crate::i18n::t;
use crate::viewer::review::{self, Decision, Flash, View};
use gpui::*;
use picto_library::selection::SelectionTarget;
use picto_library::{Lifecycle, RootId};
use std::time::Instant;

/// Tells the first press of a review key from the repeats that the system
/// sends while the key is held down. A repeat reviews nothing. GPUI does not
/// tell an action whether its key press is a repeat, so the shell reports
/// each key press here before and after it is dispatched: a review key
/// counts as held from its press until it is released or another key is
/// pressed.
#[derive(Default)]
pub(super) struct ReviewKeys {
    /// The review key that is held down, if one is.
    held: Option<String>,
    /// True when a review key's action ran for the key press that is being
    /// dispatched.
    pressed: bool,
}

impl ReviewKeys {
    /// Called before a key press is dispatched. A press of a key other than
    /// the held review key ends the hold.
    fn before(&mut self, key: &str) {
        if self.held.as_deref() != Some(key) {
            self.held = None;
        }
    }

    /// Called by the review action. Returns true when the key press that
    /// ran the action is a repeat of the held key. Otherwise it records, for
    /// [`Self::after`], that a review key was pressed, and returns false.
    fn repeat(&mut self) -> bool {
        if self.held.is_some() {
            return true;
        }
        self.pressed = true;
        false
    }

    /// Called after a key press was dispatched. If the key press ran a
    /// review action, the key counts as held from now on.
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
    /// Returns whether the media view or Quick Look is open.
    pub(super) fn viewing(&self, cx: &App) -> bool {
        self.session.read(cx).viewer || self.quick_look.read(cx).is_open()
    }

    /// Returns the item that the media view or Quick Look shows, if that
    /// item is in the Inbox.
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

    /// Returns the item that `view` shows, or `None` while `view` is closed.
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

    /// Handles a click on the Accept or the Reject button, and the button's
    /// key. It reviews the item being viewed, and starts the button's flash
    /// if the review was taken.
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

    /// Handles a review shortcut while the media view or Quick Look is
    /// open. A key that is held down reviews one item, not one item for each
    /// repeat.
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

    /// Reviews `id` with `decision`. It shows the next item and saves the
    /// review. When the next item is not loaded yet, it loads the next page
    /// of items first, and `continue_review` does the rest. Returns false
    /// when the review is not taken: while a confirmation dialog is open,
    /// and when the item is already reviewed.
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
            // The next item is not loaded: it is in the items after the
            // loaded ones. Those are loaded before the review is saved, so
            // that the reviewed item is still in the list when the item
            // after it is looked up (the items loaded next start one page
            // before the reviewed item).
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

    /// Continues the review that was waiting for the next page of items.
    /// The session has stopped loading by then: the page has arrived, or it
    /// could not be loaded.
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
            // The next items did not load. The view stays on the item and
            // the review is not saved, so the user can try the review
            // again. Going on would show the item before this one.
            self.reviewer.abandon(ticket, id);
            crate::ui::notify::error(
                review_failure(decision),
                t("The next items could not be loaded."),
                cx,
            );
            return;
        }
        // The view moves on only if it still shows the reviewed item.
        if self.shown_in(view, cx) == Some(id) {
            let destination = loaded.or_else(|| self.reviewer.preceding(&items, id));
            self.move_review_on(ticket, view, destination, window, cx);
        }
        self.save_review(ticket, id, decision, window, cx);
    }

    /// Shows `destination` in `view`, which is the item to view after the
    /// reviewed one. When `destination` is `None`, no item is left to
    /// review: the view is closed and the selection is cleared. It does
    /// nothing unless `ticket` is the latest review and has not moved the
    /// view yet (`Reviewer::move_on`).
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
        // After a trackpad flick the system keeps sending scroll events.
        // They are suppressed, so that they do not pan or zoom the next
        // item.
        crate::ui::momentum::suppress();
        self.show_for_review(view, destination, window, cx);
        if destination.is_none() {
            self.session
                .update(cx, |session, cx| session.clear_selection(cx));
        }
    }

    /// Shows and selects `item` in `view`, and opens the view again if it
    /// was closed. When `item` is `None`, or is not in the session's list of
    /// items, it closes `view`.
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
        // When the edit fails, the session shows a notification with this
        // title.
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

    /// Called when the save of the review `ticket` of `id` is over.
    /// `committed` says whether the review was saved. When it was not
    /// saved, and the view still shows what the review moved it to, the
    /// view goes back to the item.
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
        // `shown` is what the view shows now. It is `None` while the view
        // is closed, which is how a review of the last item leaves it.
        let shown = self.shown_in(view, cx);
        if let Some(back) = self.reviewer.saved(ticket, id, committed, listed, shown) {
            self.show_for_review(view, Some(back), window, cx);
        }
        cx.notify();
    }

    /// Called after each key press is dispatched. It reports the key to
    /// `review_keys`. When a review button got the keyboard focus from a
    /// click, a key press makes the button's focus ring show, unless Command
    /// or Control is held.
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

    /// Called before a key press reaches the window's key bindings. It
    /// reports the key to `review_keys`.
    pub(super) fn review_keystroke_coming(&mut self, event: &KeystrokeEvent) {
        self.review_keys.before(&event.keystroke.key);
    }

    /// Called when the session's list of items has changed. It tells the
    /// reviewer, which stops skipping the saved items that have left the
    /// list, and it continues a review that was waiting for the next page.
    pub(super) fn review_items_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.listed_ids(cx);
        self.reviewer.items_changed(&items);
        if self.pending_review.is_some() {
            self.continue_review(window, cx);
        }
    }

    /// Returns the review pill while the item being viewed is in the Inbox,
    /// and `None` otherwise.
    pub(super) fn review_controls(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.review_flash.is_some_and(|flash| flash.over()) {
            self.review_flash = None;
        }
        if self.review_item(cx).is_none() {
            // The buttons are not drawn, so neither of them has the
            // keyboard focus any more.
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
                    // A clicked button takes the keyboard focus. Its focus
                    // ring is not shown until a key is pressed.
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

/// Returns the title of the notification that says a review could not be
/// saved.
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
        // A key that runs no review action does not count as held.
        keys.after("x");
        keys.released();
        keys.before("right");
        keys.after("right");
        keys.before("z");
        assert!(!keys.repeat());
    }
}
