use super::tooltip::{self, Tip};
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The quiet glyph button of table rows and job strips (`.querySmallBtn`):
/// the secondary text colour at 60%, rising to full strength over the hover
/// overlay in 100 ms. Switched on (`.querySmallBtnActive`) it rests at full
/// strength in the primary text colour; disabled it fades to 20%.
#[derive(IntoElement)]
pub struct SmallButton {
    id: ElementId,
    icon: &'static str,
    glyph: f32,
    size: f32,
    radius: f32,
    active: bool,
    disabled: bool,
    tip: Option<Tip>,
    on_click: Option<ClickHandler>,
}

pub fn small_button(id: impl Into<ElementId>, icon: &'static str) -> SmallButton {
    SmallButton {
        id: id.into(),
        icon,
        glyph: 14.,
        size: theme::ICON_BUTTON,
        radius: theme::RADIUS_SM,
        active: false,
        disabled: false,
        tip: None,
        on_click: None,
    }
}

impl SmallButton {
    pub fn glyph(mut self, size: f32) -> Self {
        self.glyph = size;
        self
    }
    /// A larger square, for the places that override the 24px size.
    pub fn size(mut self, size: f32, radius: f32) -> Self {
        self.size = size;
        self.radius = radius;
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn tip(mut self, tip: Tip) -> Self {
        self.tip = Some(tip);
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

impl RenderOnce for SmallButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        if self.disabled && fade.read(cx).on() {
            fade.update(cx, |fade, _| fade.set(false, motion::SMALL_BUTTON));
        }
        let hover = fade.read(cx).value_with(motion::EASE);
        if fade.read(cx).animating() || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let (color, rest) = if self.active {
            (theme.text(), 1.)
        } else {
            (theme.text_secondary(), 0.6)
        };
        let opacity = if self.disabled {
            0.2
        } else {
            rest + (1. - rest) * hover
        };
        let mut button = div()
            .id(self.id)
            .occlude()
            .relative()
            .size(px(self.size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(self.radius))
            // The whole button fades in CSS; here the fill and the glyph do,
            // so the tooltip below it stays at full strength.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(self.radius))
                    .bg(theme.hover_overlay())
                    .opacity(if self.disabled { 0. } else { opacity * hover }),
            )
            .child(super::icon(self.icon, self.glyph, color).opacity(opacity));
        if self.disabled {
            return button;
        }
        if let Some(tip) = &self.tip {
            button = button.children(hint.read(cx).layer(tip, self.size, cx));
        }
        let mut button = button.cursor_pointer().on_hover(move |hovered, _, cx| {
            fade.update(cx, |fade, cx| {
                fade.set(*hovered, motion::SMALL_BUTTON);
                cx.notify();
            });
            hint.update(cx, |hint, cx| hint.set(*hovered, cx));
        });
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
