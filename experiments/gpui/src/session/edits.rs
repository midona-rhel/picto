//! The viewer, and library edits: run on the library's thread, reported
//! as production reports them, and followed by the reads they need.
use super::{Change, Session, Visit};
use crate::backend;
use gpui::*;
use picto_library::RootId;

impl Session {
    // ── Viewer ──────────────────────────────────────────────────

    /// Opens the selected item. It is a place of its own: Back returns to
    /// the grid it was opened from, where that was scrolled to.
    pub fn open_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer && self.focus.is_some() {
            self.viewer = true;
            self.record(Visit {
                scope: self.scope,
                viewing: true,
                place: None,
            });
            cx.emit(Change::Viewer);
            cx.emit(Change::Navigation);
        }
    }

    /// Returns to the grid. Closing an item opened from this grid is the
    /// same as going back from it.
    pub fn close_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer {
            return;
        }
        self.viewer = false;
        let grid = Visit {
            scope: self.scope,
            viewing: false,
            place: None,
        };
        if self.history_index > 0 && self.history[self.history_index - 1] == grid {
            self.history_index -= 1;
        } else if self.history[self.history_index] != grid {
            self.record(grid);
        }
        cx.emit(Change::Viewer);
        cx.emit(Change::Navigation);
    }

    pub fn step_member(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self
            .details
            .as_ref()
            .map_or(1, |details| details.media.len());
        self.member = self
            .member
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        cx.emit(Change::Viewer);
    }

    // ── Edits, each through the library's own owner ─────────────

    /// Runs an edit on the library's thread, reports a failure on the
    /// console, and reads again what `reread` names, whether or not the edit
    /// went through. Hands back the edit's answer once the reads are asked
    /// for, so a view's own follow-up comes after them. What was shown stays
    /// until each read arrives. The sidebar's destinations and counts follow
    /// the library's change events by themselves. Production tells the user
    /// of a failed edit only where the call says so; such a call uses
    /// [`Session::edit_reporting`].
    pub fn edit<T: Send + 'static, E: std::fmt::Display + Send + 'static>(
        &mut self,
        work: impl FnOnce(&picto_library::Library) -> Result<T, E> + Send + 'static,
        reread: Reread,
        cx: &mut Context<Self>,
    ) -> futures::channel::oneshot::Receiver<Result<T, E>> {
        self.edit_reporting(Report::default(), work, reread, cx)
    }

    /// [`Session::edit`], telling the user what `report` says: a failure as
    /// an error notification with its title, and a broad change as the
    /// notification with Undo that production announces it with.
    pub fn edit_reporting<T: Send + 'static, E: std::fmt::Display + Send + 'static>(
        &mut self,
        report: Report,
        work: impl FnOnce(&picto_library::Library) -> Result<T, E> + Send + 'static,
        reread: Reread,
        cx: &mut Context<Self>,
    ) -> futures::channel::oneshot::Receiver<Result<T, E>> {
        let (answer, receiver) = futures::channel::oneshot::channel();
        let announce = report.announce;
        let job = self.run(move |library| {
            let before = announce.map(|_| crate::history::newest(library));
            let result = work(library);
            let announced = match (announce, &result) {
                (Some(command), Ok(_)) => {
                    crate::history::announced(library, command, before.flatten())
                }
                _ => None,
            };
            let navigation = reread
                .navigation
                .then(|| backend::navigation(library).ok())
                .flatten();
            (result, navigation, announced)
        });
        cx.spawn(async move |session, cx| {
            let Ok((result, navigation, announced)) = job.await else {
                return;
            };
            let _ = session.update(cx, |this, cx| {
                if let Some((nav, tags)) = navigation {
                    this.replace_navigation(nav, tags, cx);
                }
                if let Err(error) = &result {
                    let message = error.to_string();
                    match &report.failure {
                        Some(title) if report.shows(&message) => {
                            crate::ui::notify::error(title.clone(), message, cx)
                        }
                        // Production leaves it to the console.
                        _ => eprintln!("picto: {message}"),
                    }
                }
                if let Some(entry) = &announced {
                    crate::history::announce(entry, cx.entity(), cx);
                }
                this.reread(reread, cx);
            });
            let _ = answer.send(result);
        })
        .detach();
        receiver
    }

    /// Reads again what an edit may have changed, keeping what is shown
    /// until each answer arrives.
    pub fn reread(&mut self, reread: Reread, cx: &mut Context<Self>) {
        if reread.details {
            self.refresh_details(cx);
        }
        if reread.summary {
            cx.emit(Change::Edited);
        }
        if reread.results {
            self.refresh_results(cx);
        }
    }

    /// Moves `target` to the trash, the Inbox or the library, and reads the
    /// destination again in place. The answer says whether it went through.
    pub fn set_lifecycle(
        &mut self,
        target: picto_library::selection::SelectionTarget,
        lifecycle: picto_library::Lifecycle,
        cx: &mut Context<Self>,
    ) -> futures::channel::oneshot::Receiver<Result<(), picto_library::LibraryError>> {
        self.edit(
            move |library| library.set_lifecycle(&target, lifecycle).map(|_| ()),
            Reread::RESULTS,
            cx,
        )
    }

    /// Fetches the selected item's details again, after an edit changed
    /// them.
    pub fn refresh_details(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_item().map(|item| item.root_id) {
            self.read_details(id, cx);
        }
    }

    /// Reads an item's details, which are shown if it is still the one
    /// selected when they arrive.
    pub(super) fn read_details(&mut self, id: RootId, cx: &mut Context<Self>) {
        let answer = self.run(move |library| library.details(id));
        cx.spawn(async move |session, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = session.update(cx, |this, cx| match result {
                Ok(details) if this.selected_item().is_some_and(|item| item.root_id == id) => {
                    let name = details.root.name.clone();
                    if let Some(focused) = &mut this.focus {
                        focused.item.name = name.clone();
                    }
                    if let Some(item) = this.items.iter_mut().find(|item| item.root_id == id) {
                        item.name = name;
                    }
                    this.details = Some(details);
                    cx.emit(Change::Details);
                }
                Ok(_) => {}
                Err(error) => eprintln!("picto: {error}"),
            });
        })
        .detach();
    }

    /// Undoes the library's last edit, whatever is selected (`edit.undo` is
    /// app-wide): an undone move to the trash brings back an item that is no
    /// longer in the grid, and an undone folder edit changes the sidebar. So
    /// the results, the selection and the destinations are read again.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.step_history_entry(true, cx);
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.step_history_entry(false, cx);
    }

    /// `historyRuntime.perform`: undoes or redoes the library's newest
    /// change, and says what it did ("Undone — …") or why it could not
    /// ("Could not undo").
    pub fn step_history_entry(&mut self, undo: bool, cx: &mut Context<Self>) {
        let failure = if undo {
            "Could not undo"
        } else {
            "Could not redo"
        };
        let answer = self.edit_reporting(
            Report::failure(failure),
            move |library| crate::history::step(library, undo),
            Reread::EVERYTHING,
            cx,
        );
        cx.spawn(async move |session, cx| {
            if let Ok(Ok(stepped)) = answer.await {
                let _ = session.update(cx, |_, cx| {
                    crate::history::show_step(&stepped, undo, cx.entity(), cx)
                });
            }
        })
        .detach();
    }
}

