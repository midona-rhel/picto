use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;

type ToggleHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// How a check box is filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// Unticked: the control surface inside a ring.
    Empty,
    /// `OverlayShell`'s `.checkBoxChecked`: the primary text colour with the
    /// app background's tick.
    Checked,
    /// The primary colour in a 10% black ring with a white tick: a ticked
    /// settings check, or `FolderTree`'s `.checkSelected`.
    Selected,
    /// `.checkBoxFilterChecked`: a filter includes it; the primary colour
    /// with a white tick and no ring.
    Included,
    /// `.checkBoxExcluded`: a filter leaves it out; the negative colour with
    /// a white cross.
    Excluded,
    /// `FolderPickerModal`'s `.checkRemove`: a folder the items will leave;
    /// the negative colour with a white minus.
    Removed,
}

/// Which of the two check boxes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    /// `OverlayShell.module.css` `.checkBox`: 14px, ringed by the pressed
    /// overlay until it is filled.
    Compact,
    /// `Settings.module.css` `.checkboxItem`'s indicator: 16px, ringed by
    /// `--color-contrast-30` until it is ticked.
    Regular,
}

/// A check box, square or (as a radio button) round.
#[derive(IntoElement)]
pub struct CheckBox {
    mark: Mark,
    size: Size,
    round: bool,
}

pub fn check_box(mark: Mark, size: Size) -> CheckBox {
    CheckBox {
        mark,
        size,
        round: false,
    }
}

impl CheckBox {
    /// A radio button: round, with a dot for its tick.
    pub fn round(mut self, round: bool) -> Self {
        self.round = round;
        self
    }
}

impl RenderOnce for CheckBox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (side, ring, tick, tick_size) = match self.size {
            Size::Compact => (14., theme.pressed_overlay(), "picker-check", 10.),
            Size::Regular => (16., theme.contrast(30.), "settings-check", 11.),
        };
        let base = div()
            .relative()
            .size(px(side))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(if self.round {
                side / 2.
            } else {
                theme::RADIUS_SM
            }))
            .border_1();
        let clear = gpui::transparent_black();
        let (fill, border, glyph): (Rgba, Hsla, Option<(&'static str, Rgba)>) = match self.mark {
            Mark::Empty => (theme.control_surface(), ring.into(), None),
            Mark::Checked => (theme.text(), clear, Some((tick, theme.background()))),
            Mark::Selected => (
                theme.primary(),
                rgba(theme::BLACK_10).into(),
                Some((tick, theme.on_primary())),
            ),
            Mark::Included => (theme.primary(), clear, Some((tick, theme.on_primary()))),
            Mark::Excluded => (theme.negative(), clear, Some(("x", theme.on_primary()))),
            Mark::Removed => (theme.negative(), clear, Some(("minus", theme.on_primary()))),
        };
        let base = base.bg(fill).border_color(border);
        match glyph {
            // `::after { inset: 3px }` inside the border, with a 10% ring.
            Some((_, color)) if self.round => base.child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(3.))
                    .size(px(side - 8.))
                    .rounded_full()
                    .bg(color)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_10).into())
                            .spread_radius(px(1.)),
                    ]),
            ),
            Some((name, color)) => base.child(super::icon(name, tick_size, color)),
            None => base,
        }
    }
}

/// A labelled checkbox or radio row (`.checkboxItem` in
/// `Settings.module.css`): a 16px indicator and a label, over a highlight
/// that fades in over 100 ms under the pointer. The row reaches 6px left of
/// its slot, so the indicator lines up with text above it and the highlight
/// has room around it. Disabled, it dims to half and takes no clicks.
#[derive(IntoElement)]
pub struct Check {
    id: ElementId,
    label: SharedString,
    /// A radio button rather than a checkbox.
    radio: bool,
    checked: bool,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

pub fn check(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Check {
    Check {
        id: id.into(),
        label: label.into(),
        radio: false,
        checked,
        disabled: false,
        on_toggle: None,
    }
}

impl Check {
    pub fn radio(mut self) -> Self {
        self.radio = true;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_toggle(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Check {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let strength = hover.read(cx).value_with(motion::EASE);
        if hover.read(cx).animating() {
            window.request_animation_frame();
        }
        // A radio group is one stop, at its chosen button, as the browser
        // makes it; a disabled row is none.
        let focus = (!self.disabled && (!self.radio || self.checked))
            .then(|| super::focus::control(&self.id, window, cx));
        // `.checkboxItem > input:focus-visible + .checkboxIndicator`: 1px of
        // the primary colour, 2px out from the indicator.
        let ring = focus.as_ref().and_then(|focus| {
            focus.ring(
                super::focus::Ring {
                    width: 1.,
                    offset: 2.,
                    color: theme.primary(),
                },
                if self.radio { 8. } else { theme::RADIUS_SM },
                0.,
            )
        });
        let indicator = div().relative().flex_none().mr(px(6.)).children(ring).child(
            check_box(
                if self.checked {
                    Mark::Selected
                } else {
                    Mark::Empty
                },
                Size::Regular,
            )
            .round(self.radio),
        );
        let mut row = div()
            .id(self.id)
            .relative()
            .min_h(px(26.))
            // `margin-left: -6px` on a full-width row: shifted, not widened.
            .ml(px(-6.))
            .mr(px(6.))
            .pl(px(6.))
            .pr(px(8.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .text_color(theme.text())
            .text(theme::TEXT_MD)
            .line_height(px(16.))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .bg(theme.surface_hover())
                    .opacity(strength),
            )
            .child(indicator)
            .child(div().relative().min_w_0().child(self.label));
        if self.disabled {
            return row.opacity(0.5);
        }
        row = row.cursor_pointer().on_hover(move |hovered, _, cx| {
            hover.update(cx, |hover, cx| {
                hover.set(*hovered, motion::ROW_HOVER);
                cx.notify();
            })
        });
        let handler: Option<Rc<dyn Fn(&mut Window, &mut App)>> = self.on_toggle.map(Rc::from);
        if let Some(handler) = handler.clone() {
            row = row.on_click(move |_, window, cx| handler(window, cx));
        }
        if let Some(focus) = focus {
            row = focus.attach(row, super::focus::Kind::Check, move |_, window, cx| {
                if let Some(handler) = &handler {
                    handler(window, cx);
                }
            });
        }
        row
    }
}
