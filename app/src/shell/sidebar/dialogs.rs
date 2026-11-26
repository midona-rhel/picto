//! The sidebar's dialogs: the confirmation before folders and smart folders
//! are deleted and before a folder's watch is removed, the dialog that sets
//! the auto tags of folders, and the dialog that sets the directory a folder
//! watches.
use super::{Sidebar, folder_id, tree};
use crate::i18n::t;
use crate::session::{Reread, edit_then_in};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look as ButtonLook, menu::Return, modal, modal::Size};
use gpui::prelude::FluentBuilder;
use gpui::*;
use picto_library::{FolderId, Lifecycle};

/// What a confirmation dialog does when the user confirms.
#[derive(Clone)]
pub enum Confirmed {
    /// Deletes the folders and smart folders with these node ids.
    Delete(Vec<String>),
    /// Stops the folder from watching its directory.
    RemoveWatch(FolderId),
}

pub enum Dialog {
    /// A confirmation: a title, a message, a Cancel button, and a button
    /// labelled `label` that confirms.
    Confirm {
        title: &'static str,
        message: String,
        label: &'static str,
        then: Confirmed,
    },
    /// The auto-tags dialog. `tags` are the tags that every one of
    /// `folders` adds to the items that are put into it. Each change is
    /// saved at once.
    AutoTags {
        folders: Vec<FolderId>,
        tags: Vec<String>,
        /// The bounds at which the add button was last drawn. The tag
        /// picker opens above them.
        add_button: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    },
    /// The dialog that sets the directory on disk that a folder watches.
    /// New files in that directory are imported into the folder.
    Watch {
        folder: FolderId,
        path: Option<String>,
        enabled: bool,
        subfolders: bool,
        status: Lifecycle,
        /// The result of the command `imports.folder.analyze` for the
        /// chosen directory.
        analysis: WatchAnalysis,
        /// The number of analyses that were started. The answer to an
        /// analysis that is not the latest one is ignored.
        asked: u64,
    },
}

/// The state of the analysis of the chosen directory's structure.
pub enum WatchAnalysis {
    /// No directory is chosen yet.
    None,
    Checking,
    Done(serde_json::Value),
    Failed,
}

/// Returns the message that asks whether to delete the folder `name`.
pub fn folder_deletion(name: &str) -> String {
    crate::tf!(
        "Delete \"{value0}\" and all its subfolders? Media inside these folders will remain untouched.",
        name
    )
}

/// Returns the message that asks whether to delete `count` selected folders
/// or smart folders.
pub fn bulk_deletion(count: usize) -> String {
    if count == 1 {
        t("Delete the selected item? Its folder and subfolders will be deleted. Media inside these folders will remain untouched.").into()
    } else {
        crate::tf!(
            "Delete {value0} selected items? All selected folders and their subfolders will be deleted. Media inside these folders will remain untouched.",
            count
        )
    }
}

/// Returns the message that asks whether to delete the smart folder `name`.
/// For a group the message says that its child smart folders are deleted
/// too.
pub fn smart_folder_deletion(name: &str, group: bool) -> String {
    if group {
        format!("Delete \"{name}\" and its child smart folders? This does not delete any media.")
    } else {
        format!("Delete \"{name}\"? This only removes the smart folder, not its contents.")
    }
}

impl Sidebar {
    /// Opens a dialog. The dialog takes keyboard focus, and `home` receives
    /// the focus when the dialog closes.
    pub(super) fn open_dialog(
        &mut self,
        dialog: Dialog,
        cx: &mut Context<Self>,
    ) -> &mut modal::Dialog<Dialog> {
        self.dialog.open(dialog, Return::To(self.home.clone()), cx)
    }

