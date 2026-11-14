//! The window's side of exporting: the File menu's and the keys' Export
//! Originals (`file.export`), Export As (`file.exportAs`) and Export as
//! Picto Pack, on the selection (`applicationMenuRuntime.ts`), and Import
//! Picto Pack. The rows of the menus and the inspector start theirs through
//! [`crate::export`] directly.
use super::Shell;
use crate::grid::menu::Action;
use crate::keymap::{ExportAs, ExportOriginals, ExportPictoPack, ImportPictoPack};
use gpui::*;

impl Shell {
    /// `chooseAndImportPictoPack`, when an import may start.
    fn import_pack(&mut self, cx: &mut Context<Self>) {
        let allowed = self.imports.update(cx, |imports, cx| imports.allow(cx));
        if allowed {
            self.exports
                .update(cx, |exports, cx| exports.choose_pack(cx));
        }
    }

    /// The export rows of the application menu and their keys, on the
    /// selection or the item the viewer shows. A row is enabled while
    /// something handles its action, so these follow production's rules: a
    /// selection for the exports (`hasSelection`), no blocking task for the
    /// import.
    pub(super) fn export_actions(&self, mut root: Div, cx: &mut Context<Self>) -> Div {
        let session = self.session.read(cx);
        if session.viewer || session.selection_count() > 0 {
            root = root
                .on_action(cx.listener(|this, _: &ExportOriginals, window, cx| {
                    this.act_on_selection(Action::ExportOriginals, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ExportAs, window, cx| {
                    this.act_on_selection(Action::ExportAs, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ExportPictoPack, window, cx| {
                    this.act_on_selection(Action::ExportPictoPack, window, cx)
                }));
        }
        root.on_action(cx.listener(|this, _: &ImportPictoPack, _, cx| this.import_pack(cx)))
    }
}
