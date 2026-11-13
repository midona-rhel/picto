use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

/// A full-width inspector action (`.flowActionBtn`, `.emptySectionActionBtn`):
/// a glyph and a label on the hover surface, which deepens to the active
/// surface under the pointer over 150 ms.
#[derive(IntoElement)]
pub struct ActionRow {
    id: ElementId,
    icon: &'static str,
    glyph: f32,
    label: SharedString,
    height: f32,
    disabled: bool,
}

pub fn action_row(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: impl Into<SharedString>,
) -> ActionRow {
    ActionRow {
        id: id.into(),
        icon,
        glyph: 15.,
        label: label.into(),
        height: theme::CONTROL_SM,
        disabled: false,
    }
}

impl ActionRow {
    pub fn glyph(mut self, size: f32) -> Self {
        self.glyph = size;
        self
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    /// `:disabled`: no surface, a quarter strength, and no hover.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for ActionRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let hover = fade.read(cx).value();
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        let row = div()
            .id(self.id)
            .w_full()
            .h(px(self.height))
            .pl(px(16.))
            .pr(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .text(theme::TEXT_SM);
        if self.disabled {
            return row
                .opacity(0.25)
                .child(super::icon(self.icon, self.glyph, theme.text()).opacity(0.8))
                .child(self.label);
        }
        row.bg(theme::blend(
            theme.surface_hover(),
            theme.surface_active(),
            hover,
        ))
        .cursor_pointer()
        .on_hover(move |hovered, _, cx| {
            fade.update(cx, |fade, cx| {
                fade.set(*hovered, motion::BUTTON_HOVER);
                cx.notify();
            })
        })
        .child(super::icon(self.icon, self.glyph, theme.text()).opacity(0.8 + 0.2 * hover))
        .child(self.label)
    }
}
