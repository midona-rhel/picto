use super::text_input::TextInput;
use crate::theme::Text as _;
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
    /// `TagManagerScreen.module.css` `.titlebarSearch input`: the Tags
    /// screen's search in the titlebar.
    TitlebarSearch,
    /// `GlassInput.module.css` `.input`: a text field in a dialog.
    Modal,
    /// `.input` at the small control height, as a smart-folder rule's
    /// value field is (`.rule input`).
    ModalSmall,
    /// `GlassInput.module.css` `.searchWrap`: a dialog's search, an icon and
    /// the text with no frame of their own.
    ModalSearch,
    /// `Settings.module.css` `.sidebarSearch`, with room for its glyph.
    SettingsSearch,
    /// `ShortcutsPanel.module.css` `.searchInput`.
    Form,
    /// `GlassInput` as `CompactNumberInput` sizes it, with room on the
    /// right for its stepper.
    Number,
    /// `.renameInput`: a name edited in place, in a 20px box outlined in the
    /// primary colour over the app background.
    Rename,
    /// `.rangeEditor input`: the filter bar's 26px number fields.
    Range,
    /// `.hexField input`: a colour's code, with room on the left for its
    /// swatch.
    Hex,
    /// `.saveFilterEditor input`: the saved filter's name.
    SavedFilter,
}

struct Frame {
    height: f32,
    radius: f32,
    border: (Rgba, Rgba),
    fill: (Rgba, Rgba),
    /// How long the border and fill take to change on focus, and on what
    /// curve.
    focus: Duration,
    curve: motion::CubicBezier,
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
                curve: motion::EASE_DEFAULT,
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
                curve: motion::EASE_DEFAULT,
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
                curve: motion::EASE_DEFAULT,
                left: 10.,
                right: 10.,
                drop: 1.,
            },
            Look::TitlebarSearch => Frame {
                height: theme::CONTROL_SM,
                radius: theme::RADIUS_CONTROL,
                border: (theme.border(), theme.border_focus()),
                fill: (theme.surface_hover(), theme.control_surface()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
                left: 24.,
                right: 24.,
                drop: 0.,
            },
            Look::ModalSmall => Frame {
                height: theme::CONTROL_SM,
                ..Look::Modal.frame(theme)
            },
            Look::Modal => Frame {
                height: theme::CONTROL_LG,
                radius: theme::RADIUS_SM,
                border: (theme.border(), theme.primary_at(50.)),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: motion::FIELD_BORDER,
                curve: motion::EASE,
                left: 10.,
                right: 10.,
                drop: 0.,
            },
            Look::ModalSearch => {
                let clear: Rgba = gpui::transparent_black().into();
                Frame {
                    height: 36.,
                    radius: 0.,
                    border: (clear, clear),
                    fill: (clear, clear),
                    focus: Duration::ZERO,
                    curve: motion::EASE_DEFAULT,
                    // 10px in, a 14px icon and a 6px gap; the border is clear.
                    left: 29.,
                    right: 0.,
                    drop: 0.,
                }
            }
            Look::SettingsSearch | Look::Form => Frame {
                height: theme::CONTROL_LG,
                radius: theme::RADIUS_MD,
                border: (theme.border(), theme.primary_at(60.)),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
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
                curve: motion::EASE_DEFAULT,
                left: 8.,
                right: 25.,
                drop: 0.,
            },
            Look::Range => Frame {
                height: theme::CONTROL_SM,
                radius: theme::RADIUS_MD,
                border: (theme.border_secondary(), theme.border()),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
                left: 6.,
                right: 6.,
                drop: 0.,
            },
            Look::Hex => Frame {
                height: theme::CONTROL_LG,
                radius: theme::RADIUS_MD,
                border: (theme.border(), theme.primary()),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
                left: 32.,
                right: 10.,
                drop: 0.,
            },
            Look::SavedFilter => Frame {
                height: theme::CONTROL_MD,
                radius: theme::RADIUS_MD,
                border: (theme.border_secondary(), theme.primary()),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
                left: 8.,
                right: 8.,
                drop: 0.,
            },
            Look::Rename => Frame {
                height: 20.,
                radius: 3.,
                border: (theme.primary(), theme.primary()),
                fill: (theme.background(), theme.background()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
                left: 0.,
                right: 0.,
                drop: 0.,
            },
        }
    }
}

