//! Permanent deletion, as `permanentDeleteRequest.ts` asks for it: the
//! selection summary and the `undoDeleteLimit` setting decide whether the
//! delete can be undone, a confirmation says which, and the application
//! deletes (`items.delete`). If the writer finds the delete can no longer be
//! undone, a second confirmation asks again rather than quietly breaking the
//! first one's promise.
//!
//! The app runs on a disposable copy, so the files this removes are the
//! copy's; the library the copy was made from is never touched.
use crate::session::Session;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look,
    modal::{Presence, Size},
};
use gpui::*;
use picto_library::selection::SelectionTarget;
use serde_json::{Value, json};

/// The error the writer gives when a delete promised as undoable is not.
const NOT_UNDOABLE: &str = "operation requires non-undoable confirmation";

/// `permanentDeleteMessage`: what the confirmation says about undo.
pub fn message(count: Option<u128>, undo_limit: u64) -> String {
    let limit = undo_limit as u128;
    if undo_limit == 0 {
        return "Delete undo is disabled. This cannot be undone.".into();
    }
    match count {
        Some(count) if count <= limit => {
            "You can undo this until Picto closes. After that the files are removed.".into()
        }
        None => format!(
            "Deleting up to {} media items, including collection members, can be undone until Picto closes. Larger deletes cannot be undone.",
            crate::grid::format::grouped(undo_limit)
        ),
        Some(_) => format!(
            "This exceeds the undo limit of {} media items, including collection members, so it cannot be undone.",
            crate::grid::format::grouped(undo_limit)
        ),
    }
}

/// The confirmation's title and button.
pub fn wording(selected: u64, empty_trash: bool) -> (String, &'static str) {
    if empty_trash {
        ("Empty Trash".into(), "Delete All")
    } else if selected == 1 {
        ("Delete permanently?".into(), "Delete Permanently")
    } else {
        (
            format!(
                "Delete {} items permanently?",
                crate::grid::format::grouped(selected)
            ),
            "Delete Permanently",
        )
    }
}

struct Request {
    target: Value,
    title: String,
    label: &'static str,
    message: String,
    /// The promise the confirmation made: that the delete can be undone.
    require_undoable: bool,
    presence: Presence,
    busy: bool,
}

/// The one permanent-delete dialog of the main window. The grid's menu and
/// the sidebar's Empty Trash ask it; it draws itself over the whole window.
pub struct PermanentDelete {
    session: Entity<Session>,
    request: Option<Request>,
    focus: FocusHandle,
    /// What had the keyboard before the dialog took it.
    return_focus: Option<FocusHandle>,
    /// The keyboard goes back once the dialog has left.
    refocus: bool,
}

impl PermanentDelete {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            request: None,
            focus: cx.focus_handle(),
            return_focus: None,
            refocus: false,
        }
    }

    /// Asks to delete `target` for good: reads how many media items it
    /// holds and the undo limit, then shows the confirmation.
    pub fn request(
        &mut self,
        target: SelectionTarget,
        empty_trash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.request.is_some() {
            return;
        }
        let Ok(target) = serde_json::to_value(&target) else {
            return;
        };
        self.return_focus = window.focused(cx);
        let (settings, summary) = {
            let session = self.session.read(cx);
            (
                session.dispatch("settings.get", json!({})),
                session.dispatch("items.selection_summary", json!({ "target": target })),
            )
        };
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let undo_limit = match settings.await {
                Ok(Ok(settings)) => settings["undoDeleteLimit"].as_u64().unwrap_or(0),
                _ => 0,
            };
            let summary = match summary.await {
                Ok(Ok(summary)) => summary,
                Ok(Err(error)) => {
                    // TODO: an error toast titled "Delete Permanently".
                    eprintln!("picto: {error}");
                    return;
                }
                Err(_) => return,
            };
            let selected = summary["selected_count"].as_u64().unwrap_or(0);
            let media = summary["media_count"]
                .as_u64()
                .map(u128::from)
                .unwrap_or(u128::MAX);
            if selected == 0 {
                return;
            }
            let (title, label) = wording(selected, empty_trash);
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.request = Some(Request {
                        target,
                        title,
                        label,
                        message: message(Some(media), undo_limit),
                        require_undoable: undo_limit > 0 && media <= undo_limit as u128,
                        presence: Presence::new(),
                        busy: false,
                    });
                    window.focus(&this.focus, cx);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = &mut self.request {
            request.presence.close();
            self.refocus = true;
            cx.notify();
        }
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.request.as_mut().filter(|request| !request.busy) else {
            return;
        };
        request.busy = true;
        let require_undoable = request.require_undoable;
        let answer = self.session.read(cx).dispatch(
            "items.delete",
            json!({ "target": request.target, "require_undoable": require_undoable }),
        );
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Err(error)) if require_undoable && error.contains(NOT_UNDOABLE) => {
                        // Ask again, now saying it cannot be undone.
                        if let Some(request) = &mut this.request {
                            request.message =
                                "This delete can no longer be undone. Delete permanently anyway?"
                                    .into();
                            request.require_undoable = false;
                            request.busy = false;
                        }
                        cx.notify();
                        return;
                    }
                    Ok(Err(error)) => {
                        // TODO: an error toast titled "Delete Permanently".
                        eprintln!("picto: {error}");
                    }
                    _ => {}
                }
                this.close(cx);
                // What was deleted is gone from the grid and the viewer.
                this.session.update(cx, |session, cx| {
                    session.close_viewer(cx);
                    session.reload(cx);
                });
            });
        })
        .detach();
        cx.notify();
    }
}

