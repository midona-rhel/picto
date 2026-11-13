//! Permanent deletion of items: the confirmation that says whether it can
//! be undone, Empty Trash, and the delete through the library.
use super::item_menu::{Run, Target};
use crate::grid::format::grouped;
use crate::session::Reread;
use crate::shell::Shell;
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::selection::SelectionTarget;
use std::rc::Rc;

/// A permanent delete that was confirmed, and what to ask if it must be
/// confirmed again.
#[derive(Clone)]
struct Deletion {
    selection: SelectionTarget,
    /// Rejected from the Inbox rather than deleted from the Trash.
    reject: bool,
    title: String,
    label: &'static str,
    /// Told whether the delete went through, once it has or has not.
    done: Option<Done>,
}

/// Hears whether a delete went through: false when it was cancelled or
/// failed.
pub(super) type Done = Rc<dyn Fn(&mut Shell, bool, &mut Window, &mut Context<Shell>)>;

/// What tells `done` a delete was turned down.
fn cancelled(done: &Option<Done>) -> Option<Run> {
    let done = done.clone()?;
    Some(Rc::new(move |this, window, cx| {
        done(this, false, window, cx)
    }))
}

/// `permanentDeleteMessage`.
fn permanent_delete_message(media: Option<u128>, undo_limit: i64) -> String {
    let limit = grouped(undo_limit.max(0) as u64);
    match media {
        _ if undo_limit == 0 => "Delete undo is disabled. This cannot be undone.".to_owned(),
        Some(count) if count <= undo_limit as u128 => {
            "You can undo this until Picto closes. After that the files are removed.".to_owned()
        }
        None => format!(
            "Deleting up to {limit} media items, including collection members, can be undone until Picto closes. Larger deletes cannot be undone."
        ),
        Some(_) => format!(
            "This exceeds the undo limit of {limit} media items, including collection members, so it cannot be undone."
        ),
    }
}

impl Shell {
    /// `requestPermanentDelete`: asks first, saying whether the delete can
    /// be undone, and deletes through the library. A rejection (the
    /// setting that skips the Trash) asks only when it cannot be undone.
    ///
    /// Only a marked disposable copy of a library is changed this way. The
    /// library keeps the files of an undoable delete until it leaves the
    /// history; the application removes the files a larger delete frees.
    pub(super) fn request_permanent_delete(
        &mut self,
        target: Target,
        reject: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_deletion(
            target.selection(self.session.read(cx)),
            reject,
            false,
            None,
            window,
            cx,
        );
    }

    /// `rejectTarget` when the setting deletes rejected items: deletes
    /// `selection` as the Reject row does, asking first only when it cannot
    /// be undone, and tells `done` whether it went through.
    pub(super) fn reject_by_deleting(
        &mut self,
        selection: SelectionTarget,
        done: Done,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_deletion(selection, true, false, Some(done), window, cx);
    }

    /// The sidebar's Empty Trash: everything in the Trash, after the same
    /// confirmation, titled "Empty Trash" with "Delete All".
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
        self.library(
            Box::new(move |library| library.selection_summary(&read)),
            move |this, summary, window, cx| {
                let count = summary.selected_count;
                if count == 0 {
                    if let Some(done) = &done {
                        done(this, false, window, cx);
                    }
                    return;
                }
                let undoable = undo_limit > 0 && summary.media_count <= undo_limit as u128;
                let (title, label) = if empty_trash {
                    ("Empty Trash".to_owned(), "Delete All")
                } else if count == 1 {
                    ("Delete permanently?".to_owned(), "Delete Permanently")
                } else {
                    (
                        format!("Delete {} items permanently?", grouped(count)),
                        "Delete Permanently",
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
                let message = permanent_delete_message(Some(summary.media_count), undo_limit);
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
            window,
            cx,
        );
    }

    fn delete_permanently(
        &mut self,
        delete: Deletion,
        require_undoable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.session.read(cx).disposable() {
            eprintln!("picto: refusing to delete from a library that is not a disposable copy");
            if let Some(done) = &delete.done {
                done(self, false, window, cx);
            }
            return;
        }
        let undo_limit = crate::settings::current(cx).undo_delete_limit.max(0) as usize;
        let (selection, reject) = (delete.selection.clone(), delete.reject);
        // The viewer closes when what it shows is deleted (`deleteTarget`);
        // an Inbox review has already moved it on.
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
        let answer = self.session.read(cx).run(move |library| {
            let deleted = if reject {
                library.delete_rejected(&selection, crate::now_ms(), undo_limit, require_undoable)
            } else {
                library.permanently_delete(
                    &selection,
                    crate::now_ms(),
                    undo_limit,
                    require_undoable,
                )
            };
            // The files a delete that cannot be undone frees are removed from
            // the copy's blobs by the application's media worker
            // (`drain_blob_cleanup`), as in production.
            deleted.map(|_| ())
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, move |this, window, cx| match result {
                Ok(()) => {
                    this.session
                        .update(cx, |session, cx| session.reread(Reread::RESULTS, cx));
                    if let Some(done) = &delete.done {
                        done(this, true, window, cx);
                    }
                }
                // The library resolved a different selection or limit: never
                // drop the promise of undo silently; ask again.
                Err(picto_library::LibraryError::UndoLimitExceeded) if require_undoable => {
                    let cancel = cancelled(&delete.done);
                    this.ask_with(
                        delete.title.clone(),
                        "This delete can no longer be undone. Delete permanently anyway?",
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
                    eprintln!("picto: {error}");
                    if let Some(done) = &delete.done {
                        done(this, false, window, cx);
                    }
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::permanent_delete_message;

    #[test]
    fn delete_messages_say_whether_undo_is_possible() {
        assert_eq!(
            permanent_delete_message(Some(3), 0),
            "Delete undo is disabled. This cannot be undone."
        );
        assert_eq!(
            permanent_delete_message(Some(500), 500),
            "You can undo this until Picto closes. After that the files are removed."
        );
        assert_eq!(
            permanent_delete_message(Some(1500), 1000),
            "This exceeds the undo limit of 1,000 media items, including collection members, so it cannot be undone."
        );
        assert!(permanent_delete_message(None, 500).starts_with("Deleting up to 500 media"));
    }
}
