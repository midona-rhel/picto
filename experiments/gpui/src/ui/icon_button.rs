use super::tooltip::{self, Tip};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// What an icon button keeps between frames. Each value moves as the CSS
/// transition on `.iconBtn` does, over 150 ms.
struct State {
    hover: motion::Fade,
    press: motion::Fade,
    /// On while the button is disabled.
    dimmed: motion::Fade,
}

/// The shared 24px icon button of `iconButton.module.css`. The glyph rests at
/// 94% and lifts to full strength over an 8% overlay on hover; a press, or the
/// toggled state at rest, shows the 14% overlay instead; disabled dims to 20%.
/// It occludes what is behind it, so a button in a titlebar strip never drags
/// the window.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: &'static str,
    glyph: f32,
    disabled: bool,
    toggled: bool,
    badge: Option<usize>,
    tip: Option<Tip>,
    on_click: Option<ClickHandler>,
}

pub fn icon_button(id: impl Into<ElementId>, icon: &'static str) -> IconButton {
    IconButton {
        id: id.into(),
        icon,
        glyph: 16.,
        disabled: false,
        toggled: false,
        badge: None,
        tip: None,
        on_click: None,
    }
}

impl IconButton {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The glyph's size, for the few buttons whose artwork is not 16px.
    pub fn glyph(mut self, size: f32) -> Self {
        self.glyph = size;
        self
    }

    /// Shows the button as switched on (`.iconBtnActive`).
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.toggled = toggled;
        self
    }

    /// A count in the button's top-right corner, hidden at zero.
    pub fn badge(mut self, count: usize) -> Self {
        self.badge = (count > 0).then_some(count);
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

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let disabled = self.disabled;
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            hover: motion::Fade::default(),
            press: motion::Fade::default(),
            dimmed: motion::Fade::at(disabled),
        });
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        if state.read(cx).dimmed.on() != disabled {
            state.update(cx, |state, _| {
                state.dimmed.set(disabled, motion::BUTTON_HOVER);
                if disabled {
                    state.hover.set(false, motion::BUTTON_HOVER);
                    state.press.set(false, motion::BUTTON_HOVER);
                }
            });
            hint.update(cx, |hint, cx| hint.set(false, cx));
        }
        let (hover, press, dimmed, animating) = {
            let state = state.read(cx);
            (
                state.hover.value(),
                state.press.value(),
                state.dimmed.value(),
                state.hover.animating() || state.press.animating() || state.dimmed.animating(),
            )
        };
        if animating || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        // Rest shows nothing, or the active overlay when toggled; hovering
        // moves to the hover overlay, and a press to the active one.
        let rest = if self.toggled { 14. } else { 0. };
        let hovered = rest + (8. - rest) * hover;
        let overlay = hovered + (14. - hovered) * press;
        let glyph_opacity = if self.toggled {
            1.
        } else {
            0.94 + 0.06 * hover
        };

        let mut button = div()
            .id(self.id)
            .occlude()
            .relative()
            .size(px(theme::ICON_BUTTON))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_SM))
                    .bg(theme.contrast(overlay))
                    .opacity(1. - 0.8 * dimmed),
            )
            .child(
                super::icon(self.icon, self.glyph, theme.text())
                    .opacity(glyph_opacity * (1. - 0.8 * dimmed)),
            );
        if let Some(count) = self.badge {
            button = button.child(
                div()
                    .absolute()
                    .top(px(1.))
                    .right(px(1.))
                    .min_w(px(12.))
                    .h(px(12.))
                    .px(px(3.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(theme.primary())
                    .text_color(white())
                    .text(theme::TEXT_XS)
                    .line_height(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(count.to_string()),
            );
        }
        if disabled {
            return button;
        }
        if let Some(tip) = &self.tip {
            button = button.children(hint.read(cx).layer(tip, theme::ICON_BUTTON, cx));
        }
        let (entered, down, up, out) = (state.clone(), state.clone(), state.clone(), state);
        let release = |state: &Entity<State>, cx: &mut App| {
            if state.read(cx).press.on() {
                state.update(cx, |state, cx| {
                    state.press.set(false, motion::BUTTON_HOVER);
                    cx.notify();
                });
            }
        };
        let mut button = button
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                entered.update(cx, |state, cx| {
                    state.hover.set(*hovered, motion::BUTTON_HOVER);
                    cx.notify();
                });
                hint.update(cx, |hint, cx| hint.set(*hovered, cx));
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                down.update(cx, |state, cx| {
                    state.press.set(true, motion::BUTTON_HOVER);
                    cx.notify();
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| release(&up, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| release(&out, cx));
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
