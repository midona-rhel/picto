//! Draws the zoom control in the titlebar. It is a horizontal track 80 px
//! long with a round knob. It is used in two places.
//!
//! In the grid's toolbar it sets the grid's zoom, and it has a − button on its
//! left and a + button on its right. In the grid and waterfall layouts the
//! track has one knob position for each number of columns the grid can have
//! at its current width. The smallest tiles (the most columns) are at the
//! left end and one column is at the right end. Each position gives a
//! different number of columns, and no number of columns is left out. The
//! knob's place on the track follows the size of the tiles, so it moves only
//! slightly, and only in one direction, while the grid is made wider or
//! narrower without a change of zoom. In the justified layout, which has a
//! target row height and no columns, the knob's position is that height and
//! it can be anywhere on the track.
//!
//! In the viewer's toolbar it sets the zoom percentage of the picture on a
//! logarithmic scale, and it has no buttons.
use crate::i18n::t;
use crate::{theme, ui};
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// The event the control emits when the user changes the zoom with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Changed {
    /// The user moved the knob to the position for this number of columns.
    Columns(usize),
    /// The user moved the knob to this row height, in pixels (justified
    /// layout).
    Height(f32),
    /// The user moved the viewer's knob to this zoom percentage.
    Percent(f32),
    /// The user pressed the + button (`larger` is true) or the − button, or
    /// assistive technology asked to increment or decrement the slider. The
    /// grid decides what one step is (`Session::zoom_step`).
    Step { larger: bool },
}

/// The zoom the control shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// The grid has `count` columns, and the zoom allows at most `most`
    /// columns at the grid's current width. `exact` and `span` are the same
    /// two numbers before they are rounded to whole columns
    /// (`layout::Zoom::Columns`). The knob is placed with them.
    Columns {
        count: usize,
        most: usize,
        exact: f32,
        span: f32,
    },
    /// The justified layout's target row height in pixels. `least` and `most`
    /// are the ends of the track.
    Height { height: f32, least: f32, most: f32 },
    /// The viewer's zoom in percent.
    Percent(f32),
}

const TRACK: f32 = 80.;
const KNOB: f32 = 12.;

/// The viewer's zoom range in percent, 5 to 800. `POSITION_STEP` is the
/// distance the viewer's knob moves in one step, on a track that is numbered
/// 0 to 100.
const PERCENT_MINIMUM: f32 = 5.;
const PERCENT_MAXIMUM: f32 = 800.;
const POSITION_STEP: f32 = 0.5;

/// Returns the knob position, from 0 to 100, for a zoom percentage in the
/// viewer. 5% is at 0, 100% is at 50 and 800% is at 100. The scale is
/// logarithmic on each side of 50.
fn percent_to_position(percent: f32) -> f32 {
    if percent <= 100. {
        50. * (percent / PERCENT_MINIMUM).ln() / (100. / PERCENT_MINIMUM).ln()
    } else {
        50. + 50. * (percent / 100.).ln() / (PERCENT_MAXIMUM / 100.).ln()
    }
}

/// Returns the zoom percentage for a knob position from 0 to 100. This is the
/// inverse of [`percent_to_position`].
fn position_to_percent(position: f32) -> f32 {
    if position <= 50. {
        PERCENT_MINIMUM * (100. / PERCENT_MINIMUM).powf(position / 50.)
    } else {
        100. * (PERCENT_MAXIMUM / 100.).powf((position - 50.) / 50.)
    }
}

/// Returns the container that the zoom control is placed in, in the titlebar.
/// It is filled with the app's background colour, and a 20 px wide gradient to
/// its left goes from transparent to that colour, so a long scope title is
/// hidden where it reaches the control. A press on it does not drag the
/// window.
pub fn section(cx: &App) -> Div {
    let background = theme::theme(cx).background();
    div()
        .relative()
        .occlude()
        .flex()
        .items_center()
        .gap(px(6.))
        .flex_none()
        .bg(background)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(relative(1.))
                .w(px(20.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(
                        Rgba {
                            a: 0.,
                            ..background
                        },
                        0.,
                    ),
                    linear_color_stop(background, 1.),
                )),
        )
}

pub struct Zoom {
    value: Value,
    /// True while the mouse button is held after a press on the control. While
    /// it is true the knob follows the pointer even when the pointer is
    /// outside the control.
    dragging: Rc<Cell<bool>>,
}

impl EventEmitter<Changed> for Zoom {}