    /// Closes the dialog with its fade-out. The close button, Escape and a
    /// click on the backdrop run this.
    pub(super) fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog.dismiss(cx);
    }

    /// Removes the dialog at once, without a fade-out. Cancel runs this, and
    /// so does a dialog whose action has been carried out.
    pub(super) fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog.close(cx);
    }

    /// Opens the confirmation that asks whether to delete the folders and
    /// smart folders `nodes`.
    pub(super) fn confirm_delete(&mut self, nodes: Vec<String>, cx: &mut Context<Self>) {
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
                    (t("Delete Folder"), folder_deletion(&name))
                } else {
                    (
                        t("Delete Smart Folder"),
                        smart_folder_deletion(&name, group),
                    )
                }
            }
            _ => (t("Delete Selected"), bulk_deletion(nodes.len())),
        };
        self.open_dialog(
            Dialog::Confirm {
                title,
                message,
                label: t("Delete"),
                then: Confirmed::Delete(nodes),
            },
            cx,
        );
    }

    /// Opens the auto-tags dialog for `folders`. The dialog starts with the
    /// auto tags that all of the folders have in common.
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
                    let _ = cx.update(|_, cx| {
                        crate::ui::notify::error(
                            t("Could not load folder auto tags"),
                            error.to_string(),
                            cx,
                        )
                    });
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
            let _ = this.update(cx, move |this, cx| {
                this.open_dialog(
                    Dialog::AutoTags {
                        folders: unique,
                        tags: common,
                        add_button: Default::default(),
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    /// Opens the tag picker above the add button of the auto-tags dialog.
    /// The picker shows the dialog's tags as chosen.
    fn open_auto_tag_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::AutoTags {
            tags, add_button, ..
        }) = self.dialog.content()
        else {
            return;
        };
        let records = tags.iter().map(|tag| crate::tags::record(tag)).collect();
        let anchor = add_button.get().origin;
        self.auto_tag_picker.update(cx, |picker, cx| {
            picker.open_above(
                point(f32::from(anchor.x), f32::from(anchor.y)),
                records,
                window,
                cx,
            )
        });
    }

    /// Handles a tag that was chosen or unchosen in the tag picker. The tag
    /// is added to the dialog's tags or removed from them, and the tags are
    /// saved.
    pub(super) fn auto_tag_picked(
        &mut self,
        event: &crate::inspector::tag_picker::Event,
        cx: &mut Context<Self>,
    ) {
        let crate::inspector::tag_picker::Event::Toggle { name, add, .. } = event else {
            return;
        };
        let Some(Dialog::AutoTags { folders, tags, .. }) = self.dialog.content_mut() else {
            return;
        };
        if *add {
            if !tags.contains(name) {
                tags.push(name.clone());
            }
        } else {
            tags.retain(|tag| tag != name);
        }
        let (folders, names) = (folders.clone(), tags.clone());
        self.save_auto_tags(folders, names, cx);
        cx.notify();
    }

    /// Removes `tag` from the dialog's tags and saves them. The remove
    /// button of the tag's chip runs this.
    fn remove_auto_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        let Some(Dialog::AutoTags { folders, tags, .. }) = self.dialog.content_mut() else {
            return;
        };
        tags.retain(|current| current != tag);
        let (folders, names) = (folders.clone(), tags.clone());
        self.save_auto_tags(folders, names, cx);
        cx.notify();
    }

    /// Draws the content of the auto-tags dialog: the label "Automatically
    /// add tags", and below it the tags as chips that each have a remove
    /// button, and the add button. While there are no tags, the add button
    /// is as wide as the content and is labelled "Add tags". Once there are
    /// tags, it is a 26px square after the last chip.
    fn tag_assignment(
        tags: &[String],
        add_button: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let show_prefixes = crate::settings::current(cx).show_tag_prefixes;
        let mut values = div()
            .min_h(px(26.))
            .flex()
            .flex_wrap()
            .items_center()
            .content_start()
            .gap(px(4.));
        for (position, tag) in tags.iter().enumerate() {
            let (namespace, subtag) = crate::tags::split(tag);
            let namespace = namespace.unwrap_or("general");
            let removed = tag.clone();
            values = values.child(
                ui::chip(("auto-tag", position), subtag.to_owned(), {
                    crate::tags::chip_color(namespace)
                })
                .namespace(
                    (show_prefixes && crate::tags::chip_shows_namespace(namespace))
                        .then(|| namespace.to_owned().into()),
                )
                .on_remove(cx.listener(move |this, _, _, cx| this.remove_auto_tag(&removed, cx))),
            );
        }
        let empty = tags.is_empty();
        let measure = canvas(move |bounds, _, _| add_button.set(bounds), |_, _, _, _| {})
            .absolute()
            .inset_0();
        let add = div()
            .id("auto-tags-add")
            .role(Role::Button)
            .aria_label(t("Add tags"))
            .group("auto-tags-add")
            .relative()
            .h(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .bg(theme.surface_hover())
            .text_color(if empty {
                theme.text_tertiary()
            } else {
                theme.text_secondary()
            })
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface_active()).text_color(theme.text()))
            .child(measure)
            .on_click(cx.listener(|this, _, window, cx| this.open_auto_tag_picker(window, cx)));
        let glyph = || {
            svg()
                .path("plus")
                .size(px(14.))
                .text_color(if empty {
                    theme.text_tertiary()
                } else {
                    theme.text_secondary()
                })
                .group_hover("auto-tags-add", move |style| style.text_color(theme.text()))
        };
        let add = if empty {
            add.w_full()
                .gap(px(8.))
                .pl(px(16.))
                .pr(px(24.))
                .font_family(theme::FONT_UI)
                .text(theme::TEXT_MD)
                .line_height(px(26.))
                .child(glyph())
                .child(t("Add tags"))
        } else {
            add.w(px(26.)).child(glyph())
        };
        // The add button is a tab stop, so it can be reached and pressed
        // with the keyboard.
        let add = ui::focus::stop(
            "auto-tags-add",
            add,
            cx.listener(|this, _, window, cx| this.open_auto_tag_picker(window, cx)),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text(theme::TEXT_MD)
            .line_height(px(theme::LINE_HEIGHT_MD))
            .text_color(theme.text())
            .child(t("Automatically add tags"))
            .child(values.child(add))
    }

    /// Saves `names` as the auto tags of each of `folders`. A tag that the
    /// library does not have yet is created.
    fn save_auto_tags(
        &mut self,
        folders: Vec<FolderId>,
        names: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let work = move |library: &picto_library::Library| -> picto_library::Result<()> {
            let ids = if names.is_empty() {
                Vec::new()
            } else {
                library.ensure_tag_definitions(&names)?.0
            };
            for folder in folders {
                library.set_folder_auto_tags(folder, ids.clone())?;
            }
            Ok(())
        };
        self.session.update(cx, |session, cx| {
            let _ = session.edit_reporting(
                crate::session::Report::failure(t("Could not set folder auto tags")),
                work,
                Reread::NOTHING,
                cx,
            );
        });
    }

    /// Opens the watch dialog for `folder`, filled with the folder's watch
    /// as the library stores it. The menu row "Attach Watched Folder..."
    /// runs this.
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
            let _ = this.update(cx, move |this, cx| {
                // The dialog is filled from the stored watch only when that
                // watch is enabled and has a path.
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
                        analysis: WatchAnalysis::None,
                        asked: 0,
                    },
                    cx,
                );
                this.analyze_watch(cx);
            });
        })
        .detach();
    }

    /// Opens the system's folder chooser and puts the chosen directory in
    /// the watch dialog. The Browse button runs this.
    fn browse_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = crate::prompt::folder(t("Select folder to watch"), cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(chosen)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(Dialog::Watch { path, .. }) = this.dialog.content_mut() {
                    *path = Some(chosen.to_string_lossy().into_owned());
                    cx.notify();
                }
                this.analyze_watch(cx);
            });
        })
        .detach();
    }

    /// Asks the application to analyse the structure of the chosen directory
    /// as an import into the folder (`imports.folder.analyze`). The dialog
    /// shows a warning that is made from the result
    /// (`consolidation_message`). The analysis is started again whenever the
    /// directory or "Watch Subfolders" changes.
    fn analyze_watch(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Watch {
            folder,
            path,
            subfolders,
            analysis,
            asked,
            ..
        }) = self.dialog.content_mut()
        else {
            return;
        };
        *asked += 1;
        let Some(path) = path.clone() else {
            *analysis = WatchAnalysis::None;
            return;
        };
        *analysis = WatchAnalysis::Checking;
        let generation = *asked;
        let answer = self.session.read(cx).dispatch(
            "imports.folder.analyze",
            serde_json::json!({
                "path": path,
                "destination_folder_id": folder.0,
                "include_subfolders": *subfolders,
                "include_source_root": false,
            }),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(Dialog::Watch {
                    analysis, asked, ..
                }) = this.dialog.content_mut()
                    && *asked == generation
                {
                    *analysis = match result {
                        Ok(Ok(found)) => WatchAnalysis::Done(found),
                        _ => WatchAnalysis::Failed,
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Saves the watch dialog and closes it. An enabled watch is saved with
    /// the command `folders.watch.set`, and a watch that is turned off is
    /// removed with `folders.watch.clear`. The application checks the
    /// directory, which must exist and be outside the library, and its
    /// watcher imports the files that arrive there.
    fn save_watch(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Watch {
            folder,
            path: Some(path),
            enabled,
            subfolders,
            status,
            ..
        }) = self.dialog.content()
        else {
            return;
        };
        let (folder, path, enabled, subfolders, status) =
            (*folder, path.clone(), *enabled, *subfolders, *status);
        self.close_dialog(cx);
        if enabled {
            watch_command(
                &self.session,
                "folders.watch.set",
                serde_json::json!({
                    "folder_id": folder.0,
                    "path": path,
                    "include_subfolders": subfolders,
                    "lifecycle": if status == Lifecycle::Active { "active" } else { "inbox" },
                }),
                cx,
            );
        } else {
            clear_watch(&self.session, folder, cx);
        }
    }

    fn confirmed(&mut self, then: Confirmed, window: &mut Window, cx: &mut Context<Self>) {
        self.close_dialog(cx);
        match then {
            Confirmed::Delete(nodes) => self.delete(nodes, window, cx),
            Confirmed::RemoveWatch(id) => clear_watch(&self.session, id, cx),
        }
    }

    /// Draws the open dialog over the whole window. Returns `None` when no
    /// dialog is open.
    pub(super) fn dialog_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let layer = self.dialog.layer(window, cx)?;
        let theme = theme::theme(cx);
        let frame = |title: &'static str, size: Size, cx: &mut Context<Self>| {
            ui::modal("sidebar-dialog", title, layer.presence)
                .size(size)
                .focus(&layer.focus)
                .on_close(
                    cx.listener(|this, _: &(), _, cx| this.dismiss_dialog(cx))
                        .into_handler(),
                )
        };
        let cancel = ui::action_button("sidebar-dialog-cancel", ButtonLook::Standard)
            .label(t("Cancel"))
            .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx)));
        let element = match &layer.content {
            Dialog::AutoTags {
                tags, add_button, ..
            } => {
                let control = Self::tag_assignment(tags, add_button.clone(), cx);
                frame(t("Set Auto Tags"), Size::Small, cx).child(control)
            }
            Dialog::Watch {
                path,
                enabled,
                subfolders,
                status,
                analysis,
                ..
            } => {
                let analyzing = matches!(analysis, WatchAnalysis::Checking);
                let failed = matches!(analysis, WatchAnalysis::Failed);
                let consolidation = match analysis {
                    WatchAnalysis::Done(found) => {
                        crate::imports::plan::consolidation_message(found)
                    }
                    _ => None,
                };
                let help = |text: String| {
                    // Help text is drawn at `TEXT_SM` on an 18px line.
                    div()
                        .text(theme::TEXT_SM)
                        .line_height(px(18.))
                        .text_color(theme.text_secondary())
                        .child(text)
                };
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
                frame(t("Auto-Import Folder"), Size::Medium, cx)
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
                                    .child(field_label(t("Watch Folder")))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(ui::field::read_only_modal(
                                                "watch-folder",
                                                path.clone().map(SharedString::from),
                                                t("Select a folder..."),
                                                cx,
                                            ))
                                            .child(
                                                ui::action_button(
                                                    "watch-browse",
                                                    ButtonLook::Standard,
                                                )
                                                .label(t("Browse"))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.browse_watch(window, cx)
                                                })),
                                            ),
                                    ),
                            )
                            .child(spread(
                                t("Enable Watch"),
                                ui::toggle("watch-enabled", enabled)
                                    .on_toggle(cx.listener(move |this, _: &(), _, cx| {
                                        if let Some(Dialog::Watch { enabled, .. }) = this.dialog.content_mut()
                                        {
                                            *enabled = !*enabled;
                                            cx.notify();
                                        }
                                    }).into_handler())
                                    .into_any_element(),
                            ))
                            .child(spread(
                                t("Watch Subfolders"),
                                ui::toggle("watch-subfolders", subfolders)
                                    .on_toggle(cx.listener(move |this, _: &(), _, cx| {
                                        if let Some(Dialog::Watch { subfolders, .. }) = this.dialog.content_mut()
                                        {
                                            *subfolders = !*subfolders;
                                            cx.notify();
                                        }
                                        this.analyze_watch(cx);
                                    }).into_handler())
                                    .into_any_element(),
                            ))
                            .when(analyzing, |stack| {
                                stack.child(help(t("Checking the folder structure...").into()))
                            })
                            .when_some(consolidation, |stack, message| {
                                stack.child(ui::form::warning_box(theme, message))
                            })
                            .when(failed, |stack| {
                                stack.child(ui::form::warning_box(
                                    theme,
                                    t("Picto could not inspect this folder. Choose it again or check that it is still available."),
                                ))
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(field_label(t("Import Status")))
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
                                                if let Some(Dialog::Watch { status, .. }) = this.dialog.content_mut()
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
                            .child(help(
                                t("New files added to this folder will be automatically imported into the library.").into(),
                            )),
                    )
                    .footer(
                        div()
                            .mr_auto()
                            .child(
                                ui::action_button("watch-remove", ButtonLook::Danger)
                                    .label(t("Remove Watch"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(Dialog::Watch { folder, .. }) = this.dialog.content()
                                        {
                                            let folder = *folder;
                                            this.close_dialog(cx);
                                            clear_watch(&this.session, folder, cx);
                                        }
                                    })),
                            ),
                    )
                    .footer(cancel)
                    .footer(
                        ui::action_button("watch-save", ButtonLook::Primary)
                            .label(t("Save"))
                            .disabled(path.is_none() || analyzing || failed)
                            .on_click(cx.listener(|this, _, _, cx| this.save_watch(cx))),
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
                    .on_cancel(move |_, cx| {
                        let _ = cancel.update(cx, |this, cx| this.close_dialog(cx));
                    })
                    .on_close(move |_, cx| {
                        let _ = close.update(cx, |this, cx| this.dismiss_dialog(cx));
                    })
                    .into_any_element(),
                );
            }
        };
        Some(element.into_any_element())
    }
}

