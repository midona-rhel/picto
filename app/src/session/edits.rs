//! The part of `Session` that opens and closes the viewer and runs edits of
//! the library. An edit runs on the library's thread. When it has finished,
//! the session reports a failure and reads again what the edit may have
//! changed. This file also holds the functions with which a view runs an
//! edit or a read and continues when the answer has arrived.
use super::{Change, Session, Visit};
use crate::backend;
use crate::i18n::t;
use gpui::*;
use picto_library::RootId;

impl Session {
    // The methods below open and close the viewer.

    /// Opens the viewer on the item the inspector shows and adds a Back and
    /// Forward history entry for it. Back then closes the viewer and shows
    /// the grid again at its scroll position. Does nothing when the viewer
    /// is already open or the inspector shows no item.
    pub fn open_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer && self.focus.is_some() {
            self.viewer = true;
            let filters = self.filters_step();
            self.record(Visit {
                scope: self.scope,
                viewing: true,
                place: None,
                filters,
            });
            cx.emit(Change::Viewer);
            cx.emit(Change::Navigation);
        }
    }

    /// Closes the viewer and shows the grid. When the history entry before
    /// the current one is this grid, which it is when the viewer was opened
    /// from it, the history goes back to that entry, so closing the viewer
    /// has the same result as Back. Otherwise a history entry for the grid
    /// is added, unless the current entry already is the grid.
    pub fn close_viewer(&mut self, cx: &mut Context<Self>) {
        if !self.viewer {
            return;
        }
        self.viewer = false;
        let grid = Visit {
            scope: self.scope,
            viewing: false,
            place: None,
            filters: self.filters_step(),
        };
        if self.history_index > 0 && self.history[self.history_index - 1] == grid {
            self.history_index -= 1;
        } else if self.history[self.history_index] != grid {
            self.record(grid);
        }
        cx.emit(Change::Viewer);
        cx.emit(Change::Navigation);
    }

    /// Moves the viewer `delta` members along the collection it has open: 1
    /// is the next member and -1 is the previous one. The index stays
    /// between the first member and the last member.
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

    // The methods below run edits of the library and read again what an edit
    // changed.

    /// Runs the edit `work` on the library's thread. When the edit has
    /// finished, whether it succeeded or failed, the session reads again
    /// what `reread` names. The views keep showing what they showed until
    /// each read arrives. A failure is printed to the console and is not
    /// shown to the user; an edit whose failure the user must see uses
    /// [`Session::edit_reporting`]. Returns a receiver for the edit's
    /// result. The result is sent after the reads have been started, so what
    /// a view does with the result happens after them. The sidebar's
    /// destinations and counts need no `reread`: the session reads them
    /// again when the application layer reports the change.
    pub fn edit<T: Send + 'static, E: std::fmt::Display + Send + 'static>(
        &mut self,
        work: impl FnOnce(&picto_library::Library) -> Result<T, E> + Send + 'static,
        reread: Reread,
        cx: &mut Context<Self>,
    ) -> futures::channel::oneshot::Receiver<Result<T, E>> {
        self.edit_reporting(Report::default(), work, reread, cx)
    }

    /// Does what [`Session::edit`] does, and tells the user what `report`
    /// asks for. With `report.failure`, a failed edit is shown as an error
    /// notification with that title. With `report.announce`, a successful
    /// edit that recorded an undo entry of that command is shown as a
    /// notification that describes the change and has an Undo button.
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
                        // A failure that `report` does not ask to show is
                        // only printed to the console.
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

    /// Starts the reads that `reread` names, except the read of the
    /// sidebar's destinations, which `edit_reporting` makes together with
    /// the edit. The views keep showing what they showed until each answer
    /// arrives.
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

    /// Moves the items in `target` to the trash, to the Inbox or to the
    /// library, as `lifecycle` says, and then reads the destination's
    /// results again in place. Returns a receiver for the edit's result.
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

    /// Reads the details of the item the inspector shows again, after an
    /// edit changed them.
    pub fn refresh_details(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_item().map(|item| item.root_id) {
            self.read_details(id, cx);
        }
    }

    /// Reads the details of the item `id` from the library. When they
    /// arrive, they are shown only if that item is still the one the
    /// inspector shows. The item's name in the grid is updated from them.
    pub(super) fn read_details(&mut self, id: RootId, cx: &mut Context<Self>) {
        let answer = self.run(move |library| library.details(id));
        then_on(
            answer,
            cx,
            move |this: &mut Self, result, cx| match result {
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
            },
        );
    }

    /// Undoes the library's newest change. This does not depend on what is
    /// selected or on which destination is open. An undone move to the trash
    /// brings back an item that the grid no longer holds, and an undone
    /// folder edit changes the sidebar, so the results, the selection and
    /// the destinations are all read again.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.step_history_entry(true, cx);
    }

    /// Redoes the change that was undone last, and reads the same things
    /// again as `undo` does.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.step_history_entry(false, cx);
    }

    /// Undoes the library's newest change when `undo` is true, and redoes
    /// the change that was undone last otherwise. On success a notification
    /// titled "Undone" or "Redone" describes the change. On failure an error
    /// notification titled "Could not undo" or "Could not redo" is shown.
    pub fn step_history_entry(&mut self, undo: bool, cx: &mut Context<Self>) {
        let failure = if undo {
            t("Could not undo")
        } else {
            t("Could not redo")
        };
        let answer = self.edit_reporting(
            Report::failure(failure),
            move |library| crate::history::step(library, undo),
            Reread::EVERYTHING,
            cx,
        );
        then_on(answer, cx, move |_: &mut Self, result, cx| {
            if let Ok(stepped) = result {
                crate::history::show_step(&stepped, undo, cx.entity(), cx)
            }
        });
    }
}

