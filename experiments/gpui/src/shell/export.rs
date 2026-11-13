//! The window's side of exporting: the File menu's and the keys' Export
//! Originals (`file.export`), Export As (`file.exportAs`) and Export as
//! Picto Pack, on the selection (`applicationMenuRuntime.ts`), and Import
//! Picto Pack. The rows of the menus and the inspector start theirs through
//! [`crate::export`] directly.
use super::Shell;
use crate::export::{Level, notify};
use crate::grid::menu::Action;
use crate::keymap::{ExportAs, ExportOriginals, ExportPictoPack, ImportPictoPack};
use gpui::*;

impl Shell {
    /// The export of the selection, or of the item the viewer shows. With
    /// nothing selected production says so rather than exporting nothing.
    fn export_selection(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        if !session.viewer && session.selection_count() == 0 {
            notify(Level::Info, "Select files to export", "");
            return;
        }
        self.act_on_selection(action, window, cx);
    }

    /// `chooseAndImportPictoPack`, when an import may start.
    fn import_pack(&mut self, cx: &mut Context<Self>) {
        let allowed = self.imports.update(cx, |imports, cx| imports.allow(cx));
        if allowed {
            self.exports
                .update(cx, |exports, cx| exports.choose_pack(cx));
        }
    }

    pub(super) fn export_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ExportOriginals, window, cx| {
            this.export_selection(Action::ExportOriginals, window, cx)
        }))
        .on_action(cx.listener(|this, _: &ExportAs, window, cx| {
            this.export_selection(Action::ExportAs, window, cx)
        }))
        .on_action(cx.listener(|this, _: &ExportPictoPack, window, cx| {
            this.export_selection(Action::ExportPictoPack, window, cx)
        }))
        .on_action(cx.listener(|this, _: &ImportPictoPack, _, cx| this.import_pack(cx)))
    }
}