impl Value {
    /// Returns the knob's position along the track, from 0 at the left end to
    /// 1 at the right end.
    fn along(self) -> f32 {
        match self {
            // A grid too narrow for two columns has one knob position. It is
            // drawn at the right end, where one column always is.
            Value::Columns { most, span, .. } if most <= 1 || span <= 1. => 1.,
            // The whole numbers `count` and `most` each change in a jump at
            // a different width, so a knob placed with them would move left
            // and right by turns while the grid is resized. The numbers
            // before rounding change evenly with the width.
            Value::Columns { exact, span, .. } => (span - exact) / (span - 1.),
            Value::Height {
                height,
                least,
                most,
            } => (height - least) / (most - least).max(1.),
            Value::Percent(percent) => percent_to_position(percent) / 100.,
        }
        .clamp(0., 1.)
    }

    /// Returns the value for a knob position `along` the track, from 0 to 1.
    /// For columns it is the nearest whole number of columns. For a row
    /// height it is rounded to a whole pixel. For the viewer the position is
    /// rounded to half a unit of its 0 to 100 track.
    fn at(self, along: f32) -> Self {
        match self {
            Value::Columns { most, span, .. } => {
                let count = (span - along * (span - 1.).max(0.)).round() as usize;
                let count = count.clamp(1, most.max(1));
                Value::Columns {
                    count,
                    most,
                    exact: count as f32,
                    span,
                }
            }
            Value::Height { least, most, .. } => Value::Height {
                height: (least + (most - least) * along).round(),
                least,
                most,
            },
            Value::Percent(_) => Value::Percent(position_to_percent(
                (along * 100. / POSITION_STEP).round() * POSITION_STEP,
            )),
        }
    }

    /// Returns whether the value is at the end of its range in the direction
    /// of a step, so the step would change nothing. The − and + buttons are
    /// disabled when this is true for their direction.
    fn at_end(self, larger: bool) -> bool {
        match (self, larger) {
            (Value::Columns { count, .. }, true) => count <= 1,
            (Value::Columns { count, most, .. }, false) => count >= most,
            (Value::Height { height, most, .. }, true) => height >= most,
            (Value::Height { height, least, .. }, false) => height <= least,
            (Value::Percent(percent), true) => percent >= PERCENT_MAXIMUM,
            (Value::Percent(percent), false) => percent <= PERCENT_MINIMUM,
        }
    }

    /// Returns the numbers reported to assistive technology for the slider:
    /// the current value, the smallest value and the largest value. For
    /// columns, the value is the number of the knob's position counted from
    /// the left end, where the smallest tiles are, starting at 1.
    fn numbers(self) -> (f64, f64, f64) {
        match self {
            Value::Columns { count, most, .. } => {
                ((most + 1).saturating_sub(count) as f64, 1., most as f64)
            }
            Value::Height {
                height,
                least,
                most,
            } => (height as f64, least as f64, most as f64),
            Value::Percent(percent) => (
                percent as f64,
                PERCENT_MINIMUM as f64,
                PERCENT_MAXIMUM as f64,
            ),
        }
    }
}

impl Zoom {
    /// Creates the grid's zoom control. It shows one column of at most one
    /// until the toolbar gives it the grid's zoom with [`Zoom::show`], so both
    /// buttons are disabled until then.
    pub fn new() -> Self {
        Self {
            value: Value::Columns {
                count: 1,
                most: 1,
                exact: 1.,
                span: 1.,
            },
            dragging: Rc::new(Cell::new(false)),
        }
    }

    /// Creates the viewer's zoom control, at 100%.
    pub fn percent() -> Self {
        Self {
            value: Value::Percent(100.),
            dragging: Rc::new(Cell::new(false)),
        }
    }

    /// Shows `value` in the control without emitting [`Changed`]. The toolbars
    /// call this when the zoom changed for another reason than this control,
    /// for example a zoom key. It does nothing while the user is dragging the
    /// knob, so the knob stays under the pointer.
    pub fn show(&mut self, value: Value, cx: &mut Context<Self>) {
        if self.dragging.get() {
            return;
        }
        let value = match value {
            Value::Percent(percent) => {
                Value::Percent(percent.clamp(PERCENT_MINIMUM, PERCENT_MAXIMUM))
            }
            value => value,
        };
        if value != self.value {
            self.value = value;
            cx.notify();
        }
    }

    /// Handles the pointer at the horizontal position `x` during a press or a
    /// drag on the control. The knob's centre moves to the allowed position
    /// nearest the pointer, and [`Changed`] is emitted if the value changed.
    fn at(&mut self, x: Pixels, track: Bounds<Pixels>, cx: &mut Context<Self>) {
        let along = ((f32::from(x - track.left()) - KNOB / 2.)
            / (f32::from(track.size.width) - KNOB))
            .clamp(0., 1.);
        let value = self.value.at(along);
        if value != self.value {
            self.value = value;
            cx.emit(match value {
                Value::Columns { count, .. } => Changed::Columns(count),
                Value::Height { height, .. } => Changed::Height(height),
                Value::Percent(percent) => Changed::Percent(percent),
            });
            cx.notify();
        }
    }

