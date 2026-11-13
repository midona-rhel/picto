//! The titlebar's zoom slider (`TitlebarControls.tsx`): the grid's tile size
//! between two step buttons (`TitlebarZoomSlider`), or the viewer's zoom
//! percentage on a logarithmic track with no buttons (`TitlebarRangeSlider`
//! as `ViewerToolbar` uses it).
use crate::{theme, ui};
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// The user moved the zoom to this value: a tile size for the grid, a
/// percentage for the viewer.
pub struct Changed(pub f32);

// `ZOOM_MIN`, `ZOOM_MAX`, `ZOOM_STEP` and the slider's own step in
// `GridToolbar.tsx`.
const MINIMUM: f32 = 150.;
const MAXIMUM: f32 = 900.;
const BUTTON_STEP: f32 = 50.;
const SLIDER_STEP: f32 = 10.;
const TRACK: f32 = 80.;
const KNOB: f32 = 12.;

/// The viewer's range in percent (`MIN_SCALE` and `MAX_SCALE` of
/// `useImageZoom.ts`), and its slider's step along the 0–100 track.
const PERCENT_MINIMUM: f32 = 5.;
const PERCENT_MAXIMUM: f32 = 800.;
const POSITION_STEP: f32 = 0.5;

/// The grid's tile size one step larger or smaller, as the slider's buttons
/// and the zoom keys move it.
pub fn step_tile(size: f32, larger: bool) -> f32 {
    let step = if larger { BUTTON_STEP } else { -BUTTON_STEP };
    (size + step).clamp(MINIMUM, MAXIMUM)
}

/// Where a zoom percentage sits on the viewer's 0–100 track: 5% at the left
/// end, 100% in the middle, 800% at the right end, logarithmic on each side
/// (`zoomToSlider` in `GridToolbar.tsx`).
pub fn percent_to_position(percent: f32) -> f32 {
    if percent <= 100. {
        50. * (percent / PERCENT_MINIMUM).ln() / (100. / PERCENT_MINIMUM).ln()
    } else {
        50. + 50. * (percent / 100.).ln() / (PERCENT_MAXIMUM / 100.).ln()
    }
}

/// The inverse of [`percent_to_position`] (`sliderToZoom`).
pub fn position_to_percent(position: f32) -> f32 {
    if position <= 50. {
        PERCENT_MINIMUM * (100. / PERCENT_MINIMUM).powf(position / 50.)
    } else {
        100. * (PERCENT_MAXIMUM / 100.).powf((position - 50.) / 50.)
    }
}

/// `.sliderSection`: what the zoom sits on in the titlebar. It has the app
/// background, with a 20px fade on its left, so a long title disappears
/// under it, and it never drags the window.
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

#[derive(Clone, Copy, PartialEq)]
enum Scale {
    /// The grid's tile size, linear, in steps of ten, with step buttons.
    TileSize,
    /// The viewer's percentage, logarithmic, without buttons.
    Percent,
}

pub struct Zoom {
    value: f32,
    scale: Scale,
    /// Set while the knob is held, so the drag follows the pointer anywhere
    /// in the window, as a native range input does.
    dragging: Rc<Cell<bool>>,
}

impl EventEmitter<Changed> for Zoom {}

impl Zoom {
    /// The grid's tile-size zoom.
    pub fn new() -> Self {
        Self {
            value: 220.,
            scale: Scale::TileSize,
            dragging: Rc::new(Cell::new(false)),
        }
    }

    /// The viewer's percentage zoom.
    pub fn percent() -> Self {
        Self {
            value: 100.,
            scale: Scale::Percent,
            dragging: Rc::new(Cell::new(false)),
        }
    }

    fn bounds(&self) -> (f32, f32) {
        match self.scale {
            Scale::TileSize => (MINIMUM, MAXIMUM),
            Scale::Percent => (PERCENT_MINIMUM, PERCENT_MAXIMUM),
        }
    }

    /// Moves the control to a value that came from elsewhere, without
    /// reporting it back as a user change. While the knob is held it stays
    /// under the pointer, as the viewer's slider ignores the live zoom then.
    pub fn show(&mut self, value: f32, cx: &mut Context<Self>) {
        if self.dragging.get() {
            return;
        }
        let (minimum, maximum) = self.bounds();
        let value = value.clamp(minimum, maximum);
        if value != self.value {
            self.value = value;
            cx.notify();
        }
    }

    fn set(&mut self, value: f32, cx: &mut Context<Self>) {
        let (minimum, maximum) = self.bounds();
        let value = value.clamp(minimum, maximum);
        if value != self.value {
            self.value = value;
            cx.emit(Changed(value));
            cx.notify();
        }
    }

    /// The knob's place along the track, 0 to 1.
    fn along(&self) -> f32 {
        match self.scale {
            Scale::TileSize => (self.value - MINIMUM) / (MAXIMUM - MINIMUM),
            Scale::Percent => percent_to_position(self.value) / 100.,
        }
        .clamp(0., 1.)
    }

    /// Puts the knob's centre under the pointer, snapped to the slider's step.
    fn at(&mut self, x: Pixels, track: Bounds<Pixels>, cx: &mut Context<Self>) {
        let along = ((f32::from(x - track.left()) - KNOB / 2.)
            / (f32::from(track.size.width) - KNOB))
            .clamp(0., 1.);
        let value = match self.scale {
            Scale::TileSize => {
                let value = MINIMUM + (MAXIMUM - MINIMUM) * along;
                (value / SLIDER_STEP).round() * SLIDER_STEP
            }
            Scale::Percent => {
                position_to_percent((along * 100. / POSITION_STEP).round() * POSITION_STEP)
            }
        };
        self.set(value, cx);
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
                    .left(px(self.along() * (TRACK - KNOB)))
                    .size(px(KNOB))
                    .rounded_full()
                    .bg(theme.text()),
            )
            .child(pointer);
        if self.scale == Scale::Percent {
            return slider.into_any_element();
        }
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                ui::icon_button("zoom-out", "toolbar-minus")
                    .disabled(self.value <= MINIMUM)
                    .tip(ui::tip("Zoom out").shortcut("view.zoomOut", cx))
                    .on_click(
                        cx.listener(|this, _, _, cx| this.set(step_tile(this.value, false), cx)),
                    ),
            )
            .child(slider)
            .child(
                ui::icon_button("zoom-in", "toolbar-plus")
                    .disabled(self.value >= MAXIMUM)
                    .tip(ui::tip("Zoom in").shortcut("view.zoomIn", cx))
                    .on_click(
                        cx.listener(|this, _, _, cx| this.set(step_tile(this.value, true), cx)),
                    ),
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
        // 20% is halfway (in ratio) from 5% to 100%, so a quarter along.
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
