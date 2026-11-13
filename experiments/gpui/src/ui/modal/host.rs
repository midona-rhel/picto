//! Holding a view's open dialog: what it shows, its time on screen, the
//! focus that holds the keyboard while no field in it has it, and where the
//! keyboard goes back to once it has gone.
use super::Presence;
use crate::ui::menu::{Return, host::give_back};
use gpui::*;

/// The open dialog. Draw it with its `presence` and `focus`.
pub struct Dialog<T> {
    pub content: T,
    pub presence: Presence,
    /// Holds the keyboard while no field in the dialog has it.
    pub focus: FocusHandle,
    /// What takes the keyboard when the dialog is first drawn.
    first: Option<FocusHandle>,
    back: Return,
    back_to: Option<FocusHandle>,
    /// The keyboard has been given back, as the dialog began to leave.
    returned: bool,
}

impl<T> Dialog<T> {
    /// A field that takes the keyboard when the dialog opens, rather than
    /// the dialog itself.
    pub fn focus_first(&mut self, handle: FocusHandle) {
        self.first = Some(handle);
    }
}

/// One dialog at a time for its owner. The owner draws what
/// [`Host::layer`] returns from its `render`, which drops the dialog once it
/// has faded out and keeps the owner repainting until then: a faded
/// [`super::Modal`] draws nothing and asks for no more frames, so without
/// that its owner could keep it forever.
pub struct Host<T> {
    open: Option<Dialog<T>>,
    /// A dialog closed at once, whose keyboard goes back at the next draw.
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
    /// Opens `content` over whatever was open. It takes the keyboard when it
    /// is first drawn, and gives it back to `back` once it leaves.
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

    /// What the open dialog shows.
    pub fn content(&self) -> Option<&T> {
        self.open.as_ref().map(|dialog| &dialog.content)
    }

    pub fn content_mut(&mut self) -> Option<&mut T> {
        self.open.as_mut().map(|dialog| &mut dialog.content)
    }

    /// The close button, Escape, the backdrop, or a confirmation: the
    /// dialog fades out, and the keyboard goes back at once.
    pub fn dismiss<V: 'static>(&mut self, cx: &mut Context<V>) {
        if let Some(dialog) = &mut self.open {
            dialog.presence.close();
            cx.notify();
        }
    }

    /// Cancel, or a dialog whose work is done: it goes at once.
    pub fn close<V: 'static>(&mut self, cx: &mut Context<V>) -> Option<T> {
        let dialog = self.open.take()?;
        if !dialog.returned {
            self.left = Some((dialog.focus, dialog.back_to));
        }
        cx.notify();
        Some(dialog.content)
    }

    /// The open dialog, to draw from the owner's `render`.
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