/// Turns a view listener for the empty event `()` into a handler that takes
/// no event. The dialog's `on_close` and a toggle's `on_toggle` take such a
/// handler.
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
    fn deletion_messages_read_as_the_earlier_versions_sidebar_wrote_them() {
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
    /// Opens the folder picker in its move mode at the position of the last
    /// context menu. "Move to..." for folders runs this. The picker offers
    /// Library and every folder that is neither moving nor under a moving
    /// folder. When all moving folders have the same parent, that parent is
    /// shown as chosen. The picker's Move button moves the folders
    /// (`move_folders_to`). A folder in `nodes` that is inside another
    /// folder in `nodes` is not moved by itself.
    pub(super) fn pick_folder_parent(
        &mut self,
        nodes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = tree::nodes(&self.session.read(cx).nav, "folder:");
        let moving = tree::top_level(&tree, &nodes);
        let available = tree::move_targets(&tree, &moving)
            .iter()
            .filter_map(|id| folder_id(id))
            .collect::<Vec<_>>();
        let parents = moving
            .iter()
            .map(|id| {
                tree.iter()
                    .find(|node| node.id == *id)
                    .and_then(|node| node.parent.as_deref().and_then(folder_id))
            })
            .collect::<Vec<_>>();
        let current = parents
            .first()
            .copied()
            .flatten()
            .filter(|first| parents.iter().all(|parent| *parent == Some(*first)));
        self.moving_folders = moving.iter().filter_map(|id| folder_id(id)).collect();
        let anchor = point(
            f32::from(self.menu_position.x),
            f32::from(self.menu_position.y),
        );
        self.folder_picker.update(cx, |picker, cx| {
            picker.open_for_parent(Some(anchor), available, current, window, cx)
        });
    }

    /// Moves the folders in `moving_folders` under `parent`, or to the top
    /// level when `parent` is `None`. The order of the parent's children is
    /// not changed. The Move button of the folder picker runs this.
    pub(super) fn move_folders_to(
        &mut self,
        parent: Option<FolderId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moving = std::mem::take(&mut self.moving_folders);
        if moving.is_empty() {
            return;
        }
        edit_then_in(
            &self.session,
            crate::session::Report::folder_depth(),
            Reread::NOTHING,
            move |library| -> picto_library::Result<_> {
                for id in moving {
                    library.move_folder(id, parent)?;
                }
                Ok(())
            },
            window,
            cx,
            |_, _, _, _| {},
        );
    }
}

/// Removes the watch of `folder` with the command `folders.watch.clear`.
fn clear_watch(session: &Entity<crate::session::Session>, folder: FolderId, cx: &mut App) {
    watch_command(
        session,
        "folders.watch.clear",
        serde_json::json!({ "folder_id": folder.0 }),
        cx,
    );
}

/// Sends a watch command to the application. The application announces that
/// `folders` changed, and the sidebar and the inspector update from that
/// announcement. When the application refuses the command, for example
/// because the directory is gone or is inside the library, an error
/// notification titled "Could not watch folder" is shown.
fn watch_command(
    session: &Entity<crate::session::Session>,
    command: &'static str,
    args: serde_json::Value,
    cx: &mut App,
) {
    let answer = session.read(cx).dispatch(command, args);
    cx.spawn(async move |cx| {
        if let Ok(Err(error)) = answer.await {
            let _ = cx.update(|cx| crate::ui::notify::error("Could not watch folder", error, cx));
        }
    })
    .detach();
}
