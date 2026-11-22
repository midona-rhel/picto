//! Deletes items permanently. This file has the confirmation dialog, which
//! says whether the deletion can be undone, the Empty Trash command, and the
//! call to the library that deletes the items.
use super::item_menu::{Run, Target};
use crate::i18n::format::grouped;
use crate::i18n::t;
use crate::session::{Report, Reread, edit_then_in, read_then_in};
use crate::shell::Shell;
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::selection::SelectionTarget;
use std::rc::Rc;

/// A permanent deletion that is ready to run: what it deletes, and the title
/// and the button label of its confirmation dialog, which are used again if
/// the user has to confirm a second time.
#[derive(Clone)]
struct Deletion {
    selection: SelectionTarget,
    /// True when the items are deleted because the user rejected them in
    /// the Inbox, and false for Delete Permanently and Empty Trash.
    reject: bool,
    title: String,
    label: &'static str,
    /// Called once the deletion has been carried out, cancelled or failed,
    /// with whether it was carried out.
    done: Option<Done>,
}

/// A function that is told whether a deletion was carried out. It is told
/// false when the user cancelled the deletion and when the deletion failed.
pub(super) type Done = Rc<dyn Fn(&mut Shell, bool, &mut Window, &mut Context<Shell>)>;

/// Returns the function that the confirmation dialog runs when the user
/// cancels. It calls `done` with false. Returns `None` when `done` is `None`.
fn cancelled(done: &Option<Done>) -> Option<Run> {
    let done = done.clone()?;
    Some(Rc::new(move |this, window, cx| {
        done(this, false, window, cx)
    }))
}

/// Returns the message of the confirmation dialog for a deletion of `media`
/// media items. It says that the deletion cannot be undone when `undo_limit`
/// is 0 or `media` is greater than `undo_limit`, and otherwise that it can be
/// undone until Picto closes.
fn permanent_delete_message(media: u128, undo_limit: i64) -> String {
    let limit = grouped(undo_limit.max(0) as u64);
    match media {
        _ if undo_limit == 0 => t("Delete undo is disabled. This cannot be undone.").to_owned(),
        count if count <= undo_limit as u128 => {
            t("You can undo this until Picto closes. After that the files are removed.").to_owned()
        }
        _ => crate::tf!(
            "This exceeds the undo limit of {value0} media items, including collection members, so it cannot be undone.",
            limit
        ),
    }
}

impl Shell {
    /// Asks the user to confirm the permanent deletion of `target`, in a
    /// dialog that says whether the deletion can be undone, and then deletes
    /// through the library. When `reject` is true, the items were rejected
    /// and the settings say that rejected items are deleted; the user is
    /// then asked only if the deletion cannot be undone.
    ///
    /// Only the library that is open in this window is changed
    /// (`libraries::writable`), whether it is a disposable copy or a library
    /// the user keeps. The library deletes the items itself
    /// (`Library::permanently_delete` and `Library::delete_rejected`). It
    /// keeps the files of a deletion that can be undone until that deletion
    /// leaves the history. The files of a deletion that cannot be undone
    /// are removed by the application's media worker. Nothing in this file
    /// removes a file.
    pub(super) fn request_permanent_delete(
        &mut self,
        target: Target,
        reject: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_deletion(target.selection(), reject, false, None, window, cx);
    }

    /// Deletes `selection` because its items were rejected and the settings
    /// say that rejected items are deleted. The user is asked to confirm
    /// only when the deletion cannot be undone. `done` is called with
    /// whether the deletion was carried out.
    pub(super) fn reject_by_deleting(
        &mut self,
        selection: SelectionTarget,
        done: Done,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_deletion(selection, true, false, Some(done), window, cx);
    }

