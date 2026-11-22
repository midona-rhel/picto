use crate::{motion, theme};
use gpui::*;
use std::rc::Rc;

type ToggleHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// A switch: a track 32px wide and 20px high with a 16px knob. When it is
/// switched on, the track is filled with the primary colour, and the knob
/// moves 12px to the right and goes from 60% to full strength. This takes 200
/// ms and follows `motion::EASE`. A disabled switch is drawn at 35% and takes
/// no clicks.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    on: bool,
    disabled: bool,
    /// The name that assistive technology gives the switch.
    name: Option<SharedString>,
    on_toggle: Option<ToggleHandler>,
}

pub fn toggle(id: impl Into<ElementId>, on: bool) -> Toggle {
    Toggle {
        id: id.into(),
        on,
        disabled: false,
        name: None,
        on_toggle: None,
    }
}

impl Toggle {
    /// Sets the name that assistive technology gives the switch. It is usually
    /// the label of the row that the switch is in.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Sets the handler that is called on a click. The switch does not change
    /// `on` itself; the caller changes the value that it passes in.
    pub fn on_toggle(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let on = self.on;
        let fade = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::at(on));
        if fade.read(cx).on() != on {
            fade.update(cx, |fade, _| fade.set(on, motion::TOGGLE_SWITCH));
        }
        let amount = fade.read(cx).value_with(motion::EASE);
        if fade.read(cx).animating() {
            window.request_animation_frame();
        }
        // The track goes from `theme::TOGGLE_TRACK_OFF` to the primary colour.
        let track = theme::blend(rgba(theme::TOGGLE_TRACK_OFF), theme.primary(), amount);
        let focus = super::focus::control(&self.id, window, cx);
        let mut toggle = div()
            .id(self.id)
            .role(Role::Switch)
            .aria_toggled(super::a11y::toggled(on))
            .aria_disabled(self.disabled)
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
                        BoxShadow::new(px(0.), px(1.), rgba(theme::BLACK_10).into())
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
                    .bg(rgb(theme::TOGGLE_KNOB))
                    .opacity(0.6 + 0.4 * amount)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(theme::BLACK_20).into())
                            .blur_radius(px(3.)),
                    ]),
            );
        if let Some(name) = self.name {
            toggle = toggle.aria_label(name);
        }
        if self.disabled {
            return toggle.opacity(0.35);
        }
        toggle = toggle.cursor_pointer();
        // The switch is a Tab stop, and Enter and Space switch it. It draws no
        // focus ring.
        let handler: Option<Rc<dyn Fn(&mut Window, &mut App)>> = self.on_toggle.map(Rc::from);
        if let Some(handler) = handler.clone() {
            toggle = toggle.on_click(move |_, window, cx| {
                cx.stop_propagation();
                handler(window, cx);
            });
        }
        focus.attach(toggle, super::focus::Kind::Button, move |_, window, cx| {
            if let Some(handler) = &handler {
                handler(window, cx);
            }
        })
    }
}
