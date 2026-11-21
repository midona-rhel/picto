//! The parts of exporting that belong to the main window: the handlers of
//! Export Originals (`file.export`), Export As (`file.exportAs`), Export as
//! Picto Pack and Import Picto Pack, for the File menu's rows and their
//! keyboard shortcuts. The three exports act on the selected items. The
//! rows of the item menus and the inspector start their exports through
//! [`crate::export`] directly.
use super::Shell;
use crate::grid::menu::Action;
use crate::keymap::{ExportAs, ExportOriginals, ExportPictoPack, ImportPictoPack};
use gpui::*;

impl Shell {
    /// Lets the user choose a Picto Pack file and imports it
    /// (`Exports::choose_pack`). It does nothing when `Imports::allow` says
    /// that an import may not start now.
    fn import_pack(&mut self, cx: &mut Context<Self>) {
        let allowed = self.imports.update(cx, |imports, cx| imports.allow(cx));
        if allowed {
            self.exports
                .update(cx, |exports, cx| exports.choose_pack(cx));
        }
    }

    /// Adds to `root` the handlers of the application menu's export rows
    /// and of their keyboard shortcuts, and returns `root`. The three
    /// exports are handled only while the viewer is open or at least one
    /// item is selected, and they act on the item the viewer shows or on
    /// the selected items. A menu row is enabled while something handles
    /// its action, so the export rows are enabled exactly then. Import
    /// Picto Pack is always handled, and `import_pack` checks whether an
    /// import may start.
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