struct DeleteGlobal(Entity<PermanentDelete>);

impl Global for DeleteGlobal {}

/// Creates the main window's permanent-delete dialog.
pub fn init(session: Entity<Session>, cx: &mut App) {
    let dialog = cx.new(|cx| PermanentDelete::new(session, cx));
    cx.set_global(DeleteGlobal(dialog));
}

/// The dialog, for the shell to draw.
pub fn dialog(cx: &App) -> Entity<PermanentDelete> {
    cx.global::<DeleteGlobal>().0.clone()
}

/// Asks to delete `target` permanently, after a confirmation.
pub fn request(target: SelectionTarget, empty_trash: bool, window: &mut Window, cx: &mut App) {
    dialog(cx).update(cx, |dialog, cx| {
        dialog.request(target, empty_trash, window, cx)
    });
}

impl Render for PermanentDelete {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self
            .request
            .as_ref()
            .is_some_and(|request| request.presence.finished())
        {
            self.request = None;
        }
        if self.request.is_none() && std::mem::take(&mut self.refocus) {
            match self.return_focus.take() {
                Some(handle) => window.focus(&handle, cx),
                None => window.blur(cx),
            }
        }
        let Some(request) = &self.request else {
            return div().into_any_element();
        };
        let theme = theme::theme(cx);
        ui::modal("permanent-delete", request.title.clone(), request.presence)
            .size(Size::Small)
            .focus(&self.focus)
            .layer(20)
            .on_close({
                let this = cx.weak_entity();
                move |_, cx| {
                    let _ = this.update(cx, |this, cx| this.close(cx));
                }
            })
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .line_height(px(19.5))
                    .text_color(theme.text_secondary())
                    .child(request.message.clone()),
            )
            .footer(
                ui::action_button("permanent-delete-cancel", Look::Standard)
                    .label("Cancel")
                    .disabled(request.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
            .footer(
                ui::action_button("permanent-delete-confirm", Look::Danger)
                    .label(request.label)
                    .disabled(request.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{message, wording};

    #[test]
    fn the_confirmation_says_whether_the_delete_can_be_undone() {
        assert_eq!(
            message(Some(3), 0),
            "Delete undo is disabled. This cannot be undone."
        );
        assert!(message(Some(3), 500).starts_with("You can undo this"));
        assert!(message(Some(500), 500).starts_with("You can undo this"));
        assert!(message(Some(501), 500).contains("exceeds the undo limit of 500"));
        assert!(message(None, 1000).contains("up to 1,000 media items"));
    }

    #[test]
    fn the_confirmation_is_titled_for_what_it_deletes() {
        assert_eq!(
            wording(1, false),
            ("Delete permanently?".into(), "Delete Permanently")
        );
        assert_eq!(
            wording(1200, false),
            (
                "Delete 1,200 items permanently?".into(),
                "Delete Permanently"
            )
        );
        assert_eq!(wording(7, true), ("Empty Trash".into(), "Delete All"));
    }
}