    /// Makes one step toward larger or smaller, for the − and + buttons and
    /// for assistive technology. In the grid's control this only emits
    /// [`Changed::Step`], because the grid decides what a step is: one column,
    /// or a factor of 1.1 of the row height. In the viewer's control it moves
    /// the knob by half a unit of its 0 to 100 track.
    fn step(&mut self, larger: bool, cx: &mut Context<Self>) {
        if self.value.at_end(larger) {
            return;
        }
        match self.value {
            Value::Percent(percent) => {
                let step = if larger {
                    POSITION_STEP
                } else {
                    -POSITION_STEP
                };
                let along = (percent_to_position(percent) + step).clamp(0., 100.);
                let next = position_to_percent(along).clamp(PERCENT_MINIMUM, PERCENT_MAXIMUM);
                if next != percent {
                    self.value = Value::Percent(next);
                    cx.emit(Changed::Percent(next));
                    cx.notify();
                }
            }
            Value::Columns { .. } | Value::Height { .. } => cx.emit(Changed::Step { larger }),
        }
    }
}

impl Render for Zoom {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let entity = cx.entity().downgrade();
        let dragging = self.dragging.clone();
        // An invisible element that covers the control and handles the
        // pointer. A press anywhere on the control moves the knob there and
        // starts a drag, and the knob follows the pointer until the mouse
        // button is released.
        let pointer = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let zoom = entity.clone();
                super::track_drag(
                    bounds,
                    dragging.clone(),
                    move |at, _, cx| {
                        let _ = zoom.update(cx, |zoom, cx| zoom.at(at.x, bounds, cx));
                    },
                    |_| {},
                    window,
                );
            },
        )
        .size_full();
        // Assistive technology gets the control as a slider named "Zoom", with
        // its current, smallest and largest value. An increment or decrement
        // from assistive technology makes the same step as the + or − button.
        let (value, minimum, maximum) = self.value.numbers();
        let stepped = |larger: bool| {
            let zoom = cx.entity().downgrade();
            move |_: Option<&gpui::accesskit::ActionData>, _: &mut Window, cx: &mut App| {
                let _ = zoom.update(cx, |zoom, cx| zoom.step(larger, cx));
            }
        };
        let slider = div()
            .id("zoom-slider")
            .role(Role::Slider)
            .aria_label(t("Zoom"))
            .aria_numeric_value(value)
            .aria_min_numeric_value(minimum)
            .aria_max_numeric_value(maximum)
            .on_a11y_action(AccessibleAction::Increment, stepped(true))
            .on_a11y_action(AccessibleAction::Decrement, stepped(false))
            .relative()
            .w(px(TRACK))
            .h(px(theme::ICON_BUTTON))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(11.))
                    .h(px(3.))
                    .rounded_full()
                    .bg(theme.border()),
            )
            .child(
                div()
                    .absolute()
                    .top(px((theme::ICON_BUTTON - KNOB) / 2.))
                    .left(px(self.value.along() * (TRACK - KNOB)))
                    .size(px(KNOB))
                    .rounded_full()
                    .bg(theme.text()),
            )
            .child(pointer);
        if matches!(self.value, Value::Percent(_)) {
            return slider.into_any_element();
        }
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                ui::icon_button("zoom-out", "toolbar-minus")
                    .disabled(self.value.at_end(false))
                    .tip(ui::tip(t("Zoom out")).shortcut("view.zoomOut", cx))
                    .on_click(cx.listener(|this, _, _, cx| this.step(false, cx))),
            )
            .child(slider)
            .child(
                ui::icon_button("zoom-in", "toolbar-plus")
                    .disabled(self.value.at_end(true))
                    .tip(ui::tip(t("Zoom in")).shortcut("view.zoomIn", cx))
                    .on_click(cx.listener(|this, _, _, cx| this.step(true, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{percent_to_position, position_to_percent};

    #[test]
    fn the_viewer_track_is_logarithmic_about_actual_size() {
        assert!(percent_to_position(5.).abs() < 1e-4);
        assert!((percent_to_position(100.) - 50.).abs() < 1e-4);
        assert!((percent_to_position(800.) - 100.).abs() < 1e-4);
        // On a logarithmic scale the midpoint between 5% and 100% is their
        // geometric mean, 5 × √20, about 22.4%. It is halfway between
        // positions 0 and 50, at position 25.
        assert!((percent_to_position(5. * 20f32.sqrt()) - 25.).abs() < 1e-3);
        assert!((position_to_percent(75.) - 100. * 8f32.sqrt()).abs() < 1e-2);
    }

    #[test]
    fn positions_and_percentages_round_trip() {
        for step in 0..=200 {
            let position = step as f32 * 0.5;
            let back = percent_to_position(position_to_percent(position));
            assert!((back - position).abs() < 1e-3, "{position} -> {back}");
        }
    }
}
