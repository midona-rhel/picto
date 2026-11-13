use super::text_input::TextInput;
use crate::{motion, theme};
use gpui::*;
use std::time::Duration;

/// Which of the app's field frames to draw.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// `GridToolbar.module.css` `.searchField`
    Search,
    /// `Sidebar.module.css` `.treeFilterField`
    SidebarFilter,
    /// `InspectorField.module.css`
    Inspector,
    /// `Settings.module.css` `.sidebarSearch`, with room for its glyph.
    SettingsSearch,
    /// `ShortcutsPanel.module.css` `.searchInput`.
    Form,
    /// `GlassInput` as `CompactNumberInput` sizes it, with room on the
    /// right for its stepper.
    Number,
}

struct Frame {
    height: f32,
    radius: f32,
    border: (Rgba, Rgba),
    fill: (Rgba, Rgba),
    /// How long the border and fill take to change on focus.
    focus: Duration,
    /// Space left of the text, inside the border.
    left: f32,
    right: f32,
    /// How far below centre the text sits. The inspector field pads its
    /// 20px line 6px from the top of a 32px box, a pixel low.
    drop: f32,
}

impl Look {
    fn frame(self, theme: theme::Theme) -> Frame {
        match self {
            Look::Search => Frame {
                height: theme::ICON_BUTTON,
                radius: theme::RADIUS_CONTROL,
                border: (theme.border(), theme.border_focus()),
                // `color-mix(control-surface 72%, transparent)` at rest.
                fill: (
                    theme::blend(
                        gpui::transparent_black().into(),
                        theme.control_surface(),
                        0.72,
                    ),
                    theme.control_surface(),
                ),
                focus: motion::FIELD_FOCUS,
                left: 27.,
                right: 6.,
                drop: 0.,
            },
            Look::SidebarFilter => Frame {
                height: theme::CONTROL_MD,
                radius: theme::RADIUS_MD,
                border: (theme.border_secondary(), theme.border_focus()),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                left: 30.,
                right: 10.,
                drop: 0.,
            },
            Look::Inspector => Frame {
                height: theme::CONTROL_LG,
                radius: theme::RADIUS_MD,
                border: (theme.border_secondary(), theme.primary()),
                fill: (theme.control_surface(), theme.surface_hover()),
                focus: motion::NORMAL,
                left: 10.,
                right: 10.,
                drop: 1.,
            },
            Look::SettingsSearch | Look::Form => Frame {
                height: theme::CONTROL_LG,
                radius: theme::RADIUS_MD,
                border: (theme.border(), theme.primary_at(60.)),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                left: if self == Look::Form { 12. } else { 30. },
                right: if self == Look::Form { 12. } else { 8. },
                drop: 0.,
            },
            Look::Number => Frame {
                height: theme::CONTROL_SM,
                radius: theme::RADIUS_SM,
                border: (theme.border(), theme.primary_at(50.)),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: motion::STEPPER,
                left: 8.,
                right: 25.,
                drop: 0.,
            },
        }
    }
}

/// The frame around a text input: its fill and border, how they change on
/// focus, and the icon and clear button some fields carry. A press anywhere
/// in the frame focuses the input.
#[derive(IntoElement)]
pub struct Field {
    id: ElementId,
    input: Entity<TextInput>,
    look: Look,
    /// Glyph, size, distance from the left edge, colour.
    icon: Option<(&'static str, f32, f32, Rgba)>,
    clearable: bool,
}

pub fn field(id: impl Into<ElementId>, input: &Entity<TextInput>, look: Look) -> Field {
    Field {
        id: id.into(),
        input: input.clone(),
        look,
        icon: None,
        clearable: false,
    }
}

impl Field {
    pub fn icon(mut self, name: &'static str, size: f32, left: f32, color: Rgba) -> Self {
        self.icon = Some((name, size, left, color));
        self
    }

    /// Shows a clear button at the right while the field has text.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }
}

impl RenderOnce for Field {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let frame = self.look.frame(theme);
        let handle = self.input.read(cx).focus_handle(cx);
        let focused = handle.is_focused(window);
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::at(focused));
        if fade.read(cx).on() != focused {
            fade.update(cx, |fade, _| fade.set(focused, frame.focus));
        }
        let strength = fade.read(cx).value();
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        let has_text = !self.input.read(cx).is_empty();
        let clear = self.clearable && has_text;

        let mut field = div()
            .id(self.id)
            .occlude()
            .relative()
            .w_full()
            .h(px(frame.height))
            .flex_none()
            .rounded(px(frame.radius))
            .border_1()
            .border_color(theme::blend(frame.border.0, frame.border.1, strength))
            .bg(theme::blend(frame.fill.0, frame.fill.1, strength))
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&handle, cx);
            });
        if self.look == Look::Search {
            // `inset 0 1px 0` at 3% contrast: a lit top edge.
            field = field.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(frame.radius - 1.))
                    .right(px(frame.radius - 1.))
                    .h(px(1.))
                    .bg(theme.contrast(3.)),
            );
        }
        if let Some((name, size, left, color)) = self.icon {
            // Measured from the frame's outer edge, as the CSS positions it.
            field = field.child(
                div()
                    .absolute()
                    .left(px(left - 1.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(super::icon(name, size, color)),
            );
        }
        field = field.child(
            div()
                .size_full()
                .pl(px(frame.left))
                .pr(px(if clear { 26. } else { frame.right }))
                // The input centres its line, so twice the drop above it.
                .pt(px(2. * frame.drop))
                .child(self.input.clone()),
        );
        if clear {
            let input = self.input;
            field = field.child(
                div()
                    .id("clear")
                    .absolute()
                    .top_0()
                    .right_0()
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .group("clear")
                    .cursor_pointer()
                    .child(
                        svg()
                            .path("x")
                            .size(px(12.))
                            .text_color(theme.text_tertiary())
                            .opacity(0.8)
                            .group_hover("clear", move |style| {
                                style.text_color(theme.text()).opacity(1.)
                            }),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| input.update(cx, |input, cx| input.clear(cx))),
            );
        }
        field
    }
}
