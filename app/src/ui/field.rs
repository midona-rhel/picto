use super::text_input::TextInput;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::time::Duration;

/// Says which of the app's field frames is drawn. The frames differ in
/// height, corner radius, border, fill and padding (`Look::frame`).
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// The search field in the grid's toolbar.
    Search,
    /// The filter field above the sidebar's tree.
    SidebarFilter,
    /// A field in the inspector.
    Inspector,
    /// The search field of the Tags screen, in the titlebar.
    TitlebarSearch,
    /// A text field in a dialog.
    Modal,
    /// A dialog's text field at the small control height. The value field of
    /// a smart folder rule uses it.
    ModalSmall,
    /// The field that renames a subfolder tile in the grid. It is a dialog's
    /// text field at a height of 24px, with 6px of padding at each side.
    TileRename,
    /// The search field of a dialog. It is an icon and the text, with no fill
    /// and no border.
    ModalSearch,
    /// The search field in the sidebar of the Settings window, with room at
    /// the left for its glyph.
    SettingsSearch,
    /// The search field of the Shortcuts panel in Settings. It is the frame
    /// of `SettingsSearch` with 12px of padding at each side.
    Form,
    /// The text field inside a number input (`ui::number_input`), with space
    /// at the right for the stepper.
    Number,
    /// The filter bar's 26px number fields for the two ends of a range.
    Range,
    /// The field for a colour's code in the colour editor, with room at the
    /// left for the colour's swatch.
    Hex,
    /// The field for a saved filter's name.
    SavedFilter,
    /// A dialog's field for several lines of text. It is three lines tall
    /// unless `rows` says otherwise, and its text starts 8px below its top
    /// edge.
    TextArea,
    /// A field next to its label in a form row of a dialog. Its fill is
    /// darker than the dialog.
    FormRow,
}

struct Frame {
    height: f32,
    radius: f32,
    border: (Rgba, Rgba),
    fill: (Rgba, Rgba),
    /// How long the border and the fill take to change when the field gets or
    /// loses the keyboard focus, and the timing curve of the change.
    focus: Duration,
    curve: motion::CubicBezier,
    /// The space to the left of the text, inside the border.
    left: f32,
    right: f32,
    /// How far below the centre the text is drawn. The inspector's field has a
    /// 20px line that starts 6px from the top of a 32px box, which is one
    /// pixel below the centre.
    drop: f32,
}

impl Look {
    fn frame(self, theme: theme::Theme) -> Frame {
        match self {
            Look::Search => Frame {
                height: theme::ICON_BUTTON,
                radius: theme::RADIUS_CONTROL,
                border: (theme.border(), theme.border_focus()),
                // While the field does not have the focus, its fill is the
                // control surface colour at 72% opacity.
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
            Look::TileRename => Frame {
                height: 24.,
                left: 6.,
                right: 6.,
                ..Look::Modal.frame(theme)
            },
            Look::TextArea => Frame {
                // The height is three lines of text, the padding above and
                // below them, and the 1px border at the top and at the bottom.
                height: 3. * super::text_input::AREA_LINE + 2. * AREA_PADDING + 2.,
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
                    // The text starts 30px from the field's outer edge: 10px
                    // of padding, a 14px icon and a 6px gap. One of the 30px
                    // is the transparent border.
                    left: 29.,
                    right: 0.,
                    drop: 0.,
                }
            }
            Look::SettingsSearch => Frame {
                height: theme::CONTROL_LG,
                radius: theme::RADIUS_MD,
                border: (theme.border(), theme.primary_at(60.)),
                fill: (theme.control_surface(), theme.control_surface()),
                focus: Duration::ZERO,
                curve: motion::EASE_DEFAULT,
                left: 30.,
                right: 8.,
                drop: 0.,
            },
            Look::Form => Frame {
                left: 12.,
                right: 12.,
                ..Look::SettingsSearch.frame(theme)
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
            Look::FormRow => Frame {
                height: theme::CONTROL_MD,
                radius: theme::RADIUS_MD,
                border: (theme.border(), theme.primary_at(60.)),
                fill: (rgba(theme::BLACK_25), rgba(theme::BLACK_25)),
                // The border's colour changes over `motion::FAST` when the
                // field gets or loses the focus.
                focus: motion::FAST,
                curve: motion::EASE_DEFAULT,
                left: 8.,
                right: 8.,
                drop: 0.,
            },
        }
    }
}

/// Returns an inspector field that cannot be edited. It has the field's
/// normal frame and shows its value in the tertiary text colour, cut with an
/// ellipsis when the value does not fit. It does not react to the pointer.
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
        // Inside the border the padding is 6px above the text and 10px at each
        // side. The line of text is 20px tall.
        .pl(px(frame.left))
        .pr(px(frame.right))
        .pt(px(6.))
        .font_family(theme::FONT_INSPECTOR)
        .text(theme::TEXT_MD)
        .line_height(px(20.))
        .text_color(theme.text_tertiary())
        .child(div().h(px(20.)).truncate().child(value.into()))
}

