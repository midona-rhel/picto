use crate::{theme, ui};
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// The user moved the zoom to this tile size.
pub struct Changed(pub f32);

// `ZOOM_MIN`, `ZOOM_MAX`, `ZOOM_STEP` and the slider's own step in
// `GridToolbar.tsx`.
const MINIMUM: f32 = 150.;
const MAXIMUM: f32 = 900.;
const BUTTON_STEP: f32 = 50.;
const SLIDER_STEP: f32 = 10.;
const TRACK: f32 = 80.;
const KNOB: f32 = 12.;

/// The toolbar's zoom control: a slider between two step buttons.
pub struct Zoom {
    value: f32,
    /// Set while the knob is held, so the drag follows the pointer anywhere
    /// in the window, as a native range input does.
    dragging: Rc<Cell<bool>>,
}

impl EventEmitter<Changed> for Zoom {}

impl Zoom {
    pub fn new() -> Self {
        Self {
            value: 220.,
            dragging: Rc::new(Cell::new(false)),
        }
    }

    /// Moves the control to a value that came from elsewhere, without
    /// reporting it back as a user change.
    pub fn show(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = value.clamp(MINIMUM, MAXIMUM);
        cx.notify();
    }

    fn set(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(MINIMUM, MAXIMUM);
        if value != self.value {
            self.value = value;
            cx.emit(Changed(value));
            cx.notify();
        }
    }

    /// Puts the knob's centre under the pointer, in steps of ten.
    fn at(&mut self, x: Pixels, track: Bounds<Pixels>, cx: &mut Context<Self>) {
        let along =
            (f32::from(x - track.left()) - KNOB / 2.) / (f32::from(track.size.width) - KNOB);
        let value = MINIMUM + (MAXIMUM - MINIMUM) * along.clamp(0., 1.);
        self.set((value / SLIDER_STEP).round() * SLIDER_STEP, cx);
    }
}

impl Render for Zoom {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let entity = cx.entity().downgrade();
        let dragging = self.dragging.clone();
        // The slider's own pointer handling: a press anywhere on it starts a
        // drag that follows the pointer until the button is released.
        let pointer = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let (zoom, held) = (entity.clone(), dragging.clone());
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && bounds.contains(&event.position)
                    {
                        held.set(true);
                        let _ = zoom.update(cx, |zoom, cx| zoom.at(event.position.x, bounds, cx));
                        cx.stop_propagation();
                    }
                });
                let (zoom, held) = (entity.clone(), dragging.clone());
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && held.get() {
                        let _ = zoom.update(cx, |zoom, cx| zoom.at(event.position.x, bounds, cx));
                    }
                });
                let held = dragging.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                    if phase == DispatchPhase::Capture {
                        held.set(false);
                    }
                });
            },
        )
        .size_full();
        let slider = div()
            .id("zoom-slider")
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
                    .left(px(
                        (self.value - MINIMUM) / (MAXIMUM - MINIMUM) * (TRACK - KNOB)
                    ))
                    .size(px(KNOB))
                    .rounded_full()
                    .bg(theme.text()),
            )
            .child(pointer);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                ui::icon_button("zoom-out", "toolbar-minus")
                    .disabled(self.value <= MINIMUM)
                    .tip(ui::tip("Zoom out").keys(&["-"]))
                    .on_click(cx.listener(|this, _, _, cx| this.set(this.value - BUTTON_STEP, cx))),
            )
            .child(slider)
            .child(
                ui::icon_button("zoom-in", "toolbar-plus")
                    .disabled(self.value >= MAXIMUM)
                    .tip(ui::tip("Zoom in").keys(&["+"]))
                    .on_click(cx.listener(|this, _, _, cx| this.set(this.value + BUTTON_STEP, cx))),
            )
    }
}
