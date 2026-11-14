use super::focus::{self, SliderKey, slider_key};
use crate::theme;
use gpui::prelude::FluentBuilder as _;
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
    filled: bool,
    /// `GlassModal.module.css` `.rangeInput`, as wide as its container.
    dialog: bool,
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
        filled: false,
        dialog: false,
        on_change: None,
    }
}

impl Slider {
    /// The track's length, for the few sliders that are not 200px.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// `.toleranceSlider`: the track in the primary colour up to the thumb,
    /// and a thumb with a border and a small shadow.
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    /// `.rangeInput` in a dialog (`GlassModal.module.css`): as wide as its
    /// container, a 4px track at the primary border colour, and a 14px
    /// thumb in the primary colour that overhangs the track's 4px of
    /// layout.
    pub fn dialog(mut self) -> Self {
        self.dialog = true;
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
/// The dialog look's thumb and track.
const DIALOG_THUMB: f32 = 14.;
const DIALOG_TRACK: f32 = 4.;

/// The value at `x` pixels along the track, snapped to the step. The thumb's
/// centre travels from half a thumb in at each end, as Chromium draws it.
fn value_along(x: f32, width: f32, thumb: f32, min: f64, max: f64, step: f64) -> f64 {
    let travel = width - thumb;
    let along = ((x - thumb / 2.) / travel).clamp(0., 1.) as f64;
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
        let dialog = self.dialog;
        let focus = super::focus::control(&self.id, window, cx);
        let keyed = handler.clone();
        let root = div().id(self.id).occlude().relative().cursor_pointer();
        let root = if dialog {
            // The track's 4px are the element's layout; the thumb hangs
            // over it, centred, and its travel is the track less a thumb.
            root.w_full()
                .h(px(DIALOG_TRACK))
                .rounded(px(2.))
                .bg(theme.border())
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right(px(DIALOG_THUMB))
                        .top(px((DIALOG_TRACK - DIALOG_THUMB) / 2.))
                        .h(px(DIALOG_THUMB))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left(relative(along))
                                .size(px(DIALOG_THUMB))
                                .rounded_full()
                                .bg(theme.primary()),
                        ),
                )
        } else {
            root.w(px(width))
                .h(px(THUMB))
                .flex_none()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px((THUMB - 3.) / 2.))
                        .h(px(3.))
                        .rounded(px(2.))
                        .bg(theme.border())
                        // The filled part runs to the thumb's place along the
                        // whole track, as `--tolerance-progress` does.
                        .when(self.filled, |track| {
                            track.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(relative(along))
                                    .rounded(px(2.))
                                    .bg(theme.primary()),
                            )
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px((width - THUMB) * along))
                        .size(px(THUMB))
                        .rounded_full()
                        .bg(theme.text())
                        .when(self.filled, |thumb| {
                            thumb.border_1().border_color(theme.border()).shadow(vec![
                                BoxShadow::new(px(0.), px(1.), rgba(theme::BLACK_25).into())
                                    .blur_radius(px(2.)),
                            ])
                        }),
                )
        };
        let thumb = if dialog { DIALOG_THUMB } else { THUMB };
        let root = root.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let report = {
                        let handler = handler.clone();
                        move |x: Pixels, cx: &mut App| {
                            let next = value_along(
                                f32::from(x - bounds.left()),
                                f32::from(bounds.size.width),
                                thumb,
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
            .left_0()
            .right_0()
            // The dialog's thumb overhangs its track, and takes presses
            // where it overhangs.
            .top(px(if dialog {
                (DIALOG_TRACK - DIALOG_THUMB) / 2.
            } else {
                0.
            }))
            .h(px(thumb)),
        );
        // `<input type="range">`'s keys; it draws no ring.
        let moved = move |key: SliderKey| {
            let keyed = keyed.clone();
            move |cx: &mut App| {
                let next = slider_key(key, value, min, max, step);
                if let Some(handler) = &keyed
                    && next != value
                {
                    handler(next, cx);
                }
            }
        };
        let root = root
            .on_action({
                let moved = moved(SliderKey::Down);
                move |_: &focus::Left, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::Down);
                move |_: &focus::Down, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::Up);
                move |_: &focus::Right, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::Up);
                move |_: &focus::Up, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::PageDown);
                move |_: &focus::PageDown, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::PageUp);
                move |_: &focus::PageUp, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::Home);
                move |_: &focus::Home, _, cx| moved(cx)
            })
            .on_action({
                let moved = moved(SliderKey::End);
                move |_: &focus::End, _, cx| moved(cx)
            });
        focus.attach(root, focus::Kind::Slider, |_, _, _| {})
    }
}

#[cfg(test)]
mod tests {
    use super::{WIDTH, value_along};

    fn value_at(x: f32, min: f64, max: f64, step: f64) -> f64 {
        value_along(x, WIDTH, super::THUMB, min, max, step)
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
