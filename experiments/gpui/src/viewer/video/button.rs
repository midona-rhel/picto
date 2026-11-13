//! `.icBtn` of `VideoPlayer.module.css`: the player's 32px control, white on
//! the dark controls. Transparent at rest with the glyph at 80%; hovered it
//! takes a 15% white fill (over 150 ms, `ease`) and the glyph lifts to 95%;
//! pressed the fill is 25%. A button can be "active" (`.icBtnActive`), which
//! keeps the glyph at 95%. Its tooltip opens above it.
use crate::theme::Text as _;
use crate::ui::tooltip::{self, Tip};
use crate::{motion, theme, ui};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

struct State {
    hover: motion::Fade,
    press: motion::Fade,
}

/// What the button shows: a glyph of a size, or a short label in the
/// monospace face (the rate button).
enum Content {
    Glyph(&'static str, f32),
    Label(SharedString),
}

#[derive(IntoElement)]
pub struct MediaButton {
    id: ElementId,
    content: Content,
    active: bool,
    tip: Tip,
    on_click: Option<ClickHandler>,
}

pub fn media_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    size: f32,
    tip: Tip,
) -> MediaButton {
    MediaButton {
        id: id.into(),
        content: Content::Glyph(glyph, size),
        active: false,
        tip: tip.above(),
        on_click: None,
    }
}

/// `.icBtn.rateButton`: as wide as its label, at least 28px, 4px either side.
pub fn label_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tip: Tip,
) -> MediaButton {
    MediaButton {
        id: id.into(),
        content: Content::Label(label.into()),
        active: false,
        tip: tip.above(),
        on_click: None,
    }
}

impl MediaButton {
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
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

impl RenderOnce for MediaButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            hover: motion::Fade::default(),
            press: motion::Fade::default(),
        });
        let hint = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
            cx,
            |_, _| tooltip::Hover::default(),
        );
        let (hover, press, animating, hovered) = {
            let state = state.read(cx);
            (
                state.hover.value_with(motion::EASE),
                state.press.value_with(motion::EASE),
                state.hover.animating() || state.press.animating(),
                state.hover.on(),
            )
        };
        if animating || hint.read(cx).animating() {
            window.request_animation_frame();
        }
        let rest = rgba(0xffffff00);
        let fill = theme::blend(
            theme::blend(rest, rgba(theme::MEDIA_BUTTON_HOVER), hover),
            rgba(theme::MEDIA_BUTTON_ACTIVE),
            press,
        );
        // The glyph's colour is not transitioned in the CSS.
        let glyph = if hovered || self.active {
            rgba(theme::MEDIA_GLYPH_STRONG)
        } else {
            rgba(theme::MEDIA_GLYPH)
        };
        let mut button = div()
            .id(self.id)
            .occlude()
            .relative()
            .h(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_CONTROL))
            .bg(fill)
            .cursor_pointer();
        button = match self.content {
            Content::Glyph(name, size) => button.w(px(32.)).child(ui::icon(name, size, glyph)),
            Content::Label(label) => button
                .min_w(px(28.))
                .px(px(4.))
                .font_family(theme::FONT_MONO)
                .text(theme::TEXT_CAPTION)
                .text_color(glyph)
                .whitespace_nowrap()
                .child(label),
        };
        let layer = hint.read(cx).layer(&self.tip, 32., cx);
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
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| release(&out, cx))
            .children(layer);
        if let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| {
                cx.stop_propagation();
                handler(event, window, cx);
            });
        }
        button
    }
}