    /// Carries out the sidebar's Empty Trash command. It deletes everything
    /// in the Trash after the same confirmation dialog, whose title is
    /// "Empty Trash" and whose button is "Delete All".
    pub(super) fn empty_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection = SelectionTarget::Query {
            query: picto_library::query::RootQuery {
                scope: ItemScope::Trash,
                view: Default::default(),
            },
            excluded_root_ids: Vec::new(),
        };
        self.request_deletion(selection, false, true, None, window, cx);
    }

    fn request_deletion(
        &mut self,
        selection: SelectionTarget,
        reject: bool,
        empty_trash: bool,
        done: Option<Done>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let undo_limit = crate::settings::current(cx).undo_delete_limit;
        let read = selection.clone();
        read_then_in(
            &self.session,
            move |library| library.selection_summary(&read),
            window,
            cx,
            move |this, summary, window, cx| {
                let Ok(summary) = summary else {
                    return;
                };
                let count = summary.selected_count;
                if count == 0 {
                    if let Some(done) = &done {
                        done(this, false, window, cx);
                    }
                    return;
                }
                let undoable = undo_limit > 0 && summary.media_count <= undo_limit as u128;
                let (title, label) = if empty_trash {
                    (t("Empty Trash").to_owned(), t("Delete All"))
                } else if count == 1 {
                    (t("Delete permanently?").to_owned(), t("Delete Permanently"))
                } else {
                    (
                        crate::tf!("Delete {value0} items permanently?", grouped(count)),
                        t("Delete Permanently"),
                    )
                };
                let delete = Deletion {
                    selection: selection.clone(),
                    reject,
                    title,
                    label,
                    done: done.clone(),
                };
                if reject && undoable {
                    this.delete_permanently(delete, undoable, window, cx);
                    return;
                }
                let message = permanent_delete_message(summary.media_count, undo_limit);
                this.ask_with(
                    delete.title.clone(),
                    message,
                    label,
                    true,
                    Rc::new(move |this, window, cx| {
                        this.delete_permanently(delete.clone(), undoable, window, cx)
                    }),
                    cancelled(&done),
                    cx,
                );
            },
        );
    }

    fn delete_permanently(
        &mut self,
        delete: Deletion,
        require_undoable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Items are deleted only from the library that is open in this
        // window (`Session::writable_root`). Without one, an error is shown.
        if self.session.read(cx).writable_root(cx).is_none() {
            crate::ui::notify::error(
                t("Delete Permanently"),
                crate::i18n::en("No library is open."),
                cx,
            );
            if let Some(done) = &delete.done {
                done(self, false, window, cx);
            }
            return;
        }
        let undo_limit = crate::settings::current(cx).undo_delete_limit.max(0) as usize;
        let (selection, reject) = (delete.selection.clone(), delete.reject);
        // The viewer is closed when the item it shows is among the deleted
        // items, and always when the selection is a query. After an Inbox
        // review the viewer already shows another item, so it stays open.
        let shows_deleted = {
            let session = self.session.read(cx);
            match &selection {
                SelectionTarget::Explicit { root_ids } => session
                    .selected_item()
                    .is_some_and(|item| root_ids.contains(&item.root_id)),
                _ => true,
            }
        };
        if shows_deleted {
            self.session
                .update(cx, |session, cx| session.close_viewer(cx));
        }
        // A notification with an Undo button reports the deletion
        // (`Report::announce`). The files that are no longer needed after a
        // deletion that cannot be undone are removed from the library's
        // blobs by the application's media worker (`drain_blob_cleanup`).
        let command = if reject {
            "items.reject"
        } else {
            "items.delete"
        };
        let work = move |library: &picto_library::Library| {
            let now = crate::now_ms();
            if reject {
                library.delete_rejected(&selection, now, undo_limit, require_undoable)
            } else {
                library.permanently_delete(&selection, now, undo_limit, require_undoable)
            }
        };
        let report = Report::announce(command);
        edit_then_in(
            &self.session,
            report,
            Reread::RESULTS,
            work,
            window,
            cx,
            move |this, deleted, window, cx| match deleted {
                Ok(_) => {
                    if let Some(done) = &delete.done {
                        done(this, true, window, cx);
                    }
                }
                // The dialog said that this deletion can be undone, but the
                // library found that it cannot: the selection or the limit
                // has changed since the dialog was built. The user is asked
                // again before anything is deleted without undo.
                Err(picto_library::LibraryError::UndoLimitExceeded) if require_undoable => {
                    let cancel = cancelled(&delete.done);
                    this.ask_with(
                        delete.title.clone(),
                        t("This delete can no longer be undone. Delete permanently anyway?"),
                        delete.label,
                        true,
                        Rc::new(move |this, window, cx| {
                            this.delete_permanently(delete.clone(), false, window, cx)
                        }),
                        cancel,
                        cx,
                    );
                }
                Err(error) => {
                    crate::ui::notify::error(t("Delete Permanently"), error.to_string(), cx);
                    if let Some(done) = &delete.done {
                        done(this, false, window, cx);
                    }
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::permanent_delete_message;

    #[test]
    fn delete_messages_say_whether_undo_is_possible() {
        assert_eq!(
            permanent_delete_message(3, 0),
            "Delete undo is disabled. This cannot be undone."
        );
        assert_eq!(
            permanent_delete_message(500, 500),
            "You can undo this until Picto closes. After that the files are removed."
        );
        assert_eq!(
            permanent_delete_message(1500, 1000),
            "This exceeds the undo limit of 1,000 media items, including collection members, so it cannot be undone."
        );
    }
}
