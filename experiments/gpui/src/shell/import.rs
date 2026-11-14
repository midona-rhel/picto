//! The window's side of importing: files dragged from the system over the
//! window (`GridScreen.tsx`'s drop handler and `.dropOverlay`), and the
//! import commands of the menus and keys, which all import into the grid's
//! destination through [`crate::imports`].
use super::Shell;
use crate::i18n::t;
use crate::imports::{self, plan::Destination};
use crate::keymap::{ImportFiles, ImportFolder, PasteImport};
use crate::theme;
use gpui::*;
use std::path::PathBuf;

/// Which import a menu row or key asks for.
#[derive(Clone, Copy)]
pub(crate) enum Import {
    Files,
    Folder,
    Paste,
}

impl Shell {
    /// Where files dropped now would go: nowhere while a viewer, Quick
    /// Look or a screen has the window, or in a media-matches destination.
    fn drop_destination(&self, cx: &App) -> Option<Destination> {
        let session = self.session.read(cx);
        if session.viewer || session.screen().is_some() || self.quick_look.read(cx).is_open() {
            return None;
        }
        let grid = self.grid_scope(cx);
        grid.takes_drops().then(|| grid.import_destination())
    }

    /// Starts an import into the grid's destination.
    pub(crate) fn import(&mut self, kind: Import, window: &mut Window, cx: &mut Context<Self>) {
        let destination = self.grid_scope(cx).import_destination();
        self.imports.update(cx, |imports, cx| match kind {
            Import::Files => imports.choose_files(destination, window, cx),
            Import::Folder => imports.choose_folder(destination, window, cx),
            Import::Paste => imports.paste(destination, window, cx),
        });
    }

    /// The window takes files dragged over it from the system: it notes
    /// them as they come over, lets them go when they leave, and imports
    /// them when they are dropped anywhere in it. The release is watched
    /// for the whole window, whatever lies under the pointer.
    pub(super) fn file_drop(&self, root: Div, cx: &mut Context<Self>) -> Div {
        let shell = cx.entity().downgrade();
        let watch = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let released = shell.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !cx.has_active_drag() {
                        return;
                    }
                    let _ = released.update(cx, |this, cx| this.files_dropped(window, cx));
                });
                let left = shell.clone();
                window.on_mouse_event(move |event: &FileDropEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && matches!(event, FileDropEvent::Exited) {
                        let _ = left.update(cx, |this, cx| {
                            if this.file_drag.take().is_some() {
                                cx.notify();
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_0();
        root.on_drag_move::<ExternalPaths>(cx.listener(
            |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                // The grid's own items, dragged out a moment ago, are not
                // files to import (`isInternalDragOrigin`).
                if crate::grid::drag::model(cx).read(cx).dragging_out() {
                    return;
                }
                if this.file_drag.is_none() {
                    this.file_drag = Some(event.drag(cx).paths().to_vec());
                    cx.notify();
                }
            },
        ))
        .child(watch)
    }

    /// The files were let go over the window.
    fn files_dropped(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(paths) = self.file_drag.take() else {
            return;
        };
        cx.notify();
        let Some(destination) = self.drop_destination(cx) else {
            return;
        };
        self.drop_files(paths, destination, window, cx);
    }

    fn drop_files(
        &mut self,
        paths: Vec<PathBuf>,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.imports.update(cx, |imports, cx| {
            imports.drop_paths(paths, destination, window, cx)
        });
    }

    /// `.dropOverlay`: while files are over the window, the grid's area is
    /// ringed in the primary colour on a faint tint, with the badge near its
    /// bottom saying where they will go. It takes no pointer.
    pub(super) fn drop_overlay(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        // A drag that ended without the window seeing it leaves nothing.
        if self.file_drag.is_some() && !cx.has_active_drag() {
            self.file_drag = None;
        }
        self.file_drag.as_ref()?;
        let destination = self.drop_destination(cx)?;
        let theme = theme::theme(cx);
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_end()
                .justify_center()
                .pb(px(24.))
                .rounded(px(theme::RADIUS_LG))
                .border_2()
                .border_color(theme.primary())
                .bg(theme.primary_at(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.))
                        .px(px(24.))
                        .py(px(10.))
                        .rounded(px(theme::RADIUS_MD))
                        .bg(theme.primary())
                        .text_color(theme.on_primary())
                        .text_size(px(theme::TEXT_MD))
                        .line_height(px(theme::TEXT_MD * theme::LINE_HEIGHT_BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .child(t("Drop files to import"))
                        .children(destination.folder.map(|_| {
                            div()
                                .text_size(px(theme::TEXT_SM))
                                .line_height(px(theme::TEXT_SM * theme::LINE_HEIGHT_BODY))
                                .opacity(0.8)
                                .child(t("into current folder"))
                        })),
                ),
        )
    }

    /// The import keys: Import Files (`file.import`), Import Folder (the
    /// application menu's fixed ⇧⌘I) and Paste Import (`edit.pasteImport`,
    /// not in the Trash, smart folders, Recently Viewed or media matches).
    pub(super) fn import_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &ImportFiles, window, cx| this.import(Import::Files, window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ImportFolder, window, cx| {
                this.import(Import::Folder, window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &PasteImport, window, cx| {
            if this.grid_scope(cx).imports() {
                this.import(Import::Paste, window, cx);
            } else {
                cx.propagate();
            }
        }))
    }

    /// Keeps the task bar clear of the rails.
    pub(super) fn place_task_bar(&self, left: f32, right: f32, cx: &mut Context<Self>) {
        self.imports.update(cx, |imports, cx| {
            if imports.rails != (left, right) {
                imports.rails = (left, right);
                cx.notify();
            }
        });
    }
}

/// Whether anything can be pasted as an import now.
pub(super) fn can_paste(cx: &App) -> bool {
    imports::has_clipboard_import(cx)
}
