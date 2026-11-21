//! The parts of importing that belong to the main window. The first part is
//! the handling of files that are dragged from the operating system over
//! the window and dropped on it, with the overlay that is drawn while they
//! are over it (the drop handler and `.dropOverlay` of production's
//! `GridScreen.tsx`). The second part is the import commands of the menus
//! and keys: Import Files, Import Folder and Paste Import. All of them
//! import into the destination that the grid shows, and the import itself is
//! done by [`crate::imports`].
use super::Shell;
use crate::i18n::t;
use crate::imports::{self, plan::Destination};
use crate::keymap::PasteImport;
use crate::theme;
use gpui::*;
use std::path::PathBuf;

/// The import that a menu row or a key asks for: Import Files, Import Folder
/// or Paste Import.
#[derive(Clone, Copy)]
pub(crate) enum Import {
    Files,
    Folder,
    Paste,
}

impl Shell {
    /// Returns the destination that files dropped on the window at this
    /// moment would be imported into. It returns `None` when a drop imports
    /// nothing: while the viewer, Quick Look or a screen (Tags,
    /// Subscriptions or Duplicates) is shown, and when the grid's
    /// destination does not accept dropped files
    /// (`GridScope::takes_drops`).
    fn drop_destination(&self, cx: &App) -> Option<Destination> {
        let session = self.session.read(cx);
        if session.viewer || session.screen().is_some() || self.quick_look.read(cx).is_open() {
            return None;
        }
        let grid = self.grid_scope(cx);
        grid.takes_drops().then(|| grid.import_destination())
    }

    /// Starts the import `kind` into the destination that the grid shows.
    /// Import Files and Import Folder open a file chooser or a folder
    /// chooser first, and Paste Import imports what is on the clipboard.
    pub(crate) fn import(&mut self, kind: Import, window: &mut Window, cx: &mut Context<Self>) {
        let destination = self.grid_scope(cx).import_destination();
        self.imports.update(cx, |imports, cx| match kind {
            Import::Files => imports.choose_files(destination, window, cx),
            Import::Folder => imports.choose_folder(destination, window, cx),
            Import::Paste => imports.paste(destination, window, cx),
        });
    }

    /// Adds to `root` the handling of files dragged from the operating
    /// system over the window, and returns `root`.
    ///
    /// - When the files come over the window, their paths are stored in
    ///   `file_drag`, and the drop overlay is drawn while they are stored.
    /// - When the files leave the window, `file_drag` is cleared.
    /// - When the button is released with the files over the window, the
    ///   files are imported (`files_dropped`). The release is listened for
    ///   in the whole window, so the files are imported wherever in the
    ///   window they are dropped.
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
                // For 3 seconds after the grid has given its items to the
                // operating system as dragged files (`drag::OUT_GUARD`),
                // files that come over the window are taken to be those
                // items. They are already in the library, so they are not
                // stored for an import (production's `isInternalDragOrigin`).
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

    /// Called when the button is released while files from the operating
    /// system are over the window. It clears `file_drag`, which removes the
    /// drop overlay, and imports the files into the destination that
    /// `drop_destination` returns. Nothing is imported when it returns
    /// `None`.
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

    /// Imports the dropped files `paths` into `destination`.
    fn drop_files(
        &mut self,
        paths: Vec<PathBuf>,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A drag from a web browser carries more than file paths: the
        // image's data and its addresses. The vendored GPUI keeps what the
        // platform read from the drag, and it is passed on to the import
        // here when there is any.
        let data = gpui::take_external_drop_data().filter(|data| !data.is_empty());
        self.imports.update(cx, |imports, cx| {
            imports.drop_with_data(paths, data, destination, window, cx)
        });
    }

    /// Returns the overlay that is drawn over the grid's area while files
    /// from the operating system are over the window (production's
    /// `.dropOverlay`), or `None` when there are no such files or a drop
    /// would import nothing. The overlay has a 2 px border in the primary
    /// colour and a fill of the primary colour at 10% opacity. A badge 24 px
    /// above its bottom edge says "Drop files to import", with "into
    /// current folder" under it when the destination is a folder. The
    /// overlay has no listeners, so pointer events pass through it.
    pub(super) fn drop_overlay(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        // The drag can end without the window being told, for example
        // when it is cancelled outside the window. `file_drag` is cleared
        // here once GPUI reports no drag in progress.
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

    /// Adds to `root` the handler of the Paste Import key
    /// (`edit.pasteImport`), and returns `root`. The key imports what is on
    /// the clipboard when the grid's destination accepts imports
    /// (`GridScope::imports`). In the other destinations (the Trash, a smart
    /// folder, Recently Viewed and media matches) the key is passed on to
    /// the next handler. Import Files and Import Folder are rows of the
    /// application menu and are handled in `menu_commands`.
    pub(super) fn import_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &PasteImport, window, cx| {
            if this.grid_scope(cx).imports() {
                this.import(Import::Paste, window, cx);
            } else {
                cx.propagate();
            }
        }))
    }

    /// Tells the imports view how wide the sidebar (`left`) and the
    /// inspector (`right`) are in this frame, in pixels, so that the bar of
    /// background tasks is drawn between them and not under them.
    ///
    /// The shell calls this while it builds a frame. The imports view is a
    /// child of the shell that is not cached, so GPUI builds it later in
    /// the same frame and it reads the new widths then. The imports view is
    /// therefore not asked to be drawn again here.
    pub(super) fn place_task_bar(&self, left: f32, right: f32, cx: &mut Context<Self>) {
        self.imports
            .update(cx, |imports, _| imports.rails = (left, right));
    }
}

/// Returns true when the clipboard holds something that Paste Import can
/// import.
pub(super) fn can_paste(cx: &App) -> bool {
    imports::has_clipboard_import(cx)
}
