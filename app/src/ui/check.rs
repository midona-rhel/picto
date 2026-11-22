use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;

type ToggleHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// How a check box is filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// The box is not ticked. It is filled with the control surface colour and
    /// has a ring.
    Empty,
    /// The box is ticked. It is filled with the primary text colour and has a
    /// tick in the app's background colour. The pickers and the tag and folder
    /// dialogs use this mark.
    Checked,
    /// The box is ticked. It is filled with the primary colour, inside a ring
    /// of black at 10%, and has a white tick. A ticked check box in Settings
    /// looks like this, and so does a chosen folder in a list of folders.
    Selected,
    /// A filter includes it. The box is green with a white tick and no ring.
    /// Green is used, and not the primary colour of an ordinary ticked box, so
    /// that "included by the filter" can be told apart from "ticked", and so
    /// that it goes with the red of `Excluded`.
    Included,
    /// A filter excludes it. The box has the negative colour and a white
    /// cross.
    Excluded,
}

/// Which of the two sizes of check box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    /// 14px. Until it is filled it has a ring in the pressed overlay colour.
    Compact,
    /// 16px. Until it is ticked it has a ring in `Theme::contrast(30.)`. The
    /// rows of [`Check`] use this size.
    Regular,
}

/// A check box. It is square, or round when it is a radio button.
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
    /// Makes it a radio button: round, with a dot where a check box has a
    /// tick.
    pub fn round(mut self, round: bool) -> Self {
        self.round = round;
        self
    }
}

impl RenderOnce for CheckBox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (side, ring, tick, tick_size) = match self.size {
            Size::Compact => (14., theme.pressed_overlay(), "check", 10.),
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
            Mark::Included => (
                rgb(theme::SUCCESS),
                clear,
                Some((tick, theme.on_primary())),
            ),
            // i18n: an icon's name
            Mark::Excluded => (theme.negative(), clear, Some(("x", theme.on_primary()))),
        };
        let base = base.bg(fill).border_color(border);
        match glyph {
            // A radio button's dot lies 3px inside the border, and has a 1px
            // line of black at 10% around it.
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

/// A row with a check box or a radio button and its label: a 16px indicator
/// and the label. Under the pointer the row gets a fill that fades in over 100
/// ms. The row starts 6px to the left of its place in the layout, so that the
/// indicator lines up with the text above it and the fill has space around the
/// indicator. A disabled row is drawn at half strength and does not react to
/// clicks.
///
/// A row can have two more texts on the label's line. The detail is drawn
/// after the label in small text, and the trailing text is drawn at the right
/// end of the row. Both are drawn in the secondary text colour. Such a row
/// takes the whole width that it is given. The label takes at most half of
/// it, and the label and the detail are cut off with an ellipsis where the
/// row is too narrow for them.
#[derive(IntoElement)]
pub struct Check {
    id: ElementId,
    label: SharedString,
    trailing: Option<SharedString>,
    detail: Option<SharedString>,
    /// Whether this is a radio button and not a check box.
    radio: bool,
    checked: bool,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

pub fn check(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Check {
    Check {
        id: id.into(),
        label: label.into(),
        trailing: None,
        detail: None,
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
    /// Sets the text at the right end of the label's line, for example a
    /// count.
    pub fn trailing(mut self, trailing: impl Into<SharedString>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }
    /// Sets the text after the label, for example a path.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
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
        let (strength, set_hover) =
            motion::hover_fade(self.id.clone(), motion::ROW_HOVER, motion::EASE, window, cx);
        // A group of radio buttons is one Tab stop, at the button that is
        // chosen, as in a browser. A disabled row is not a Tab stop.
        let focus = (!self.disabled && (!self.radio || self.checked))
            .then(|| super::focus::control(&self.id, window, cx));
        // The focus ring is 1px wide, in the primary colour, 2px outside the
        // indicator.
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
        let indicator = div()
            .relative()
            .flex_none()
            .mr(px(6.))
            .children(ring)
            .child(
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
        // A row with a detail or a trailing text lays its texts out in a
        // line that takes the rest of the row's width. A row with only a
        // label is as wide as the label.
        let texts = if self.trailing.is_none() && self.detail.is_none() {
            div().relative().min_w_0().child(self.label.clone())
        } else {
            let detail = match self.detail {
                Some(detail) => div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .child(detail),
                // Without a detail, an empty element takes the room, so that
                // the trailing text is at the right end.
                None => div().flex_1(),
            };
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .max_w(relative(0.5))
                        .truncate()
                        .child(self.label.clone()),
                )
                .child(detail)
                .children(self.trailing.map(|trailing| {
                    div()
                        .flex_none()
                        .text_color(theme.text_secondary())
                        .font_features(theme::tabular_figures())
                        .child(trailing)
                }))
        };
        let mut row = div()
            .id(self.id)
            .role(if self.radio {
                Role::RadioButton
            } else {
                Role::CheckBox
            })
            .aria_label(self.label)
            .aria_toggled(super::a11y::toggled(self.checked))
            .aria_disabled(self.disabled)
            .relative()
            .min_h(px(26.))
            // The negative left margin moves the row 6px to the left, and the
            // right margin of 6px makes it keep its width.
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
            .child(texts);
        if self.disabled {
            return row.opacity(0.5);
        }
        row = row.cursor_pointer().on_hover(set_hover);
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
