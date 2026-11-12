use gpui::*;
pub struct Changed(pub f32);
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
        self.value = value.clamp(100., 450.);
        cx.notify();
    }
    fn set(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = value.clamp(100., 450.);
        cx.emit(Changed(self.value));
        cx.notify();
    }
    fn at(&mut self, x: Pixels, cx: &mut Context<Self>) {
        self.set(
            100. + 350. * (f32::from(x - self.bounds.origin.x) / 80.).clamp(0., 1.),
            cx,
        );
    }
}
impl Render for Zoom {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let slider = div()
            .id("zoom-slider")
            .relative()
            .w(px(80.))
            .h(px(24.))
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
                    .top(px(10.))
                    .h(px(3.))
                    .rounded_full()
                    .bg(rgb(0x343538)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(5.))
                    .left(px((self.value - 100.) / 350. * 68.))
                    .size(px(12.))
                    .rounded_full()
                    .bg(rgb(0xf8f9fb)),
            );
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .id("zoom-out")
                    .w(px(24.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text_size(px(18.))
                    .rounded_md()
                    .hover(|s| s.bg(rgb(0x303134)))
                    .child("−")
                    .on_click(cx.listener(|this, _, _, cx| this.set(this.value - 50., cx))),
            )
            .child(slider)
            .child(
                div()
                    .id("zoom-in")
                    .w(px(24.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text_size(px(18.))
                    .rounded_md()
                    .hover(|s| s.bg(rgb(0x303134)))
                    .child("+")
                    .on_click(cx.listener(|this, _, _, cx| this.set(this.value + 50., cx))),
            )
    }
}