/// A view's library edit and what the view does once it is done: `work`
/// runs as [`Session::edit_reporting`] runs it (a failure reported under
/// `report`'s title, a broad change announced with Undo, `reread` read
/// again), then `then` has its answer on the view, if the view and the
/// session are still there. A view that needs its window afterwards uses
/// [`edit_then_in`]; an edit nothing follows calls `edit_reporting` itself.
pub fn edit_then<V: 'static, T: Send + 'static, E: std::fmt::Display + Send + 'static>(
    session: &Entity<Session>,
    report: Report,
    reread: Reread,
    work: impl FnOnce(&picto_library::Library) -> Result<T, E> + Send + 'static,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Result<T, E>, &mut Context<V>) + 'static,
) {
    let answer = session.update(cx, |session, cx| {
        session.edit_reporting(report, work, reread, cx)
    });
    then_on(answer, cx, then);
}

/// Hands `answer` to `then` on the view once it arrives, if the view is
/// still there; nothing happens when no answer comes.
fn then_on<V: 'static, R: 'static>(
    answer: futures::channel::oneshot::Receiver<R>,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, R, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |view, cx| {
        if let Ok(result) = answer.await {
            let _ = view.update(cx, move |view, cx| then(view, result, cx));
        }
    })
    .detach();
}

/// [`edit_then`] for a follow-up that needs the view's window (a field to
/// focus, a dialog to open).
pub fn edit_then_in<V: 'static, T: Send + 'static, E: std::fmt::Display + Send + 'static>(
    session: &Entity<Session>,
    report: Report,
    reread: Reread,
    work: impl FnOnce(&picto_library::Library) -> Result<T, E> + Send + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Result<T, E>, &mut Window, &mut Context<V>) + 'static,
) {
    let answer = session.update(cx, |session, cx| {
        session.edit_reporting(report, work, reread, cx)
    });
    cx.spawn_in(window, async move |view, cx| {
        if let Ok(result) = answer.await {
            let _ = view.update_in(cx, move |view, window, cx| then(view, result, window, cx));
        }
    })
    .detach();
}

