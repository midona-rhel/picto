//! The sidebar's dialogs: the confirmations (`ConfirmModal`) before deleting
//! folders and smart folders and before removing a watched folder.
use super::{Sidebar, folder_id};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::tag_token_input::{self, Found, TagTokenInput};
use crate::ui::{self, action_button::Look as ButtonLook, modal::Presence, modal::Size};
use gpui::*;
use picto_library::{FolderId, Lifecycle};
use std::rc::Rc;

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
    /// `FolderAutoTagsModal`: the tags every chosen folder gives the items
    /// it receives. Each change is saved at once.
    AutoTags {
        tags: Entity<TagTokenInput>,
        _changed: Subscription,
    },
    /// `FolderWatchModal`: the directory a folder imports from.
    Watch {
        folder: FolderId,
        path: Option<String>,
        enabled: bool,
        subfolders: bool,
        status: Lifecycle,
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

    /// `openFolderAutoTagsEditor`: the tags all chosen folders share.
    pub(super) fn edit_auto_tags(
        &mut self,
        folders: Vec<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut unique = Vec::new();
        for folder in folders {
            if !unique.contains(&folder) {
                unique.push(folder);
            }
        }
        if unique.is_empty() {
            return;
        }
        let asked = unique.clone();
        let answer = self.session.read(cx).run(move |library| {
            asked
                .iter()
                .map(|folder| library.folder_auto_tags(*folder))
                .collect::<picto_library::Result<Vec<_>>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let configured = match answer.await {
                Ok(Ok(configured)) => configured,
                Ok(Err(error)) => {
                    // TODO: show as a notification ("Could not load folder
                    // auto tags").
                    eprintln!("sidebar: {error}");
                    return;
                }
                Err(_) => return,
            };
            let common = configured.iter().skip(1).fold(
                configured.first().cloned().unwrap_or_default(),
                |common, next| {
                    common
                        .into_iter()
                        .filter(|tag| next.contains(tag))
                        .collect()
                },
            );
            let _ = this.update_in(cx, move |this, window, cx| {
                let session = this.session.clone();
                let search: tag_token_input::Search = Rc::new(move |query, cx| {
                    session.read(cx).run(move |library| {
                        library
                            .tag_page(None, Some(&query), 0, 12)
                            .map(|(tags, _, _)| {
                                tags.into_iter()
                                    .map(|tag| Found {
                                        namespace: tag.namespace,
                                        subname: tag.subname,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                });
                let tags = cx.new(|cx| TagTokenInput::new(common, search, cx));
                let folders = unique.clone();
                let changed = cx.subscribe_in(
                    &tags,
                    window,
                    move |this, tags, _: &tag_token_input::Changed, window, cx| {
                        let names = tags.read(cx).values().to_vec();
                        this.save_auto_tags(folders.clone(), names, window, cx);
                    },
                );
                let focus = tags.read(cx).input_focus(cx);
                this.open_dialog(
                    Dialog::AutoTags {
                        tags,
                        _changed: changed,
                    },
                    window,
                    cx,
                );
                window.focus(&focus, cx);
            });
        })
        .detach();
    }

    /// `foldersController.setAutoTags` for each folder.
    fn save_auto_tags(
        &mut self,
        folders: Vec<FolderId>,
        names: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            move |library| {
                let ids = if names.is_empty() {
                    Vec::new()
                } else {
                    library.ensure_tag_definitions(&names)?.0
                };
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_millis() as i64)
                    .unwrap_or(0);
                for folder in folders {
                    library.set_folder_auto_tags(folder, ids.clone(), now)?;
                }
                Ok(())
            },
            |_, (), _, _| {},
            window,
            cx,
        );
    }

    /// "Attach Watched Folder...": the folder's watch as it is stored.
    pub(super) fn edit_watch(
        &mut self,
        folder: FolderId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = self.session.read(cx).run(move |library| {
            library.folders().map(|folders| {
                folders
                    .into_iter()
                    .find(|record| record.folder_id == folder)
            })
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(record)) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, move |this, window, cx| {
                // `folderWatchInitial`: an enabled watch fills the dialog.
                let watched =
                    record.filter(|record| record.watch_enabled && record.watch_path.is_some());
                this.open_dialog(
                    Dialog::Watch {
                        folder,
                        path: watched
                            .as_ref()
                            .and_then(|record| record.watch_path.clone()),
                        enabled: true,
                        subfolders: watched
                            .as_ref()
                            .is_some_and(|record| record.watch_subfolders),
                        status: match watched.map(|record| record.watch_lifecycle) {
                            Some(Lifecycle::Active) => Lifecycle::Active,
                            _ => Lifecycle::Inbox,
                        },
                    },
                    window,
                    cx,
                );
            });
        })
        .detach();
    }

    /// Browse: the system's folder chooser.
    fn browse_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder to watch".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(chosen) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(Layer {
                    dialog: Dialog::Watch { path, .. },
                    ..
                }) = &mut this.dialog
                {
                    *path = Some(chosen.to_string_lossy().into_owned());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// `foldersController.setWatchConfig`: stores the watch, or clears it
    /// when the watch is turned off.
    // TODO: watching the directory and importing what arrives needs the
    // application backend; only the setting is stored here.
    fn save_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Layer {
            dialog:
                Dialog::Watch {
                    folder,
                    path: Some(path),
                    enabled,
                    subfolders,
                    status,
                },
            ..
        }) = &self.dialog
        else {
            return;
        };
        let (folder, path, enabled, subfolders, status) =
            (*folder, path.clone(), *enabled, *subfolders, *status);
        self.close_dialog(window, cx);
        self.edit(
            move |library| {
                if enabled {
                    library.set_folder_watch(folder, &path, subfolders, status)
                } else {
                    library.clear_folder_watch(folder)
                }
                .map(|_| ())
            },
            |_, (), _, _| {},
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
            Dialog::AutoTags { tags, .. } => {
                frame("Set Auto Tags", Size::Small, cx).child(tags.clone())
            }
            Dialog::Watch {
                path,
                enabled,
                subfolders,
                status,
                ..
            } => {
                let field_label = |text: &'static str| {
                    div()
                        .text(theme::TEXT_MD)
                        .line_height(px(theme::LINE_HEIGHT_MD))
                        .text_color(theme.text())
                        .child(text)
                };
                let spread = |text: &'static str, control: AnyElement| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(field_label(text))
                        .child(control)
                };
                let (enabled, subfolders, status) = (*enabled, *subfolders, *status);
                frame("Auto-Import Folder", Size::Medium, cx)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(field_label("Watch Folder"))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                // A read-only `GlassInput`.
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .h(px(theme::CONTROL_LG))
                                                    .px(px(10.))
                                                    .flex()
                                                    .items_center()
                                                    .rounded(px(theme::RADIUS_SM))
                                                    .border_1()
                                                    .border_color(theme.border())
                                                    .bg(theme.control_surface())
                                                    .text_color(if path.is_some() {
                                                        theme.text()
                                                    } else {
                                                        theme.text_tertiary()
                                                    })
                                                    .child(
                                                        div().min_w_0().truncate().child(
                                                            path.clone().unwrap_or(
                                                                "Select a folder...".into(),
                                                            ),
                                                        ),
                                                    ),
                                            )
                                            .child(
                                                ui::action_button(
                                                    "watch-browse",
                                                    ButtonLook::Standard,
                                                )
                                                .label("Browse")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.browse_watch(window, cx)
                                                })),
                                            ),
                                    ),
                            )
                            .child(spread(
                                "Enable Watch",
                                ui::toggle("watch-enabled", enabled)
                                    .on_toggle(cx.listener(move |this, _: &(), _, cx| {
                                        if let Some(Layer {
                                            dialog: Dialog::Watch { enabled, .. },
                                            ..
                                        }) = &mut this.dialog
                                        {
                                            *enabled = !*enabled;
                                            cx.notify();
                                        }
                                    }).into_handler())
                                    .into_any_element(),
                            ))
                            .child(spread(
                                "Watch Subfolders",
                                ui::toggle("watch-subfolders", subfolders)
                                    .on_toggle(cx.listener(move |this, _: &(), _, cx| {
                                        if let Some(Layer {
                                            dialog: Dialog::Watch { subfolders, .. },
                                            ..
                                        }) = &mut this.dialog
                                        {
                                            *subfolders = !*subfolders;
                                            cx.notify();
                                        }
                                    }).into_handler())
                                    .into_any_element(),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(field_label("Import Status"))
                                    .child({
                                        let sidebar = cx.entity().downgrade();
                                        ui::select(
                                            "watch-status",
                                            if status == Lifecycle::Active {
                                                "active"
                                            } else {
                                                "inbox"
                                            },
                                            vec![
                                                ui::choice("inbox", "Inbox"),
                                                ui::choice("active", "Active"),
                                            ],
                                        )
                                        .width(160.)
                                        .on_change(move |value, _, cx| {
                                            let _ = sidebar.update(cx, |this, cx| {
                                                if let Some(Layer {
                                                    dialog: Dialog::Watch { status, .. },
                                                    ..
                                                }) = &mut this.dialog
                                                {
                                                    *status = if value == "active" {
                                                        Lifecycle::Active
                                                    } else {
                                                        Lifecycle::Inbox
                                                    };
                                                    cx.notify();
                                                }
                                            });
                                        })
                                    }),
                            )
                            .child(
                                div()
                                    .text(theme::TEXT_SM)
                                    .text_color(theme.text_secondary())
                                    .child("New files added to this folder will be automatically imported into the library."),
                            ),
                    )
                    .footer(
                        div()
                            .mr_auto()
                            .child(
                                ui::action_button("watch-remove", ButtonLook::Danger)
                                    .label("Remove Watch")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(Layer {
                                            dialog: Dialog::Watch { folder, .. },
                                            ..
                                        }) = &this.dialog
                                        {
                                            let folder = *folder;
                                            this.close_dialog(window, cx);
                                            this.edit(
                                                move |library| {
                                                    library.clear_folder_watch(folder).map(|_| ())
                                                },
                                                |_, (), _, _| {},
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            ),
                    )
                    .footer(cancel)
                    .footer(
                        ui::action_button("watch-save", ButtonLook::Primary)
                            .label("Save")
                            .disabled(path.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.save_watch(window, cx))),
                    )
            }
            Dialog::Confirm {
                title,
                message,
                label,
                then,
            } => {
                let then = then.clone();
                let sidebar = cx.entity().downgrade();
                let (cancel, close) = (sidebar.clone(), sidebar.clone());
                return Some(
                    ui::confirm::confirm(
                        "sidebar-confirm",
                        *title,
                        message.clone(),
                        *label,
                        layer.presence,
                    )
                    .danger(true)
                    .focus(&layer.focus)
                    .on_confirm(move |window, cx| {
                        let _ =
                            sidebar.update(cx, |this, cx| this.confirmed(then.clone(), window, cx));
                    })
                    .on_cancel(move |window, cx| {
                        let _ = cancel.update(cx, |this, cx| this.close_dialog(window, cx));
                    })
                    .on_close(move |window, cx| {
                        let _ = close.update(cx, |this, cx| this.dismiss_dialog(window, cx));
                    })
                    .into_any_element(),
                );
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

impl Sidebar {
    /// "Move to..." for folders: production opens the folder picker in its
    /// move mode (`FolderPickerPanel` with `onApplyFolderParent`), offering
    /// Library and every folder that is neither moving nor under a moving
    /// one (`tree::move_targets`), with the folders' shared parent chosen,
    /// and its Move button moves the topmost of them there
    /// (`foldersController.moveMany`, as `drag::move_folders` does).
    // TODO: open that picker once the shared folder picker is in this
    // branch; the row does nothing until then.
    pub(super) fn pick_folder_parent(
        &mut self,
        _: Vec<String>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
}
