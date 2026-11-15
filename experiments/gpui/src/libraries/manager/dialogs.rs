//! The Library Manager's questions: Delete…, the consent to change a
//! library on disk, and Remove from List.
use super::*;

impl Manager {
    /// `deleteLibrary`'s question, as production's message box words it,
    /// naming the full path.
    pub(super) fn deletion(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, presence) = self.deleting.as_ref()?;
        let name = crate::libraries::name(path);
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.deleting = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.deleting = None;
            cx.notify();
        });
        let deleted = path.clone();
        let accept =
            cx.listener(move |this: &mut Self, _: &(), _, cx| this.delete(deleted.clone(), cx));
        // Production's main process writes these in English, without keys.
        let message = format!(
            "Delete library \"{name}\"?\n\nThis will permanently delete all files in:\n{path}\n\nThis cannot be undone."
        );
        Some(
            ui::confirm::confirm(
                "library-delete",
                en("Delete Library"),
                message,
                t("Delete"),
                *presence,
            )
            .danger(true)
            .busy(self.busy)
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    /// The consent question, asked here before Rename, Show in File
    /// Manager, Move… or Delete… on a library the user has not agreed to.
    pub(super) fn consent(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, root, _, presence) = self.asking.as_ref()?;
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking = None;
            cx.notify();
        });
        let accept = cx.listener(|this: &mut Self, _: &(), window, cx| {
            if let Some((identity, root, act, _)) = this.asking.take() {
                host::agree(&identity, cx);
                cx.notify();
                // After the question has let the keyboard go, so a rename's
                // field keeps it.
                cx.defer_in(window, move |this, window, cx| {
                    this.perform(root.to_string_lossy().into_owned(), act, window, cx)
                });
            }
        });
        Some(
            ui::confirm::confirm(
                "library-manager-consent",
                en("Open this library directly?"),
                host::consent_message(root),
                en("Continue"),
                *presence,
            )
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    pub(super) fn removal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, presence) = self.removing.as_ref()?;
        let name = crate::libraries::name(path);
        let path = path.clone();
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.removing = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.removing = None;
            cx.notify();
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
                *presence,
            )
            .danger(true)
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }
}
