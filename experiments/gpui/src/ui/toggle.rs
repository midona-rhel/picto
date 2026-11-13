use crate::{motion, theme};
use gpui::*;

type ToggleHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// `ToggleSwitch.tsx`: a 32×20 track with a 16px knob. Switching on fills
/// the track with the primary colour and slides the knob 12px while it
/// brightens from 60%, all over 200 ms on CSS `ease`. Disabled, it dims to
/// 35% and takes no clicks.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    on: bool,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

pub fn toggle(id: impl Into<ElementId>, on: bool) -> Toggle {
    Toggle {
        id: id.into(),
        on,
        disabled: false,
        on_toggle: None,
    }
}

impl Toggle {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Called with the click; the owner flips the value it passed in.
    pub fn on_toggle(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

/// `transition: 0.2s ease`
const SWITCH: std::time::Duration = std::time::Duration::from_millis(200);

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let on = self.on;
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::at(on));
        if fade.read(cx).on() != on {
            fade.update(cx, |fade, _| fade.set(on, SWITCH));
        }
        let amount = fade.read(cx).value_with(motion::EASE);
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        // `rgba(128, 128, 128, 0.25)`, then `--color-primary`.
        let track = theme::blend(rgba(0x80808040), theme.primary(), amount);
        let mut toggle = div()
            .id(self.id)
            .occlude()
            .relative()
            .w(px(32.))
            .h(px(20.))
            .flex_none()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(10.))
                    .bg(track)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(0x0000001a).into())
                            .blur_radius(px(2.))
                            .inset(),
                    ]),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(2.))
                    .left(px(2. + 12. * amount))
                    .size(px(16.))
                    .rounded_full()
                    .bg(rgb(0xffffff))
                    .opacity(0.6 + 0.4 * amount)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(0x00000033).into()).blur_radius(px(3.)),
                    ]),
            );
        if self.disabled {
            return toggle.opacity(0.35);
        }
        toggle = toggle.cursor_pointer();
        if let Some(handler) = self.on_toggle {
            toggle = toggle.on_click(move |_, window, cx| {
                cx.stop_propagation();
                handler(window, cx);
            });
        }
        toggle
    }
}
