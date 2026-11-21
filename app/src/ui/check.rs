use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;

type ToggleHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// How a check box is filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// Not ticked: the control surface colour inside a ring.
    Empty,
    /// `OverlayShell`'s `.checkBoxChecked`: filled with the primary text
    /// colour, with a tick in the app's background colour.
    Checked,
    /// Filled with the primary colour, inside a ring of black at 10%, with a
    /// white tick. A ticked check box in Settings, and `FolderTree`'s
    /// `.checkSelected`.
    Selected,
    /// A filter includes it: green with a white tick and no ring. The
    /// Electron app's `.checkBoxFilterChecked` is the primary colour, the
    /// same blue as an ordinary ticked box. Green is used here so that
    /// "included by the filter" is told apart from "ticked", and so that it
    /// pairs with the red of `Excluded`.
    Included,
    /// `.checkBoxExcluded`: a filter excludes it. The negative colour with
    /// a white cross.
    Excluded,
}

/// Which of the two sizes of check box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    /// `OverlayShell.module.css` `.checkBox`: 14px. Until it is filled it has
    /// a ring in the pressed overlay colour.
    Compact,
    /// The indicator of `Settings.module.css` `.checkboxItem`: 16px. Until it
    /// is ticked it has a ring in `--color-contrast-30`.
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
            // `::after { inset: 3px }`: the dot is 3px inside the border, and
            // the border is black at 10%.
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

/// A row with a check box or a radio button and its label (`.checkboxItem` in
/// `Settings.module.css`): a 16px indicator and the label. Under the pointer
/// the row gets a fill that fades in over 100 ms. The row starts 6px to the
/// left of its place in the layout, so that the indicator lines up with the
/// text above it and the fill has space around the indicator. A disabled row
/// is drawn at half strength and does not react to clicks.
#[derive(IntoElement)]
pub struct Check {
    id: ElementId,
    label: SharedString,
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
        let (strength, set_hover) =
            motion::hover_fade(self.id.clone(), motion::ROW_HOVER, motion::EASE, window, cx);
        // A group of radio buttons is one Tab stop, at the button that is
        // chosen, as in a browser. A disabled row is not a Tab stop.
        let focus = (!self.disabled && (!self.radio || self.checked))
            .then(|| super::focus::control(&self.id, window, cx));
        // `.checkboxItem > input:focus-visible + .checkboxIndicator`: a 1px
        // ring in the primary colour, 2px outside the indicator.
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
        let mut row = div()
            .id(self.id)
            .role(if self.radio {
                Role::RadioButton
            } else {
                Role::CheckBox
            })
            .aria_label(self.label.clone())
            .aria_toggled(super::a11y::toggled(self.checked))
            .aria_disabled(self.disabled)
            .relative()
            .min_h(px(26.))
            // `margin-left: -6px` on a row as wide as its container: the row
            // is moved to the left and keeps its width.
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