/// What [`Session::edit_reporting`] tells the user about an edit, as the
/// production call it stands for does.
#[derive(Clone, Default)]
pub struct Report {
    /// The title of the error notification a failure shows.
    pub failure: Option<SharedString>,
    /// Only a folder nested too deep is reported (`warnOnFolderDepth`),
    /// as the warning every error about it becomes.
    pub folder_depth: bool,
    /// The history command a broad change records, which is announced with
    /// an Undo button (`announceUndoableMutation`).
    pub announce: Option<&'static str>,
}

impl Report {
    pub fn failure(title: impl Into<SharedString>) -> Self {
        Self {
            failure: Some(title.into()),
            ..Self::default()
        }
    }

    pub fn folder_depth() -> Self {
        Self {
            failure: Some("Folder depth limit".into()),
            folder_depth: true,
            ..Self::default()
        }
    }

    pub fn announce(command: &'static str) -> Self {
        Self {
            announce: Some(command),
            ..Self::default()
        }
    }

    /// Whether a failure with `message` is shown.
    fn shows(&self, message: &str) -> bool {
        self.failure.is_some()
            && (!self.folder_depth || message.contains(crate::ui::notify::FOLDER_DEPTH_LIMIT))
    }

    pub fn and_announce(mut self, command: &'static str) -> Self {
        self.announce = Some(command);
        self
    }
}

/// What an edit may have changed, and so what [`Session::edit`] reads again
/// once it is done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reread {
    /// The selected item's details.
    pub details: bool,
    /// The summary the inspector shows of several selected items
    /// ([`Change::Edited`]).
    pub summary: bool,
    /// The grid's results, read in place: the same items stay selected and
    /// the grid where it was scrolled ([`Session::refresh_results`]).
    pub results: bool,
    /// The sidebar's destinations, read with the edit and put in place
    /// before its answer is handed back, for a follow-up that needs them at
    /// once, such as a new folder renamed in place.
    pub navigation: bool,
}

impl Reread {
    pub const NOTHING: Self = Self {
        details: false,
        summary: false,
        results: false,
        navigation: false,
    };
    pub const DETAILS: Self = Self {
        details: true,
        ..Self::NOTHING
    };
    /// The selected items: the one shown and the summary of them all.
    pub const SELECTION: Self = Self {
        details: true,
        summary: true,
        ..Self::NOTHING
    };
    /// The selected items, and the results they may have left or joined.
    pub const RESULTS: Self = Self {
        results: true,
        ..Self::SELECTION
    };
    /// The results alone, for an edit of what the destination holds rather
    /// than of its items.
    pub const RESULTS_ONLY: Self = Self {
        results: true,
        ..Self::NOTHING
    };
    pub const NAVIGATION: Self = Self {
        navigation: true,
        ..Self::NOTHING
    };
    /// Whatever an edit could have touched, for undo and redo.
    pub const EVERYTHING: Self = Self {
        navigation: true,
        ..Self::RESULTS
    };
}

#[cfg(test)]
mod tests {
    use super::then_on;

    #[test]
    fn an_answer_reaches_the_view_only_while_it_is_there() {
        use futures::channel::oneshot;
        use gpui::AppContext as _;
        struct View(Vec<u32>);
        let mut cx = gpui::TestAppContext::single();
        let view = cx.new(|_| View(Vec::new()));
        // The answer arrives: the view has it.
        let (send, answer) = oneshot::channel();
        view.update(&mut cx, |_, cx| {
            then_on(answer, cx, |view: &mut View, value, _| view.0.push(value))
        });
        send.send(1).unwrap();
        cx.run_until_parked();
        assert_eq!(view.read_with(&cx, |view, _| view.0.clone()), [1]);
        // No answer comes (the session is gone): nothing happens.
        let (send, answer) = oneshot::channel::<u32>();
        view.update(&mut cx, |_, cx| {
            then_on(answer, cx, |view: &mut View, value, _| view.0.push(value))
        });
        drop(send);
        cx.run_until_parked();
        assert_eq!(view.read_with(&cx, |view, _| view.0.clone()), [1]);
        // The view is gone by the time the answer comes.
        let (send, answer) = oneshot::channel::<u32>();
        let gone = cx.new(|_| View(Vec::new()));
        let weak = gone.downgrade();
        gone.update(&mut cx, |_, cx| {
            then_on(answer, cx, |_: &mut View, _, _| panic!("the view is gone"))
        });
        drop(gone);
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
        let _ = send.send(2);
        cx.run_until_parked();
    }
}
