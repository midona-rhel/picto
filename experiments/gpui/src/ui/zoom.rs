use crate::{theme, ui};
use gpui::*;

/// The user moved the zoom to this tile size.
pub struct Changed(pub f32);

const MINIMUM: f32 = 100.;
const MAXIMUM: f32 = 450.;
const TRACK: f32 = 80.;
const KNOB: f32 = 12.;

/// The toolbar's zoom control: a slider between two step buttons.
pub struct Zoom {
    value: f32,
    bounds: Bounds<Pixels>,
    dragging: bool,
}

impl EventEmitter<Changed> for Zoom {}

impl Zoom {
    pub fn new() -> Self {
        Self {
            value: 200.,
            bounds: Bounds::default(),
            dragging: false,
        }
    }

    /// Moves the control to a value that came from elsewhere, without
    /// reporting it back as a user change.
    pub fn show(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = value.clamp(MINIMUM, MAXIMUM);
        cx.notify();
    }

    fn set(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = value.clamp(MINIMUM, MAXIMUM);
        cx.emit(Changed(self.value));
        cx.notify();
    }

    fn at(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let along = (f32::from(x - self.bounds.origin.x) / TRACK).clamp(0., 1.);
        self.set(MINIMUM + (MAXIMUM - MINIMUM) * along, cx);
    }
}

impl Render for Zoom {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let entity = cx.entity().downgrade();
        let slider = div()
            .id("zoom-slider")
            .relative()
            .w(px(TRACK))
            .h(px(theme::ICON_BUTTON))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.dragging = true;
                    this.at(event.position.x, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging {
                    this.at(event.position.x, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = entity.update(cx, |this, _| this.bounds = bounds);
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(11.))
                    .h(px(3.))
                    .rounded_full()
                    .bg(theme.contrast(12.)),
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
            );
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                ui::icon_button("zoom-out", "toolbar-minus")
                    .on_click(cx.listener(|this, _, _, cx| this.set(this.value - 50., cx))),
            )
            .child(slider)
            .child(
                ui::icon_button("zoom-in", "toolbar-plus")
                    .on_click(cx.listener(|this, _, _, cx| this.set(this.value + 50., cx))),
            )
    }
}