/// An inspector field that cannot be edited (`.fieldControl.fieldDisabled`):
/// the field's frame at rest with its value in the tertiary colour, ending in
/// an ellipsis when it does not fit. It takes no pointer input.
pub fn read_only(value: impl Into<SharedString>, cx: &App) -> Div {
    let theme = theme::theme(cx);
    let frame = Look::Inspector.frame(theme);
    div()
        .w_full()
        .h(px(frame.height))
        .flex_none()
        .rounded(px(frame.radius))
        .border_1()
        .border_color(frame.border.0)
        .bg(frame.fill.0)
        .overflow_hidden()
        // `padding: 6px 10px` inside the border, around a 20px line.
        .pl(px(frame.left))
        .pr(px(frame.right))
        .pt(px(6.))
        .font_family(theme::FONT_INSPECTOR)
        .text(theme::TEXT_MD)
        .line_height(px(20.))
        .text_color(theme.text_tertiary())
        .child(div().h(px(20.)).truncate().child(value.into()))
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
        let strength = fade.read(cx).value_with(frame.curve);
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        let has_text = !self.input.read(cx).is_empty();
        let clear = self.clearable && has_text;
        // The field's width, measured each frame, for the expanded overlay
        // that is drawn outside the field's own layout.
        let width = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "width".into()),
            cx,
            |_, _| 0f32,
        );
        let expanded = self.input.read(cx).expanded(window);
        let opening = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "expand".into()),
            cx,
            |_, _| motion::Fade::default(),
        );
        if opening.read(cx).on() != expanded {
            opening.update(cx, |fade, _| {
                // `fieldExpand`: 100 ms from 92% opacity on the way in; no
                // fade on the way back.
                if expanded {
                    fade.set(true, motion::FIELD_EXPAND);
                } else {
                    *fade = motion::Fade::default();
                }
            });
        }
        if self.look == Look::Inspector && expanded {
            return self.expanded(width, opening, window, cx).into_any_element();
        }
        let measure = {
            let width = width.clone();
            canvas(
                move |bounds, _, cx| {
                    let measured = f32::from(bounds.size.width);
                    if *width.read(cx) != measured {
                        width.update(cx, |width, _| *width = measured);
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };

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
            })
            .child(measure);
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
                .pr(px(if clear && self.look != Look::TitlebarSearch {
                    26.
                } else {
                    frame.right
                }))
                // The input centres its line, so twice the drop above it.
                .pt(px(2. * frame.drop))
                .child(self.input.clone()),
        );
        if clear && self.look == Look::TitlebarSearch {
            // `.titlebarSearch button`: 24px, 1px in from the right edge, with
            // its own hover fill.
            let input = self.input;
            field = field.child(
                div()
                    .id("clear")
                    .absolute()
                    .top_0()
                    .right_0()
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS_CONTROL))
                    .group("clear")
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.subtle_overlay()))
                    .child(
                        svg()
                            .path("x")
                            .size(px(12.))
                            .text_color(theme.text_secondary())
                            .group_hover("clear", move |style| style.text_color(theme.text())),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| input.update(cx, |input, cx| input.clear(cx))),
            );
        } else if clear {
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
        field.into_any_element()
    }
}

impl Field {
    /// `.fieldControlExpanded`: focused text that overflows its line is
    /// drawn over what follows, as tall as it wraps up to 240px (then it
    /// scrolls), on the rail colour with a lifted shadow. The field keeps its
    /// 32px place in the column.
    fn expanded(
        self,
        width: Entity<f32>,
        opening: Entity<motion::Fade>,
        window: &mut Window,
        cx: &mut App,
    ) -> Div {
        let theme = theme::theme(cx);
        let frame = self.look.frame(theme);
        if opening.read(cx).animating() {
            window.request_animation_frame();
        }
        let shown = 0.92 + 0.08 * opening.read(cx).value_with(motion::EASE_DEFAULT);
        let area = self.input.read(cx).area().clone();
        let handle = self.input.read(cx).focus_handle(cx);
        let overlay = div()
            .id(self.id)
            .occlude()
            .w(px(*width.read(cx)))
            .min_h(px(frame.height))
            .max_h(px(EXPANDED_MAX))
            .overflow_y_scroll()
            .track_scroll(&area)
            .rounded(px(frame.radius))
            .border_1()
            .border_color(frame.border.1)
            .bg(theme.rail())
            .shadow(vec![
                BoxShadow::new(px(0.), px(8.), rgba(0x00000038).into()).blur_radius(px(20.)),
                BoxShadow::new(px(0.), px(1.), rgba(0x0000002e).into()).blur_radius(px(3.)),
            ])
            .opacity(shown)
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&handle, cx);
            })
            .child(
                div()
                    .w_full()
                    .pl(px(frame.left))
                    .pr(px(frame.right))
                    .py(px(6.))
                    .child(self.input.clone()),
            );
        div()
            .relative()
            .w_full()
            .h(px(frame.height))
            .flex_none()
            .child(deferred(anchored().child(overlay)).with_priority(2))
    }
}

/// `.fieldControlExpanded { max-height: 240px }`
const EXPANDED_MAX: f32 = 240.;
