//! Keeps the one dialog that a view has open: its content, its [`Presence`],
//! the focus handle that has the keyboard focus while no field in the dialog
//! has it, and the control that gets the keyboard focus back when the dialog
//! closes.
use super::Presence;
use crate::ui::menu::{Return, host::give_back};
use gpui::*;

/// The dialog that is open. The owner draws it with its `presence` and its
/// `focus`.
pub struct Dialog<T> {
    pub content: T,
    pub presence: Presence,
    /// The focus handle that has the keyboard focus while no field in the
    /// dialog has it.
    pub focus: FocusHandle,
    /// The focus handle that gets the keyboard focus when the dialog is first
    /// drawn. It is `None` after that.
    first: Option<FocusHandle>,
    back: Return,
    back_to: Option<FocusHandle>,
    /// Whether the keyboard focus has already been returned. That happens when
    /// the dialog starts to fade out.
    returned: bool,
}

impl<T> Dialog<T> {
    /// Makes `handle`, a field of the dialog, take the keyboard focus when the
    /// dialog opens, in place of the dialog itself.
    pub fn focus_first(&mut self, handle: FocusHandle) {
        self.first = Some(handle);
    }
}

/// Keeps at most one open dialog for a view. The view draws what
/// [`Host::layer`] returns from its `render`. `layer` removes the dialog once
/// it has faded out, and asks for another frame until then. This is needed
/// because a [`super::Modal`] that has faded out draws nothing and asks for no
/// more frames, so nothing else would make the view draw again and remove the
/// dialog.
pub struct Host<T> {
    open: Option<Dialog<T>>,
    /// The focus handles of a dialog that was closed without a fade-out: its
    /// own, and the one that the keyboard focus returns to. The focus is
    /// returned at the next draw.
    left: Option<(FocusHandle, Option<FocusHandle>)>,
}

impl<T> Default for Host<T> {
    fn default() -> Self {
        Self {
            open: None,
            left: None,
        }
    }
}

impl<T> Host<T> {
    /// Opens a dialog with `content`, and closes the one that was open. The
    /// dialog takes the keyboard focus when it is first drawn. When it closes,
    /// the focus returns to `back`.
    pub fn open<V: 'static>(
        &mut self,
        content: T,
        back: Return,
        cx: &mut Context<V>,
    ) -> &mut Dialog<T> {
        self.close(cx);
        let focus = cx.focus_handle();
        cx.notify();
        self.open.insert(Dialog {
            content,
            presence: Presence::new(),
            first: Some(focus.clone()),
            focus,
            back,
            back_to: None,
            returned: false,
        })
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn get_mut(&mut self) -> Option<&mut Dialog<T>> {
        self.open.as_mut()
    }

    /// Returns the content of the open dialog.
    pub fn content(&self) -> Option<&T> {
        self.open.as_ref().map(|dialog| &dialog.content)
    }

    pub fn content_mut(&mut self) -> Option<&mut T> {
        self.open.as_mut().map(|dialog| &mut dialog.content)
    }

    /// Starts the dialog's fade-out. The close button, Escape, a click on the
    /// backdrop and a confirmation call this. The keyboard focus is returned at
    /// the next draw, before the fade-out has ended.
    pub fn dismiss<V: 'static>(&mut self, cx: &mut Context<V>) {
        if let Some(dialog) = &mut self.open {
            dialog.presence.close();
            cx.notify();
        }
    }

    /// Removes the dialog at once, without a fade-out, and returns its content.
    /// Cancel calls this, and so does a dialog whose work is done.
    pub fn close<V: 'static>(&mut self, cx: &mut Context<V>) -> Option<T> {
        let dialog = self.open.take()?;
        if !dialog.returned {
            self.left = Some((dialog.focus, dialog.back_to));
        }
        cx.notify();
        Some(dialog.content)
    }

    /// Returns the open dialog, for the owner to draw in its `render`. Before
    /// that, it returns the keyboard focus for a dialog that has closed,
    /// removes a dialog that has faded out, and gives the keyboard focus to a
    /// dialog that is drawn for the first time.
    pub fn layer(&mut self, window: &mut Window, cx: &mut App) -> Option<&Dialog<T>> {
        if let Some((focus, back)) = self.left.take() {
            let still_there = focus.contains_focused(window, cx);
            give_back(back, still_there, window, cx);
        }
        if let Some(dialog) = self.open.take_if(|dialog| dialog.presence.finished())
            && !dialog.returned
        {
            let still_there = dialog.focus.contains_focused(window, cx);
            give_back(dialog.back_to, still_there, window, cx);
        }
        let dialog = self.open.as_mut()?;
        if let Some(first) = dialog.first.take() {
            dialog.back_to = dialog.back.resolve(window, cx);
            window.focus(&first, cx);
        }
        if dialog.presence.closing() {
            if !dialog.returned {
                dialog.returned = true;
                let still_there = dialog.focus.contains_focused(window, cx);
                give_back(dialog.back_to.clone(), still_there, window, cx);
            }
            window.request_animation_frame();
        }
        Some(dialog)
    }
}