/// Returns a dialog field that cannot be typed in. It has the dialog field's
/// frame at 80% opacity. It shows a value that was chosen somewhere else (for
/// example a folder chosen in a dialog), or the placeholder while there is no
/// value, cut with an ellipsis when it does not fit. Tab still stops at it,
/// and while it has the keyboard focus its border is the primary colour at
/// 50% opacity.
pub fn read_only_modal(
    id: impl Into<ElementId>,
    value: Option<SharedString>,
    placeholder: impl Into<SharedString>,
    cx: &App,
) -> super::focus::Stop {
    let theme = theme::theme(cx);
    let frame = Look::Modal.frame(theme);
    let (text, color) = match value {
        Some(value) => (value, theme.text()),
        None => (placeholder.into(), theme.text_tertiary()),
    };
    let id = id.into();
    let field = div()
        .id(id.clone())
        .role(Role::TextInput)
        .aria_read_only(true)
        .aria_value(text.clone())
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(frame.height))
        .pl(px(frame.left))
        .pr(px(frame.right))
        .flex()
        .items_center()
        .rounded(px(frame.radius))
        .border_1()
        .border_color(frame.border.0)
        .bg(frame.fill.0)
        .opacity(0.8)
        .font_family(theme::FONT_UI)
        .text(theme::TEXT_MD)
        .text_color(color)
        .child(div().min_w_0().truncate().child(text));
    // The focus colour is drawn as a 1px ring exactly over the border, so
    // that the border looks as if it had changed colour.
    let ring = super::focus::Ring {
        width: 1.,
        offset: -1.,
        color: theme.primary_at(50.),
    };
    super::focus::stop(id, field, |_, _, _| {}).ring(ring, frame.radius, 1.)
}

/// Draws the frame around a text input: its fill and border, how they change
/// when the input gets the focus, and the icon and the clear button that
/// some fields have. A press anywhere in the frame gives the input the focus.
#[derive(IntoElement)]
pub struct Field {
    id: ElementId,
    input: Entity<TextInput>,
    look: Look,
    /// The icon's name, its size, its distance from the left edge, and its
    /// colour.
    icon: Option<(&'static str, f32, f32, Rgba)>,
    clearable: bool,
    /// What assistive technology calls the clear button.
    clear_name: Option<SharedString>,
    /// The height of a text area in lines. It is three lines when this is
    /// not given.
    rows: Option<usize>,
}

pub fn field(id: impl Into<ElementId>, input: &Entity<TextInput>, look: Look) -> Field {
    Field {
        id: id.into(),
        input: input.clone(),
        look,
        icon: None,
        clearable: false,
        clear_name: None,
        rows: None,
    }
}

impl Field {
    pub fn icon(mut self, name: &'static str, size: f32, left: f32, color: Rgba) -> Self {
        self.icon = Some((name, size, left, color));
        self
    }

    /// Sets the name that assistive technology gives the clear button. The
    /// name is "Clear" when none is given.
    pub fn clear_name(mut self, name: impl Into<SharedString>) -> Self {
        self.clear_name = Some(name.into());
        self
    }

    /// Shows a clear button at the right while the field has text.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// Makes a text area `rows` lines tall, not counting its padding and
    /// border.
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = Some(rows);
        self
    }
}

