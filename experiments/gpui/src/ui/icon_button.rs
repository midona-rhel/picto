use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The shared 24px icon button of `iconButton.module.css`: the glyph at 94%,
/// lifting to full strength over a hover overlay that fades in across 150 ms,
/// a stronger overlay while pressed, and 20% when disabled. It occludes what
/// is behind it, so a button in a titlebar strip never drags the window.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: &'static str,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

pub fn icon_button(id: impl Into<ElementId>, icon: &'static str) -> IconButton {
    IconButton {
        id: id.into(),
        icon,
        disabled: false,
        on_click: None,
    }
}

impl IconButton {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let button = div()
            .id(self.id.clone())
            .occlude()
            .relative()
            .size(px(theme::ICON_BUTTON))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM));
        let glyph = super::icon(self.icon, 16., theme.text());
        if self.disabled {
            return button.opacity(0.2).child(glyph);
        }
        let fade = window.use_keyed_state(self.id, cx, |_, _| motion::Fade::default());
        let hover = fade.read(cx).value();
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        let mut button = button
            .active(move |style| style.bg(theme.active_overlay()))
            .on_hover(move |hovered, _, cx| {
                fade.update(cx, |fade, cx| {
                    fade.set(*hovered, motion::BUTTON_HOVER);
                    cx.notify();
                })
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_SM))
                    .bg(theme.hover_overlay())
                    .opacity(hover),
            )
            .child(glyph.opacity(0.94 + 0.06 * hover));
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
