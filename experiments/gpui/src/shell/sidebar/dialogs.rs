//! The sidebar's dialogs: the confirmations (`ConfirmModal`) before deleting
//! folders and smart folders and before removing a watched folder.
use super::{Sidebar, folder_id};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look as ButtonLook, modal::Presence, modal::Size};
use gpui::*;
use picto_library::FolderId;

/// What a confirmation does when confirmed.
#[derive(Clone)]
pub enum Confirmed {
    /// Deletes these folder and smart-folder definitions.
    Delete(Vec<String>),
    /// Stops watching a folder's directory.
    RemoveWatch(FolderId),
}

pub enum Dialog {
    /// `ConfirmModal`: a message, Cancel, and the confirming button.
    Confirm {
        title: &'static str,
        message: String,
        label: &'static str,
        then: Confirmed,
    },
}

/// The open dialog, its time on screen, and the focus that holds the
/// keyboard while no field in it has it.
pub struct Layer {
    pub dialog: Dialog,
    pub presence: Presence,
    pub focus: FocusHandle,
}

/// `singleFolderDeletionMessage`.
pub fn folder_deletion(name: &str) -> String {
    format!(
        "Delete \"{name}\" and all its subfolders? Media inside these folders will remain untouched."
    )
}

/// `bulkFolderDeletionMessage`.
pub fn bulk_deletion(count: usize) -> String {
    if count == 1 {
        "Delete the selected item? Its folder and subfolders will be deleted. Media inside these folders will remain untouched.".into()
    } else {
        format!(
            "Delete {count} selected items? All selected folders and their subfolders will be deleted. Media inside these folders will remain untouched."
        )
    }
}

/// The smart-folder menu's confirmation: a group takes its children with it.
pub fn smart_folder_deletion(name: &str, group: bool) -> String {
    if group {
        format!("Delete \"{name}\" and its child smart folders? This does not delete any media.")
    } else {
        format!("Delete \"{name}\"? This only removes the smart folder, not its contents.")
    }
}

impl Sidebar {
    /// Opens a dialog and gives it the keyboard.
    pub(super) fn open_dialog(
        &mut self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.dialog = Some(Layer {
            dialog,
            presence: Presence::new(),
            focus,
        });
        cx.notify();
    }

    /// The close button, Escape and the backdrop: the dialog fades out.
    pub(super) fn dismiss_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(layer) = &mut self.dialog {
            layer.presence.close();
            window.focus(&self.home, cx);
            cx.notify();
        }
    }

    /// Cancel, or a dialog whose work is done: it goes at once.
    pub(super) fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.take().is_some() {
            window.focus(&self.home, cx);
            cx.notify();
        }
    }

    /// Asks before deleting folders and smart folders, in the Electron
    /// sidebar's words.
    pub(super) fn confirm_delete(
        &mut self,
        nodes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, message) = match nodes.as_slice() {
            [node] => {
                let (name, group) = {
                    let nav = &self.session.read(cx).nav;
                    nav.iter()
                        .find(|entry| entry.id() == *node)
                        .map(|entry| (entry.label.clone(), entry.group))
                        .unwrap_or_default()
                };
                if folder_id(node).is_some() {
                    ("Delete Folder", folder_deletion(&name))
                } else {
                    ("Delete Smart Folder", smart_folder_deletion(&name, group))
                }
            }
            _ => ("Delete Selected", bulk_deletion(nodes.len())),
        };
        self.open_dialog(
            Dialog::Confirm {
                title,
                message,
                label: "Delete",
                then: Confirmed::Delete(nodes),
            },
            window,
            cx,
        );
    }

    fn confirmed(&mut self, then: Confirmed, window: &mut Window, cx: &mut Context<Self>) {
        self.close_dialog(window, cx);
        match then {
            Confirmed::Delete(nodes) => self.delete(nodes, window, cx),
            Confirmed::RemoveWatch(id) => self.edit(
                move |library| library.clear_folder_watch(id).map(|_| ()),
                |_, (), _, _| {},
                window,
                cx,
            ),
        }
    }

    /// The open dialog, drawn over the whole window.
    pub(super) fn dialog_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self
            .dialog
            .as_ref()
            .is_some_and(|layer| layer.presence.finished())
        {
            self.dialog = None;
        }
        let layer = self.dialog.as_ref()?;
        let theme = theme::theme(cx);
        let frame = |title: &'static str, size: Size, cx: &mut Context<Self>| {
            ui::modal("sidebar-dialog", title, layer.presence)
                .size(size)
                .focus(&layer.focus)
                .on_close(
                    cx.listener(|this, _: &(), window, cx| this.dismiss_dialog(window, cx))
                        .into_handler(),
                )
        };
        let cancel = ui::action_button("sidebar-dialog-cancel", ButtonLook::Standard)
            .label("Cancel")
            .on_click(cx.listener(|this, _, window, cx| this.close_dialog(window, cx)));
        let element = match &layer.dialog {
            Dialog::Confirm {
                title,
                message,
                label,
                then,
            } => {
                let then = then.clone();
                frame(title, Size::Small, cx)
                    .child(
                        div()
                            .text(theme::TEXT_MD)
                            .line_height(px(19.5))
                            .text_color(theme.text_secondary())
                            .child(message.clone()),
                    )
                    .footer(cancel)
                    .footer(
                        ui::action_button("sidebar-dialog-confirm", ButtonLook::Danger)
                            .label(*label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirmed(then.clone(), window, cx)
                            })),
                    )
            }
        };
        let _ = window;
        Some(element.into_any_element())
    }
}

/// A view listener taking no event, as the dialog's close handler.
trait IntoHandler {
    fn into_handler(self) -> impl Fn(&mut Window, &mut App) + 'static;
}

impl<F: Fn(&(), &mut Window, &mut App) + 'static> IntoHandler for F {
    fn into_handler(self) -> impl Fn(&mut Window, &mut App) + 'static {
        move |window, cx| self(&(), window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{bulk_deletion, folder_deletion, smart_folder_deletion};

    #[test]
    fn deletion_messages_read_as_the_electron_sidebar_writes_them() {
        assert_eq!(
            folder_deletion("Trips"),
            "Delete \"Trips\" and all its subfolders? Media inside these folders will remain untouched."
        );
        assert!(bulk_deletion(1).starts_with("Delete the selected item?"));
        assert!(bulk_deletion(3).starts_with("Delete 3 selected items?"));
        assert!(smart_folder_deletion("Red", true).contains("child smart folders"));
        assert!(smart_folder_deletion("Red", false).ends_with("not its contents."));
    }
}

// PENDING-STUBS
impl Sidebar {
    pub(super) fn pick_folder_parent(
        &mut self,
        _: Vec<String>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
    pub(super) fn edit_auto_tags(
        &mut self,
        _: Vec<FolderId>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
    pub(super) fn edit_watch(&mut self, _: FolderId, _: &mut Window, _: &mut Context<Self>) {}
}
