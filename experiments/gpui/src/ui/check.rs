use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ToggleHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

#[derive(Clone, Copy, PartialEq)]
pub enum Mark {
    /// A square box with a tick.
    Checkbox,
    /// A round button with a white dot.
    Radio,
}

/// A labelled checkbox or radio row (`.checkboxItem` in
/// `Settings.module.css`): a 16px indicator and a label, over a highlight
/// that fades in over 100 ms under the pointer. The row reaches 6px left of
/// its slot, so the indicator lines up with text above it and the highlight
/// has room around it. Disabled, it dims to half and takes no clicks.
#[derive(IntoElement)]
pub struct Check {
    id: ElementId,
    label: SharedString,
    mark: Mark,
    checked: bool,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

pub fn check(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Check {
    Check {
        id: id.into(),
        label: label.into(),
        mark: Mark::Checkbox,
        checked,
        disabled: false,
        on_toggle: None,
    }
}

impl Check {
    pub fn radio(mut self) -> Self {
        self.mark = Mark::Radio;
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
        let hover = window.use_keyed_state(self.id.clone(), cx, |_, _| motion::Fade::default());
        let strength = hover.read(cx).value_with(motion::EASE);
        if hover.read(cx).animating() {
            window.request_animation_frame();
        }
        let radius = match self.mark {
            Mark::Checkbox => px(4.),
            Mark::Radio => px(8.),
        };
        let mut indicator = div()
            .relative()
            .size(px(16.))
            .flex_none()
            .mr(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius)
            .border_1();
        indicator = if self.checked {
            indicator.border_color(rgba(0x0000001a)).bg(theme.primary())
        } else {
            indicator
                .border_color(theme.contrast(30.))
                .bg(theme.control_surface())
        };
        if self.checked {
            indicator = indicator.child(match self.mark {
                Mark::Checkbox => {
                    super::icon("settings-check", 11., rgb(0xffffff)).into_any_element()
                }
                // `::after { inset: 3px }` inside the border, with a 10% ring.
                Mark::Radio => div()
                    .absolute()
                    .top(px(3.))
                    .left(px(3.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(rgb(0xffffff))
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), rgba(0x0000001a).into())
                            .spread_radius(px(1.)),
                    ])
                    .into_any_element(),
            });
        }
        let mut row = div()
            .id(self.id)
            .relative()
            .min_h(px(26.))
            // `margin-left: -6px` on a full-width row: shifted, not widened.
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
        row = row.cursor_pointer().on_hover(move |hovered, _, cx| {
            hover.update(cx, |hover, cx| {
                hover.set(*hovered, motion::ROW_HOVER);
                cx.notify();
            })
        });
        if let Some(handler) = self.on_toggle {
            row = row.on_click(move |_, window, cx| handler(window, cx));
        }
        row
    }
}