/// Runs the edit `work` for a view and then calls `then` on the view with
/// the edit's result. The edit runs as [`Session::edit_reporting`] runs it:
/// `report` decides what the user is told, and `reread` decides what is read
/// again. `then` is not called when the view or the session no longer exists
/// at that time. A view whose `then` needs its window uses [`edit_then_in`].
/// A view that does nothing after the edit calls `edit_reporting` directly.
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

/// Calls `then` on the view with the value that `answer` receives, if the
/// view still exists at that time. `then` is not called when the sender of
/// `answer` is dropped without sending a value.
pub(super) fn then_on<V: 'static, R: 'static>(
    answer: futures::channel::oneshot::Receiver<R>,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, R, &mut Context<V>) + 'static,
) {
    when_answered(answer, cx, |view, result, cx| {
        if let Some(result) = result {
            then(view, result, cx);
        }
    });
}

/// Calls `then` on the view when `answer` receives a value, or when its
/// sender is dropped without sending one, if the view still exists at that
/// time. `then` gets `Some(value)` in the first case and `None` in the
/// second. This is for a caller that must do something in both cases, for
/// example record that a read is no longer running.
pub(super) fn when_answered<V: 'static, R: 'static>(
    answer: futures::channel::oneshot::Receiver<R>,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Option<R>, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |view, cx| {
        let result = answer.await.ok();
        let _ = view.update(cx, move |view, cx| then(view, result, cx));
    })
    .detach();
}

/// Does what [`edit_then`] does, and also gives `then` the view's window,
/// which `then` needs for example to focus a field or to open a dialog.
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

/// Runs the read `work` on the library's thread for a view (see
/// [`Session::run`]) and then calls `then` on the view with its result, if
/// the view still exists at that time. Nothing is reported to the user and
/// nothing is read again afterwards, because a read does not change the
/// library.
pub fn read_then<V: 'static, T: Send + 'static>(
    session: &Entity<Session>,
    work: impl FnOnce(&picto_library::Library) -> T + Send + 'static,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
    let answer = session.read(cx).run(work);
    then_on(answer, cx, then);
}

/// Does what [`read_then`] does, and also gives `then` the view's window.
pub fn read_then_in<V: 'static, T: Send + 'static>(
    session: &Entity<Session>,
    work: impl FnOnce(&picto_library::Library) -> T + Send + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
) {
    let answer = session.read(cx).run(work);
    cx.spawn_in(window, async move |view, cx| {
        if let Ok(result) = answer.await {
            let _ = view.update_in(cx, move |view, window, cx| then(view, result, window, cx));
        }
    })
    .detach();
}

