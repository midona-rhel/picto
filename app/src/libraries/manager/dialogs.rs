//! Draws the Library Manager's questions: Delete…, the consent to change a
//! library on disk, and Remove from List. Each is kept in a
//! [`ui::modal::Host`], which removes the dialog after it has faded out and
//! gives the keyboard focus back to what had it before the dialog opened.
use super::*;

impl Manager {
    /// Draws the Delete… question. Its message names the library and the
    /// full path of its folder.
    pub(super) fn deletion(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.deleting.layer(window, cx)?;
        let (path, presence, focus) = (
            dialog.content.clone(),
            dialog.presence,
            dialog.focus.clone(),
        );
        let name = crate::libraries::name(&path);
        let close = cx.listener(|this: &mut Self, _: &(), _, cx| this.deleting.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.deleting.close(cx);
        });
        let deleted = path.clone();
        let accept =
            cx.listener(move |this: &mut Self, _: &(), _, cx| this.delete(deleted.clone(), cx));
        // The locale files have no key for this message, so it is shown in
        // English in every language.
        let message = format!(
            "Delete library \"{name}\"?\n\nThis will permanently delete all files in:\n{path}\n\nThis cannot be undone."
        );
        Some(
            ui::confirm::confirm(
                "library-delete",
                en("Delete Library"),
                message,
                t("Delete"),
                presence,
            )
            .danger(true)
            .busy(self.busy)
            .focus(&focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    /// Draws the consent question. It is asked before Rename, Show in File
    /// Manager, Move… or Delete… on a library that the user has not yet
    /// agreed to.
    pub(super) fn consent(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.asking.layer(window, cx)?;
        let (root, presence, focus) = (
            dialog.content.1.clone(),
            dialog.presence,
            dialog.focus.clone(),
        );
        let close = cx.listener(|this: &mut Self, _: &(), _, cx| this.asking.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking.close(cx);
        });
        let accept = cx.listener(|this: &mut Self, _: &(), window, cx| {
            if let Some((identity, root, act)) = this.asking.content().cloned() {
                this.asking.dismiss(cx);
                host::agree(&identity, cx);
                // The action runs after the question has given up the
                // keyboard focus. Otherwise the question would take the
                // focus away from the rename field that Rename opens.
                cx.defer_in(window, move |this, window, cx| {
                    this.perform(root.to_string_lossy().into_owned(), act, window, cx)
                });
            }
        });
        Some(
            ui::confirm::confirm(
                "library-manager-consent",
                en("Open this library directly?"),
                host::consent_message(&root),
                en("Continue"),
                presence,
            )
            .focus(&focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    pub(super) fn removal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.removing.layer(window, cx)?;
        let (path, presence, focus) = (
            dialog.content.clone(),
            dialog.presence,
            dialog.focus.clone(),
        );
        let name = crate::libraries::name(&path);
        let close = cx.listener(|this: &mut Self, _: &(), _, cx| this.removing.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.removing.close(cx);
        });
        let accept =
            cx.listener(move |this: &mut Self, _: &(), _, cx| this.remove(path.clone(), cx));
        Some(
            ui::confirm::confirm(
                "library-remove",
                t("Remove from List"),
                crate::tf!(
                    "Remove \"{value0}\" from this list?\n\nThe library and its files remain on disk.",
                    name
                ),
                t("Remove"),
                presence,
            )
            .danger(true)
            .focus(&focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }
}
