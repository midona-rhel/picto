//! `ConfirmModal`: a small dialog asking before a destructive action, with
//! its message, Cancel, and the confirming button (red when dangerous).
use crate::theme;
use crate::theme::Text as _;
use crate::ui::action_button::{Look, action_button};
use crate::ui::modal::{Presence, Size, modal};
use gpui::*;
use std::rc::Rc;

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Confirm {
    id: ElementId,
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    danger: bool,
    presence: Presence,
    focus: Option<FocusHandle>,
    on_confirm: Option<Handler>,
    on_cancel: Option<Handler>,
    on_close: Option<Handler>,
}

/// A confirmation titled `title` saying `message`; the confirming button
/// reads `confirm_label`.
pub fn confirm(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    confirm_label: impl Into<SharedString>,
    presence: Presence,
) -> Confirm {
    Confirm {
        id: id.into(),
        title: title.into(),
        message: message.into(),
        confirm_label: confirm_label.into(),
        danger: false,
        presence,
        focus: None,
        on_confirm: None,
        on_cancel: None,
        on_close: None,
    }
}

impl Confirm {
    /// The confirming button in the negative colour (`danger`).
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }
    /// The focus the dialog holds, so Escape reaches it.
    pub fn focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }
    pub fn on_confirm(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }
    /// The Cancel button; without it, Cancel closes as the close button does.
    pub fn on_cancel(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }
    /// The close button, Escape and a click on the backdrop.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Confirm {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let cancel = self.on_cancel.clone().or(self.on_close.clone());
        let mut dialog = modal(self.id.clone(), self.title, self.presence).size(Size::Small);
        if let Some(focus) = &self.focus {
            dialog = dialog.focus(focus);
        }
        if let Some(close) = self.on_close {
            dialog = dialog.on_close(move |window, cx| close(window, cx));
        }
        let confirm = self.on_confirm;
        dialog
            // The message at 13px on a 1.5 line height (`.helpText`, as
            // `ConfirmModal` styles it).
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .line_height(px(19.5))
                    .text_color(theme.text_secondary())
                    .child(self.message),
            )
            .footer(
                action_button("confirm-cancel", Look::Standard)
                    .label("Cancel")
                    .on_click(move |_, window, cx| {
                        if let Some(cancel) = &cancel {
                            cancel(window, cx);
                        }
                    }),
            )
            .footer(
                action_button(
                    "confirm-primary",
                    if self.danger {
                        Look::Danger
                    } else {
                        Look::Primary
                    },
                )
                .label(self.confirm_label)
                .on_click(move |_, window, cx| {
                    if let Some(confirm) = &confirm {
                        confirm(window, cx);
                    }
                }),
            )
    }
}
