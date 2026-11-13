use crate::theme;
use gpui::*;
use std::{cell::Cell, rc::Rc};

type ChangeHandler = Rc<dyn Fn(f64, &mut App) + 'static>;

/// `.rangeInput` in `Settings.module.css`: a 200px, 3px track at the
/// primary border colour with a 12px round thumb in the text colour. A press
/// on the track moves the thumb there, and dragging moves it in steps.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    width: f32,
    on_change: Option<ChangeHandler>,
}

pub fn slider(id: impl Into<ElementId>, value: f64, min: f64, max: f64, step: f64) -> Slider {
    Slider {
        id: id.into(),
        value,
        min,
        max,
        step,
        width: WIDTH,
        on_change: None,
    }
}

impl Slider {
    /// The track's length, for the few sliders that are not 200px.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Called with each new value while the thumb moves.
    pub fn on_change(mut self, handler: impl Fn(f64, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

const WIDTH: f32 = 200.;
const THUMB: f32 = 12.;

/// The value at `x` pixels along the track, snapped to the step. The thumb's
/// centre travels from half a thumb in at each end, as Chromium draws it.
fn value_along(x: f32, width: f32, min: f64, max: f64, step: f64) -> f64 {
    let travel = width - THUMB;
    let along = ((x - THUMB / 2.) / travel).clamp(0., 1.) as f64;
    let raw = min + along * (max - min);
    let snapped = min + ((raw - min) / step).round() * step;
    snapped.clamp(min, max)
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let dragging =
            window.use_keyed_state(self.id.clone(), cx, |_, _| Rc::new(Cell::new(false)));
        let dragging = dragging.read(cx).clone();
        let along = if self.max > self.min {
            ((self.value - self.min) / (self.max - self.min)).clamp(0., 1.) as f32
        } else {
            0.
        };
        let (min, max, step, value) = (self.min, self.max, self.step, self.value);
        let handler = self.on_change;
        let width = self.width;
        div()
            .id(self.id)
            .occlude()
            .relative()
            .w(px(width))
            .h(px(THUMB))
            .flex_none()
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px((THUMB - 3.) / 2.))
                    .h(px(3.))
                    .rounded(px(2.))
                    .bg(theme.border()),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px((width - THUMB) * along))
                    .size(px(THUMB))
                    .rounded_full()
                    .bg(theme.text()),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let report = {
                            let handler = handler.clone();
                            move |x: Pixels, cx: &mut App| {
                                let next = value_along(
                                    f32::from(x - bounds.left()),
                                    width,
                                    min,
                                    max,
                                    step,
                                );
                                if let Some(handler) = &handler
                                    && next != value
                                {
                                    handler(next, cx);
                                }
                            }
                        };
                        let (down, report_down) = (dragging.clone(), report.clone());
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble
                                && event.button == MouseButton::Left
                                && bounds.contains(&event.position)
                            {
                                down.set(true);
                                report_down(event.position.x, cx);
                                cx.stop_propagation();
                            }
                        });
                        let moved = dragging.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && moved.get() {
                                report(event.position.x, cx);
                            }
                        });
                        let up = dragging.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                            if phase == DispatchPhase::Capture {
                                up.set(false);
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{WIDTH, value_along};

    fn value_at(x: f32, min: f64, max: f64, step: f64) -> f64 {
        value_along(x, WIDTH, min, max, step)
    }

    #[test]
    fn the_track_maps_to_stepped_values() {
        // The thumb's centre is 6px in at either end.
        assert_eq!(value_at(0., 100., 600., 50.), 100.);
        assert_eq!(value_at(6., 100., 600., 50.), 100.);
        assert_eq!(value_at(194., 100., 600., 50.), 600.);
        assert_eq!(value_at(100., 100., 600., 50.), 350.);
        assert_eq!(value_at(100., 5., 95., 1.), 50.);
    }
}