/// Says what [`Session::edit_reporting`] tells the user about an edit.
#[derive(Clone, Default)]
pub struct Report {
    /// The title of the error notification that is shown when the edit
    /// fails. With `None`, a failure is only printed to the console.
    pub failure: Option<SharedString>,
    /// When true, a failure is shown only if it is the library's error
    /// about a folder that would be nested too deep. The notification shows
    /// that error as a warning that explains the limit. Any other failure is
    /// only printed to the console.
    pub folder_depth: bool,
    /// The name of the history command that the edit records. When the edit
    /// succeeds and has recorded an undo entry of this command, a
    /// notification describes the change and has an Undo button.
    pub announce: Option<&'static str>,
}

impl Report {
    /// Returns a report that shows a failed edit as an error notification
    /// titled `title`.
    pub fn failure(title: impl Into<SharedString>) -> Self {
        Self {
            failure: Some(title.into()),
            ..Self::default()
        }
    }

    /// Returns a report that shows a failed edit only if the failure is the
    /// error about a folder that would be nested too deep.
    pub fn folder_depth() -> Self {
        Self {
            failure: Some(t("Folder depth limit").into()),
            folder_depth: true,
            ..Self::default()
        }
    }

    /// Returns a report that announces a successful edit recorded under the
    /// history command `command`, and shows no failure.
    pub fn announce(command: &'static str) -> Self {
        Self {
            announce: Some(command),
            ..Self::default()
        }
    }

    /// Whether a failed edit with the error message `message` is shown to
    /// the user.
    fn shows(&self, message: &str) -> bool {
        self.failure.is_some()
            && (!self.folder_depth || message.contains(crate::ui::notify::FOLDER_DEPTH_LIMIT))
    }

    /// Returns this report with the announcement of a successful edit
    /// recorded under the history command `command` added.
    pub fn and_announce(mut self, command: &'static str) -> Self {
        self.announce = Some(command);
        self
    }
}

/// Says what an edit may have changed. [`Session::edit`] reads these things
/// again when the edit has finished.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reread {
    /// The details of the item the inspector shows.
    pub details: bool,
    /// The inspector's summary of several selected items. The session emits
    /// [`Change::Edited`], and the inspector then reads the summary again.
    pub summary: bool,
    /// The grid's results, refreshed in place: the same items stay selected
    /// and the grid keeps its scroll position
    /// ([`Session::refresh_results`]).
    pub results: bool,
    /// The sidebar's destinations. They are read on the library's thread
    /// directly after the edit, and they replace the session's list before
    /// the edit's result is sent. This is for an edit whose caller needs the
    /// new list at once, such as a new folder whose name the user then edits
    /// in the sidebar.
    pub navigation: bool,
}

impl Reread {
    /// Nothing is read again.
    pub const NOTHING: Self = Self {
        details: false,
        summary: false,
        results: false,
        navigation: false,
    };
    /// Only the details of the item the inspector shows.
    pub const DETAILS: Self = Self {
        details: true,
        ..Self::NOTHING
    };
    /// The details of the item the inspector shows and the summary of all
    /// selected items.
    pub const SELECTION: Self = Self {
        details: true,
        summary: true,
        ..Self::NOTHING
    };
    /// What `SELECTION` reads, and the grid's results, because the edit may
    /// have removed items from the destination or added items to it.
    pub const RESULTS: Self = Self {
        results: true,
        ..Self::SELECTION
    };
    /// Only the grid's results. This is for an edit that changes which items
    /// the destination holds and does not change the items themselves.
    pub const RESULTS_ONLY: Self = Self {
        results: true,
        ..Self::NOTHING
    };
    /// Only the sidebar's destinations.
    pub const NAVIGATION: Self = Self {
        navigation: true,
        ..Self::NOTHING
    };
    /// Everything: what `RESULTS` reads and the sidebar's destinations. Undo
    /// and redo use it, because they can change any of these.
    pub const EVERYTHING: Self = Self {
        navigation: true,
        ..Self::RESULTS
    };
}