impl RenderOnce for Field {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let mut frame = self.look.frame(theme);
        if let Some(rows) = self.rows.filter(|_| self.look == Look::TextArea) {
            frame.height = rows as f32 * super::text_input::AREA_LINE + 2. * AREA_PADDING + 2.;
        }
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
        let clear_name = self
            .clear_name
            .clone()
            .unwrap_or_else(|| crate::i18n::t("Clear").into());
        // The field's width is measured in each frame. The expanded overlay,
        // which is drawn outside the field's own layout, uses it.
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
                // The overlay fades in from 92% opacity over 100 ms
                // (`motion::FIELD_EXPAND`). It does not fade when it closes.
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
                    // The canvas is inside the field's 1px border, and the
                    // overlay is as wide as the field with its border, so
                    // 2px are added.
                    let measured = f32::from(bounds.size.width) + 2.;
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
            .on_mouse_down(MouseButton::Left, {
                let input = self.input.clone();
                move |event, window, cx| {
                    window.focus(&handle, cx);
                    input.update(cx, |input, cx| input.press_outside(event, window, cx));
                }
            })
            .child(measure);
        if matches!(
            self.look,
            Look::Modal | Look::ModalSmall | Look::TileRename | Look::TextArea | Look::FormRow
        ) {
            // A field of a dialog has the primary text colour, whatever colour
            // the dialog's body gives its text.
            field = field.text_color(theme.text());
        }
        if self.look == Look::Search {
            // A 1px line at 3% contrast along the top edge, inside the
            // border. It is slightly lighter than the fill.
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
            // `left` is measured from the frame's outer edge. This element is
            // inside the 1px border, so 1px is taken off.
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
        let inner = div()
            .size_full()
            .pl(px(frame.left))
            .pr(px(
                if clear && !matches!(self.look, Look::TitlebarSearch | Look::SettingsSearch) {
                    26.
                } else {
                    frame.right
                },
            ))
            // A one-line input centres its line, so the space above it is
            // twice the look's drop. A text area starts at its padding.
            .pt(px(if self.look == Look::TextArea {
                AREA_PADDING
            } else {
                2. * frame.drop
            }));
        field = field.child(if self.look == Look::TextArea {
            // A text area can hold more rows than its box shows. The box
            // scrolls, and it is the box that the text input scrolls to
            // keep the row with the caret in view (`TextInput::area`).
            let area = self.input.read(cx).area().clone();
            inner
                .id("area")
                .pb(px(AREA_PADDING))
                .overflow_y_scroll()
                .track_scroll(&area)
                .child(self.input.clone())
                .into_any_element()
        } else {
            inner.child(self.input.clone()).into_any_element()
        });
        if clear && self.look == Look::SettingsSearch {
            // The clear button of the Settings search field is a bold cross
            // in an 8.5px square whose centre is 16.5px from the right edge.
            let input = self.input;
            field = field.child(
                div()
                    .id("clear")
                    .role(Role::Button)
                    .aria_label(clear_name.clone())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(12.))
                    .w(px(9.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(super::icon("search-cancel", 8.5, theme.text()))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| input.update(cx, |input, cx| input.clear(cx))),
            );
        } else if clear && self.look == Look::TitlebarSearch {
            // The clear button of the Tags screen's search field is 24px,
            // 1px from the field's right edge, and has a background while the
            // pointer is over it.
            let input = self.input;
            field = field.child(
                div()
                    .id("clear")
                    .role(Role::Button)
                    .aria_label(clear_name.clone())
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
                            // i18n: an icon's name
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
                    .role(Role::Button)
                    .aria_label(clear_name.clone())
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
                            // i18n: an icon's name
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
    /// Draws the expanded inspector field. When the field has the focus and
    /// its text needs more than one line, the text is drawn in an overlay
    /// over whatever follows the field. The overlay is as tall as the
    /// wrapped text, up to 240px, and scrolls beyond that. It has the rail's
    /// colour and a shadow. The field itself keeps its 32px place in the
    /// column.
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
            // The overlay's fill is the focused field's own fill over the
            // rail's colour, as one opaque colour.
            .bg({
                let fill = frame.fill.1;
                theme::blend(theme.rail(), Rgba { a: 1., ..fill }, fill.a)
            })
            .shadow(vec![
                BoxShadow::new(px(0.), px(8.), rgba(theme::BLACK_22).into()).blur_radius(px(20.)),
                BoxShadow::new(px(0.), px(1.), rgba(theme::BLACK_18).into()).blur_radius(px(3.)),
            ])
            .opacity(shown)
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, {
                let input = self.input.clone();
                move |event, window, cx| {
                    window.focus(&handle, cx);
                    input.update(cx, |input, cx| input.press_outside(event, window, cx));
                }
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
            .child(deferred(anchored().child(overlay)).with_priority(super::layer::OVERLAY))
    }
}

/// The padding above and below the text of a text area (`Look::TextArea`).
const AREA_PADDING: f32 = 8.;

/// The greatest height of the overlay of an expanded inspector field.
const EXPANDED_MAX: f32 = 240.;
