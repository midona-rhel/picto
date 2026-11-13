use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// The compact number field (`CompactNumberInput.tsx` over `GlassInput`): a
/// 26px field with the value set right, and a stepper of two chevrons at
/// half strength that come up to full under the pointer.
#[derive(IntoElement)]
pub struct NumberInput {
    id: ElementId,
    value: SharedString,
    width: f32,
    disabled: bool,
}

pub fn number_input(id: impl Into<ElementId>, value: impl Into<SharedString>) -> NumberInput {
    NumberInput {
        id: id.into(),
        value: value.into(),
        width: 100.,
        disabled: false,
    }
}

impl NumberInput {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for NumberInput {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let stepper = |name: &'static str, icon: &'static str| {
            let step = div()
                .id(name)
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .child(
                    super::icon(icon, 11., theme.text())
                        .opacity(if self.disabled { 0.2 } else { 0.5 })
                        .group_hover(name, |style| style.opacity(1.)),
                );
            if self.disabled {
                step
            } else {
                // TODO: step the value by one and commit it.
                step.group(name).cursor_pointer()
            }
        };
        div()
            .id(self.id)
            .relative()
            .w(px(self.width))
            .h(px(theme::CONTROL_SM))
            .flex_none()
            .pl(px(8.))
            .pr(px(25.))
            .flex()
            .items_center()
            .justify_end()
            .rounded(px(theme::RADIUS_SM))
            .border_1()
            .border_color(theme.border())
            .bg(theme.control_surface())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_SM)
            .text_color(theme.text())
            .opacity(if self.disabled { 0.5 } else { 1. })
            .child(self.value)
            .child(
                div()
                    .absolute()
                    .top(px(1.))
                    .bottom(px(1.))
                    .right(px(3.))
                    .w(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(stepper("number-up", "chevron-up-2"))
                    .child(stepper("number-down", "chevron-down-2")),
            )
    }
}
